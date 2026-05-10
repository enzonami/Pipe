/// Moby class mesh extraction
/// Ported from rac_moby_extractor.py
///
/// Parses MobyClassHeader → packet table → VIF command lists → vertex tables.
/// Handles the "Insomniac vertex index quirk" (7-vertex-ahead encoding),
/// spherical-coordinate normals, triangle strips with ADGIF markers.

use std::collections::BTreeMap;
use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::vif;

/// Parse MobyClassHeader (0x48 bytes) and return packet info
struct MobyClassInfo {
    packet_table_offset: u32,
    high_lod_count: u32,
    low_lod_count: u32,
    metal_count: u32,
    total_packets: u32,
}

fn parse_moby_class_header(data: &[u8], base_ofs: usize) -> Option<MobyClassInfo> {
    if base_ofs + 0x48 > data.len() {
        return None;
    }
    let pto = r_u32(data, base_ofs) as i32;
    if pto == 0 {
        return None;
    }
    let high_lod = r_u8(data, base_ofs + 4) as u32;
    let low_lod = r_u8(data, base_ofs + 5) as u32;
    let metal = r_u8(data, base_ofs + 6) as u32;
    let _metal_begin = r_u8(data, base_ofs + 7) as u32;
    let seq_count = r_u8(data, base_ofs + 12) as u32;

    // Determine header end from sequence offset table
    let mut header_end: u32 = 0x48;
    for i in 0..seq_count {
        let so = base_ofs + 0x48 + i as usize * 4;
        if so + 4 <= data.len() {
            let seq_off = r_u32(data, so) as i32;
            if seq_off > 0x48 {
                header_end = seq_off as u32;
                break;
            }
        }
    }

    let pto = if (pto as u32) < header_end { header_end } else { pto as u32 };

    Some(MobyClassInfo {
        packet_table_offset: pto,
        high_lod_count: high_lod,
        low_lod_count: low_lod,
        metal_count: metal,
        total_packets: high_lod + low_lod + metal,
    })
}

/// A parsed packet entry
#[derive(Default)]
struct PacketEntry {
    vif_list_offset: u32,
    vif_list_size: u32,
    vertex_offset: u32,
    vertex_data_size: u32,
    transfer_vertex_count: u32,
    unknown_d: u8,
    unknown_e: u8,
    is_metal: bool,
    st_data: Vec<u8>,
    index_data: Vec<u8>,
    texture_data: Vec<u8>,
    vertex_table: Option<VertexTable>,
}

/// Parsed vertex table
struct VertexTable {
    vertices: Vec<MobyVertex>,
    duplicate_vertices: Vec<u32>,
    two_way_blend_count: u32,
    three_way_blend_count: u32,
    main_vertex_count: u32,
}

/// A single moby vertex entry (0x10 bytes from vertex table)
#[derive(Clone, Default)]
struct MobyVertex {
    low_halfword: u16,
    normal_az: u8,
    normal_el: u8,
    x: i16,
    y: i16,
    z: i16,
    /// Computed vertex index (after 7-ahead fixup)
    vertex_index: Option<u32>,
    /// Computed normal
    nx: f32,
    ny: f32,
    nz: f32,
}

fn read_vertex_table(
    data: &[u8],
    header_ofs: usize,
    transfer_vertex_count: u32,
    vertex_data_size: u32,
    _d: u8,
    _e: u8,
) -> Option<VertexTable> {
    if header_ofs + 16 > data.len() {
        return None;
    }

    let hdr0 = r_u16(data, header_ofs);
    let hdr1 = r_u16(data, header_ofs + 2);
    let hdr2 = r_u16(data, header_ofs + 4);
    let hdr3 = r_u16(data, header_ofs + 6);
    let hdr4 = r_u16(data, header_ofs + 8);
    let hdr5 = r_u16(data, header_ofs + 10);
    let hdr6 = r_u16(data, header_ofs + 12);
    let hdr7 = r_u16(data, header_ofs + 14);

    // Detect metal: zeros at odd offsets [1,3,5], values at even [0,2,4,6], no vertex_table_offset
    let is_metal = hdr1 == 0 && hdr3 == 0 && hdr5 == 0;

    let (matrix_transfer_count, two_way_blend_count, three_way_blend_count,
         main_vertex_count, duplicate_vertex_count, hdr_transfer_vertex_count,
         vertex_table_offset, _unknown_e) = if is_metal {
        (0u32, 0u32, hdr2 as u32, hdr4 as u32, hdr6 as u32, hdr0 as u32, 16u32, hdr7)
    } else {
        (hdr0 as u32, hdr1 as u32, hdr2 as u32, hdr3 as u32, hdr4 as u32,
         hdr5 as u32, hdr6 as u32, hdr7)
    };

    if hdr_transfer_vertex_count != transfer_vertex_count {
        // warning only
    }

    let in_file_vertex_count = two_way_blend_count + three_way_blend_count + main_vertex_count;
    let vertex_ofs = header_ofs + vertex_table_offset as usize;

    if vertex_ofs + in_file_vertex_count as usize * 16 > data.len() {
        return None;
    }

    // Epilogue count
    let epilogue_vertex_count = {
        let vdata_sz = vertex_data_size as usize;
        let vto = vertex_table_offset as usize;
        let ep = vdata_sz.wrapping_sub(vto / 16).wrapping_sub(in_file_vertex_count as usize);
        ep.min(6)
    };

    // Read MobyVertex entries (0x10 bytes each)
    let mut raw_vertices: Vec<MobyVertex> = Vec::new();
    for i in 0..in_file_vertex_count as usize {
        let vo = vertex_ofs + i * 16;
        let low = r_u16(data, vo);
        let az = r_u8(data, vo + 8);
        let el = r_u8(data, vo + 9);
        let vx = r_s16(data, vo + 10);
        let vy = r_s16(data, vo + 12);
        let vz = r_s16(data, vo + 14);
        raw_vertices.push(MobyVertex {
            low_halfword: low,
            normal_az: az,
            normal_el: el,
            x: vx,
            y: vy,
            z: vz,
            vertex_index: None, // computed later
            nx: 0.0,
            ny: 0.0,
            nz: 0.0,
        });
    }

    let mut blob_ofs = header_ofs + 16;

    // Matrix transfers (standard only)
    let _preloop_matrices: Vec<(u8, u8)> = if !is_metal {
        let mut mats = Vec::new();
        for i in 0..matrix_transfer_count as usize {
            let mo = blob_ofs + i * 2;
            if mo + 2 <= data.len() {
                mats.push((data[mo], data[mo + 1]));
            }
        }
        blob_ofs += matrix_transfer_count as usize * 2;
        // Alignment
        if blob_ofs % 4 != 0 { blob_ofs += 2; }
        if blob_ofs % 8 != 0 { blob_ofs += 4; }
        mats
    } else {
        Vec::new()
    };

    // Duplicate vertices
    let duplicate_vertices: Vec<u32> = if is_metal {
        let metal_dup_ofs = vertex_ofs + (in_file_vertex_count as usize + epilogue_vertex_count) * 16;
        let mut dupes = Vec::new();
        for i in 0..duplicate_vertex_count as usize {
            let do_ = metal_dup_ofs + i * 2;
            if do_ + 2 <= data.len() {
                let dupe_raw = r_u16(data, do_);
                dupes.push((dupe_raw >> 7) as u32);
            }
        }
        dupes
    } else {
        let mut dupes = Vec::new();
        for i in 0..duplicate_vertex_count as usize {
            let do_ = blob_ofs + i * 2;
            if do_ + 2 <= data.len() {
                let dupe_raw = r_u16(data, do_);
                dupes.push((dupe_raw >> 7) as u32);
            }
        }
        dupes
    };

    // Compute normals from spherical coords
    for v in &mut raw_vertices {
        let az_rad = (v.normal_az as f32) * std::f32::consts::PI / 128.0;
        let el_rad = (v.normal_el as f32) * std::f32::consts::PI / 128.0;
        let cos_az = az_rad.cos();
        let sin_az = az_rad.sin();
        let cos_el = el_rad.cos();
        let sin_el = el_rad.sin();
        v.nx = sin_az * cos_el;
        v.ny = cos_az * cos_el;
        v.nz = sin_el;
    }

    // Fix vertex indices (7-vertex-ahead quirk)
    for i in 7..raw_vertices.len() {
        raw_vertices[i - 7].vertex_index = Some((raw_vertices[i].low_halfword & 0x1ff) as u32);
    }

    // Epilogue vertices
    if epilogue_vertex_count > 0 {
        let mut ev_ofs = vertex_ofs + in_file_vertex_count as usize * 16;
        for ei in 0..epilogue_vertex_count as usize {
            if ev_ofs + 16 <= data.len() {
                let ev_low = r_u16(data, ev_ofs);
                let dest_idx = ((in_file_vertex_count as usize) + ei).wrapping_sub(7);
                if dest_idx < raw_vertices.len() {
                    raw_vertices[dest_idx].vertex_index = Some((ev_low & 0x1ff) as u32);
                }
                ev_ofs += 16;
            }
        }
        // Additional indices from last epilogue vertex
        let last_ev_ofs = ev_ofs - 16;
        let last_v_idx = in_file_vertex_count as usize + epilogue_vertex_count;
        for i in 0..(6usize.wrapping_sub(epilogue_vertex_count as usize)) {
            let dest_idx = last_v_idx.wrapping_add(i).wrapping_sub(7);
            if dest_idx < raw_vertices.len() && last_ev_ofs + 4 + i * 2 + 2 <= data.len() {
                let vi = r_u16(data, last_ev_ofs + 4 + i * 2);
                raw_vertices[dest_idx].vertex_index = Some((vi & 0x1ff) as u32);
            }
        }
    }

    // Assign remaining vertex indices from own low_halfword
    for v in &mut raw_vertices {
        if v.vertex_index.is_none() {
            v.vertex_index = Some((v.low_halfword & 0x1ff) as u32);
        }
    }

    Some(VertexTable {
        vertices: raw_vertices,
        duplicate_vertices,
        two_way_blend_count,
        three_way_blend_count,
        main_vertex_count,
    })
}

/// Parse a moby class data at base_ofs in core_data
fn parse_moby_class(data: &[u8], base_ofs: usize, _o_class: i32) -> Option<Vec<PacketEntry>> {
    let info = parse_moby_class_header(data, base_ofs)?;
    if info.packet_table_offset == 0 || info.total_packets == 0 {
        return None;
    }

    let pt_ofs = base_ofs + info.packet_table_offset as usize;
    let total = info.total_packets as usize;
    if pt_ofs + total * 16 > data.len() {
        return None;
    }

    let mut packets = Vec::new();

    for pi in 0..total {
        let pe_ofs = pt_ofs + pi * 16;
        let pe0 = r_u32(data, pe_ofs);
        let pe1 = r_u16(data, pe_ofs + 4);
        let _pe2 = r_u16(data, pe_ofs + 6);
        let pe3 = r_u32(data, pe_ofs + 8);
        let pe4 = r_u8(data, pe_ofs + 12);
        let pe5 = r_u8(data, pe_ofs + 13);
        let pe6 = r_u8(data, pe_ofs + 14);
        let pe7 = r_u8(data, pe_ofs + 15);

        let vif_abs = base_ofs + pe0 as usize;
        let vif_size = pe1 as usize * 16;
        let vert_abs = base_ofs + pe3 as usize;
        let vert_size = pe4 as u32 * 16;

        let is_metal = pi >= (info.high_lod_count + info.low_lod_count) as usize;

        // Parse VIF command list with 0x52 RAW_DATA handling
        let (commands, _consumed) = read_vif_command_list_moby(data, vif_abs, vif_size);

        // Extract UNPACK data
        let mut st_data: Option<Vec<u8>> = None;
        let mut index_data: Option<Vec<u8>> = None;
        let mut v4_5_index_data: Option<Vec<u8>> = None;
        let mut texture_data: Option<Vec<u8>> = None;

        for cmd in &commands {
            let vnvl = cmd.vnvl_raw;
            if vnvl == 0x05 && cmd.data.is_some() {
                // V2_16 = ST data
                st_data = Some(cmd.data.as_ref().unwrap().clone());
            } else if vnvl == 0x0E {
                // V4_8 = standard index data
                if index_data.is_none() {
                    index_data = Some(cmd.data.as_ref().unwrap_or(&vec![]).clone());
                }
            } else if vnvl == 0x0F {
                // V4_5 = metal index data
                if is_metal {
                    v4_5_index_data = Some(cmd.data.as_ref().unwrap_or(&vec![]).clone());
                }
            } else if vnvl == 0x0C {
                // V4_32 = texture primitive data
                if texture_data.is_none() {
                    texture_data = Some(cmd.data.as_ref().unwrap_or(&vec![]).clone());
                }
            }
        }

        // Metal: use V4_5 as index data
        if is_metal {
            if let Some(v4_5) = v4_5_index_data {
                index_data = Some(v4_5);
            }
        }

        // Parse vertex table
        let vt = if vert_abs + 16 <= data.len() && vert_size > 0 {
            read_vertex_table(data, vert_abs, pe7 as u32, pe4 as u32, pe5, pe6)
        } else {
            None
        };

        packets.push(PacketEntry {
            vif_list_offset: pe0,
            is_metal,
            st_data: st_data.unwrap_or_default(),
            index_data: index_data.unwrap_or_default(),
            texture_data: texture_data.unwrap_or_default(),
            vertex_table: vt,
            transfer_vertex_count: pe7 as u32,
            ..Default::default()
        });
    }

    Some(packets)
}

/// A VIF command with raw UNPACK data captured inline
#[derive(Debug, Clone)]
struct VifCommandEx {
    vnvl_raw: u8,
    num: u16,
    imm: u16,
    data: Option<Vec<u8>>,
}

/// Parse VIF command list with 0x52 RAW_DATA support
fn read_vif_command_list_moby(data: &[u8], base_ofs: usize, max_size: usize) -> (Vec<VifCommandEx>, usize) {
    let mut commands = Vec::new();
    let mut offset = base_ofs;
    let end = (base_ofs + max_size).min(data.len());
    let mut prev_unpack_end = 0;

    while offset + 4 <= end {
        let val = r_u32(data, offset);
        let cmd_byte = (val >> 24) & 0x7f;
        let num = {
            let n = ((val >> 16) & 0xff) as u16;
            if n == 0 { 256 } else { n }
        };

        let (cmd, imm, _qwc) = vif::read_vif_code(val);
        let mut pkt_size = 4u32;

        // NOP, FLUSHA, etc.
        if cmd == vif::VifCmd::Nop || cmd == vif::VifCmd::FlushA || cmd == vif::VifCmd::FlushE {
            offset += 4;
            continue;
        }

        if cmd == vif::VifCmd::DirDma || cmd == vif::VifCmd::DirDmaIce {
            break;
        }

        if cmd.is_unpack() {
            let elem_sz = vif::unpack_element_size(cmd_byte as u8);
            let total = (num as u32) * (elem_sz as u32);
            let aligned = (total + 15) & !15;
            let qwc = aligned / 16;
            pkt_size = 4 + qwc * 16;
            let data_start = offset + 4;
            let data_end = (data_start + qwc as usize * 16).min(end);
            let raw_data = data[data_start..data_end].to_vec();

            let vnvl = (cmd_byte & 0x0f) as u8;
            commands.push(VifCommandEx {
                vnvl_raw: vnvl,
                num,
                imm,
                data: Some(raw_data),
            });
            prev_unpack_end = offset + pkt_size as usize;
        } else if cmd_byte == 0x52 {
            // RAW_DATA: scan forward for next UNPACK
            let data_start = if prev_unpack_end > 0 { prev_unpack_end } else { offset + 4 };
            let mut scan = data_start;
            let mut next_unpack = end;
            while scan + 4 <= end {
                let sv = r_u32(data, scan);
                let sc = (sv >> 24) & 0x7f;
                if (sc & 0x60) == 0x60 {
                    next_unpack = scan;
                    break;
                }
                scan += 4;
            }
            let data_size = next_unpack - data_start;
            if data_size > 0 || next_unpack == end {
                let actual_size = if next_unpack == end { end - data_start } else { data_size };
                let raw_data = data[data_start..data_start + actual_size].to_vec();
                let qwc = (actual_size + 15) / 16;
                // Treat as synthetic V4_8 UNPACK
                commands.push(VifCommandEx {
                    vnvl_raw: 0x0E, // V4_8
                    num: ((qwc * 16) as u16), // approximate
                    imm: 0,
                    data: Some(raw_data),
                });
                pkt_size = 4 + actual_size as u32;
            }
        } else {
            // Unknown command - skip
            offset += 4;
            continue;
        }

        offset += pkt_size as usize;
    }

    (commands, offset - base_ofs)
}

/// Recover triangle strip mesh data from packet entries
fn recover_mesh(packets: &[PacketEntry], scale: f32) -> Option<MeshData> {
    let mut all_vertices: Vec<(f32, f32, f32)> = Vec::new();
    let mut all_normals: Vec<(f32, f32, f32)> = Vec::new();
    let mut all_texcoords: Vec<(f32, f32)> = Vec::new();
    let mut all_indices: Vec<u32> = Vec::new();
    let mut mat_groups: BTreeMap<u32, Vec<[u32; 3]>> = BTreeMap::new();

    for packet in packets {
        let vt = match &packet.vertex_table {
            Some(v) => v,
            None => continue,
        };

        let st_data = &packet.st_data;
        let index_data = &packet.index_data;
        let is_metal = packet.is_metal;
        let sts: Vec<(f32, f32)> = if !st_data.is_empty() {
            (0..st_data.len() / 4).map(|i| {
                let off = i * 4;
                let s_raw = r_s16(st_data, off) as f32 / 4096.0;
                let t_raw = r_s16(st_data, off + 2) as f32 / 4096.0;
                (s_raw, t_raw)
            }).collect()
        } else {
            Vec::new()
        };

        // Build local vertex list
        let mut local_vertices: Vec<LocalVert> = Vec::new();
        let mut vertex_cache: Vec<Option<LocalVert>> = vec![None; 512];

        for (i, v) in vt.vertices.iter().enumerate() {
            let vx = v.x as f32 * scale;
            let vy = v.y as f32 * scale;
            let vz = v.z as f32 * scale;
            let (u, vt_uv) = if i < sts.len() { sts[i] } else { (0.0, 0.0) };
            let vi = v.vertex_index.unwrap_or(i as u32) & 0x1ff;
            let vert_data = LocalVert {
                pos: (vx, vy, vz),
                normal: (v.nx, v.ny, v.nz),
                uv: (u, vt_uv),
            };
            vertex_cache[vi as usize] = Some(vert_data.clone());
            local_vertices.push(vert_data);
        }

        // Duplicate vertices (same position, new UV)
        for (j, &dupe_vi) in vt.duplicate_vertices.iter().enumerate() {
            let cache_idx = (dupe_vi & 0x1ff) as usize;
            let st_idx = vt.vertices.len() + j;
            if let Some(ref cv) = vertex_cache[cache_idx] {
                let (u, vt_uv) = if st_idx < sts.len() { sts[st_idx] } else { (0.0, 0.0) };
                local_vertices.push(LocalVert {
                    pos: cv.pos,
                    normal: cv.normal,
                    uv: (u, vt_uv),
                });
            }
        }

        // Process index buffer
        let idx_body_raw: Vec<i16>;
        let mut use_adgif = false;
        let secret_indices: Vec<i16>;

        if is_metal {
            // Metal: raw unsigned byte indices
            idx_body_raw = index_data.iter().map(|&b| b as i16).collect();
            secret_indices = Vec::new();
        } else if index_data.is_empty() || index_data.len() < 4 {
            // Non-indexed: sequential strip
            if local_vertices.len() < 3 {
                continue;
            }
            idx_body_raw = (0..local_vertices.len()).map(|i| (i + 1) as i16).collect();
            secret_indices = Vec::new();
        } else {
            if index_data.len() >= 84 {
                use_adgif = true;
                // Header: u8 u0, u8 tex_unpack_offset, s8 secret_index, u8 pad
                let secret = index_data[2] as i8;
                let mut secs: Vec<i16> = vec![secret as i16];
                // Additional secrets from texture primitives at +0x0c within each 0x40 block
                let td = &packet.texture_data;
                if !td.is_empty() {
                    for ti in 0..td.len() / 0x40 {
                        let to = ti * 0x40;
                        if to + 0x40 <= td.len() {
                            let secret_raw = r_u32(td, to + 0x0c);
                            secs.push((secret_raw & 0xff) as i16);
                        }
                    }
                }
                secret_indices = secs;
                idx_body_raw = index_data[4..84.min(index_data.len())].iter().map(|&b| b as i16).collect();
            } else {
                // Compact: no header, no ADGIF
                secret_indices = Vec::new();
                idx_body_raw = index_data.iter().map(|&b| {
                    if b < 128 { b as i16 } else { (b as i16) - 256 }
                }).collect();
            }
        }

        // Convert signed bytes
        let idx_body: Vec<i16> = if is_metal {
            idx_body_raw  // unsigned
        } else {
            idx_body_raw
        };

        // Build triangle strip
        let mut strip: Vec<u32> = Vec::new();
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        let mut tri_materials: Vec<u32> = Vec::new();
        let mut secret_ptr = 0;
        let mut active_tex_prim: u32 = 0;

        let mut i = 0;
        while i < idx_body.len() {
            let val = idx_body[i];

            if is_metal {
                if val == 0 {
                    strip.clear();
                    i += 1;
                    continue;
                }
                let vi = val as u32;
                if vi >= local_vertices.len() as u32 {
                    i += 1;
                    continue;
                }
                strip.push(vi);
            } else {
                if !use_adgif && val == 0 {
                    i += 1;
                    continue;
                }

                if use_adgif && val == 0 {
                    // ADGIF marker
                    let secret = if secret_ptr < secret_indices.len() {
                        secret_indices[secret_ptr]
                    } else {
                        0
                    };
                    secret_ptr += 1;
                    let secret_s = if secret < 128 { secret } else { secret - 256 };

                    if secret_s == 0 {
                        break; // END
                    }
                    // Texture change
                    if secret_ptr >= 2 {
                        active_tex_prim = (secret_ptr - 2) as u32;
                    }
                    i += 1;
                    continue;
                }

                if val < 0 {
                    // Double-negative = strip break with borrow
                    if i + 1 < idx_body.len() && idx_body[i + 1] < 0 {
                        if strip.len() >= 2 {
                            let last2 = &strip[strip.len() - 2..];
                            strip = last2.to_vec();
                        } else {
                            strip.clear();
                        }
                        i += 2;
                        continue;
                    }
                    // Single negative = fresh start
                    strip.clear();
                    i += 1;
                    continue;
                }

                // Positive: vertex index = value - 1
                let vi = (val - 1) as u32;
                if vi >= local_vertices.len() as u32 {
                    i += 1;
                    continue;
                }
                strip.push(vi);
            }

            // Emit triangle
            if strip.len() >= 3 {
                let k = strip.len() - 1;
                let tri = if k % 2 == 0 {
                    [strip[k - 2], strip[k - 1], strip[k]]
                } else {
                    [strip[k - 1], strip[k - 2], strip[k]]
                };

                // Skip degenerate
                let p0 = local_vertices[tri[0] as usize].pos;
                let p1 = local_vertices[tri[1] as usize].pos;
                let p2 = local_vertices[tri[2] as usize].pos;
                if p0 == p1 || p1 == p2 || p0 == p2 {
                    i += 1;
                    continue;
                }

                triangles.push(tri);
                tri_materials.push(active_tex_prim);
            }

            i += 1;
        }

        // Map triangles to global vertex list
        for (tri_idx, &tri) in triangles.iter().enumerate() {
            let mat_prim = *tri_materials.get(tri_idx).unwrap_or(&0);
            let mut new_tri = [0u32; 3];
            for (j, &idx) in tri.iter().enumerate() {
                let lv = &local_vertices[idx as usize];
                all_vertices.push(lv.pos);
                all_normals.push(lv.normal);
                all_texcoords.push(lv.uv);
                let vi = (all_vertices.len() - 1) as u32;
                all_indices.push(vi);
                new_tri[j] = vi;
            }
            mat_groups.entry(mat_prim).or_default().push(new_tri);
        }
    }

    if all_vertices.is_empty() {
        return None;
    }

    Some(MeshData {
        vertices: all_vertices,
        normals: all_normals,
        texcoords: all_texcoords,
        indices: all_indices,
        mat_groups,
    })
}

#[derive(Clone)]
struct LocalVert {
    pos: (f32, f32, f32),
    normal: (f32, f32, f32),
    uv: (f32, f32),
}

struct MeshData {
    vertices: Vec<(f32, f32, f32)>,
    normals: Vec<(f32, f32, f32)>,
    texcoords: Vec<(f32, f32)>,
    indices: Vec<u32>,
    mat_groups: BTreeMap<u32, Vec<[u32; 3]>>,
}

fn write_obj_mtl(
    obj_path: &Path,
    mtl_path: &Path,
    mesh: &MeshData,
    mtl_name: &str,
) -> Result<(), String> {
    use std::io::Write;

    // Write OBJ
    let mut f = std::fs::File::create(obj_path).map_err(|e| format!("create obj: {}", e))?;
    writeln!(f, "# Moby mesh - {} vertices", mesh.vertices.len()).ok();
    writeln!(f, "mtllib {}", mtl_path.file_name().unwrap().to_string_lossy()).ok();
    writeln!(f, "o moby_mesh").ok();

    for v in &mesh.vertices {
        writeln!(f, "v {:.6} {:.6} {:.6}", v.0, v.1, v.2).ok();
    }
    for uv in &mesh.texcoords {
        writeln!(f, "vt {:.6} {:.6}", uv.0, uv.1).ok();
    }
    for n in &mesh.normals {
        writeln!(f, "vn {:.6} {:.6} {:.6}", n.0, n.1, n.2).ok();
    }
    writeln!(f, "s off").ok();

    if !mesh.mat_groups.is_empty() {
        for (mat_key, triples) in &mesh.mat_groups {
            let mat_name = if *mat_key == 0 {
                mtl_name.to_string()
            } else {
                format!("mtex_{}", mat_key)
            };
            writeln!(f, "usemtl {}", mat_name).ok();
            for tri in triples {
                writeln!(f, "f {}/{}/{} {}/{}/{} {}/{}/{}",
                    tri[0] + 1, tri[0] + 1, tri[0] + 1,
                    tri[1] + 1, tri[1] + 1, tri[1] + 1,
                    tri[2] + 1, tri[2] + 1, tri[2] + 1).ok();
            }
        }
    } else {
        writeln!(f, "usemtl {}", mtl_name).ok();
        for i in (0..mesh.indices.len()).step_by(3) {
            if i + 2 < mesh.indices.len() {
                let v0 = mesh.indices[i] + 1;
                let v1 = mesh.indices[i + 1] + 1;
                let v2 = mesh.indices[i + 2] + 1;
                writeln!(f, "f {} {} {}", v0, v1, v2).ok();
            }
        }
    }

    // Write MTL
    let mut mf = std::fs::File::create(mtl_path).map_err(|e| format!("create mtl: {}", e))?;
    writeln!(mf, "# Moby mesh material").ok();
    writeln!(mf, "newmtl default").ok();
    writeln!(mf, "Ka 0.8 0.8 0.8").ok();
    writeln!(mf, "Kd 0.8 0.8 0.8").ok();
    writeln!(mf, "Ks 0.2 0.2 0.2").ok();
    writeln!(mf, "Ns 50.0").ok();

    // Per-texture materials
    for (mat_key, _) in &mesh.mat_groups {
        if *mat_key != 0 {
            let mat_name = format!("mtex_{}", mat_key);
            writeln!(mf, "\nnewmtl {}", mat_name).ok();
            writeln!(mf, "Ka 0.8 0.8 0.8").ok();
            writeln!(mf, "Kd 0.8 0.8 0.8").ok();
            writeln!(mf, "Ks 0.2 0.2 0.2").ok();
            writeln!(mf, "Ns 50.0").ok();
        }
    }

    Ok(())
}

fn process_level(scripts_dir: &Path, unpacked: &Path, level_num: u32, target_class: Option<i32>) -> Result<(), String> {
    let data_dir = unpacked.join(format!("LEVEL{:03}", level_num)).join("data_wad");
    let core_data_path = data_dir.join("core_data.bin");
    let moby_json_path = data_dir.join("moby_classes.json");

    if !core_data_path.exists() {
        return Err(format!("LEVEL{:03}: core_data not found", level_num));
    }

    let core_data = std::fs::read(&core_data_path)
        .map_err(|e| format!("read core_data: {}", e))?;

    let moby_json_str = std::fs::read_to_string(&moby_json_path)
        .map_err(|e| format!("read moby_classes.json: {}", e))?;
    let moby_json: serde_json::Value = serde_json::from_str(&moby_json_str)
        .map_err(|e| format!("parse moby_classes.json: {}", e))?;

    let entries = moby_json["entries"].as_array().ok_or("no entries")?;
    println!("\n=== LEVEL{:03}: {} moby class entries ===", level_num, entries.len());

    let out_dir = crate::common::meshes_dir(scripts_dir).join(format!("LEVEL{:03}", level_num));
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("mkdir: {}", e))?;

    let mut mesh_count = 0;
    let scale = 1.0 / 1024.0;

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

        let base_ofs = wad_off as usize;
        if base_ofs + 0x48 > core_data.len() {
            continue;
        }

        let packets = match parse_moby_class(&core_data, base_ofs, o_class) {
            Some(p) => p,
            None => continue,
        };

        // Filter to packets with vertex data
        let valid: Vec<&PacketEntry> = packets.iter()
            .filter(|p| p.vertex_table.is_some() && (!p.st_data.is_empty() || p.is_metal))
            .collect();

        if valid.is_empty() {
            continue;
        }

        let hi = packets.iter().filter(|p| !p.is_metal).count();
        let metal = packets.iter().filter(|p| p.is_metal).count();
        println!("  Class {}: {}hi/{}metal packets", o_class, hi, metal);

        let mesh_data = match recover_mesh(&packets, scale) {
            Some(m) => m,
            None => continue,
        };

        if !mesh_data.vertices.is_empty() {
            let obj_path = out_dir.join(format!("class_{}.obj", o_class));
            let mtl_path = out_dir.join(format!("class_{}.mtl", o_class));
            write_obj_mtl(&obj_path, &mtl_path, &mesh_data, &format!("class_{}", o_class))?;
            println!("    -> OBJ: {} ({} verts)", obj_path.display(), mesh_data.vertices.len());
            mesh_count += 1;
        }
    }

    println!("  Total: {} meshes", mesh_count);
    Ok(())
}

pub fn run(scripts_dir: &Path, args: &MobyArgs) -> Result<(), String> {
    let unpacked = crate::common::unpacked_dir(scripts_dir);
    let level_filter = args.level.unwrap_or(-1);
    let target_class = args.class;

    level_dispatch(level_filter, |level_num| {
        process_level(scripts_dir, &unpacked, level_num, target_class)
    })
}
