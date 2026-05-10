//! SPACE Mesh Decoder for Ratchet & Clank: Up Your Arsenal.

use std::path::Path;
use crate::cli::*;
use crate::common::{r_u32, r_u16};

/// GS register names
const GS_REGISTERS: &[(u16, &str)] = &[
    (0x00, "PRIM"), (0x01, "RGBAQ"), (0x02, "ST"), (0x03, "UV"),
    (0x04, "XYZF2"), (0x05, "XYZ2"), (0x06, "TEX0_1"), (0x07, "TEX0_2"),
    (0x08, "CLAMP_1"), (0x09, "CLAMP_2"), (0x0A, "FOG"), (0x0B, "XYZF3"),
    (0x0C, "XYZ3"), (0x0F, "TEX1_1"), (0x14, "TEX1_2"), (0x15, "TEX2_1"),
    (0x16, "TEX2_2"), (0x17, "XYOFFSET_1"), (0x18, "XYOFFSET_2"),
    (0x19, "PRMODECONT"), (0x1A, "PRMODE"), (0x1B, "TEXCLUT"),
    (0x1C, "SCANMSK"), (0x1D, "MIPTBP1_1"), (0x1E, "MIPTBP1_2"),
    (0x1F, "MIPTBP2_1"), (0x20, "MIPTBP2_2"), (0x21, "TEXA"),
    (0x22, "FOGCOL"), (0x25, "TEXFLUSH"), (0x26, "SCISSOR_1"),
    (0x27, "SCISSOR_2"), (0x28, "ALPHA_1"), (0x29, "ALPHA_2"),
    (0x2A, "DIMX"), (0x2B, "DTHE"), (0x2C, "COLCLAMP"), (0x2D, "TEST_1"),
    (0x2E, "TEST_2"), (0x2F, "PABE"), (0x30, "FBA_1"), (0x31, "FBA_2"),
    (0x32, "FRAME_1"), (0x33, "FRAME_2"), (0x34, "ZBUF_1"),
    (0x35, "ZBUF_2"), (0x36, "BITBLTBUF"), (0x37, "TRXPOS"),
    (0x38, "TRXREG"), (0x39, "TRXDIR"), (0x3A, "HWREG"),
];

fn reg_name(r: u16) -> &'static str {
    for &(val, name) in GS_REGISTERS {
        if val == r { return name; }
    }
    "UNKNOWN"
}

/// Parse the space mesh block header and return (magic, vertex_group_count, sub_type, section_count, section_offsets)
fn parse_block_header(data: &[u8]) -> Option<(u32, u16, u16, u32, Vec<u32>)> {
    if data.len() < 0x14 { return None; }
    let magic = r_u32(data, 0);
    if magic != 0x000009D8 { return None; }
    let field4 = r_u32(data, 4);
    let vtx_group_count = (field4 & 0xFFFF) as u16;
    let sub_type = (field4 >> 16) as u16;
    let _sentinel = r_u32(data, 8);
    let sect_cnt = r_u32(data, 0x0C);
    if sect_cnt > 16 { return None; }
    let mut offsets = Vec::new();
    for i in 0..sect_cnt as usize {
        offsets.push(r_u32(data, 0x10 + i * 4));
    }
    Some((magic, vtx_group_count, sub_type, sect_cnt, offsets))
}

/// Read an f32 at an offset
fn r_f32_at(data: &[u8], off: usize) -> f32 {
    let b: [u8; 4] = data[off..off + 4].try_into().unwrap_or([0; 4]);
    f32::from_le_bytes(b)
}

/// Parse Section 1 (sub-mesh metadata)
fn parse_section1(section: &[u8]) -> serde_json::Value {
    use serde_json::json;
    if section.len() < 0x30 {
        return json!({"error": "Section 1 too short"});
    }

    let type_id = r_u32(section, 0);
    let sub_cnt = r_u32(section, 4);
    let field_8 = r_u32(section, 8);
    let vert_size = r_u32(section, 0x0C);

    let bbox: Vec<f32> = (0..4).map(|i| r_f32_at(section, 0x10 + i * 4)).collect();
    let field_u16 = r_u16(section, 0x20);

    // Parse sub-mesh offset table (u32 pairs at +0x24)
    let offset_table_size = 0xE4usize - 0x24; // 192 bytes = 48 u32 values
    let mut off_u32 = Vec::new();
    for i in 0..(offset_table_size / 4) {
        let off = 0x24 + i * 4;
        if off + 4 <= section.len() {
            off_u32.push(r_u32(section, off));
        }
    }

    // Extract sub-mesh byte offsets (every other u32)
    let mut sub_mesh_offsets = Vec::new();
    for i in (0..off_u32.len()).step_by(2) {
        let bo = off_u32[i];
        if bo == 0 && i > 0 { continue; }
        sub_mesh_offsets.push(bo);
    }

    // Remove duplicates
    let mut unique_offsets: Vec<u32> = Vec::new();
    for &o in &sub_mesh_offsets {
        if !unique_offsets.contains(&o) {
            unique_offsets.push(o);
        }
    }

    json!({
        "type": type_id,
        "sub_mesh_count": sub_cnt,
        "field_8": field_8,
        "vert_data_size": vert_size,
        "bbox": bbox,
        "field_at_0x20": field_u16,
        "sub_mesh_offsets": unique_offsets,
        "offset_table_raw": off_u32,
    })
}

/// Parse a display list section (u16 stream with 0xFFFF separators)
fn parse_display_list(section: &[u8]) -> (Vec<Vec<u16>>, Vec<u16>) {
    if section.len() < 2 {
        return (Vec::new(), Vec::new());
    }

    // Read all u16 values
    let u16_count = section.len() / 2;
    let mut u16_vals = Vec::with_capacity(u16_count);
    for i in 0..u16_count {
        u16_vals.push(r_u16(section, i * 2));
    }

    // Find all 0xFFFF positions
    let ffff_positions: Vec<usize> = u16_vals.iter().enumerate()
        .filter(|&(_, v)| *v == 0xFFFF)
        .map(|(i, _)| i)
        .collect();

    let mut parts = Vec::new();
    for pi in 0..ffff_positions.len() {
        let start = ffff_positions[pi] + 1;
        let end = if pi + 1 < ffff_positions.len() {
            ffff_positions[pi + 1]
        } else {
            u16_vals.len()
        };
        if start < end {
            parts.push(u16_vals[start..end].to_vec());
        }
    }

    let header = if !ffff_positions.is_empty() {
        u16_vals[..ffff_positions[0]].to_vec()
    } else {
        u16_vals.clone()
    };

    (parts, header)
}

/// Categorize a display list part
fn classify_part(values: &[u16]) -> &'static str {
    if values.is_empty() { return "EMPTY"; }
    if values[0] == 52 { return "GS_SETUP"; }   // 0x34 = ZBUF_1
    if (values[0] == 0 || values[0] == 1) && values.len() > 4 { return "VERTEX_DATA"; }
    "OTHER"
}

/// Analyze a single mesh block and return JSON
fn analyze_block(data: &[u8], block_idx: u32) -> serde_json::Value {
    use serde_json::json;

    let Some((magic, vtx_group_count, sub_type, sect_cnt, offsets)) = parse_block_header(data) else {
        return json!({"error": format!("Block {}: invalid header or magic", block_idx)});
    };

    // Build section end positions
    let mut ends = offsets.clone();
    ends.remove(0);
    ends.push(data.len() as u32);

    let sections: Vec<&[u8]> = (0..sect_cnt as usize).map(|i| {
        let start = offsets[i] as usize;
        let end = if i + 1 < offsets.len() {
            offsets[i + 1] as usize
        } else {
            data.len()
        };
        &data[start..end.min(data.len())]
    }).collect();

    // Section 0: float descriptors
    let s0_info = if sections.len() > 0 {
        let s0 = sections[0];
        let tuple_size = 32usize;
        let num_tuples = s0.len() / tuple_size;
        let mut float_sample = Vec::new();
        for fi in 0..(num_tuples.min(4) * 8) {
            let off = fi * 4;
            if off + 4 <= s0.len() {
                float_sample.push(r_f32_at(s0, off));
            }
        }
        json!({
            "size": s0.len(),
            "num_tuples": num_tuples,
            "float_samples": float_sample,
        })
    } else {
        json!(null)
    };

    // Section 1: sub-mesh metadata
    let s1_info = if sections.len() > 1 {
        parse_section1(sections[1])
    } else {
        json!(null)
    };

    // Section 2: display list
    let s2_info = if sections.len() > 2 {
        let (parts, hdr) = parse_display_list(sections[2]);
        let part_types: Vec<&str> = parts.iter().map(|p| classify_part(p)).collect();
        let gs_count = part_types.iter().filter(|&&t| t == "GS_SETUP").count();
        let vert_count = part_types.iter().filter(|&&t| t == "VERTEX_DATA").count();

        // Get vertex offsets from VERTEX_DATA parts
        let mut type_a_offsets: Vec<u16> = Vec::new();
        let mut type_b_offsets: Vec<u16> = Vec::new();
        for p in &parts {
            if classify_part(p) != "VERTEX_DATA" { continue; }
            for i in (0..p.len()-1).step_by(2) {
                let byte_off = p[i];
                let attr = p[i + 1];
                if attr == 0 && byte_off < 50000 {
                    type_a_offsets.push(byte_off);
                } else {
                    type_b_offsets.push(byte_off);
                }
            }
        }

        json!({
            "size": sections[2].len(),
            "header": hdr.iter().map(|&v| v as i32).collect::<Vec<_>>(),
            "part_count": parts.len(),
            "gs_setup": gs_count,
            "vertex_data": vert_count,
            "other": part_types.len() - gs_count - vert_count,
            "type_a_offsets": type_a_offsets.iter().map(|&v| v as u32).collect::<Vec<_>>(),
            "type_b_offsets": type_b_offsets.iter().map(|&v| v as u32).collect::<Vec<_>>(),
        })
    } else {
        json!(null)
    };

    // Section 3: secondary display list / vertex storage
    let s3_info = if sections.len() > 3 {
        let (parts, hdr) = parse_display_list(sections[3]);
        json!({
            "size": sections[3].len(),
            "header": hdr.iter().map(|&v| v as i32).collect::<Vec<_>>(),
            "part_count": parts.len(),
        })
    } else {
        json!(null)
    };

    json!({
        "block_index": block_idx,
        "magic": format!("0x{:08X}", magic),
        "vertex_group_count": vtx_group_count,
        "sub_type": sub_type,
        "section_count": sect_cnt,
        "section_offsets": offsets,
        "file_size": data.len(),
        "sections": {
            "0": s0_info,
            "1": s1_info,
            "2": s2_info,
            "3": s3_info,
        },
    })
}

pub fn run(scripts_dir: &Path, args: &SpaceMeshDecodeArgs) -> Result<(), String> {
    if let Some(ref input_path) = args.input {
        // Single file mode
        let data = std::fs::read(input_path)
            .map_err(|e| format!("Failed to read {}: {}", input_path, e))?;
        let result = analyze_block(&data, 0);
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
        return Ok(());
    }

    // Directory mode: read all blocks from SPACE dir
    let space_dir = crate::common::unpacked_dir(scripts_dir).join("GLOBAL").join("SPACE");
    if !space_dir.exists() {
        return Err(format!("SPACE directory not found: {}", space_dir.display()));
    }

    let mut entries: Vec<_> = std::fs::read_dir(&space_dir)
        .map_err(|e| format!("Failed to read {}: {}", space_dir.display(), e))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with("block_") && name.ends_with(".bin")
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let data = std::fs::read(entry.path())
            .map_err(|e| format!("Failed to read {}: {}", name, e))?;

        // Check magic
        if data.len() < 2 || data[0] != 0xD8 || data[1] != 0x09 {
            continue;
        }

        // Extract block index from filename
        let block_idx = name.strip_prefix("block_")
            .and_then(|s| s.split('_').next())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        // Filter by block index if specified
        if let Some(target) = args.block_index {
            if block_idx != target { continue; }
        }

        println!("=== {} ===", name);
        let result = analyze_block(&data, block_idx);
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
        println!();
    }

    Ok(())
}
