/// Tfrag (terrain fragment) mesh extraction
/// Ported from rac_tfrag_extractor.py
///
/// Parses TfragsHeader -> table of TfragHeaders -> VIF command lists per LOD -> vertex data.
/// Based on Wrench source (chaoticgd/wrench): tfrag_low.h, tfrag_low.cpp, tfrag_high.cpp.
///
/// Tfrag is the only extractor that doesn't use a JSON class file;
/// it reads the header directly from core_data at core_header.json["tfrags"].

use std::collections::BTreeMap;
use std::path::Path;
use crate::cli::*;
use crate::common::*;

/// Convert u16 to VU fixed-point 12.4 float (signed, divide by 4096)
fn vu_fixed12_from_u16(val: u16) -> f32 {
    let signed = if val < 0x8000 { val as i32 } else { val as i32 - 0x10000 };
    signed as f32 / 4096.0
}

/// Convert u8 byte to signed byte
fn to_s8(val: u8) -> i8 {
    val as i8
}

/// Parse a TfragHeader (0x40 bytes)
#[allow(dead_code)]
struct TfragHeader {
    bsphere: [f32; 4],
    data: u32,
    lod_2_ofs: u16,
    shared_ofs: u16,
    lod_1_ofs: u16,
    lod_0_ofs: u16,
    tex_ofs: u16,
    rgba_ofs: u16,
    common_size: u8,
    lod_2_size: u8,
    lod_1_size: u8,
    lod_0_size: u8,
    lod_2_rgba_count: u8,
    lod_1_rgba_count: u8,
    lod_0_rgba_count: u8,
    base_only: u8,
    texture_count: u8,
    rgba_size: u8,
    rgba_verts_loc: u8,
    occl_index_stash: u8,
    msphere_count: u8,
    flags: u8,
    msphere_ofs: u16,
    light_ofs: u16,
    light_end_ofs: u16,
    dir_lights_one: u8,
    dir_lights_upd: u8,
    point_lights: u16,
    cube_ofs: u16,
    occl_index: u16,
    vert_count: u8,
    tri_count: u8,
    mip_dist: u16,
}

fn read_tfrag_header(data: &[u8], ofs: usize) -> Option<TfragHeader> {
    if ofs + 0x40 > data.len() {
        return None;
    }

    let bsphere = [
        f32::from_le_bytes(data[ofs..ofs + 4].try_into().ok()?),
        f32::from_le_bytes(data[ofs + 4..ofs + 8].try_into().ok()?),
        f32::from_le_bytes(data[ofs + 8..ofs + 12].try_into().ok()?),
        f32::from_le_bytes(data[ofs + 12..ofs + 16].try_into().ok()?),
    ];

    let u32s = [
        r_u32(data, ofs + 0x10),
        r_u32(data, ofs + 0x14),
        r_u32(data, ofs + 0x18),
        r_u32(data, ofs + 0x1c),
    ];

    let u8s: [u8; 16] = data[ofs + 0x20..ofs + 0x30].try_into().ok()?;
    let msphere_ofs = r_u16(data, ofs + 0x2e);
    let light_ofs = r_u16(data, ofs + 0x30);
    let light_end_ofs = r_u16(data, ofs + 0x32);
    let point_lights = r_u16(data, ofs + 0x36);
    let cube_ofs = r_u16(data, ofs + 0x38);
    let occl_index = r_u16(data, ofs + 0x3a);
    let vert_count = r_u8(data, ofs + 0x3c);
    let tri_count = r_u8(data, ofs + 0x3d);
    let mip_dist = r_u16(data, ofs + 0x3e);

    Some(TfragHeader {
        bsphere,
        data: u32s[0],
        lod_2_ofs: (u32s[1] & 0xffff) as u16,
        shared_ofs: ((u32s[1] >> 16) & 0xffff) as u16,
        lod_1_ofs: (u32s[2] & 0xffff) as u16,
        lod_0_ofs: ((u32s[2] >> 16) & 0xffff) as u16,
        tex_ofs: (u32s[3] & 0xffff) as u16,
        rgba_ofs: ((u32s[3] >> 16) & 0xffff) as u16,
        common_size: u8s[0],
        lod_2_size: u8s[1],
        lod_1_size: u8s[2],
        lod_0_size: u8s[3],
        lod_2_rgba_count: u8s[4],
        lod_1_rgba_count: u8s[5],
        lod_0_rgba_count: u8s[6],
        base_only: u8s[7],
        texture_count: u8s[8],
        rgba_size: u8s[9],
        rgba_verts_loc: u8s[10],
        occl_index_stash: u8s[11],
        msphere_count: u8s[12],
        flags: u8s[13],
        dir_lights_one: u8s[14],
        dir_lights_upd: u8s[15],
        msphere_ofs,
        light_ofs,
        light_end_ofs,
        point_lights,
        cube_ofs,
        occl_index,
        vert_count,
        tri_count,
        mip_dist,
    })
}

/// VIF command parsed from data
#[derive(Clone)]
struct VifCmdParsed {
    cmd: u8,
    num: u16,
    offset_in_chain: usize,
    unpack: Option<VifUnpackData>,
}

#[derive(Clone)]
struct VifUnpackData {
    vnvl_raw: u8,
    qwc: u32,
    data: Vec<u8>,
}

fn unpack_element_size(vnvl_raw: u8) -> u8 {
    let vn = ((vnvl_raw >> 2) & 3) + 1;
    let vl_bits: &[u8] = &[32, 16, 8, 5];
    let vl = vl_bits[(vnvl_raw & 3) as usize];
    (vn * vl + 7) / 8
}

fn read_vif_command_list(data: &[u8], base_ofs: usize, max_size: usize) -> (Vec<VifCmdParsed>, usize) {
    let mut commands = Vec::new();
    let mut ofs = base_ofs;
    let end = (base_ofs + max_size).min(data.len());

    while ofs + 4 <= end {
        let val = r_u32(data, ofs);
        let cmd_byte = ((val >> 24) & 0x7f) as u8;
        let num = {
            let n = ((val >> 16) & 0xff) as u16;
            if n == 0 { 256 } else { n }
        };

        let cmd_entry = VifCmdParsed {
            cmd: cmd_byte,
            num,
            offset_in_chain: ofs - base_ofs,
            unpack: None,
        };

        let pkt_size: usize;

        // Check if UNPACK (bits 5 and 6 set)
        if (cmd_byte & 0x60) == 0x60 {
            let vnvl = ((val >> 24) & 0x0f) as u8;
            let elem_sz = unpack_element_size(vnvl);
            let total = (num as u32) * elem_sz as u32;
            let qwc = (total + 15) / 16;
            let data_start = ofs + 4;
            let data_end = (data_start + qwc as usize * 16).min(end);
            let raw_data = data[data_start..data_end].to_vec();

            let mut cmd = cmd_entry;
            cmd.unpack = Some(VifUnpackData { vnvl_raw: vnvl, qwc, data: raw_data });
            commands.push(cmd);
            pkt_size = 4 + qwc as usize * 16;
        } else {
            // Non-UNPACK commands
            match cmd_byte {
                0x00 | 0x01 | 0x02 | 0x03 | 0x04 | 0x05 | 0x06 | 0x07 | 0x08 | 0x09 |
                0x0a | 0x0b | 0x0c | 0x0d | 0x0e | 0x0f |
                0x14 | 0x15 | // MSCAL, MSCALF
                0x50 | 0x51 | // FLUSHE, FLUSHA
                0x58 | 0x59 | 0x5a => {
                    commands.push(cmd_entry);
                    pkt_size = 4;
                }
                0x20 => { // STMASK
                    commands.push(cmd_entry);
                    pkt_size = 8;
                }
                0x30 | 0x31 | 0x32 => { // STROW, STCOL
                    commands.push(cmd_entry);
                    pkt_size = 20; // 4 code + 16 data
                }
                0x4a => { // MPG (or STCYCL in some docs)
                    commands.push(cmd_entry);
                    pkt_size = 4 + num as usize * 8;
                }
                0x60 | 0x61 => { // DIRECT, DIRECTHL
                    let sz = (val & 0xffff) as usize;
                    let dsize = if sz == 0 { 65536 } else { sz };
                    commands.push(cmd_entry);
                    pkt_size = 4 + dsize * 16;
                }
                _ => {
                    // Unknown - skip 4 bytes and continue
                    commands.push(cmd_entry);
                    pkt_size = 4;
                }
            }
        }

        ofs += pkt_size;
    }

    (commands, ofs - base_ofs)
}

fn filter_vif_unpacks(commands: &[VifCmdParsed]) -> Vec<&VifCmdParsed> {
    commands.iter().filter(|c| c.unpack.is_some()).collect()
}

fn read_unpack_v4_8(cmd: &VifCmdParsed) -> Vec<[u8; 4]> {
    let data = match &cmd.unpack {
        Some(u) => &u.data,
        None => return Vec::new(),
    };
    let num = cmd.num as usize;
    let mut elems = Vec::new();
    for i in 0..num {
        let off = i * 4;
        if off + 4 <= data.len() {
            elems.push([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        }
    }
    elems
}

fn read_unpack_v4_16(cmd: &VifCmdParsed) -> Vec<[i16; 4]> {
    let data = match &cmd.unpack {
        Some(u) => &u.data,
        None => return Vec::new(),
    };
    let num = cmd.num as usize;
    let mut elems = Vec::new();
    for i in 0..num {
        let off = i * 8;
        if off + 8 <= data.len() {
            elems.push([
                r_s16(data, off),
                r_s16(data, off + 2),
                r_s16(data, off + 4),
                r_s16(data, off + 6),
            ]);
        }
    }
    elems
}

fn read_unpack_v3_16(cmd: &VifCmdParsed) -> Vec<[i16; 3]> {
    let data = match &cmd.unpack {
        Some(u) => &u.data,
        None => return Vec::new(),
    };
    let num = cmd.num as usize;
    let mut elems = Vec::new();
    for i in 0..num {
        let off = i * 6;
        if off + 6 <= data.len() {
            elems.push([
                r_s16(data, off),
                r_s16(data, off + 2),
                r_s16(data, off + 4),
            ]);
        }
    }
    elems
}

fn read_unpack_v4_32(cmd: &VifCmdParsed) -> Vec<[i32; 4]> {
    let data = match &cmd.unpack {
        Some(u) => &u.data,
        None => return Vec::new(),
    };
    let num = cmd.num as usize;
    let mut elems = Vec::new();
    for i in 0..num {
        let off = i * 16;
        if off + 16 <= data.len() {
            elems.push([
                r_s32(data, off),
                r_s32(data, off + 4),
                r_s32(data, off + 8),
                r_s32(data, off + 12),
            ]);
        }
    }
    elems
}

/// Decode triangle/quad strips into faces.
/// Matches Wrench recover_faces() in tfrag_high.cpp.
/// Returns list of (ad_gif, [i0, i1, i2, i3]) where i3=-1 for triangles.
fn recover_faces(strips: &[[u8; 4]], indices: &[u8]) -> Vec<(i32, [i32; 4])> {
    let mut faces: Vec<(i32, [i32; 4])> = Vec::new();
    let mut active_ad_gif: i32 = -1;
    let mut next_strip: usize = 0;

    for strip in strips {
        let vertex_count = to_s8(strip[0]) as i32; // s8 vertex_count_and_flag
        if vertex_count <= 0 {
            if vertex_count == 0 {
                break;
            } else if to_s8(strip[2]) >= 0 {
                active_ad_gif = (strip[2] as i32) / 5;
            }
            continue; // vertex_count negative = marker, skip to next strip
        }

        if vertex_count % 2 == 0 {
            // Quads: swizzled winding (Wrench: k ^ (k > 1))
            for i in (0..vertex_count - 2).step_by(2) {
                let ad_gif = active_ad_gif;
                let mut inds = [-1i32; 4];
                for j in 0..4 {
                    let k = 3 - j;
                    let idx = next_strip + i as usize + (k ^ (if k > 1 { 1 } else { 0 }));
                    if idx < indices.len() {
                        inds[j] = indices[idx] as i32;
                    }
                }
                faces.push((ad_gif, inds));
            }
        } else {
            // Triangles
            for i in 0..vertex_count - 2 {
                let ad_gif = active_ad_gif;
                let i0 = next_strip + i as usize;
                let i1 = next_strip + i as usize + 1;
                let i2 = next_strip + i as usize + 2;
                let mut inds = [-1i32; 4];
                if i0 < indices.len() { inds[0] = indices[i0] as i32; }
                if i1 < indices.len() { inds[1] = indices[i1] as i32; }
                if i2 < indices.len() { inds[2] = indices[i2] as i32; }
                faces.push((ad_gif, inds));
            }
        }

        next_strip += vertex_count as usize;
    }

    faces
}

/// Flatten 2D V4_8 data to a byte array
fn flatten_indices(data_list: &[[u8; 4]]) -> Vec<u8> {
    let mut flat = Vec::new();
    for elem in data_list {
        for &b in elem {
            flat.push(b);
        }
    }
    flat
}

/// Parse VIF command lists for one tfrag and extract mesh data
struct TfragMeshResult {
    vertices: Vec<(f32, f32, f32)>,
    texcoords: Vec<(f32, f32)>,
    triangles: Vec<(u32, u32, u32, i32)>, // (i0, i1, i2, ad_gif)
    tex_ofs_indices: Vec<u32>,
}

fn read_tfrag_mesh(data: &[u8], table_base: usize, header: &TfragHeader, lod: u32) -> Option<TfragMeshResult> {
    let data_buf_ofs = table_base + header.data as usize;
    if data_buf_ofs >= data.len() {
        return None;
    }

    // Collected data from all LOD sections
    let mut common_positions: Vec<[i16; 3]> = Vec::new();
    let mut common_vertex_info: Vec<[i16; 4]> = Vec::new();
    let mut common_textures: Vec<[i32; 4]> = Vec::new();
    let mut lod_0_positions: Vec<[i16; 3]> = Vec::new();
    let mut lod_0_strips_raw: Vec<[u8; 4]> = Vec::new();
    let mut lod_0_indices_raw: Vec<[u8; 4]> = Vec::new();
    let mut lod_0_vertex_info: Vec<[i16; 4]> = Vec::new();
    let mut lod_0_parent_indices: Vec<[u8; 4]> = Vec::new();
    let mut lod_01_positions: Vec<[i16; 3]> = Vec::new();
    let mut lod_01_vertex_info: Vec<[i16; 4]> = Vec::new();
    let mut lod_01_parent_indices: Vec<[u8; 4]> = Vec::new();
    let mut lod_1_strips: Vec<[u8; 4]> = Vec::new();
    let mut lod_1_indices_raw: Vec<[u8; 4]> = Vec::new();
    let mut lod_2_strips: Vec<[u8; 4]> = Vec::new();
    let mut lod_2_indices: Vec<[u8; 4]> = Vec::new();
    let mut base_position: Option<(i32, i32, i32, i32)> = None;
    let mut tex_ofs_indices: Vec<u32> = Vec::new();

    // ── LOD 2 ──
    let lod2_start = data_buf_ofs + header.lod_2_ofs as usize;
    let lod2_size = (header.shared_ofs as usize).saturating_sub(header.lod_2_ofs as usize);
    if lod2_size > 0 && lod2_start + 4 <= data.len() {
        let (cmds, _) = read_vif_command_list(data, lod2_start, lod2_size);
        let unpacks = filter_vif_unpacks(&cmds);
        for up in &unpacks {
            if let Some(ref u) = up.unpack {
                if u.vnvl_raw == 0b1110 { // V4_8
                    if lod_2_strips.is_empty() {
                        lod_2_strips = read_unpack_v4_8(up);
                    } else if lod_2_indices.is_empty() {
                        lod_2_indices = read_unpack_v4_8(up);
                    }
                }
            }
        }
    }

    // ── Common ──
    let common_start = data_buf_ofs + header.shared_ofs as usize;
    let common_size = (header.lod_1_ofs as usize).saturating_sub(header.shared_ofs as usize);
    if common_size > 0 && common_start + 4 <= data.len() {
        let (cmds, _) = read_vif_command_list(data, common_start, common_size);

        // Extract base position from second STROW
        let mut strow_idx = 0u32;
        for cmd in &cmds {
            if cmd.cmd == 0x30 { // STROW
                strow_idx += 1;
                if strow_idx == 2 {
                    let strow_ofs = common_start + cmd.offset_in_chain + 4;
                    if strow_ofs + 16 <= data.len() {
                        base_position = Some((
                            r_s32(data, strow_ofs),
                            r_s32(data, strow_ofs + 4),
                            r_s32(data, strow_ofs + 8),
                            r_s32(data, strow_ofs + 12),
                        ));
                    }
                    break;
                }
            }
        }

        let unpacks = filter_vif_unpacks(&cmds);
        // Skip first UNPACK (vu_header), process the rest
        for up in unpacks.iter().skip(1) {
            if let Some(ref u) = up.unpack {
                let vnvl = u.vnvl_raw;
                if vnvl == 0b1101 && common_vertex_info.is_empty() { // V4_16
                    common_vertex_info = read_unpack_v4_16(up);
                } else if vnvl == 0b1001 && common_positions.is_empty() { // V3_16
                    common_positions = read_unpack_v3_16(up);
                } else if vnvl == 0b1100 && common_textures.is_empty() { // V4_32
                    common_textures = read_unpack_v4_32(up);
                }
            }
        }
    }

    // ── LOD 1 ──
    let lod1_start = data_buf_ofs + header.lod_1_ofs as usize;
    let lod1_size = (header.lod_0_ofs as usize).saturating_sub(header.lod_1_ofs as usize);
    if lod1_size > 0 && lod1_start + 4 <= data.len() {
        let (cmds, _) = read_vif_command_list(data, lod1_start, lod1_size);
        let unpacks = filter_vif_unpacks(&cmds);
        for up in &unpacks {
            if let Some(ref u) = up.unpack {
                if u.vnvl_raw == 0b1110 { // V4_8
                    if lod_1_strips.is_empty() {
                        lod_1_strips = read_unpack_v4_8(up);
                    } else if lod_1_indices_raw.is_empty() {
                        lod_1_indices_raw = read_unpack_v4_8(up);
                    }
                }
            }
        }
    }

    // ── LOD 01 (transition between LOD 1 and LOD 0) ──
    let lod01_start = data_buf_ofs + header.lod_0_ofs as usize;
    let lod01_size = (header.shared_ofs as usize + header.lod_1_size as usize * 0x10)
        .saturating_sub(header.lod_0_ofs as usize);
    if lod01_size > 0 && lod01_start + 4 <= data.len() {
        let (cmds, _) = read_vif_command_list(data, lod01_start, lod01_size);
        let unpacks = filter_vif_unpacks(&cmds);
        for up in &unpacks {
            if let Some(ref u) = up.unpack {
                let vnvl = u.vnvl_raw;
                if vnvl == 0b1110 && lod_01_parent_indices.is_empty() {
                    lod_01_parent_indices = read_unpack_v4_8(up);
                } else if vnvl == 0b1101 && lod_01_vertex_info.is_empty() {
                    lod_01_vertex_info = read_unpack_v4_16(up);
                } else if vnvl == 0b1001 && lod_01_positions.is_empty() {
                    lod_01_positions = read_unpack_v3_16(up);
                }
            }
        }
    }

    // ── LOD 0 ──
    let lod0_start = data_buf_ofs + header.shared_ofs as usize + header.lod_1_size as usize * 0x10;
    let mut lod0_size = (header.rgba_ofs as usize)
        .saturating_sub((header.lod_1_size as usize + header.lod_2_size as usize - header.common_size as usize) * 0x10);
    if lod0_size == 0 || lod0_size > 0x10000 {
        lod0_size = header.lod_0_size as usize * 0x10;
    }
    if lod0_size > 0 && lod0_start < data.len() && lod0_start + 4 <= data.len() {
        let (cmds, _) = read_vif_command_list(data, lod0_start, lod0_size.min(data.len() - lod0_start));
        let unpacks = filter_vif_unpacks(&cmds);

        for up in &unpacks {
            if let Some(ref u) = up.unpack {
                let vnvl = u.vnvl_raw;
                if vnvl == 0b1001 && lod_0_positions.is_empty() { // V3_16
                    lod_0_positions = read_unpack_v3_16(up);
                } else if vnvl == 0b1110 { // V4_8
                    if lod_0_strips_raw.is_empty() {
                        lod_0_strips_raw = read_unpack_v4_8(up);
                    } else if lod_0_indices_raw.is_empty() {
                        lod_0_indices_raw = read_unpack_v4_8(up);
                    } else if lod_0_parent_indices.is_empty() {
                        lod_0_parent_indices = read_unpack_v4_8(up);
                    }
                } else if vnvl == 0b1101 && lod_0_vertex_info.is_empty() { // V4_16
                    lod_0_vertex_info = read_unpack_v4_16(up);
                }
            }
        }
    }

    // ── Read tex_ofs entries ──
    let tex_data_ofs = data_buf_ofs + header.tex_ofs as usize;
    for ti in 0..header.texture_count as usize {
        let tofs = tex_data_ofs + ti * 16;
        if tofs + 16 <= data.len() {
            let v2 = r_u32(data, tofs + 8); // tex_index at u32[2]
            tex_ofs_indices.push(v2);
        }
    }

    // ── Build mesh ──
    // Combine all positions (common + lod_01 + lod_0)
    let mut all_positions: Vec<[i16; 3]> = Vec::new();
    all_positions.extend(common_positions);
    all_positions.extend(lod_01_positions);
    all_positions.extend(lod_0_positions);

    // Combine all vertex infos (common + lod_01 + lod_0)
    let mut all_vertex_infos: Vec<[i16; 4]> = Vec::new();
    all_vertex_infos.extend(common_vertex_info);
    all_vertex_infos.extend(lod_01_vertex_info);
    all_vertex_infos.extend(lod_0_vertex_info);

    if all_vertex_infos.is_empty() || all_positions.is_empty() {
        return None;
    }

    // Use faces from the requested LOD level
    let (strips, idx_flat) = match lod {
        0 => {
            if !lod_0_strips_raw.is_empty() && !lod_0_indices_raw.is_empty() {
                (lod_0_strips_raw.clone(), flatten_indices(&lod_0_indices_raw))
            } else if !lod_1_strips.is_empty() && !lod_1_indices_raw.is_empty() {
                (lod_1_strips.clone(), flatten_indices(&lod_1_indices_raw))
            } else if !lod_2_strips.is_empty() && !lod_2_indices.is_empty() {
                (lod_2_strips.clone(), flatten_indices(&lod_2_indices))
            } else {
                (Vec::new(), Vec::new())
            }
        }
        1 => {
            if !lod_1_strips.is_empty() && !lod_1_indices_raw.is_empty() {
                (lod_1_strips.clone(), flatten_indices(&lod_1_indices_raw))
            } else if !lod_0_strips_raw.is_empty() && !lod_0_indices_raw.is_empty() {
                (lod_0_strips_raw.clone(), flatten_indices(&lod_0_indices_raw))
            } else if !lod_2_strips.is_empty() && !lod_2_indices.is_empty() {
                (lod_2_strips.clone(), flatten_indices(&lod_2_indices))
            } else {
                (Vec::new(), Vec::new())
            }
        }
        _ => { // 2+
            if !lod_2_strips.is_empty() && !lod_2_indices.is_empty() {
                (lod_2_strips.clone(), flatten_indices(&lod_2_indices))
            } else if !lod_1_strips.is_empty() && !lod_1_indices_raw.is_empty() {
                (lod_1_strips.clone(), flatten_indices(&lod_1_indices_raw))
            } else if !lod_0_strips_raw.is_empty() && !lod_0_indices_raw.is_empty() {
                (lod_0_strips_raw.clone(), flatten_indices(&lod_0_indices_raw))
            } else {
                (Vec::new(), Vec::new())
            }
        }
    };

    let faces = if !strips.is_empty() && !idx_flat.is_empty() {
        recover_faces(&strips, &idx_flat)
    } else {
        Vec::new()
    };

    // Build vertices
    let (bx, by, bz) = match base_position {
        Some((x, y, z, _)) => (x as f32, y as f32, z as f32),
        None => (0.0, 0.0, 0.0),
    };

    let mut verts_out: Vec<(f32, f32, f32)> = Vec::new();
    let mut uvs_out: Vec<(f32, f32)> = Vec::new();
    let mut tris_out: Vec<(u32, u32, u32, i32)> = Vec::new();
    let mut vertex_map: BTreeMap<usize, u32> = BTreeMap::new();

    for (_fi, (ad_gif, inds)) in faces.iter().enumerate() {
        for j in 0..4 {
            let vi = inds[j] as usize;
            if vi >= all_vertex_infos.len() {
                continue;
            }
            let info = all_vertex_infos[vi];
            let pos_idx = (info[3] / 2) as usize;
            if pos_idx >= all_positions.len() {
                continue;
            }
            let pos = all_positions[pos_idx];

            let px = (bx + pos[0] as f32) / 1024.0;
            let py = (by + pos[1] as f32) / 1024.0;
            let pz = (bz + pos[2] as f32) / 1024.0;

            let mut s = vu_fixed12_from_u16(info[0] as u16);
            let mut t = vu_fixed12_from_u16(info[1] as u16);
            if s < 0.0 { s *= 0.5; }
            if t < 0.0 { t *= 0.5; }

            let key = vi;
            if !vertex_map.contains_key(&key) {
                let nv = verts_out.len() as u32;
                vertex_map.insert(key, nv);
                verts_out.push((px, py, pz));
                uvs_out.push((s, t));
            }
        }

        // Build triangle/quad
        let i0 = vertex_map.get(&(inds[0] as usize)).copied();
        let i1 = vertex_map.get(&(inds[1] as usize)).copied();
        let i2 = vertex_map.get(&(inds[2] as usize)).copied();

        if let (Some(i0v), Some(i1v), Some(i2v)) = (i0, i1, i2) {
            tris_out.push((i0v, i1v, i2v, *ad_gif));
        }

        if inds[3] >= 0 {
            let i3 = vertex_map.get(&(inds[3] as usize)).copied();
            if let (Some(i0v), Some(i1v), Some(i3v)) = (i0, i1, i3) {
                if let Some(i2v) = i2 {
                    tris_out.push((i0v, i1v, i3v, *ad_gif));
                    tris_out.push((i1v, i2v, i3v, *ad_gif));
                }
            }
        }
    }

    if verts_out.is_empty() || tris_out.is_empty() {
        return None;
    }

    Some(TfragMeshResult {
        vertices: verts_out,
        texcoords: uvs_out,
        triangles: tris_out,
        tex_ofs_indices,
    })
}

/// Write OBJ + MTL
fn write_obj_mtl(
    obj_path: &Path,
    mtl_path: &Path,
    mesh: &TfragMeshResult,
    level_name: &str,
    tfrag_idx: usize,
    tex_lookup: &BTreeMap<u32, (u32, u32, u32)>,
) -> Result<(), String> {
    use std::io::Write;

    // Collect unique material IDs
    let mat_ids: Vec<i32> = {
        let mut set: Vec<i32> = mesh.triangles.iter().map(|t| t.3).collect();
        set.sort();
        set.dedup();
        if set.is_empty() || set == vec![-1] {
            vec![0]
        } else {
            set
        }
    };

    // Write MTL
    let mut mf = std::fs::File::create(mtl_path).map_err(|e| format!("create mtl: {}", e))?;
    writeln!(mf, "# Tfrag mesh materials - Level {} fragment {}", level_name, tfrag_idx).ok();
    writeln!(mf, "# {} materials", mat_ids.len()).ok();
    for &mat_id in &mat_ids {
        writeln!(mf, "\nnewmtl tex_{}", mat_id).ok();
        writeln!(mf, "Ka 0.8 0.8 0.8").ok();
        writeln!(mf, "Kd 0.8 0.8 0.8").ok();
        writeln!(mf, "Ks 0.0 0.0 0.0").ok();
        writeln!(mf, "d 1.0").ok();
        writeln!(mf, "illum 1").ok();
        if mat_id >= 0 {
            let mat_u32 = mat_id as u32;
            if let Some(&(tex_idx, w, h)) = tex_lookup.get(&mat_u32) {
                writeln!(mf, "map_Kd ../../textures/{}/tfrag/tfrag_{:03}_w{}_h{}.png", level_name, tex_idx, w, h).ok();
            } else {
                writeln!(mf, "# map_Kd ../../textures/{}/tfrag/tfrag_NNN_wWWW_hHHH.png", level_name).ok();
            }
        } else {
            writeln!(mf, "# map_Kd ../../textures/{}/tfrag/tfrag_NNN_wWWW_hHHH.png", level_name).ok();
        }
    }

    // Write OBJ
    let mut f = std::fs::File::create(obj_path).map_err(|e| format!("create obj: {}", e))?;
    writeln!(f, "# Tfrag mesh - Level {} fragment {}", level_name, tfrag_idx).ok();
    writeln!(f, "# {} vertices, {} triangles", mesh.vertices.len(), mesh.triangles.len()).ok();
    writeln!(f, "mtllib tfrag_{:04}.mtl", tfrag_idx).ok();
    writeln!(f, "o tfrag_{}", tfrag_idx).ok();

    for &(x, y, z) in &mesh.vertices {
        writeln!(f, "v {:.6} {:.6} {:.6}", x, y, z).ok();
    }

    for &(u, v) in &mesh.texcoords {
        writeln!(f, "vt {:.6} {:.6}", u, v).ok();
    }

    writeln!(f, "s off").ok();

    for &mat_id in &mat_ids {
        writeln!(f, "g tex_{}", mat_id).ok();
        writeln!(f, "usemtl tex_{}", mat_id).ok();
        for &(i0, i1, i2, mat) in &mesh.triangles {
            if mat != mat_id {
                continue;
            }
            let (i0, i1, i2) = (i0 + 1, i1 + 1, i2 + 1);
            if i0 <= mesh.texcoords.len() as u32
                && i1 <= mesh.texcoords.len() as u32
                && i2 <= mesh.texcoords.len() as u32
            {
                writeln!(f, "f {}/{} {}/{} {}/{}", i0, i0, i1, i1, i2, i2).ok();
            } else {
                writeln!(f, "f {} {} {}", i0, i1, i2).ok();
            }
        }
    }

    Ok(())
}

/// Load tfrag texture info from core_index.bin
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
    let tt = &ch["tfrag_textures"];
    let count = tt["count"].as_i64().unwrap_or(0) as usize;
    let offset = tt["offset"].as_i64().unwrap_or(0) as usize;
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
        let w = r_u16(&idx, eo + 4) as u32;
        let h = r_u16(&idx, eo + 6) as u32;
        if w > 0 && h > 0 && w <= 1024 && h <= 1024 {
            info.insert(i as u32, (w, h));
        }
    }
    info
}

fn process_level(base: &Path, level_num: u32, target_frag: Option<i32>, lod: u32) -> Result<(), String> {
    let level_dir = base.join(format!("LEVEL{:03}", level_num)).join("data_wad");
    let core_path = level_dir.join("core_data.bin");
    let json_path = level_dir.join("core_header.json");

    if !core_path.exists() {
        return Err(format!("core_data not found for LEVEL{:03}", level_num));
    }
    if !json_path.exists() {
        return Err(format!("core_header.json not found for LEVEL{:03}", level_num));
    }

    let core_data = std::fs::read(&core_path).map_err(|e| format!("read core_data: {}", e))?;
    let core_header: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&json_path).map_err(|e| format!("read json: {}", e))?
    ).map_err(|e| format!("parse json: {}", e))?;

    let tfrags_offset = core_header.get("tfrags").and_then(|v| v.as_i64()).unwrap_or(0) as usize;

    if tfrags_offset + 16 > core_data.len() {
        println!("  SKIP: LEVEL{:03} - no tfrag data", level_num);
        return Ok(());
    }

    let table_offset = r_s32(&core_data, tfrags_offset) as usize;
    let tfrag_count = r_s32(&core_data, tfrags_offset + 4) as i32;
    let _thingy = f32::from_le_bytes(core_data[tfrags_offset + 8..tfrags_offset + 12].try_into().unwrap());

    if table_offset == 0 || tfrag_count == 0 || tfrag_count > 10000 {
        println!("  SKIP: LEVEL{:03} - no tfrags (count={})", level_num, tfrag_count);
        return Ok(());
    }

    println!("=== LEVEL{:03}: {} tfrag entries ===", level_num, tfrag_count);

    let tex_lookup = load_texture_info(&level_dir);
    let level_name = format!("LEVEL{:03}", level_num);
    let scripts_dir = base.parent().and_then(|p| p.parent()).unwrap_or(base);
    let out_dir = crate::common::meshes_dir(scripts_dir).join(&level_name);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir: {}", e))?;

    let table_base = tfrags_offset + table_offset;
    let count = tfrag_count as usize;
    let mut mesh_count = 0u32;

    for fi in 0..count {
        if let Some(tf) = target_frag {
            if fi as i32 != tf {
                continue;
            }
        }

        let entry_ofs = table_base + fi * 0x40;
        if entry_ofs + 0x40 > core_data.len() {
            break;
        }

        let header = match read_tfrag_header(&core_data, entry_ofs) {
            Some(h) => h,
            None => continue,
        };

        let mesh_data = read_tfrag_mesh(&core_data, table_base, &header, lod);
        let mesh_data = match mesh_data {
            Some(m) => m,
            None => continue,
        };

        // Build texture info (ad_gif -> tex_index, w, h)
        let mut texture_info: BTreeMap<u32, (u32, u32, u32)> = BTreeMap::new();
        for (ad_gif, &tex_idx) in mesh_data.tex_ofs_indices.iter().enumerate() {
            if let Some(&wh) = tex_lookup.get(&tex_idx) {
                texture_info.insert(ad_gif as u32, (tex_idx, wh.0, wh.1));
            }
        }

        let obj_path = out_dir.join(format!("tfrag_{:04}.obj", fi));
        let mtl_path = out_dir.join(format!("tfrag_{:04}.mtl", fi));
        write_obj_mtl(&obj_path, &mtl_path, &mesh_data, &level_name, fi, &texture_info)?;

        println!("  [{:4}] ({}/{}/{} mats)", fi, mesh_data.vertices.len(), mesh_data.triangles.len(), {
            let mut set: Vec<i32> = mesh_data.triangles.iter().map(|t| t.3).collect();
            set.sort();
            set.dedup();
            set.len()
        });
        mesh_count += 1;
    }

    println!("  => {} tfrag meshes extracted (LOD {})", mesh_count, lod);
    Ok(())
}

pub fn run(scripts_dir: &Path, args: &TfragArgs) -> Result<(), String> {
    let unpacked = crate::common::unpacked_dir(scripts_dir);
    let level_filter = args.level.unwrap_or(-1);
    let target_frag = args.tfrag_index;
    let lod = args.lod;

    crate::common::level_dispatch(level_filter, |level_num| {
        process_level(&unpacked, level_num, target_frag, lod)
    })
}
