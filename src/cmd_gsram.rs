/// GS RAM vertex data extraction
/// Ported from rac_gsram_extractor.py
///
/// The GS RAM area (448 entries × 16 bytes, grouped in sets of 4) contains
/// pre-baked geometry from VU1 processing — vertex data ready for the GS.
/// Each group of 4 entries (64 bytes) produces 1 vertex.

use std::path::Path;
use crate::cli::*;
use crate::common::level_data_dir;
use serde_json::json;

pub fn run(scripts_dir: &Path, args: &GsramArgs) -> Result<(), String> {
    let level = args.level.unwrap_or(-1);
    let levels: Vec<i32> = if level < 0 {
        (0..14).collect()
    } else {
        vec![level]
    };

    for &level in &levels {
        process_level(scripts_dir, level)?;
    }
    Ok(())
}

fn process_level(scripts_dir: &Path, level_num: i32) -> Result<(), String> {
    let data_dir = level_data_dir(scripts_dir, level_num as u32);
    let core_data_path = data_dir.join("core_data.bin");
    let core_header_path = data_dir.join("core_header.json");

    if !core_data_path.exists() {
        return Err(format!("LEVEL {:03}: core_data.bin not found", level_num));
    }
    if !core_header_path.exists() {
        return Err(format!("LEVEL {:03}: core_header.json not found", level_num));
    }

    let core_data = std::fs::read(&core_data_path)
        .map_err(|e| format!("Cannot read core_data: {}", e))?;
    let header_str = std::fs::read_to_string(&core_header_path)
        .map_err(|e| format!("Cannot read core_header: {}", e))?;
    let header: serde_json::Value = serde_json::from_str(&header_str)
        .map_err(|e| format!("Cannot parse core_header: {}", e))?;

    let gs_ram = &header["gs_ram"];
    let gs_off = gs_ram["offset"].as_i64().unwrap_or(0) as usize;
    let gs_count = gs_ram["count"].as_i64().unwrap_or(0) as usize;

    if gs_off == 0 || gs_count == 0 {
        println!("  LEVEL {:03}: no GS RAM data", level_num);
        return Ok(());
    }

    let groups = gs_count / 4;
    println!("  LEVEL {:03}: GS RAM at 0x{:x}, {} entries = {} groups", level_num, gs_off, gs_count, groups);

    let mut entries_json = Vec::new();
    for g in 0..groups {
        let base = gs_off + g * 64;
        if base + 64 > core_data.len() {
            break;
        }
        // Dump raw bytes for each of the 4 entries
        let e0_raw = &core_data[base..base + 16];
        let e1_raw = &core_data[base + 16..base + 32];
        let e2_raw = &core_data[base + 32..base + 48];
        let e3_raw = &core_data[base + 48..base + 64];
        entries_json.push(json!({
            "group": g,
            "e0": hex::encode(e0_raw),
            "e1": hex::encode(e1_raw),
            "e2": hex::encode(e2_raw),
            "e3": hex::encode(e3_raw),
        }));
    }

    let out_path = data_dir.join("gsram_entries.json");
    let out_json = json!({
        "level": level_num,
        "offset": gs_off,
        "count": gs_count,
        "groups": groups,
        "entries": entries_json,
    });
    std::fs::write(&out_path, serde_json::to_string_pretty(&out_json).unwrap())
        .map_err(|e| format!("Write error: {}", e))?;
    println!("    {} groups -> {}", groups, out_path.display());

    Ok(())
}
