/// GS stash geometry (shared class meshes) extraction
/// Ported from rac_stash_extractor.py
///
/// The GS stash contains pre-compiled VIF DMA chain data for moby classes
/// that have wad_off=0 (no standard moby class header).
///
/// Five observed formats:
///   1. Simple (floats at start): Vertex floats at +0x000, UV at +0x0F0, VIF chain
///   2. Early-VIF (VIF at start): UNPACK V3_32 loads vertex data directly into VU
///   3. Complex (display list): 0x4000xxxx header, vertex floats at +0x0C0
///   4. Minimal (tiny header): Small values + early MSCAL call
///   5. ASCII descriptor: Starts with ASCII-like data, MSCAL early

use std::collections::BTreeMap;
use std::path::Path;
use crate::cli::*;
use crate::common::*;

/// ── VIF parsing (inline, matching Python reference) ──

struct VifCode {
    cmd: u8,
    num: u16,
    execaddr: u16,
    unpack: Option<VifUnpackStash>,
}

struct VifUnpackStash {
    vnvl_raw: u8,
    vl: u8,
    vn: u8,
    elem_size: usize,
    qwc: u32,
    data: Vec<u8>,
}

fn read_vif_code(val: u32) -> VifCode {
    let cmd = ((val >> 24) & 0x7f) as u8;
    let raw_num = ((val >> 16) & 0xff) as u16;
    let num = if raw_num == 0 { 256 } else { raw_num };

    let mut result = VifCode { cmd, num, execaddr: (val & 0xffff) as u16, unpack: None };

    if (cmd & 0x60) == 0x60 {
        let vnvl = ((val >> 24) & 0x0f) as u8;
        let vn = ((vnvl >> 2) & 3) + 1;
        let vl_bits: &[u8] = &[32, 16, 8, 5];
        let vl = vl_bits[(vnvl & 3) as usize];
        let elem_size = (vn as u32 * vl as u32 + 7) as usize / 8;
        result.unpack = Some(VifUnpackStash {
            vnvl_raw: vnvl, vl, vn,
            elem_size,
            qwc: 0,
            data: Vec::new(),
        });
    }

    result
}

fn parse_vif_chain(data: &[u8], vif_offset: usize, max_size: usize) -> (Vec<VifCode>, usize) {
    let mut commands = Vec::new();
    let mut ofs = vif_offset;
    let end = (vif_offset + max_size).min(data.len());

    while ofs + 4 <= end {
        let val = r_u32(data, ofs);
        let mut code = read_vif_code(val);
        let cmd = code.cmd;
        let pkt_size: usize;

        if cmd == 0x00 || cmd == 0x01 || cmd == 0x02 || cmd == 0x05 {
            // NOP, STCYCL, STMOD
            pkt_size = 4;
        } else if cmd == 0x14 || cmd == 0x15 {
            // MSCAL, MSCALF
            code.execaddr = (val & 0xffff) as u16;
            pkt_size = 4;
            commands.push(code);
            ofs += pkt_size;
            break;
        } else if cmd == 0x31 {
            // FLUSHA
            pkt_size = 4;
        } else if (cmd & 0x60) == 0x60 {
            // UNPACK
            if let Some(ref mut up) = code.unpack {
                let total = code.num as u32 * up.elem_size as u32;
                let qwc = (total + 15) / 16;
                up.qwc = qwc;
                let data_start = ofs + 4;
                let data_end = (data_start + qwc as usize * 16).min(end);
                up.data = data[data_start..data_end].to_vec();
                pkt_size = 4 + qwc as usize * 16;
            } else {
                pkt_size = 4;
            }
        } else {
            pkt_size = 4;
        }

        if code.unpack.is_some() || !(cmd == 0x14 || cmd == 0x15) {
            commands.push(code);
        }
        ofs += pkt_size;

        if cmd == 0x14 || cmd == 0x15 {
            break;
        }
    }

    (commands, ofs - vif_offset)
}

/// ── Format detection ──

fn detect_format(data: &[u8], file_off: usize) -> &'static str {
    if file_off + 16 > data.len() {
        return "unknown";
    }

    let first_u32 = r_u32(data, file_off);
    let mut first_4 = [0u32; 4];
    for i in 0..4 {
        if file_off + i * 4 + 4 <= data.len() {
            first_4[i] = r_u32(data, file_off + i * 4);
        }
    }

    // Check for immediate VIF UNPACK in first 4 u32s
    for i in 0..4 {
        let cmd = ((first_4[i] >> 24) & 0x7f) as u8;
        if (cmd & 0x60) == 0x60 {
            let vnvl = ((first_4[i] >> 24) & 0x0f) as u8;
            let vl_bits: &[u8] = &[32, 16, 8, 5];
            let vl = vl_bits[(vnvl & 3) as usize];
            let vn = ((vnvl >> 2) & 3) + 1;
            if vl == 32 && vn == 3 {
                return "early_vif";
            }
            if vl >= 8 {
                return "early_vif";
            }
        }
    }

    // Check for simple block: starts with large float values (vertex positions)
    let f0 = f32::from_le_bytes(data[file_off..file_off + 4].try_into().unwrap_or([0u8; 4]));
    let f1 = f32::from_le_bytes(data[file_off + 4..file_off + 8].try_into().unwrap_or([0u8; 4]));
    let f2 = f32::from_le_bytes(data[file_off + 8..file_off + 12].try_into().unwrap_or([0u8; 4]));
    let mag = (f0 * f0 + f1 * f1 + f2 * f2).sqrt();

    if mag > 1000.0 && mag < 10000000.0 {
        let packed = r_u32(data, file_off + 12);
        let nx = packed & 0xFF;
        if packed == 0 || (nx > 0 && nx < 200) {
            return "simple";
        }
    }

    // Check for complex block: 0x4000xxxx pattern
    let patterns = [0x40000000u32, 0x00A00000, 0x10E00000, 0x10DA0000, 0x10D40000,
                    0x11D40000, 0x12D30000, 0x0FDA0000, 0x12D40000];
    if patterns.contains(&(first_u32 & 0xFFFF0000)) {
        return "complex";
    }

    // Check for ASCII descriptor
    let first_bytes = &data[file_off..(file_off + 4).min(data.len())];
    if first_bytes.len() >= 4 {
        let all_printable = first_bytes.iter().all(|&b| b >= 0x20 && b < 0x7f);
        let lk_prefix = first_bytes.len() >= 2 && first_bytes[0] == b'L' && first_bytes[1] == b'K';
        if all_printable || lk_prefix {
            return "ascii";
        }
    }

    // Check for MSCAL/MSCALF in first 64 bytes (minimal)
    for s in (0..64).step_by(4) {
        if file_off + s + 4 <= data.len() {
            let val = r_u32(data, file_off + s);
            let cmd = ((val >> 24) & 0x7f) as u8;
            if cmd == 0x14 || cmd == 0x15 {
                return "minimal";
            }
        }
    }

    // Check for 0x30000000, 0x45000000, 0x00000000 header
    if first_u32 == 0x30000000 || first_u32 == 0x45000000 || first_u32 == 0x00000000 {
        return "minimal";
    }

    // Check for 0x18000000 pattern
    if first_u32 == 0x18000000 || (first_u32 & 0xFF000000) == 0x18000000 {
        return "minimal";
    }

    "unknown"
}

/// ── Vertex extraction helpers ──

#[derive(Clone, Debug)]
struct StashVert {
    pos: (f32, f32, f32),
    normal: (f32, f32, f32),
}

fn extract_vertices_from_v3_32(_data: &[u8], num: u16, unpack_data: &[u8]) -> Vec<StashVert> {
    let mut vertices = Vec::new();
    let count = num as usize;
    for i in 0..count {
        let off = i * 12;
        if off + 12 > unpack_data.len() {
            break;
        }
        let x = f32::from_le_bytes(unpack_data[off..off + 4].try_into().unwrap());
        let y = f32::from_le_bytes(unpack_data[off + 4..off + 8].try_into().unwrap());
        let z = f32::from_le_bytes(unpack_data[off + 8..off + 12].try_into().unwrap());
        vertices.push(StashVert { pos: (x, y, z), normal: (0.0, 0.0, 0.0) });
    }
    vertices
}

fn extract_vertices_from_v4_32(_data: &[u8], num: u16, unpack_data: &[u8]) -> Vec<StashVert> {
    let mut vertices = Vec::new();
    let count = num as usize;
    for i in 0..count {
        let off = i * 16;
        if off + 16 > unpack_data.len() {
            break;
        }
        let x = f32::from_le_bytes(unpack_data[off..off + 4].try_into().unwrap());
        let y = f32::from_le_bytes(unpack_data[off + 4..off + 8].try_into().unwrap());
        let z = f32::from_le_bytes(unpack_data[off + 8..off + 12].try_into().unwrap());
        let packed = r_u32(unpack_data, off + 12);
        let nx = ((packed & 0xFF) as i32 - if (packed & 0x80) != 0 { 256 } else { 0 }) as f32 / 127.0;
        let ny = (((packed >> 8) & 0xFF) as i32 - if ((packed >> 8) & 0x80) != 0 { 256 } else { 0 }) as f32 / 127.0;
        let nz = (((packed >> 16) & 0xFF) as i32 - if ((packed >> 16) & 0x80) != 0 { 256 } else { 0 }) as f32 / 127.0;
        vertices.push(StashVert { pos: (x, y, z), normal: (nx, ny, nz) });
    }
    vertices
}

fn extract_vertices_from_simple_floats(data: &[u8], file_off: usize) -> Vec<StashVert> {
    let mut vertices = Vec::new();
    let max_count = 30;

    for i in 0..max_count {
        let pos = file_off + i * 16;
        if pos + 16 > data.len() {
            break;
        }
        let f4: [f32; 4] = [
            f32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()),
            f32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()),
            f32::from_le_bytes(data[pos + 8..pos + 12].try_into().unwrap()),
            f32::from_le_bytes(data[pos + 12..pos + 16].try_into().unwrap()),
        ];
        let packed = r_u32(data, pos + 12);
        let mag = (f4[0] * f4[0] + f4[1] * f4[1] + f4[2] * f4[2]).sqrt();

        if mag < 10.0 {
            if i >= 15 && mag < 0.1 {
                break;
            }
            if i >= 3 {
                let mut all_zero = true;
                for j in i..(i + 5).min(max_count) {
                    let jpos = file_off + j * 16;
                    if jpos + 16 <= data.len() {
                        let jf0 = f32::from_le_bytes(data[jpos..jpos + 4].try_into().unwrap());
                        let jf1 = f32::from_le_bytes(data[jpos + 4..jpos + 8].try_into().unwrap());
                        let jf2 = f32::from_le_bytes(data[jpos + 8..jpos + 12].try_into().unwrap());
                        if jf0.abs() > 0.1 || jf1.abs() > 0.1 || jf2.abs() > 0.1 {
                            all_zero = false;
                            break;
                        }
                    }
                }
                if all_zero {
                    break;
                }
            }
        }

        if mag >= 50.0 {
            let nx = ((packed & 0xFF) as i32 - if (packed & 0x80) != 0 { 256 } else { 0 }) as f32 / 127.0;
            let ny = (((packed >> 8) & 0xFF) as i32 - if ((packed >> 8) & 0x80) != 0 { 256 } else { 0 }) as f32 / 127.0;
            let nz = (((packed >> 16) & 0xFF) as i32 - if ((packed >> 16) & 0x80) != 0 { 256 } else { 0 }) as f32 / 127.0;
            vertices.push(StashVert { pos: (f4[0], f4[1], f4[2]), normal: (nx, ny, nz) });
        } else if mag > 1.0 {
            vertices.push(StashVert { pos: (f4[0], f4[1], f4[2]), normal: (0.0, 0.0, 0.0) });
        } else {
            break;
        }
    }

    vertices
}

fn extract_uvs(data: &[u8], uv_off: usize) -> Vec<(f32, f32)> {
    let mut uvs = Vec::new();
    let max_count = 32;
    for i in 0..max_count {
        let pos = uv_off + i * 4;
        if pos + 4 > data.len() {
            break;
        }
        let s_raw = r_s16(data, pos);
        let t_raw = r_s16(data, pos + 2);
        if s_raw == 0 && t_raw == 0 && i >= 16 {
            let mut remaining_zero = true;
            for j in i..(i + 4).min(max_count) {
                let jpos = uv_off + j * 4;
                if jpos + 4 <= data.len() {
                    let js = r_s16(data, jpos);
                    let jt = r_s16(data, jpos + 2);
                    if js != 0 || jt != 0 {
                        remaining_zero = false;
                        break;
                    }
                }
            }
            if remaining_zero {
                break;
            }
        }
        uvs.push((s_raw as f32 / 4096.0, t_raw as f32 / 4096.0));
    }
    uvs
}

fn extract_uvs_from_v2_16(raw_data: &[u8]) -> Vec<(f32, f32)> {
    let mut uvs = Vec::new();
    let count = raw_data.len() / 4;
    for i in 0..count {
        let off = i * 4;
        if off + 4 <= raw_data.len() {
            let s_raw = r_s16(raw_data, off);
            let t_raw = r_s16(raw_data, off + 2);
            uvs.push((s_raw as f32 / 4096.0, t_raw as f32 / 4096.0));
        }
    }
    uvs
}

fn scan_vertex_floats(data: &[u8], base_off: usize, max_size: usize) -> Vec<StashVert> {
    let mut vertices = Vec::new();
    let end = (base_off + max_size).min(data.len());
    let mut i = 0;

    while base_off + i + 16 <= end {
        let off = base_off + i;
        let f4: [f32; 4] = [
            f32::from_le_bytes(data[off..off + 4].try_into().unwrap()),
            f32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap()),
            f32::from_le_bytes(data[off + 8..off + 12].try_into().unwrap()),
            f32::from_le_bytes(data[off + 12..off + 16].try_into().unwrap()),
        ];
        let packed = r_u32(data, off + 12);
        let mag = (f4[0] * f4[0] + f4[1] * f4[1] + f4[2] * f4[2]).sqrt();

        if mag > 1000.0 && mag < 10000000.0 {
            let nx = ((packed & 0xFF) as i32 - if (packed & 0x80) != 0 { 256 } else { 0 }) as f32 / 127.0;
            let ny = (((packed >> 8) & 0xFF) as i32 - if ((packed >> 8) & 0x80) != 0 { 256 } else { 0 }) as f32 / 127.0;
            let nz = (((packed >> 16) & 0xFF) as i32 - if ((packed >> 16) & 0x80) != 0 { 256 } else { 0 }) as f32 / 127.0;
            vertices.push(StashVert { pos: (f4[0], f4[1], f4[2]), normal: (nx, ny, nz) });
            i += 16;
        } else {
            i += 4;
        }
    }

    vertices
}

/// ── Stash entry extraction ──

struct ExtractedEntry {
    format: String,
    vertices: Vec<StashVert>,
    uvs: Vec<(f32, f32)>,
    vif_commands: Vec<VifCode>,
    indices_raw: Vec<u8>,
    texture_data: Vec<u8>,
}

fn extract_stash_entry(data: &[u8], file_off: usize, sz_hint: usize) -> ExtractedEntry {
    let fmt = detect_format(data, file_off);
    let mut result = ExtractedEntry {
        format: fmt.to_string(),
        vertices: Vec::new(),
        uvs: Vec::new(),
        vif_commands: Vec::new(),
        indices_raw: Vec::new(),
        texture_data: Vec::new(),
    };

    let max_size = if sz_hint > 0 {
        (sz_hint.max(0x200)).min(0x800)
    } else {
        0x400
    };

    match fmt {
        "early_vif" => {
            let (vif_cmds, _) = parse_vif_chain(data, file_off, max_size);
            result.vif_commands = vif_cmds;

            for cmd in &result.vif_commands {
                if let Some(ref up) = cmd.unpack {
                    let name = format!("UNPACK_V{}_{}", up.vn, up.vl);
                    if name == "UNPACK_V3_32" {
                        let verts = extract_vertices_from_v3_32(data, cmd.num, &up.data);
                        result.vertices.extend(verts);
                    } else if name == "UNPACK_V4_32" {
                        let verts = extract_vertices_from_v4_32(data, cmd.num, &up.data);
                        result.vertices.extend(verts);
                    } else if name == "UNPACK_V4_8" {
                        result.indices_raw.extend_from_slice(&up.data);
                    } else if name == "UNPACK_V2_16" {
                        let uvs = extract_uvs_from_v2_16(&up.data);
                        result.uvs = uvs;
                    }
                }
            }
        }

        "simple" => {
            result.vertices = extract_vertices_from_simple_floats(data, file_off);

            if !result.vertices.is_empty() {
                let vif_off = file_off + 0x130;
                let uv_off = file_off + 0xF0;
                result.uvs = extract_uvs(data, uv_off);

                let (vif_cmds, _) = parse_vif_chain(data, vif_off, max_size);
                result.vif_commands = vif_cmds;

                for cmd in &result.vif_commands {
                    if let Some(ref up) = cmd.unpack {
                        let name = format!("UNPACK_V{}_{}", up.vn, up.vl);
                        if name == "UNPACK_V4_8" {
                            result.indices_raw.extend_from_slice(&up.data);
                        } else if name == "UNPACK_V4_32" {
                            result.texture_data.extend_from_slice(&up.data);
                        }
                    }
                }
            }
        }

        "complex" => {
            let vert_off = file_off + 0xC0;
            result.vertices = extract_vertices_from_simple_floats(data, vert_off);

            if !result.vertices.is_empty() {
                let vif_off = if file_off + 0x218 <= data.len() {
                    file_off + 0x218
                } else {
                    vert_off + result.vertices.len() * 16
                };
                let (vif_cmds, _) = parse_vif_chain(data, vif_off, max_size);
                result.vif_commands = vif_cmds;

                for cmd in &result.vif_commands {
                    if let Some(ref up) = cmd.unpack {
                        let name = format!("UNPACK_V{}_{}", up.vn, up.vl);
                        if name == "UNPACK_V4_8" {
                            result.indices_raw.extend_from_slice(&up.data);
                        }
                    }
                }
            }
        }

        "minimal" | "ascii" => {
            let scan_max = if sz_hint > 0 { sz_hint.max(0x200).min(0x200) } else { 0x200 };
            let mut vif_start = file_off;
            let mut found_vif = false;

            for s in (0..scan_max).step_by(4) {
                if file_off + s + 4 <= data.len() {
                    let val = r_u32(data, file_off + s);
                    let cmd = ((val >> 24) & 0x7f) as u8;
                    if (cmd & 0x60) == 0x60 || cmd == 0x14 || cmd == 0x15 {
                        vif_start = file_off + s;
                        found_vif = true;
                        break;
                    }
                }
            }

            if found_vif {
                let msize = if sz_hint > 0 { sz_hint.max(0x200).min(0x800) } else { 0x400 };
                let (vif_cmds, _) = parse_vif_chain(data, vif_start, msize);
                result.vif_commands = vif_cmds;

                for cmd in &result.vif_commands {
                    if let Some(ref up) = cmd.unpack {
                        let name = format!("UNPACK_V{}_{}", up.vn, up.vl);
                        if name == "UNPACK_V3_32" {
                            let verts = extract_vertices_from_v3_32(data, cmd.num, &up.data);
                            result.vertices.extend(verts);
                        } else if name == "UNPACK_V4_32" {
                            let verts = extract_vertices_from_v4_32(data, cmd.num, &up.data);
                            result.vertices.extend(verts);
                        } else if name == "UNPACK_V4_8" {
                            result.indices_raw.extend_from_slice(&up.data);
                        } else if name == "UNPACK_V2_16" {
                            let uvs = extract_uvs_from_v2_16(&up.data);
                            result.uvs = uvs;
                        }
                    }
                }
            }
        }

        _ => {
            // Unknown format - scan for vertex floats
            let scan_size = if sz_hint > 0 { sz_hint.max(0x200).min(0x200) } else { 0x200 };
            result.vertices = scan_vertex_floats(data, file_off, scan_size);
        }
    }

    result
}

/// ── Mesh building ──

struct StashMesh {
    vertices: Vec<(f32, f32, f32)>,
    normals: Vec<(f32, f32, f32)>,
    texcoords: Vec<(f32, f32)>,
    indices: Vec<u32>,
}

fn build_mesh(vertices: &[StashVert], uvs: &[(f32, f32)], indices_raw: &[u8]) -> Option<StashMesh> {
    if vertices.is_empty() {
        return None;
    }

    // Per-vertex UVs
    let vert_uvs: Vec<(f32, f32)> = (0..vertices.len()).map(|i| {
        if i < uvs.len() { uvs[i] } else { (0.0, 0.0) }
    }).collect();

    let mut triangles: Vec<(usize, usize, usize)> = Vec::new();

    if !indices_raw.is_empty() {
        // Process as unsigned byte indices with 0 = strip break
        let mut strip: Vec<usize> = Vec::new();

        for &byte_val in indices_raw {
            if byte_val == 0 {
                strip.clear();
                continue;
            }
            let mut vi = (byte_val as usize).wrapping_sub(1);
            if vi >= vertices.len() {
                vi = byte_val as usize;
                if vi >= vertices.len() {
                    continue;
                }
            }
            strip.push(vi);

            if strip.len() >= 3 {
                let k = strip.len() - 1;
                let tri = if k % 2 == 0 {
                    (strip[k - 2], strip[k - 1], strip[k])
                } else {
                    (strip[k - 1], strip[k - 2], strip[k])
                };

                // Check degenerate
                let p0 = vertices[tri.0].pos;
                let p1 = vertices[tri.1].pos;
                let p2 = vertices[tri.2].pos;
                if p0 == p1 || p1 == p2 || p0 == p2 {
                    continue;
                }

                triangles.push(tri);
            }
        }
    } else {
        // Sequential triangle strip
        for i in 0..vertices.len().saturating_sub(2) {
            let k = triangles.len();
            let tri = if k % 2 == 0 {
                (i, i + 1, i + 2)
            } else {
                (i + 1, i, i + 2)
            };

            let p0 = vertices[tri.0].pos;
            let p1 = vertices[tri.1].pos;
            let p2 = vertices[tri.2].pos;
            if p0 == p1 || p1 == p2 || p0 == p2 {
                continue;
            }

            triangles.push(tri);
        }
    }

    if triangles.is_empty() {
        return None;
    }

    // Build vertex map (dedup)
    let mut vert_mapping: BTreeMap<usize, u32> = BTreeMap::new();
    let mut all_verts: Vec<(f32, f32, f32)> = Vec::new();
    let mut all_norms: Vec<(f32, f32, f32)> = Vec::new();
    let mut all_uvs: Vec<(f32, f32)> = Vec::new();
    let mut all_idx: Vec<u32> = Vec::new();

    for tri in &triangles {
        for &vi in &[tri.0, tri.1, tri.2] {
            if !vert_mapping.contains_key(&vi) {
                let v = &vertices[vi];
                let u = if vi < vert_uvs.len() { vert_uvs[vi] } else { (0.0, 0.0) };
                all_verts.push(v.pos);
                all_norms.push(v.normal);
                all_uvs.push(u);
                vert_mapping.insert(vi, all_verts.len() as u32 - 1);
            }
            let gvi = vert_mapping[&vi];
            all_idx.push(gvi);
        }
    }

    Some(StashMesh { vertices: all_verts, normals: all_norms, texcoords: all_uvs, indices: all_idx })
}

/// ── OBJ/PointCloud output ──

fn write_obj(obj_path: &Path, mesh: &StashMesh, mtl_name: &str) -> bool {
    use std::io::Write;
    if mesh.vertices.is_empty() || mesh.indices.is_empty() {
        return false;
    }

    let f = match std::fs::File::create(obj_path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut f = std::io::BufWriter::new(f);

    writeln!(f, "# Stash-extracted mesh").ok();
    writeln!(f, "o stash_mesh").ok();

    for &(x, y, z) in &mesh.vertices {
        writeln!(f, "v {:.6} {:.6} {:.6}", x, y, z).ok();
    }
    for &(u, v) in &mesh.texcoords {
        writeln!(f, "vt {:.6} {:.6}", u, v).ok();
    }
    for &(x, y, z) in &mesh.normals {
        writeln!(f, "vn {:.6} {:.6} {:.6}", x, y, z).ok();
    }

    writeln!(f, "s off").ok();
    writeln!(f, "usemtl {}", mtl_name).ok();

    for i in (0..mesh.indices.len()).step_by(3) {
        if i + 2 < mesh.indices.len() {
            let v0 = mesh.indices[i] + 1;
            let v1 = mesh.indices[i + 1] + 1;
            let v2 = mesh.indices[i + 2] + 1;
            writeln!(f, "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}", v0, v1, v2).ok();
        }
    }

    true
}

fn write_pointcloud_obj(obj_path: &Path, vertices: &[StashVert], mtl_name: &str) -> bool {
    use std::io::Write;
    if vertices.is_empty() {
        return false;
    }

    let f = match std::fs::File::create(obj_path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut f = std::io::BufWriter::new(f);

    writeln!(f, "# Stash point cloud - {} vertices", vertices.len()).ok();
    writeln!(f, "o stash_points").ok();

    for v in vertices {
        writeln!(f, "v {:.6} {:.6} {:.6}", v.pos.0, v.pos.1, v.pos.2).ok();
        writeln!(f, "vn {:.6} {:.6} {:.6}", v.normal.0, v.normal.1, v.normal.2).ok();
    }

    writeln!(f, "s off").ok();
    writeln!(f, "usemtl {}", mtl_name).ok();
    write!(f, "p").ok();
    for i in 0..vertices.len() {
        if i > 0 && i % 20 == 0 {
            writeln!(f, "\np").ok();
        }
        write!(f, " {}", i + 1).ok();
    }
    writeln!(f).ok();

    true
}

/// ── Stash entry layout detection ──

#[derive(Clone, Debug)]
struct StashLayout {
    sentinel_pos: i32,
    ee_pos: usize,
    mask: u32,
    base: u32,
    valid: usize,
    sentinel_count: usize,
    total_sampled: usize,
}

fn detect_stash_entry_layout(core_data: &[u8], stash_list_off: usize, stash_count: u32) -> StashLayout {
    let max_sample = stash_count.min(50) as usize;

    // Detect sentinel position
    let mut sentinel_dist: BTreeMap<usize, usize> = BTreeMap::new();

    for i in 0..max_sample {
        let off = stash_list_off + i * 64;
        if off + 64 > core_data.len() {
            break;
        }
        for p in 0..16 {
            let val = r_u32(core_data, off + p * 4);
            if val == 0xffff00ff {
                *sentinel_dist.entry(p).or_insert(0) += 1;
                break;
            }
        }
    }

    let sentinel_pos = sentinel_dist.iter().max_by_key(|&(_, &c)| c).map(|(&p, _)| p as i32).unwrap_or(-1);
    let sentinel_count = sentinel_dist.get(&(sentinel_pos as usize)).copied().unwrap_or(0);

    // Determine best mask/base combo
    let mut best_score = 0;
    let mut best_config = StashLayout {
        sentinel_pos,
        ee_pos: 1,
        mask: 0x01FFFFFF,
        base: 0x00100000,
        valid: 0,
        sentinel_count,
        total_sampled: max_sample,
    };

    let mut ee_candidates: Vec<usize> = Vec::new();
    if sentinel_pos >= 0 {
        ee_candidates.push(sentinel_pos as usize + 1);
    }
    ee_candidates.push(1);

    if sentinel_pos >= 0 {
        let mut extra = vec![1, sentinel_pos as usize + 1];
        if sentinel_pos as i32 - 1 >= 0 { extra.push(sentinel_pos as usize - 1); }
        extra.push(4);
        extra.push(5);
        for &c in &extra {
            if c < 16 && !ee_candidates.contains(&c) {
                ee_candidates.push(c);
            }
        }
    } else {
        for &c in &[1, 4, 5, 2, 3] {
            if c < 16 && !ee_candidates.contains(&c) {
                ee_candidates.push(c);
            }
        }
    }

    for &ee_pos in &ee_candidates {
        for &mask in &[0x01FFFFFFu32, 0x00FFFFFF] {
            for &base in &[0x00100000u32, 0x00000000] {
                let mut valid = 0usize;
                for i in 0..max_sample {
                    let off = stash_list_off + i * 64 + ee_pos * 4;
                    if off + 4 > core_data.len() {
                        break;
                    }
                    let val = r_u32(core_data, off);
                    if val == 0 {
                        continue;
                    }
                    let foff = (val & mask).wrapping_sub(base) as usize;
                    if foff < core_data.len() {
                        valid += 1;
                    }
                }

                let mut score = valid;
                if sentinel_pos >= 0 && ee_pos == sentinel_pos as usize + 1 {
                    score += 10;
                }
                if score > best_score {
                    best_score = score;
                    best_config = StashLayout {
                        sentinel_pos, ee_pos, mask, base, valid,
                        sentinel_count, total_sampled: max_sample,
                    };
                }
            }
        }
    }

    best_config
}

/// ── Resolve stash entry ──

struct ResolvedEntry {
    ee_addr: u32,
    file_off: usize,
    valid: bool,
    flags: u32,
    sz_hint: u32,
}

fn resolve_stash_entry(core_data: &[u8], entry_off: usize, layout: &StashLayout) -> ResolvedEntry {
    if entry_off + 16 > core_data.len() {
        return ResolvedEntry { ee_addr: 0, file_off: 0, valid: false, flags: 0, sz_hint: 0 };
    }

    let ee_addr = r_u32(core_data, entry_off + layout.ee_pos * 4);
    let file_off = (ee_addr & layout.mask).wrapping_sub(layout.base) as usize;

    // Extract flags from sentinel position
    let flags = if layout.sentinel_pos >= 0 && (layout.sentinel_pos as usize + 1) < 16 {
        r_u32(core_data, entry_off + (layout.sentinel_pos as usize + 1) * 4)
    } else {
        0
    };

    // sz_hint from u32[7]
    let sz_hint = if entry_off + 32 <= core_data.len() {
        r_u32(core_data, entry_off + 7 * 4)
    } else {
        0
    };

    let valid = file_off < core_data.len();

    ResolvedEntry { ee_addr, file_off, valid, flags, sz_hint }
}

/// ── Find stash-to-class mapping ──

fn find_stash_to_class_mapping(level_dir: &Path) -> BTreeMap<usize, Vec<i32>> {
    let moby_json = level_dir.join("moby_classes.json");
    if !moby_json.exists() {
        return BTreeMap::new();
    }

    let content = match std::fs::read_to_string(&moby_json) {
        Ok(c) => c,
        Err(_) => return BTreeMap::new(),
    };
    let mc: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return BTreeMap::new(),
    };

    let entries = match mc["entries"].as_array() {
        Some(e) => e,
        None => return BTreeMap::new(),
    };

    let missing: Vec<i32> = entries.iter()
        .filter(|e| {
            let wad_off = e["wad_off"].as_i64().unwrap_or(-1);
            wad_off <= 0
        })
        .filter_map(|e| e["o_class"].as_i64().map(|v| v as i32))
        .collect();

    let mut mapping = BTreeMap::new();
    for (i, &cls) in missing.iter().enumerate() {
        mapping.insert(i, vec![cls]);
    }
    mapping
}

/// ── Level processing ──

fn process_level(base: &Path, level_num: u32) -> Result<(), String> {
    let level_dir = base.join(format!("LEVEL{:03}", level_num)).join("data_wad");
    let core_path = level_dir.join("core_data.bin");
    let json_path = level_dir.join("core_header.json");

    if !core_path.exists() {
        return Err(format!("core_data not found for LEVEL{:03}", level_num));
    }
    if !json_path.exists() {
        println!("  SKIP: LEVEL{:03} - no core_header.json", level_num);
        return Ok(());
    }

    let core_data = std::fs::read(&core_path).map_err(|e| format!("read core_data: {}", e))?;
    let core_header: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&json_path).map_err(|e| format!("read json: {}", e))?
    ).map_err(|e| format!("parse json: {}", e))?;

    let stash_count = core_header.get("moby_gs_stash_count").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
    let stash_list_off = core_header.get("moby_gs_stash_list").and_then(|v| v.as_i64()).unwrap_or(0) as usize;

    if stash_count == 0 || stash_list_off == 0 {
        println!("  SKIP: LEVEL{:03} - no GS stash data", level_num);
        return Ok(());
    }

    println!("=== LEVEL{:03}: {} GS stash entries ===", level_num, stash_count);
    println!("  Stash list at core offset 0x{:x}", stash_list_off);

    let level_name = format!("LEVEL{:03}", level_num);
    let scripts_dir = base.parent().and_then(|p| p.parent()).unwrap_or(base);
    let out_dir = crate::common::meshes_dir(scripts_dir).join(&level_name);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir: {}", e))?;

    let mut extracted_count = 0u32;
    let mut mesh_count = 0u32;
    let mut pc_count = 0u32;
    let mut skipped_count = 0u32;

    let class_mapping = find_stash_to_class_mapping(&level_dir);

    // Auto-detect stash entry layout
    let layout = detect_stash_entry_layout(&core_data, stash_list_off, stash_count);
    println!("  Layout: sentinel@{} ee_pos={} mask=0x{:08x} base=0x{:08x} ({}/{}) valid",
        layout.sentinel_pos, layout.ee_pos, layout.mask, layout.base,
        layout.valid, layout.total_sampled);

    let use_inline_fallback = layout.sentinel_pos < 0 && (layout.valid as f32) < (layout.total_sampled as f32) * 0.7;

    for i in 0..stash_count as usize {
        let entry_off = stash_list_off + i * 64;
        if entry_off + 16 > core_data.len() {
            break;
        }

        let resolved = resolve_stash_entry(&core_data, entry_off, &layout);
        let ee_addr = resolved.ee_addr;
        let mut file_off = resolved.file_off;
        let mut sz_hint = resolved.sz_hint as usize;

        if ee_addr == 0 {
            skipped_count += 1;
            continue;
        }

        if !resolved.valid {
            // Try inline data from stash entry fields
            let inline_floats: [f32; 3] = [
                f32::from_le_bytes(core_data[entry_off..entry_off + 4].try_into().unwrap_or([0u8; 4])),
                f32::from_le_bytes(core_data[entry_off + 4..entry_off + 8].try_into().unwrap_or([0u8; 4])),
                f32::from_le_bytes(core_data[entry_off + 8..entry_off + 12].try_into().unwrap_or([0u8; 4])),
            ];
            let inline_mag = (inline_floats[0] * inline_floats[0] + inline_floats[1] * inline_floats[1] + inline_floats[2] * inline_floats[2]).sqrt();

            if inline_mag > 1000.0 && inline_mag < 10000000.0 && use_inline_fallback {
                file_off = entry_off;
                sz_hint = 64;
            } else if use_inline_fallback {
                let data_off_candidate = if entry_off + 16 <= core_data.len() {
                    r_u32(&core_data, entry_off + 16) as usize
                } else {
                    0
                };
                if data_off_candidate > 0 && data_off_candidate < core_data.len() {
                    file_off = data_off_candidate;
                    sz_hint = if entry_off + 24 <= core_data.len() {
                        r_u32(&core_data, entry_off + 20) as usize
                    } else {
                        0
                    };
                } else {
                    println!("  [{:3}] EE=0x{:08x}: invalid file offset 0x{:x}", i, ee_addr, file_off);
                    continue;
                }
            } else {
                println!("  [{:3}] EE=0x{:08x}: invalid file offset 0x{:x}", i, ee_addr, file_off);
                continue;
            }
        }

        if file_off >= core_data.len() || file_off + 16 > core_data.len() {
            println!("  [{:3}] EE=0x{:08x}: invalid file offset 0x{:x}", i, ee_addr, file_off);
            continue;
        }

        let fmt = detect_format(&core_data, file_off);

        // Extract data
        let entry_data = extract_stash_entry(&core_data, file_off, sz_hint);
        let vertices = &entry_data.vertices;
        let uvs = &entry_data.uvs;
        let vif_commands = &entry_data.vif_commands;
        let indices_raw = &entry_data.indices_raw;

        // Determine class label
        let possible_classes = class_mapping.get(&i);
        let class_label = if let Some(classes) = possible_classes {
            classes.iter().take(3).map(|c| c.to_string()).collect::<Vec<_>>().join("_")
        } else {
            format!("stash_{}", i)
        };

        if vertices.is_empty() {
            println!("  [{:3}] {:10} EE=0x{:08x} off=0x{:x} sz=0x{:x}: no vertices",
                i, fmt, ee_addr, file_off, sz_hint);
            skipped_count += 1;
            continue;
        }

        // Build mesh
        let mesh = build_mesh(vertices, uvs, indices_raw);

        // Count VIF UNPACKs
        let vif_unpack_count = vif_commands.iter().filter(|c| c.unpack.is_some()).count();
        let vif_desc = if !vif_commands.is_empty() {
            let last = &vif_commands[vif_commands.len() - 1];
            let last_name = if let Some(ref up) = last.unpack {
                format!("UNPACK_V{}_{}", up.vn, up.vl)
            } else if last.cmd == 0x14 {
                "MSCAL".to_string()
            } else if last.cmd == 0x15 {
                "MSCALF".to_string()
            } else {
                format!("CMD_{:02x}", last.cmd)
            };
            format!(" VIF:{} last={}@{:04x}", vif_unpack_count, last_name, last.execaddr)
        } else {
            String::new()
        };

        if let Some(ref m) = mesh {
            if !m.vertices.is_empty() && !m.indices.is_empty() {
                let obj_name = format!("stash_{}_{}", i, class_label);
                let obj_path = out_dir.join(format!("{}.obj", obj_name));
                if write_obj(&obj_path, m, &obj_name) {
                    println!("  [{:3}] {:10} EE=0x{:08x} off=0x{:x}: {}v/{}uv → MESH ({} objv, {} idx){}",
                        i, fmt, ee_addr, file_off, vertices.len(), uvs.len(),
                        m.vertices.len(), m.indices.len(), vif_desc);
                    mesh_count += 1;
                    extracted_count += 1;
                } else {
                    let pc_path = out_dir.join(format!("stash_{}_{}_pc.obj", i, class_label));
                    if write_pointcloud_obj(&pc_path, vertices, &obj_name) {
                        println!("  [{:3}] {:10} EE=0x{:08x} off=0x{:x}: {}v/{}uv → PC (mesh write failed){}",
                            i, fmt, ee_addr, file_off, vertices.len(), uvs.len(), vif_desc);
                        pc_count += 1;
                        extracted_count += 1;
                    }
                }
            } else {
                let pc_name = format!("stash_{}_{}_pc", i, class_label);
                let pc_path = out_dir.join(format!("{}.obj", pc_name));
                if write_pointcloud_obj(&pc_path, vertices, &pc_name) {
                    println!("  [{:3}] {:10} EE=0x{:08x} off=0x{:x}: {}v/{}uv → PC (no indices){}",
                        i, fmt, ee_addr, file_off, vertices.len(), uvs.len(), vif_desc);
                    pc_count += 1;
                    extracted_count += 1;
                }
            }
        } else {
            let pc_name = format!("stash_{}_{}_pc", i, class_label);
            let pc_path = out_dir.join(format!("{}.obj", pc_name));
            if write_pointcloud_obj(&pc_path, vertices, &pc_name) {
                println!("  [{:3}] {:10} EE=0x{:08x} off=0x{:x}: {}v/{}uv → PC (no indices){}",
                    i, fmt, ee_addr, file_off, vertices.len(), uvs.len(), vif_desc);
                pc_count += 1;
                extracted_count += 1;
            }
        }
    }

    println!("  => Extracted: {}/{} (mesh:{} pc:{} skipped:{})",
        extracted_count, stash_count, mesh_count, pc_count, skipped_count);
    Ok(())
}

pub fn run(scripts_dir: &Path, args: &StashArgs) -> Result<(), String> {
    let unpacked = crate::common::unpacked_dir(scripts_dir);
    let level_filter = args.level.unwrap_or(-1);

    crate::common::level_dispatch(level_filter, |level_num| {
        process_level(&unpacked, level_num)
    })
}
