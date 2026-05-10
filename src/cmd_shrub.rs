/// Shrub class mesh extraction
/// Ported from rac_shrub_extractor.py
///
/// Parses GcUyaDlShrubClassHeader → packet table → VIF command lists → vertex data.
/// Shrubs use VIF UNPACK commands: positions (V4_16), attributes (V4_16), primitives (V4_32).
/// Attributes contain per-vertex texture atlas tile offset + texture page index.
/// Rendered as quad strips (billboard style): vertex pairs (left, right) per row.

use std::collections::BTreeMap;
use std::path::Path;
use crate::cli::*;
use crate::common::*;

const SHRUB_PACKET_HEADER_SIZE: usize = 0x40;

/// Per-quad vertex data from attribute fields
#[derive(Clone, Debug)]
struct ShrubVertex {
    pos: (f32, f32, f32),
    f0: i16,
    f1: i16,
    tex_page: u16,
}

/// Parse a shrub class from decompressed core_data
fn parse_shrub_class(data: &[u8], base_ofs: usize, o_class: i32) -> Option<ShrubClassInfo> {
    if base_ofs + 0x48 > data.len() {
        return None;
    }

    let packet_count = r_u32(data, base_ofs + 0x28);
    let _total_size = r_u32(data, base_ofs + 0x2c);
    let scale = f32::from_le_bytes(data[base_ofs + 0x10..base_ofs + 0x14].try_into().ok()?);

    if packet_count == 0 {
        return None;
    }

    // Read packet offset table at base_ofs + 0x40
    let mut packets = Vec::new();
    for pi in 0..packet_count as usize {
        let tbl_ofs = base_ofs + 0x40 + pi * 8;
        if tbl_ofs + 8 > data.len() {
            break;
        }
        let rel_offset = r_u32(data, tbl_ofs) as usize;
        let size = r_u32(data, tbl_ofs + 4) as usize;
        let abs_offset = base_ofs + 0x40 + rel_offset;
        packets.push(ShrubPacket { rel_offset, size, abs_offset });
    }

    Some(ShrubClassInfo { o_class, packet_count, scale, packets })
}

struct ShrubPacket {
    rel_offset: usize,
    size: usize,
    abs_offset: usize,
}

struct ShrubClassInfo {
    #[allow(dead_code)]
    o_class: i32,
    #[allow(dead_code)]
    packet_count: u32,
    scale: f32,
    packets: Vec<ShrubPacket>,
}

/// Parse a VIF command list, capturing UNPACK data
struct VifUnpack {
    vnvl_raw: u8,
    num: u16,
    data: Vec<u8>,
}

fn read_vif_unpacks(data: &[u8], base_ofs: usize, max_size: usize) -> Vec<VifUnpack> {
    let mut unpacks = Vec::new();
    let mut ofs = base_ofs;
    let end = (base_ofs + max_size).min(data.len());

    while ofs + 4 <= end {
        let val = r_u32(data, ofs);
        let cmd_byte = ((val >> 24) & 0x7f) as u8;
        let num = {
            let n = ((val >> 16) & 0xff) as u16;
            if n == 0 { 256 } else { n }
        };

        // Check if UNPACK (bits 5 and 6 set)
        if (cmd_byte & 0x60) == 0x60 {
            let vnvl = cmd_byte & 0x0f;
            let elem_sz = match vnvl {
                0b1101 => 8,  // V4_16
                0b1100 => 16, // V4_32
                _ => 1,
            };
            let total = (num as u32) * elem_sz;
            let aligned = (total + 15) & !15;
            let qwc = aligned / 16;
            let data_start = ofs + 4;
            let data_end = (data_start + qwc as usize * 16).min(end);
            let raw = data[data_start..data_end].to_vec();

            unpacks.push(VifUnpack { vnvl_raw: vnvl, num, data: raw });
            ofs += 4 + qwc as usize * 16;
        } else {
            // Skip non-UNPACK command (all 4-byte in shrub VIF)
            ofs += 4;
        }
    }

    unpacks
}

/// Extract mesh data from shrub packets
fn extract_shrub_mesh(data: &[u8], info: &ShrubClassInfo, tex_info: &BTreeMap<u32, (u32, u32)>) -> Vec<(u16, ShrubMeshGroup)> {
    let scale = info.scale;
    let s_factor = scale / 1024.0;

    let mut groups: BTreeMap<u16, ShrubMeshGroup> = BTreeMap::new();
    let mut group_order: Vec<u16> = Vec::new();

    for pkt in &info.packets {
        if pkt.size <= SHRUB_PACKET_HEADER_SIZE {
            continue;
        }

        let vif_start = pkt.abs_offset + SHRUB_PACKET_HEADER_SIZE;
        let vif_max = pkt.size - SHRUB_PACKET_HEADER_SIZE;

        if vif_start + 4 > data.len() {
            continue;
        }

        let unpacks = read_vif_unpacks(data, vif_start, vif_max);

        // Find V4_16 UNPACKs: first = positions, second = attributes
        let mut pos_unpack: Option<&VifUnpack> = None;
        let mut attr_unpack: Option<&VifUnpack> = None;

        for up in &unpacks {
            if up.vnvl_raw == 0b1101 {
                if pos_unpack.is_none() {
                    pos_unpack = Some(up);
                } else if attr_unpack.is_none() {
                    attr_unpack = Some(up);
                    break;
                }
            }
        }

        let (pos_data, attr_data) = match (pos_unpack, attr_unpack) {
            (Some(p), Some(a)) => (&p.data, &a.data),
            _ => continue,
        };

        // Parse vertex positions (V4_16: each element = 4 × s16 = 8 bytes)
        let elem_size = 8;
        let num_pos = pos_data.len() / elem_size;
        let mut positions: Vec<(f32, f32, f32)> = Vec::new();
        for i in 0..num_pos {
            let off = i * 8;
            if off + 8 > pos_data.len() {
                break;
            }
            let x = r_s16(pos_data, off) as f32;
            let y = r_s16(pos_data, off + 2) as f32;
            let z = r_s16(pos_data, off + 4) as f32;
            positions.push((x * s_factor, y * s_factor, z * s_factor));
        }

        if positions.len() < 2 {
            continue;
        }

        // Parse attributes (V4_16: each element = 4 × s16 = 8 bytes)
        let num_attr = attr_data.len() / elem_size;
        let mut attributes: Vec<(i16, i16, u16)> = Vec::new();
        for i in 0..num_attr.min(num_pos) {
            let off = i * 8;
            if off + 8 > attr_data.len() {
                break;
            }
            let f0 = r_s16(attr_data, off);
            let f1 = r_s16(attr_data, off + 2);
            let tex_page_s16 = r_s16(attr_data, off + 6);
            let tex_page = (tex_page_s16 as u16) & 0x7FFF;
            let tex_page = if tex_info.contains_key(&(tex_page as u32)) { tex_page } else { 0 };
            attributes.push((f0, f1, tex_page as u16));
        }

        if attributes.len() < 2 || attributes.len() % 2 != 0 {
            continue;
        }

        // Build quad strips (billboard style) with per-vertex UVs
        // Vertices arranged as pairs (left, right) per row
        let max_quad = attributes.len().saturating_sub(3);
        for i in (0..max_quad).step_by(2) {
            let p0 = positions[i];      // left, row i
            let p1 = positions[i + 1];  // right, row i
            let p2 = positions[i + 3];  // right, row i+1
            let p3 = positions[i + 2];  // left, row i+1

            let a0 = attributes[i];
            let a1 = attributes[i + 1];
            let a2 = attributes[i + 2];
            let a3 = attributes[i + 3];

            let tex_page = a0.2;

            // Per-quad UV normalization
            let f0_vals = [a0.0 as f32, a1.0 as f32, a2.0 as f32, a3.0 as f32];
            let f1_vals = [a0.1 as f32, a1.1 as f32, a2.1 as f32, a3.1 as f32];

            let min_f0 = f0_vals.iter().cloned().fold(f32::MAX, f32::min);
            let max_f0 = f0_vals.iter().cloned().fold(f32::MIN, f32::max);
            let min_f1 = f1_vals.iter().cloned().fold(f32::MAX, f32::min);
            let max_f1 = f1_vals.iter().cloned().fold(f32::MIN, f32::max);

            let range_f0 = if max_f0 > min_f0 { max_f0 - min_f0 } else { 1.0 };
            let range_f1 = if max_f1 > min_f1 { max_f1 - min_f1 } else { 1.0 };

            let uvs = [
                ((f0_vals[0] - min_f0) / range_f0, (f1_vals[0] - min_f1) / range_f1),
                ((f0_vals[1] - min_f0) / range_f0, (f1_vals[1] - min_f1) / range_f1),
                ((f0_vals[2] - min_f0) / range_f0, (f1_vals[2] - min_f1) / range_f1),
                ((f0_vals[3] - min_f0) / range_f0, (f1_vals[3] - min_f1) / range_f1),
            ];

            // Get or create group
            if !groups.contains_key(&tex_page) {
                group_order.push(tex_page);
                groups.insert(tex_page, ShrubMeshGroup { vertices: Vec::new(), triangles: Vec::new() });
            }
            let group = groups.get_mut(&tex_page).unwrap();
            let base = group.vertices.len() as u32;

            group.vertices.push(ShrubVert { pos: p0, uv: uvs[0] });
            group.vertices.push(ShrubVert { pos: p1, uv: uvs[1] });
            group.vertices.push(ShrubVert { pos: p2, uv: uvs[2] });
            group.vertices.push(ShrubVert { pos: p3, uv: uvs[3] });
            group.triangles.push([base, base + 1, base + 3]);
            group.triangles.push([base + 1, base + 2, base + 3]);
        }
    }

    // Build ordered result
    let mut result = Vec::new();
    for tex_page in group_order {
        if let Some(group) = groups.remove(&tex_page) {
            result.push((tex_page, group));
        }
    }
    result
}

struct ShrubVert {
    pos: (f32, f32, f32),
    uv: (f32, f32),
}

struct ShrubMeshGroup {
    vertices: Vec<ShrubVert>,
    triangles: Vec<[u32; 3]>,
}

fn load_texture_info(level_dir: &Path) -> BTreeMap<u32, (u32, u32)> {
    let ch_path = level_dir.join("core_header.json");
    let ci_path = level_dir.join("core_index.bin");
    if !ch_path.exists() || !ci_path.exists() {
        return BTreeMap::new();
    }
    let ch: serde_json::Value = match serde_json::from_str(&std::fs::read_to_string(&ch_path).unwrap_or_default()) {
        Ok(v) => v,
        Err(_) => return BTreeMap::new(),
    };
    let st = &ch["shrub_textures"];
    let count = st["count"].as_i64().unwrap_or(0) as usize;
    let offset = st["offset"].as_i64().unwrap_or(0) as usize;
    if count == 0 || offset == 0 {
        return BTreeMap::new();
    }
    let idx = match std::fs::read(&ci_path) {
        Ok(d) => d,
        Err(_) => return BTreeMap::new(),
    };
    let mut info = BTreeMap::new();
    for i in 0..count {
        let eo = offset + i * 16;
        if eo + 16 > idx.len() {
            break;
        }
        let _data_off = r_s32(&idx, eo);
        let w = r_s16(&idx, eo + 4) as u32;
        let h = r_s16(&idx, eo + 6) as u32;
        if w > 0 && h > 0 && w <= 1024 && h <= 1024 {
            info.insert(i as u32, (w, h));
        }
    }
    info
}

fn write_obj_mtl(
    obj_path: &Path,
    mtl_path: &Path,
    groups: &[(u16, ShrubMeshGroup)],
    level_name: &str,
) -> Result<(), String> {
    use std::io::Write;

    if groups.is_empty() {
        return Ok(());
    }

    let total_verts: usize = groups.iter().map(|(_, g)| g.vertices.len()).sum();
    let total_tris: usize = groups.iter().map(|(_, g)| g.triangles.len()).sum();

    // Write MTL
    let mut mf = std::fs::File::create(mtl_path).map_err(|e| format!("create mtl: {}", e))?;
    writeln!(mf, "# Shrub mesh materials - {}", level_name).ok();
    for (tex_page, _) in groups {
        writeln!(mf, "\nnewmtl tex_{}", tex_page).ok();
        writeln!(mf, "Ka 1.0 1.0 1.0").ok();
        writeln!(mf, "Kd 1.0 1.0 1.0").ok();
        writeln!(mf, "Ks 0.0 0.0 0.0").ok();
        writeln!(mf, "d 1.0").ok();
        writeln!(mf, "illum 1").ok();
        writeln!(mf, "map_Kd ../../textures/{}/shrub/shrub_{:03}.png", level_name, tex_page).ok();
    }

    // Write OBJ
    let mut f = std::fs::File::create(obj_path).map_err(|e| format!("create obj: {}", e))?;
    writeln!(f, "# Shrub mesh - {} vertices, {} triangles", total_verts, total_tris).ok();
    writeln!(f, "mtllib {}", mtl_path.file_name().unwrap().to_string_lossy()).ok();

    // Write all vertices
    for (_, group) in groups {
        for v in &group.vertices {
            writeln!(f, "v {:.6} {:.6} {:.6}", v.pos.0, v.pos.1, v.pos.2).ok();
        }
    }

    // Write all UVs (same order as vertices)
    for (_, group) in groups {
        for v in &group.vertices {
            writeln!(f, "vt {:.6} {:.6}", v.uv.0, v.uv.1).ok();
        }
    }

    // Write faces grouped by texture
    let mut vert_offset = 0u32;
    for (tex_page, group) in groups {
        if group.triangles.is_empty() {
            continue;
        }
        writeln!(f, "o shrub_tex_{}", tex_page).ok();
        writeln!(f, "usemtl tex_{}", tex_page).ok();
        writeln!(f, "s off").ok();
        for tri in &group.triangles {
            let i0 = tri[0] + vert_offset + 1;
            let i1 = tri[1] + vert_offset + 1;
            let i2 = tri[2] + vert_offset + 1;
            writeln!(f, "f {0}/{0} {1}/{1} {2}/{2}", i0, i1, i2).ok();
        }
        vert_offset += group.vertices.len() as u32;
    }

    Ok(())
}

fn process_level(base: &Path, level_num: u32, target_class: Option<i32>) -> Result<(), String> {
    let level_dir = base.join(format!("LEVEL{:03}", level_num)).join("data_wad");
    let core_path = level_dir.join("core_data.bin");
    let json_path = level_dir.join("shrub_classes.json");

    if !core_path.exists() {
        return Err(format!("core_data not found for LEVEL{:03}", level_num));
    }
    if !json_path.exists() {
        return Err(format!("shrub_classes.json not found for LEVEL{:03}", level_num));
    }

    let core_data = std::fs::read(&core_path).map_err(|e| format!("read core_data: {}", e))?;
    let shrub_json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&json_path).map_err(|e| format!("read json: {}", e))?
    ).map_err(|e| format!("parse json: {}", e))?;

    let entries = shrub_json["entries"].as_array().ok_or("no entries")?;
    let tex_info = load_texture_info(&level_dir);
    let level_name = format!("LEVEL{:03}", level_num);

    println!("=== {}: {} shrub class entries ===", level_name, entries.len());

    let scripts_dir = base.parent().and_then(|p| p.parent()).unwrap_or(base);
    let out_dir = meshes_dir(scripts_dir).join(&level_name);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir: {}", e))?;

    let mut mesh_count = 0u32;
    for entry in entries {
        let wad_off = entry["wad_off"].as_i64().unwrap_or(-1);
        let o_class = entry["o_class"].as_i64().unwrap_or(-1) as i32;

        if wad_off <= 0 {
            continue;
        }
        if let Some(tc) = target_class {
            if o_class != tc {
                continue;
            }
        }
        if (wad_off as usize) + 0x48 > core_data.len() {
            continue;
        }

        let info = match parse_shrub_class(&core_data, wad_off as usize, o_class) {
            Some(i) => i,
            None => continue,
        };

        println!("  Class {}: {} packets, scale={:.4}", o_class, info.packet_count, info.scale);

        let groups = extract_shrub_mesh(&core_data, &info, &tex_info);
        if groups.is_empty() {
            continue;
        }

        let total_v: usize = groups.iter().map(|(_, g)| g.vertices.len()).sum();
        let total_t: usize = groups.iter().map(|(_, g)| g.triangles.len()).sum();
        let tex_pages: Vec<u16> = groups.iter().map(|(tp, _)| *tp).collect();

        let obj_path = out_dir.join(format!("shrub_{}.obj", o_class));
        let mtl_path = out_dir.join(format!("shrub_{}.mtl", o_class));
        write_obj_mtl(&obj_path, &mtl_path, &groups, &level_name)?;

        println!("    -> ({}/{}/{} mats: {:?})", total_v, total_t, groups.len(), tex_pages);
        mesh_count += 1;
    }

    println!("  => {} shrub meshes extracted", mesh_count);
    Ok(())
}

pub fn run(scripts_dir: &Path, args: &ShrubArgs) -> Result<(), String> {
    let unpacked = unpacked_dir(scripts_dir);
    let level_filter = args.level.unwrap_or(-1);
    let target_class = args.class;

    level_dispatch(level_filter, |level_num| {
        process_level(&unpacked, level_num, target_class)
    })
}
