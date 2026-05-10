/// Core extractor - parse core_index.bin into structured JSON
/// Ported from rac_core_extractor.py
use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::wad;
use serde_json::json;

pub fn run(scripts_dir: &Path, args: &CoreArgs) -> Result<(), String> {
    let unpacked = crate::common::unpacked_dir(scripts_dir);
    let out_dir = crate::common::extracted_dir(scripts_dir).join("core");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("Cannot create output dir: {}", e))?;

    let level_filter = args.level.unwrap_or(-1);

    level_dispatch(level_filter, |level_num| {
        process_level(&unpacked, &out_dir, level_num)
    })
}

fn process_level(unpacked: &Path, out_dir: &Path, level_num: u32) -> Result<(), String> {
    let data_dir = unpacked.join(format!("LEVEL{:03}", level_num)).join("data_wad");
    if !data_dir.exists() {
        return Err(format!("Data not found for LEVEL {:03}", level_num));
    }

    let core_index_path = data_dir.join("core_index.bin");
    if !core_index_path.exists() {
        return Err(format!("core_index.bin not found for LEVEL {:03}", level_num));
    }

    let data = std::fs::read(&core_index_path)
        .map_err(|e| format!("Cannot read core_index.bin: {}", e))?;

    // Parse full LevelCoreHeader
    let core_hdr = wad::parse_level_core_header(&data);

    let level_out = out_dir.join(format!("LEVEL{:03}", level_num));
    std::fs::create_dir_all(&level_out)
        .map_err(|e| format!("Cannot create dir: {}", e))?;

    // Save core_header.json
    let json_path = level_out.join("core_header.json");
    let json_str = serde_json::to_string_pretty(&core_hdr)
        .map_err(|e| format!("JSON error: {}", e))?;
    std::fs::write(&json_path, &json_str)
        .map_err(|e| format!("Write error: {}", e))?;
    println!("  LEVEL {:03}: core header written to {}", level_num, json_path.display());

    // Extract class tables
    let class_config: [(&str, &str, u32); 3] = [
        ("moby_classes", "moby_classes", 0x20),
        ("tie_classes", "tie_classes", 0x20),
        ("shrub_classes", "shrub_classes", 0x30),
    ];

    let hdr_map = core_hdr.as_object().ok_or("Header not an object")?;
    for &(hdr_key, out_name, entry_sz) in &class_config {
        if let Some(ar) = hdr_map.get(hdr_key) {
            let count = ar.get("count").and_then(|c| c.as_i64()).unwrap_or(0) as i32;
            let offset = ar.get("offset").and_then(|c| c.as_i64()).unwrap_or(0) as i32;
            if count > 0 && offset >= 0 {
                let entries = wad::extract_class_entries_json(&data, offset, count, entry_sz);
                let tbl = json!({
                    "count": count,
                    "table_offset": offset,
                    "entries": entries,
                });
                let tbl_path = level_out.join(format!("{}.json", out_name));
                let tbl_str = serde_json::to_string_pretty(&tbl)
                    .map_err(|e| format!("JSON error: {}", e))?;
                std::fs::write(&tbl_path, &tbl_str)
                    .map_err(|e| format!("Write error: {}", e))?;
                println!("    {}: {} entries", out_name, count);
            }
        }
    }

    Ok(())
}
