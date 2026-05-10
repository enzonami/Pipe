/// Gameplay extractor - parse gameplay.bin into structured JSON
/// Ported from rac_gameplay_extractor.py
use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::wad;
use serde_json::{json, Value};

pub fn run(scripts_dir: &Path, args: &GameplayArgs) -> Result<(), String> {
    let unpacked = crate::common::unpacked_dir(scripts_dir);
    let level_filter = args.level.unwrap_or(-1);

    level_dispatch(level_filter, |level_num| {
        process_level(&unpacked, level_num)
    })
}

fn process_level(base: &Path, level_num: u32) -> Result<(), String> {
    let level_dir = base.join(format!("LEVEL{:03}", level_num));
    let gameplay_path = level_dir.join("gameplay.bin");

    if !gameplay_path.exists() {
        return Err(format!("gameplay.bin not found for LEVEL {:03}", level_num));
    }

    let mut data = std::fs::read(&gameplay_path)
        .map_err(|e| format!("Cannot read gameplay.bin: {}", e))?;

    // Check if WAD compressed
    if data.len() >= 3 && &data[..3] == b"WAD" {
        println!("  Decompressing WAD gameplay...");
        data = wad::decompress_wad_lz(&data)
            .map_err(|e| format!("Decompress error: {}", e))?;
    }

    println!("  LEVEL {:03}: gameplay.bin {} bytes", level_num, data.len());

    let result = parse_gameplay(&data);

    let json_path = level_dir.join("gameplay_extracted.json");
    let json_str = serde_json::to_string_pretty(&result)
        .map_err(|e| format!("JSON error: {}", e))?;
    std::fs::write(&json_path, &json_str)
        .map_err(|e| format!("Write error: {}", e))?;

    // Print summary
    print_summary(&result);
    println!("  Wrote {}", json_path.display());

    Ok(())
}

// GC/UYA Gameplay blocks (header_pointer_offset, name)
const GC_UYA_BLOCKS: &[(usize, &str)] = &[
    (0x8c, "env_sample_points"),
    (0x00, "level_settings"),
    (0x10, "us_english_help"),
    (0x14, "uk_english_help"),
    (0x18, "french_help"),
    (0x1c, "german_help"),
    (0x20, "spanish_help"),
    (0x24, "italian_help"),
    (0x28, "japanese_help"),
    (0x2c, "korean_help"),
    (0x04, "dir_lights"),
    (0x84, "env_transitions"),
    (0x08, "cameras"),
    (0x0c, "sound_instances"),
    (0x48, "moby_classes"),
    (0x4c, "moby_instances"),
    (0x5c, "pvar_table"),
    (0x60, "pvar_data"),
    (0x58, "pvar_moby_links"),
    (0x64, "pvar_relative_pointers"),
    (0x50, "moby_groups"),
    (0x54, "shared_data"),
    (0x30, "tie_classes"),
    (0x34, "tie_instances"),
    (0x94, "tie_ambient_rgbas"),
    (0x38, "tie_groups"),
    (0x3c, "shrub_classes"),
    (0x40, "shrub_instances"),
    (0x44, "shrub_groups"),
    (0x78, "paths"),
    (0x68, "cuboids"),
    (0x6c, "spheres"),
    (0x70, "cylinders"),
    (0x74, "pills"),
    (0x88, "cam_coll_grid"),
    (0x80, "point_lights"),
    (0x7c, "grind_paths"),
    (0x98, "areas"),
    (0x90, "occlusion"),
];

fn parse_gameplay(data: &[u8]) -> Value {
    // Build pointer table
    let mut pointers: Vec<(&str, i32)> = Vec::new();
    let mut ptr_map: std::collections::HashMap<&str, i32> = std::collections::HashMap::new();
    for &(hdr_off, name) in GC_UYA_BLOCKS {
        let ptr = r_s32(data, hdr_off);
        pointers.push((name, ptr));
        if ptr != 0 {
            ptr_map.insert(name, ptr);
        }
    }
    pointers.sort_by_key(|&(_, p)| p);

    let mut result = serde_json::Map::new();
    result.insert("header_pointers".into(), json!({
        "us_english_help": ptr_map.get("us_english_help"),
        "uk_english_help": ptr_map.get("uk_english_help"),
        "french_help": ptr_map.get("french_help"),
        "german_help": ptr_map.get("german_help"),
        "spanish_help": ptr_map.get("spanish_help"),
        "italian_help": ptr_map.get("italian_help"),
        "japanese_help": ptr_map.get("japanese_help"),
        "korean_help": ptr_map.get("korean_help"),
        "level_settings": ptr_map.get("level_settings"),
        "dir_lights": ptr_map.get("dir_lights"),
        "cameras": ptr_map.get("cameras"),
        "sound_instances": ptr_map.get("sound_instances"),
        "moby_classes": ptr_map.get("moby_classes"),
        "moby_instances": ptr_map.get("moby_instances"),
        "tie_instances": ptr_map.get("tie_instances"),
        "shrub_instances": ptr_map.get("shrub_instances"),
        "paths": ptr_map.get("paths"),
        "grind_paths": ptr_map.get("grind_paths"),
    }));
    result.insert("file_size".into(), json!(data.len()));

    // Helper: next block offset
    let next_offset = |name: &str, default: usize| -> usize {
        let ptr = ptr_map.get(name).copied().unwrap_or(0) as usize;
        if ptr == 0 { return default; }
        for &(n, p) in &pointers {
            if p as usize > ptr && n != name {
                return p as usize;
            }
        }
        default
    };

    // --- Level Settings ---
    if let Some(&ofs) = ptr_map.get("level_settings") {
        let ofs = ofs as usize;
        result.insert("level_settings".into(), parse_level_settings(data, ofs));
    }

    // --- Directional Lights ---
    if let Some(&ofs) = ptr_map.get("dir_lights") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut lights = Vec::new();
        for i in 0..count {
            let lo = ofs + 0x10 + i * 0x40;
            lights.push(json!({
                "colour": vec3f(data, lo),
                "colour_pad": r_f32(data, lo + 0x0c),
                "direction": vec3f(data, lo + 0x10),
                "direction_pad": r_f32(data, lo + 0x1c),
            }));
        }
        result.insert("directional_lights".into(), json!({"count": count, "lights": lights}));
    }

    // --- Cameras ---
    if let Some(&ofs) = ptr_map.get("cameras") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut cameras = Vec::new();
        for i in 0..count {
            let co = ofs + 0x10 + i * 0x20;
            cameras.push(json!({
                "type": r_s32(data, co),
                "position": vec3f(data, co + 0x04),
                "rotation": vec3f(data, co + 0x10),
                "pvar_index": r_s32(data, co + 0x1c),
            }));
        }
        result.insert("cameras".into(), json!({"count": count, "cameras": cameras}));
    }

    // --- Sound Instances ---
    if let Some(&ofs) = ptr_map.get("sound_instances") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut sounds = Vec::new();
        for i in 0..count {
            let so = ofs + 0x10 + i * 0xA0;
            sounds.push(json!({
                "o_class": r_s32(data, so),
                "m_class": r_s32(data, so + 0x04),
                "update_fun_ptr": r_s32(data, so + 0x08),
                "pvar_index": r_s32(data, so + 0x0c),
                "range": r_f32(data, so + 0x10),
                "matrix": mat4(data, so + 0x14),
                "matrix2": mat4(data, so + 0x54),
                "rotation": vec3f(data, so + 0x94),
            }));
        }
        result.insert("sound_instances".into(), json!({"count": count, "sounds": sounds}));
    }

    // --- Help Messages ---
    for lang in &["us_english_help", "uk_english_help", "french_help", "german_help",
                  "spanish_help", "italian_help", "japanese_help", "korean_help"] {
        if let Some(&ofs) = ptr_map.get(*lang) {
            result.insert(lang.to_string(), parse_help_messages(data, ofs as usize));
        }
    }

    // --- Moby Classes ---
    if let Some(&ofs) = ptr_map.get("moby_classes") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut classes = Vec::new();
        for i in 0..count {
            classes.push(r_s32(data, ofs + 4 + i * 4));
        }
        result.insert("moby_classes".into(), json!({"count": count, "class_ids": classes}));
    }

    // --- Moby Instances ---
    if let Some(&ofs) = ptr_map.get("moby_instances") {
        let ofs = ofs as usize;
        let static_count = r_s32(data, ofs) as usize;
        let _spawnable_count = r_s32(data, ofs + 4) as usize;
        let mut instances = Vec::new();
        for i in 0..static_count {
            let io = ofs + 0x10 + i * 0x88;
            let light_colour = {
                let r = r_s32(data, io + 0x74);
                let g = r_s32(data, io + 0x78);
                let b = r_s32(data, io + 0x7c);
                json!({"r": r, "g": g, "b": b})
            };
            instances.push(json!({
                "size": r_s32(data, io),
                "mission": r_s32(data, io + 0x04),
                "unknown_8": r_s32(data, io + 0x08),
                "unknown_c": r_s32(data, io + 0x0c),
                "uid": r_s32(data, io + 0x10),
                "bolts": r_s32(data, io + 0x14),
                "unknown_18": r_s32(data, io + 0x18),
                "unknown_1c": r_s32(data, io + 0x1c),
                "unknown_20": r_s32(data, io + 0x20),
                "unknown_24": r_s32(data, io + 0x24),
                "o_class": r_s32(data, io + 0x28),
                "scale": r_f32(data, io + 0x2c),
                "draw_distance": r_s32(data, io + 0x30),
                "update_distance": r_s32(data, io + 0x34),
                "unused_38": r_s32(data, io + 0x38),
                "unused_3c": r_s32(data, io + 0x3c),
                "position": vec3f(data, io + 0x40),
                "rotation": vec3f(data, io + 0x4c),
                "group": r_s32(data, io + 0x58),
                "is_rooted": r_s32(data, io + 0x5c),
                "rooted_distance": r_f32(data, io + 0x60),
                "unknown_64": r_s32(data, io + 0x64),
                "pvar_index": r_s32(data, io + 0x68),
                "occlusion": r_s32(data, io + 0x6c),
                "mode_bits": r_s32(data, io + 0x70),
                "light_colour": light_colour,
                "light": r_s32(data, io + 0x80),
                "unknown_84": r_s32(data, io + 0x84),
            }));
        }
        result.insert("moby_instances".into(), json!({
            "static_count": static_count, "count": static_count, "instances": instances
        }));
    }

    // --- Tie Instances ---
    if let Some(&ofs) = ptr_map.get("tie_instances") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut instances = Vec::new();
        for i in 0..count {
            let io = ofs + 0x10 + i * 0x60;
            instances.push(json!({
                "o_class": r_s32(data, io),
                "draw_distance": r_s32(data, io + 0x04),
                "pad_8": r_s32(data, io + 0x08),
                "occlusion_index": r_s32(data, io + 0x0c),
                "matrix": mat4(data, io + 0x10),
                "directional_lights": r_s32(data, io + 0x50),
                "uid": r_s32(data, io + 0x54),
                "pad_58": r_s32(data, io + 0x58),
                "pad_5c": r_s32(data, io + 0x5c),
            }));
        }
        result.insert("tie_instances".into(), json!({"count": count, "instances": instances}));
    }

    // --- Tie Ambient RGBAs ---
    if let Some(&ofs) = ptr_map.get("tie_ambient_rgbas") {
        let mut ofs = ofs as usize;
        let mut rgbas: Vec<Vec<u8>> = Vec::new();
        loop {
            let index = r_s16(data, ofs) as i32;
            ofs += 2;
            if index == -1 { break; }
            let count = r_u16(data, ofs) as usize;
            ofs += 2;
            let end = (ofs + count * 2).min(data.len());
            rgbas.push(data[ofs..end].to_vec());
            ofs = end;
        }
        result.insert("tie_ambient_rgbas".into(), json!(rgbas));
    }

    // --- Tie Groups ---
    if let Some(&ofs) = ptr_map.get("tie_groups") {
        result.insert("tie_groups".into(), parse_groups(data, ofs as usize));
    }

    // --- Shrub Classes ---
    if let Some(&ofs) = ptr_map.get("shrub_classes") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut classes = Vec::new();
        for i in 0..count {
            classes.push(r_s32(data, ofs + 4 + i * 4));
        }
        result.insert("shrub_classes".into(), json!({"count": count, "class_ids": classes}));
    }

    // --- Shrub Instances ---
    if let Some(&ofs) = ptr_map.get("shrub_instances") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut instances = Vec::new();
        for i in 0..count {
            let io = ofs + 0x10 + i * 0x70;
            let colour = {
                let r = r_s32(data, io + 0x50);
                let g = r_s32(data, io + 0x54);
                let b = r_s32(data, io + 0x58);
                json!({"r": r, "g": g, "b": b})
            };
            instances.push(json!({
                "o_class": r_s32(data, io),
                "draw_distance": r_f32(data, io + 0x04),
                "unused_8": r_s32(data, io + 0x08),
                "unused_c": r_s32(data, io + 0x0c),
                "matrix": mat4(data, io + 0x10),
                "colour": colour,
                "unused_5c": r_s32(data, io + 0x5c),
                "dir_lights": r_s32(data, io + 0x60),
                "unused_64": r_s32(data, io + 0x64),
                "unused_68": r_s32(data, io + 0x68),
                "unused_6c": r_s32(data, io + 0x6c),
            }));
        }
        result.insert("shrub_instances".into(), json!({"count": count, "instances": instances}));
    }

    // --- Shrub Groups ---
    if let Some(&ofs) = ptr_map.get("shrub_groups") {
        result.insert("shrub_groups".into(), parse_groups(data, ofs as usize));
    }

    // --- Moby Groups ---
    if let Some(&ofs) = ptr_map.get("moby_groups") {
        result.insert("moby_groups".into(), parse_groups(data, ofs as usize));
    }

    // --- Shared Data ---
    if let Some(&ofs) = ptr_map.get("shared_data") {
        let ofs = ofs as usize;
        let data_size = r_s32(data, ofs) as usize;
        let pointer_count = r_s32(data, ofs + 4) as usize;
        let sd_data: Vec<u8> = data[ofs + 0x10..ofs + 0x10 + data_size].to_vec();
        let mut pointers_table = Vec::new();
        for i in 0..pointer_count {
            let to = ofs + 0x10 + data_size + i * 8;
            pointers_table.push(json!({
                "pvar_index": r_u16(data, to),
                "pointer_offset": r_u16(data, to + 2),
                "shared_data_offset": r_s32(data, to + 4),
            }));
        }
        result.insert("shared_data".into(), json!({
            "data_size": data_size, "pointer_count": pointer_count,
            "data": sd_data, "pointers": pointers_table
        }));
    }

    // --- Pvar Table ---
    if let Some(&ofs) = ptr_map.get("pvar_table") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut entries = Vec::new();
        for i in 0..count {
            let po = ofs + 0x10 + i * 8;
            entries.push(json!({
                "offset": r_s32(data, po),
                "size": r_s32(data, po + 4),
            }));
        }
        result.insert("pvar_table".into(), json!({"count": count, "entries": entries}));
    }

    // --- Pvar Data ---
    if let Some(&ofs) = ptr_map.get("pvar_data") {
        let end = next_offset("pvar_data", data.len());
        result.insert("pvar_data_size".into(), json!(end.saturating_sub(ofs as usize)));
    }

    // --- Pvar Fixups ---
    for fixup_name in &["pvar_moby_links", "pvar_relative_pointers"] {
        if let Some(&ofs) = ptr_map.get(*fixup_name) {
            let mut ofs = ofs as usize;
            let mut entries = Vec::new();
            loop {
                let pvar_idx = r_s32(data, ofs);
                let fixup_off = r_u32(data, ofs + 4);
                ofs += 8;
                if pvar_idx < 0 { break; }
                entries.push(json!({"pvar_index": pvar_idx, "offset": fixup_off}));
            }
            result.insert(fixup_name.to_string(), json!(entries));
        }
    }

    // --- Shapes ---
    for shape_name in &["cuboids", "spheres", "cylinders", "pills"] {
        if let Some(&ofs) = ptr_map.get(*shape_name) {
            let ofs = ofs as usize;
            let count = r_s32(data, ofs) as usize;
            let mut shapes = Vec::new();
            for i in 0..count {
                let so = ofs + 0x10 + i * 0x80;
                shapes.push(json!({
                    "matrix": mat4(data, so),
                    "inverse_matrix": mat3x4(data, so + 0x40),
                    "rotation": vec3f(data, so + 0x70),
                    "unused_7c": r_f32(data, so + 0x7c),
                }));
            }
            result.insert(shape_name.to_string(), json!({"count": count, "shapes": shapes}));
        }
    }

    // --- Paths ---
    if let Some(&ofs) = ptr_map.get("paths") {
        result.insert("paths".into(), parse_paths(data, ofs as usize));
    }

    // --- Grind Paths ---
    if let Some(&ofs) = ptr_map.get("grind_paths") {
        result.insert("grind_paths".into(), parse_grind_paths(data, ofs as usize));
    }

    // --- Point Lights ---
    if let Some(&ofs) = ptr_map.get("point_lights") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut lights = Vec::new();
        for i in 0..count {
            let lo = ofs + 0x10 + i * 0x10;
            let vals: Vec<u16> = (0..8).map(|j| r_u16(data, lo + j * 2)).collect();
            lights.push(json!({
                "position_x": vals[0], "position_y": vals[1], "position_z": vals[2],
                "colour_r": vals[3], "colour_g": vals[4], "colour_b": vals[5],
                "radius": vals[6],
            }));
        }
        result.insert("point_lights".into(), json!({"count": count, "lights": lights}));
    }

    // --- Env Transitions ---
    if let Some(&ofs) = ptr_map.get("env_transitions") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut transitions = Vec::new();
        for i in 0..count {
            let to = ofs + 0x10 + i * 0x30;
            transitions.push(json!({
                "unknown_0": vec3f(data, to),
                "unknown_c": vec3f(data, to + 0x0c),
                "unknown_18": vec3f(data, to + 0x18),
                "unknown_24": vec3f(data, to + 0x24),
            }));
        }
        result.insert("env_transitions".into(), json!({"count": count, "transitions": transitions}));
    }

    // --- Cam Coll Grid ---
    if let Some(&ofs) = ptr_map.get("cam_coll_grid") {
        let end = next_offset("cam_coll_grid", data.len());
        result.insert("cam_coll_grid_size".into(), json!(end.saturating_sub(ofs as usize)));
    }

    // --- Env Sample Points ---
    if let Some(&ofs) = ptr_map.get("env_sample_points") {
        let ofs = ofs as usize;
        let count = r_s32(data, ofs) as usize;
        let mut points = Vec::new();
        for i in 0..count {
            let po = ofs + 0x10 + i * 0x20;
            let fog_colour = {
                let r = r_s32(data, po + 0x14);
                let g = r_s32(data, po + 0x18);
                let b = r_s32(data, po + 0x1c);
                json!({"r": r, "g": g, "b": b})
            };
            points.push(json!({
                "hero_light": r_s32(data, po),
                "pos_x": r_s16(data, po + 0x04),
                "pos_y": r_s16(data, po + 0x06),
                "pos_z": r_s16(data, po + 0x08),
                "reverb": r_u16(data, po + 0x0a),
                "fog_near_intensity": r_f32(data, po + 0x0c),
                "fog_far_intensity": r_f32(data, po + 0x10),
                "fog_colour": fog_colour,
            }));
        }
        result.insert("env_sample_points".into(), json!({"count": count, "points": points}));
    }

    // --- Occlusion ---
    if let Some(&ofs) = ptr_map.get("occlusion") {
        let ofs = ofs as usize;
        let tfrag_count = r_s32(data, ofs);
        let tie_count = r_s32(data, ofs + 4);
        let moby_count = r_s32(data, ofs + 8);
        result.insert("occlusion".into(), json!({
            "tfrag_mapping_count": tfrag_count,
            "tie_mapping_count": tie_count,
            "moby_mapping_count": moby_count,
        }));
    }

    // --- Areas ---
    if let Some(&ofs) = ptr_map.get("areas") {
        result.insert("areas".into(), parse_areas(data, ofs as usize));
    }

    Value::Object(result)
}

// ── Parsing helpers ──

fn r_f32(data: &[u8], off: usize) -> f32 {
    if off + 4 <= data.len() {
        f32::from_le_bytes(data[off..off + 4].try_into().unwrap())
    } else { 0.0 }
}

fn r_s16(data: &[u8], off: usize) -> i16 {
    if off + 2 <= data.len() {
        i16::from_le_bytes(data[off..off + 2].try_into().unwrap())
    } else { 0 }
}

fn r_u16(data: &[u8], off: usize) -> u16 {
    if off + 2 <= data.len() {
        u16::from_le_bytes(data[off..off + 2].try_into().unwrap())
    } else { 0 }
}

fn vec3f(data: &[u8], off: usize) -> Value {
    json!({"x": r_f32(data, off), "y": r_f32(data, off + 4), "z": r_f32(data, off + 8)})
}

fn mat4(data: &[u8], off: usize) -> Value {
    let mut m = Vec::with_capacity(16);
    for i in 0..16 {
        m.push(r_f32(data, off + i * 4));
    }
    json!(m)
}

fn mat3x4(data: &[u8], off: usize) -> Value {
    let mut m = Vec::with_capacity(12);
    for i in 0..12 {
        m.push(r_f32(data, off + i * 4));
    }
    json!(m)
}

fn parse_level_settings(data: &[u8], ofs: usize) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("background_colour".into(), json!({"r": r_s32(data, ofs), "g": r_s32(data, ofs + 4), "b": r_s32(data, ofs + 8)}));
    m.insert("fog_colour".into(), json!({"r": r_s32(data, ofs + 0x0c), "g": r_s32(data, ofs + 0x10), "b": r_s32(data, ofs + 0x14)}));
    m.insert("fog_near_distance".into(), json!(r_f32(data, ofs + 0x18)));
    m.insert("fog_far_distance".into(), json!(r_f32(data, ofs + 0x1c)));
    m.insert("fog_near_intensity".into(), json!(r_f32(data, ofs + 0x20)));
    m.insert("fog_far_intensity".into(), json!(r_f32(data, ofs + 0x24)));
    m.insert("death_height".into(), json!(r_f32(data, ofs + 0x28)));
    m.insert("is_spherical_world".into(), json!(r_s32(data, ofs + 0x2c)));
    m.insert("sphere_centre".into(), vec3f(data, ofs + 0x30));
    m.insert("ship_position".into(), vec3f(data, ofs + 0x3c));
    m.insert("ship_rotation_z".into(), json!(r_f32(data, ofs + 0x48)));
    m.insert("ship_path".into(), json!(r_s32(data, ofs + 0x4c)));
    m.insert("ship_camera_cuboid_start".into(), json!(r_s32(data, ofs + 0x50)));
    m.insert("ship_camera_cuboid_end".into(), json!(r_s32(data, ofs + 0x54)));
    // Chunk planes
    let cofs = ofs + 0x5c;
    let chunk_plane_count = r_s32(data, cofs + 0x0c);
    if chunk_plane_count > 0 {
        let mut planes = Vec::new();
        for i in 0..chunk_plane_count as usize {
            let cp = cofs + i * 0x20;
            planes.push(json!({
                "point": vec3f(data, cp),
                "plane_count": r_s32(data, cp + 0x0c),
                "normal": vec3f(data, cp + 0x10),
            }));
        }
        m.insert("chunk_planes".into(), json!(planes));
    }
    Value::Object(m)
}

fn parse_help_messages(data: &[u8], ofs: usize) -> Value {
    let count = r_s32(data, ofs) as usize;
    let _size = r_s32(data, ofs + 4);
    let mut entries = Vec::new();
    let table_ofs = ofs + 8;
    for i in 0..count.min(500) {
        let eo = table_ofs + i * 16;
        let str_off = r_s32(data, eo);
        let eid = r_s16(data, eo + 4);
        let short_id = r_s16(data, eo + 6);
        let third_person_id = r_s16(data, eo + 8);
        let coop_id = r_s16(data, eo + 10);
        let vag = r_s16(data, eo + 12);
        let character = r_s16(data, eo + 14);
        let string = if str_off > 0 {
            let s_off = table_ofs + str_off as usize;
            let end = data[s_off..].iter().position(|&b| b == 0).unwrap_or(data.len() - s_off);
            String::from_utf8_lossy(&data[s_off..s_off + end]).to_string()
        } else { String::new() };
        entries.push(json!({
            "id": eid, "short_id": short_id,
            "third_person_id": third_person_id, "coop_id": coop_id,
            "vag": vag, "character": character,
            "string": string,
        }));
    }
    json!({"count": count, "entries": entries})
}

fn parse_groups(data: &[u8], ofs: usize) -> Value {
    let count = r_s32(data, ofs) as usize;
    let data_size = r_s32(data, ofs + 4) as usize;
    if count == 0 { return json!([]); }
    let mut pointers = Vec::new();
    for i in 0..count {
        pointers.push(r_s32(data, ofs + 0x10 + i * 4));
    }
    let mut data_ofs = ofs + 0x10 + count * 4;
    if data_ofs % 0x10 != 0 {
        data_ofs += 0x10 - (data_ofs % 0x10);
    }
    let mut groups = Vec::new();
    for (i, &ptr) in pointers.iter().enumerate() {
        let mut members = Vec::new();
        if ptr >= 0 {
            let mut member_idx = (ptr as usize) / 2;
            while member_idx * 2 < data_size {
                let member = r_u16(data, data_ofs + member_idx * 2);
                members.push(member & 0x7fff);
                member_idx += 1;
                if member & 0x8000 != 0 { break; }
            }
        }
        groups.push(json!({"id": i, "members": members}));
    }
    json!(groups)
}

fn parse_paths(data: &[u8], ofs: usize) -> Value {
    let spline_count = r_s32(data, ofs) as usize;
    let data_offset = r_s32(data, ofs + 4) as usize;
    let abs_data_offset = ofs + data_offset;
    let mut splines = Vec::new();
    for i in 0..spline_count {
        let rel_off = r_s32(data, ofs + 0x10 + i * 4) as usize;
        let so = abs_data_offset + rel_off;
        let vert_count = r_s32(data, so) as usize;
        let mut verts = Vec::new();
        for j in 0..vert_count {
            let vo = so + 0x10 + j * 16;
            verts.push(json!({
                "x": r_f32(data, vo), "y": r_f32(data, vo + 4),
                "z": r_f32(data, vo + 8), "w": r_f32(data, vo + 12)
            }));
        }
        splines.push(json!({"vertex_count": vert_count, "vertices": verts}));
    }
    json!({"count": spline_count, "splines": splines})
}

fn parse_grind_paths(data: &[u8], ofs: usize) -> Value {
    let spline_count = r_s32(data, ofs) as usize;
    let data_offset = r_s32(data, ofs + 4) as usize;
    let mut grind_data = Vec::new();
    for i in 0..spline_count {
        let go = ofs + 0x10 + i * 0x20;
        grind_data.push(json!({
            "bounding_sphere": {
                "x": r_f32(data, go), "y": r_f32(data, go + 4),
                "z": r_f32(data, go + 8), "w": r_f32(data, go + 12)
            },
            "unknown_4": r_s32(data, go + 0x10),
            "wrap": r_s32(data, go + 0x14),
            "inactive": r_s32(data, go + 0x18),
        }));
    }
    let spline_offset_ofs = ofs + 0x10 + spline_count * 0x20;
    let abs_data_offset = ofs + data_offset;
    let mut splines = Vec::new();
    for i in 0..spline_count {
        let rel_off = r_s32(data, spline_offset_ofs + i * 4) as usize;
        let so = abs_data_offset + rel_off;
        let vert_count = r_s32(data, so) as usize;
        let mut verts = Vec::new();
        for j in 0..vert_count {
            let vo = so + 0x10 + j * 16;
            verts.push(json!({
                "x": r_f32(data, vo), "y": r_f32(data, vo + 4),
                "z": r_f32(data, vo + 8), "w": r_f32(data, vo + 12)
            }));
        }
        splines.push(json!({"vertex_count": vert_count, "vertices": verts}));
    }
    // Attach splines to grind data
    for (i, gd) in grind_data.iter_mut().enumerate() {
        if let Some(spl) = gd.as_object_mut() {
            if i < splines.len() {
                spl.insert("spline".into(), splines[i].clone());
            }
        }
    }
    json!({"count": spline_count, "grind_paths": grind_data})
}

fn parse_areas(data: &[u8], ofs: usize) -> Value {
    let _block_size = r_s32(data, ofs);
    let hdr_off = ofs + 4;
    let area_count = r_s32(data, hdr_off) as usize;
    let mut parts = Vec::new();
    for i in 0..5 {
        parts.push(r_s32(data, hdr_off + 4 + i * 4));
    }
    let table_off = hdr_off + 0x20;
    let mut areas = Vec::new();
    for i in 0..area_count {
        let ao = table_off + i * 0x30;
        let mut area = serde_json::Map::new();
        area.insert("bounding_sphere".into(), json!({
            "x": r_f32(data, ao), "y": r_f32(data, ao + 4),
            "z": r_f32(data, ao + 8), "w": r_f32(data, ao + 12)
        }));
        let mut part_counts = Vec::new();
        for j in 0..5 {
            part_counts.push(r_s16(data, ao + 0x10 + j * 2));
        }
        area.insert("part_counts".into(), json!(part_counts));
        area.insert("last_update_time".into(), json!(r_s16(data, ao + 0x1a)));
        let mut rel_offsets = Vec::new();
        for j in 0..5 {
            rel_offsets.push(r_s32(data, ao + 0x1c + j * 4));
        }
        let part_names = ["paths", "cuboids", "spheres", "cylinders", "negative_cuboids"];
        let mut links = serde_json::Map::new();
        for j in 0..5 {
            if part_counts[j] > 0 {
                let link_off = (hdr_off + parts[j] as usize) + rel_offsets[j] as usize;
                let mut link_data = Vec::new();
                for k in 0..part_counts[j] as usize {
                    link_data.push(r_s32(data, link_off + k * 4));
                }
                links.insert(part_names[j].into(), json!(link_data));
            }
        }
        area.insert("links".into(), Value::Object(links));
        areas.push(Value::Object(area));
    }
    json!({"count": area_count, "areas": areas})
}

fn print_summary(result: &Value) {
    if let Some(ls) = result.get("level_settings") {
        println!("    Level settings: death_height={}, ship=({}, {})",
            ls.get("death_height").map_or("?".into(), |v| v.to_string()),
            ls.get("ship_position").and_then(|p| p.get("x")).map_or("?".into(), |v| v.to_string()),
            ls.get("ship_position").and_then(|p| p.get("y")).map_or("?".into(), |v| v.to_string()));
    }
    for name in &["directional_lights", "cameras", "sound_instances"] {
        if let Some(v) = result.get(*name) {
            println!("    {}: {}", name, v.get("count").map_or("?".into(), |c| c.to_string()));
        }
    }
    for name in &["moby_instances", "tie_instances", "shrub_instances"] {
        if let Some(v) = result.get(*name) {
            println!("    {}: {}", name, v.get("count").map_or("?".into(), |c| c.to_string()));
        }
    }
    for name in &["moby_groups", "tie_groups", "shrub_groups"] {
        if let Some(v) = result.get(*name) {
            if let Some(arr) = v.as_array() {
                println!("    {}: {}", name, arr.len());
            }
        }
    }
    for name in &["paths", "grind_paths", "point_lights", "env_transitions", "env_sample_points", "areas"] {
        if let Some(v) = result.get(*name) {
            let c = v.get("count").or_else(|| v.get("spline_count")).map_or("?".into(), |c| c.to_string());
            println!("    {}: {}", name, c);
        }
    }
    if let Some(pt) = result.get("pvar_table") {
        println!("    pvar_table: {} entries", pt.get("count").map_or("?".into(), |c| c.to_string()));
    }
}
