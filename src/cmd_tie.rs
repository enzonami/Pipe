/// Tie class mesh extraction
/// Ported from rac_tie_extractor.py
///
/// Parses GcUyaDlTieClassHeader -> LOD packet tables -> GS packet data.
/// Extracts dinky/fat vertices with triangle strips, per-texture material groups.

use std::collections::BTreeMap;
use std::path::Path;
use crate::cli::*;
use crate::common::*;

/// Parse GcUyaDlTieClassHeader (0x80 bytes)
struct TieClassHeader {
    packets: [u32; 3],       // offsets for LOD0/1/2
    packet_count: [u32; 3],
    texture_count: u32,
    near_dist: f32,
    mid_dist: f32,
    far_dist: f32,
    scale: f32,              // from field[24] as float
    // LOD stats
    lods: [(u32, u32, u32); 3], // (vert_count, tri_count, strip_count)
}

#[allow(dead_code)]
fn parse_tie_header(data: &[u8], ofs: usize) -> Option<TieClassHeader> {
    if ofs + 0x80 > data.len() { return None; }
    let packets = [
        r_u32(data, ofs) as u32,
        r_u32(data, ofs + 4) as u32,
        r_u32(data, ofs + 8) as u32,
    ];
    let packet_count = [
        r_u8(data, ofs + 12) as u32,
        r_u8(data, ofs + 13) as u32,
        r_u8(data, ofs + 14) as u32,
    ];
    let _texture_count = r_u8(data, ofs + 15) as u32;
    let near_dist = f32::from_le_bytes(data[ofs + 16..ofs + 20].try_into().ok()?);
    let mid_dist = f32::from_le_bytes(data[ofs + 20..ofs + 24].try_into().ok()?);
    let far_dist = f32::from_le_bytes(data[ofs + 24..ofs + 28].try_into().ok()?);
    // Field at +0x60: scale (float)
    let scale = f32::from_le_bytes(data[ofs + 0x60..ofs + 0x64].try_into().ok()?);

    // LOD stats at +0x68: each LOD = 3 × i16 (vert, tri, strip)
    let lods = [
        (r_s16(data, ofs + 0x68) as u32, r_s16(data, ofs + 0x6a) as u32, r_s16(data, ofs + 0x6c) as u32),
        (r_s16(data, ofs + 0x6e) as u32, r_s16(data, ofs + 0x70) as u32, r_s16(data, ofs + 0x72) as u32),
        (r_s16(data, ofs + 0x74) as u32, r_s16(data, ofs + 0x76) as u32, r_s16(data, ofs + 0x78) as u32),
    ];

    Some(TieClassHeader { packets, packet_count, texture_count: 0, near_dist, mid_dist, far_dist, scale, lods })
}

/// Parse a packet header (0x10 bytes)
#[allow(dead_code)]
struct TiePacketHeader {
    data_offset: u32,    // relative to LOD table start
    shader_count: u8,
    vert_offset: u8,     // vertex offset
    vert_size: u8,       // vertex data size (×16)
    rgba_count: u8,
    multipass_ofs: u8,
    scissor_ofs: u8,
    scissor_size: u8,
    multipass_type: u8,
}

fn parse_packet_header(data: &[u8], ofs: usize) -> Option<TiePacketHeader> {
    if ofs + 0x10 > data.len() { return None; }
    Some(TiePacketHeader {
        data_offset: r_u32(data, ofs) as u32,
        shader_count: r_u8(data, ofs + 4),
        vert_offset: r_u8(data, ofs + 8),
        vert_size: r_u8(data, ofs + 9),
        rgba_count: r_u8(data, ofs + 10),
        multipass_ofs: r_u8(data, ofs + 11),
        scissor_ofs: r_u8(data, ofs + 12),
        scissor_size: r_u8(data, ofs + 13),
        multipass_type: r_u8(data, ofs + 14),
    })
}

fn parse_tie_packet(
    data: &[u8],
    lod_table_ofs: usize,
    _class_ofs: usize,
    packet_header: &TiePacketHeader,
    scale: f32,
    tex_slots: &[u8],
) -> BTreeMap<String, (Vec<(f32, f32, f32)>, Vec<(f32, f32)>, Vec<[u32; 3]>)> {
    let pkt_ofs = lod_table_ofs + packet_header.data_offset as usize;
    if pkt_ofs + 0x30 > data.len() {
        return BTreeMap::new();
    }

    let strip_count = r_u8(data, pkt_ofs + 0x23) as usize;

    // Read strip headers starting at +0x2c (4 bytes each)
    struct StripInfo { vertex_count: u32, gif_tag_offset: u32, winding: bool }
    let mut strips = Vec::new();
    for si in 0..strip_count {
        let so = pkt_ofs + 0x2c + si * 4;
        if so + 4 > data.len() { break; }
        strips.push(StripInfo {
            vertex_count: r_u8(data, so) as u32,
            gif_tag_offset: r_u8(data, so + 2) as u32,
            winding: r_u8(data, so + 3) != 0,
        });
    }

    let mut mats: BTreeMap<String, (Vec<(f32, f32, f32)>, Vec<(f32, f32)>, Vec<[u32; 3]>)> = BTreeMap::new();
    let s_factor = scale / 1024.0;

    for (si, s) in strips.iter().enumerate() {
        let vc = s.vertex_count as usize;
        let gif_ofs = s.gif_tag_offset as usize;
        let winding = s.winding;

        // Assign texture
        let tex_idx = if si < tex_slots.len() { tex_slots[si] } else { tex_slots[tex_slots.len() - 1] };
        let mat_name = format!("tie_tex_{}", tex_idx);

        let mut strip_verts: Vec<(f32, f32, f32)> = Vec::new();
        let mut strip_uvs: Vec<(f32, f32)> = Vec::new();

        for n in 0..vc {
            // Each vertex occupies 3 qwords (48 bytes)
            let vqw_ofs = pkt_ofs + (gif_ofs + 1 + n * 3) * 16;
            if vqw_ofs + 48 > data.len() { break; }

            // Position at +0: 3 × i16 (x, y, z)
            let vx = r_s16(data, vqw_ofs) as f32;
            let vy = r_s16(data, vqw_ofs + 2) as f32;
            let vz = r_s16(data, vqw_ofs + 4) as f32;
            let px = vx * s_factor;
            let py = vy * s_factor;
            let pz = vz * s_factor;

            // UV at +16: VU fixed-point 12.4
            let vs = r_u16(data, vqw_ofs + 16);
            let vt = r_u16(data, vqw_ofs + 18);
            let u = r_f32_12(vs);
            let v = r_f32_12(vt);

            strip_verts.push((px, py, pz));
            strip_uvs.push((u, v));
        }

        if strip_verts.len() < 3 { continue; }

        // Generate triangles from strip
        let mut ltris: Vec<[u32; 3]> = Vec::new();
        for i in 2..strip_verts.len() {
            let tri = if i % 2 == (if winding { 0 } else { 1 }) {
                [i as u32 - 2, i as u32 - 1, i as u32]
            } else {
                [i as u32, i as u32 - 1, i as u32 - 2]
            };
            ltris.push(tri);
        }

        let entry = mats.entry(mat_name).or_default();
        let base = entry.0.len() as u32;
        entry.0.extend(strip_verts);
        entry.1.extend(strip_uvs);
        for tri in ltris {
            entry.2.push([tri[0] + base, tri[1] + base, tri[2] + base]);
        }
    }

    mats
}

fn parse_tie_class(
    data: &[u8],
    base_ofs: usize,
    _o_class: i32,
    tex_slots: &[u8],
) -> Option<TieClassData> {
    let hdr = parse_tie_header(data, base_ofs)?;
    if hdr.packet_count[0] == 0 && hdr.packet_count[1] == 0 && hdr.packet_count[2] == 0 {
        return None;
    }

    let tex = if tex_slots.is_empty() { vec![0xff] } else { tex_slots.to_vec() };

    // LOD0 only
    let lod_idx = 0;
    let pc = hdr.packet_count[lod_idx] as usize;
    if pc == 0 { return None; }

    let lod_ofs = base_ofs + hdr.packets[lod_idx] as usize;
    if lod_ofs + pc * 0x10 > data.len() { return None; }

    let mut mats: BTreeMap<String, (Vec<(f32, f32, f32)>, Vec<(f32, f32)>, Vec<[u32; 3]>)> = BTreeMap::new();

    for pi in 0..pc {
        let pk_hdr = parse_packet_header(data, lod_ofs + pi * 0x10)?;
        let pkt_start = base_ofs + hdr.packets[lod_idx] as usize + pk_hdr.data_offset as usize;
        if pkt_start + 0x30 > data.len() { continue; }

        let pkt_mats = parse_tie_packet(data, lod_ofs, base_ofs, &pk_hdr, hdr.scale, &tex);
        for (mat_name, (verts, uvs, tris)) in pkt_mats {
            let entry = mats.entry(mat_name).or_default();
            let base = entry.0.len() as u32;
            entry.0.extend(verts);
            entry.1.extend(uvs);
            for t in tris {
                entry.2.push([t[0] + base, t[1] + base, t[2] + base]);
            }
        }
    }

    Some(TieClassData { mats, hdr: (), scale: hdr.scale, lod: hdr.lods[lod_idx] })
}

struct TieClassData {
    mats: BTreeMap<String, (Vec<(f32, f32, f32)>, Vec<(f32, f32)>, Vec<[u32; 3]>)>,
    hdr: (),
    scale: f32,
    lod: (u32, u32, u32),
}

#[allow(dead_code)]
fn write_obj_mtl(
    obj_path: &Path,
    mtl_path: &Path,
    data: &TieClassData,
    tex_info: &BTreeMap<u32, String>,
    _level_name: &str,
) -> Result<(), String> {
    use std::io::Write;

    if data.mats.is_empty() { return Ok(()); }

    // Build texture PNG names
    let mut mat_png: BTreeMap<String, Option<String>> = BTreeMap::new();
    for mat_name in data.mats.keys() {
        if let Some(rest) = mat_name.strip_prefix("tie_tex_") {
            if let Ok(idx) = rest.parse::<u32>() {
                mat_png.insert(mat_name.clone(), tex_info.get(&idx).cloned());
            } else {
                mat_png.insert(mat_name.clone(), None);
            }
        } else {
            mat_png.insert(mat_name.clone(), None);
        }
    }

    let total_verts: usize = data.mats.values().map(|(v, _, _)| v.len()).sum();
    let total_tris: usize = data.mats.values().map(|(_, _, t)| t.len()).sum();
    let has_tex = mat_png.values().any(|v| v.is_some());

    // Write MTL
    let mut mf = std::fs::File::create(mtl_path).map_err(|e| format!("create mtl: {}", e))?;
    writeln!(mf, "# Tie mesh materials").ok();
    for mat_name in data.mats.keys() {
        let png = mat_png.get(mat_name).and_then(|o| o.as_deref());
        writeln!(mf, "\nnewmtl {}", mat_name).ok();
        writeln!(mf, "Ka 0.8 0.8 0.8").ok();
        writeln!(mf, "Kd 0.8 0.8 0.8").ok();
        if let Some(png_name) = png {
            let rel = format!("../../textures/{}/tie/{}", _level_name, png_name);
            writeln!(mf, "Ks 0.0 0.0 0.0").ok();
            writeln!(mf, "d 1.0").ok();
            writeln!(mf, "illum 1").ok();
            writeln!(mf, "map_Kd {}", rel).ok();
        } else {
            writeln!(mf, "Ks 0.2 0.2 0.2").ok();
            writeln!(mf, "Ns 50.0").ok();
        }
    }

    // Write OBJ
    let mut f = std::fs::File::create(obj_path).map_err(|e| format!("create obj: {}", e))?;
    writeln!(f, "# Tie mesh - {} vertices, {} triangles", total_verts, total_tris).ok();
    writeln!(f, "mtllib {}", mtl_path.file_name().unwrap().to_string_lossy()).ok();
    writeln!(f, "o tie_mesh").ok();

    // Track vertex offset per material
    let mut mat_offsets: BTreeMap<String, u32> = BTreeMap::new();
    let mut running_v = 0u32;
    for mat_name in data.mats.keys() {
        mat_offsets.insert(mat_name.clone(), running_v);
        let (verts, _, _) = data.mats.get(mat_name).unwrap();
        for &(x, y, z) in verts {
            writeln!(f, "v {:.6} {:.6} {:.6}", x, y, z).ok();
        }
        if has_tex {
            for &(u, v) in &data.mats.get(mat_name).unwrap().1 {
                writeln!(f, "vt {:.6} {:.6}", u, v).ok();
            }
        }
        running_v += verts.len() as u32;
    }

    writeln!(f, "s off").ok();

    for mat_name in data.mats.keys() {
        let voff = mat_offsets.get(mat_name).copied().unwrap_or(0);
        let (_, _, tris) = data.mats.get(mat_name).unwrap();
        let has_mat_tex = mat_png.get(mat_name).and_then(|o| o.as_deref()).is_some();
        writeln!(f, "usemtl {}", mat_name).ok();
        for tri in tris {
            if has_mat_tex {
                writeln!(f, "f {}/{}/{} {}/{}/{} {}/{}/{}",
                    voff + tri[0] + 1, voff + tri[0] + 1, voff + tri[0] + 1,
                    voff + tri[1] + 1, voff + tri[1] + 1, voff + tri[1] + 1,
                    voff + tri[2] + 1, voff + tri[2] + 1, voff + tri[2] + 1).ok();
            } else {
                writeln!(f, "f {} {} {}",
                    voff + tri[0] + 1,
                    voff + tri[1] + 1,
                    voff + tri[2] + 1).ok();
            }
        }
    }

    Ok(())
}

pub fn run(scripts_dir: &Path, args: &TieArgs) -> Result<(), String> {
    let unpacked = unpacked_dir(scripts_dir);
    let level_filter = args.level.unwrap_or(-1);
    let target_class = args.class;

    level_dispatch(level_filter, |level_num| {
        process_level(&unpacked, level_num, target_class)
    })
}

fn process_level(base: &Path, level_num: u32, target_class: Option<i32>) -> Result<(), String> {
    let level_dir = base.join(format!("LEVEL{:03}", level_num)).join("data_wad");
    let core_path = level_dir.join("core_data.bin");
    let json_path = level_dir.join("tie_classes.json");

    if !core_path.exists() {
        return Err(format!("core_data.bin not found for LEVEL {:03}", level_num));
    }
    if !json_path.exists() {
        return Err(format!("tie_classes.json not found for LEVEL {:03}", level_num));
    }

    let core_data = std::fs::read(&core_path)
        .map_err(|e| format!("Cannot read core_data: {}", e))?;
    let tie_json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&json_path).map_err(|e| format!("Cannot read json: {}", e))?
    ).map_err(|e| format!("JSON parse error: {}", e))?;

    let entries = tie_json["entries"].as_array().ok_or("No entries in tie_classes.json")?;

    // Load texture info
    let tex_info = load_tie_texture_info(&level_dir);
    let level_name = format!("LEVEL{:03}", level_num);

    println!("=== {}: {} tie class entries ===", level_name, entries.len());

    let scripts_dir = base.parent().and_then(|p| p.parent()).unwrap_or(base);
    let out_dir = meshes_dir(scripts_dir).join(&level_name);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("Cannot create dir: {}", e))?;

    let mut mesh_count = 0u32;
    for entry in entries {
        let wad_off = entry["wad_off"].as_i64().unwrap_or(-1);
        let o_class = entry["o_class"].as_i64().unwrap_or(-1);
        if wad_off <= 0 { continue; }
        if let Some(tc) = target_class {
            if o_class != tc as i64 { continue; }
        }
        if wad_off as usize + 0x80 > core_data.len() { continue; }

        // Parse tex slots from hex field
        let tex_hex = entry["tex"].as_str().unwrap_or("");
        let tex_slots: Vec<u8> = if !tex_hex.is_empty() {
            (0..tex_hex.len()).step_by(2)
                .filter_map(|i| u8::from_str_radix(&tex_hex[i..(i + 2).min(tex_hex.len())], 16).ok())
                .filter(|&b| b != 0xff)
                .collect()
        } else {
            vec![]
        };

        let result = parse_tie_class(&core_data, wad_off as usize, o_class as i32, &tex_slots);
        let result = match result {
            Some(r) => r,
            None => {
                println!("  Class {}: skipped (no LOD packets)", o_class);
                continue;
            }
        };

        if result.mats.is_empty() {
            println!("  Class {}: no vertex data", o_class);
            continue;
        }

        let total_v: usize = result.mats.values().map(|(v, _, _)| v.len()).sum();
        let total_t: usize = result.mats.values().map(|(_, _, t)| t.len()).sum();
        println!("  Class {}: LOD0 ({}v/{}t/{}s) scale={:.4} -> {} verts, {} tris, {} mats",
            o_class, result.lod.0, result.lod.1, result.lod.2, result.scale,
            total_v, total_t, result.mats.len());

        let obj_path = out_dir.join(format!("tie_{}.obj", o_class));
        let mtl_path = out_dir.join(format!("tie_{}.mtl", o_class));
        write_obj_mtl(&obj_path, &mtl_path, &result, &tex_info, &level_name)?;

        mesh_count += 1;
    }

    println!("  => {} tie meshes extracted", mesh_count);
    Ok(())
}

fn load_tie_texture_info(level_dir: &Path) -> BTreeMap<u32, String> {
    let ci_path = level_dir.join("core_index.bin");
    let ch_path = level_dir.join("core_header.json");
    if !ci_path.exists() || !ch_path.exists() {
        return BTreeMap::new();
    }
    let ch: serde_json::Value = match serde_json::from_str(&std::fs::read_to_string(&ch_path).unwrap_or_default()) {
        Ok(v) => v,
        Err(_) => return BTreeMap::new(),
    };
    let tt = &ch["tie_textures"];
    let count = tt["count"].as_i64().unwrap_or(0) as usize;
    let offset = tt["offset"].as_i64().unwrap_or(0) as usize;
    if count == 0 || offset == 0 { return BTreeMap::new(); }
    let idx = match std::fs::read(&ci_path) { Ok(d) => d, Err(_) => return BTreeMap::new() };

    let mut info = BTreeMap::new();
    for i in 0..count {
        let eo = offset + i * 0x10;
        if eo + 0x10 > idx.len() { break; }
        let w = r_u16(&idx, eo + 4) as u32;
        let h = r_u16(&idx, eo + 6) as u32;
        info.insert(i as u32, format!("tie_{:03}_w{}_h{}.png", i, w, h));
    }
    info
}
