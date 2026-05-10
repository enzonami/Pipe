//! WAD Unpacker - decompress and unpack WAD files
//! Ported from rac_core_extractor.py and rac_wad_unpacker.py
//! Reads from ISO directly to handle absolute sector LBAs correctly.

use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::wad;
use serde_json::json;

pub fn run(scripts_dir: &Path, args: &WadUnpackArgs) -> Result<(), String> {
    let out_dir = crate::common::unpacked_dir(scripts_dir);
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("Cannot create output dir: {}", e))?;

    // Find ISO
    let iso_path = find_iso(scripts_dir)?;
    let iso_data = std::fs::read(&iso_path)
        .map_err(|e| format!("Cannot read ISO '{}': {}", iso_path, e))?;

    let level_filter = args.level.unwrap_or(-1);

    level_dispatch(level_filter, |level_num| {
        unpack_level_from_iso(&iso_data, &out_dir, level_num)
    })
}

fn find_iso(scripts_dir: &Path) -> Result<String, String> {
    let root = project_root(scripts_dir);
    let candidates = [
        root.join("place-ISO-here/Ratchet & Clank - Up Your Arsenal.iso"),
        root.join("place-ISO-here/Ratchet & Clank - Your Arsenal.iso"),
        root.join("Ratchet & Clank - Up Your Arsenal.iso"),
        root.join("Ratchet & Clank - Your Arsenal.iso"),
    ];
    for p in &candidates {
        if p.exists() {
            return Ok(p.to_string_lossy().to_string());
        }
    }
    Err("ISO file not found".into())
}

/// Parse TOC at LBA 1001 to find level WAD entries, then extract level data.
fn unpack_level_from_iso(iso_data: &[u8], out_dir: &Path, level_num: u32) -> Result<(), String> {
    let data_out = out_dir.join(format!("LEVEL{:03}", level_num)).join("data_wad");
    std::fs::create_dir_all(&data_out)
        .map_err(|e| format!("Cannot create dir: {}", e))?;
    let level_out = out_dir.join(format!("LEVEL{:03}", level_num));
    std::fs::create_dir_all(&level_out)
        .map_err(|e| format!("Cannot create dir: {}", e))?;

    // Find the level WAD entry in the TOC
    let level_entry = find_level_wad_entry(iso_data, level_num)?;

    // Read the WAD data from ISO (absolute LBAs - use file_lba from level header)
    let file_lba = level_entry.file_lba;
    let wad_sectors = level_entry.wad_sectors;
    let wad_start = (file_lba as usize) * (SECTOR_SIZE as usize);
    let wad_size = (wad_sectors as usize) * (SECTOR_SIZE as usize);
    if wad_start + wad_size > iso_data.len() {
        return Err(format!("WAD data at LBA {} out of ISO bounds", file_lba));
    }
    let raw = &iso_data[wad_start..wad_start + wad_size];

    println!("Unpacking LEVEL {:03} (WAD at LBA {} + {} sectors)...", level_num, file_lba, wad_sectors);

    // Parse GcUyaLevelWadHeader
    let wad_hdr_size = r_s32(raw, 0);
    let _wad_sector = r_u32(raw, 4);
    let _level_number = r_s32(raw, 8);
    let _reverb = r_s32(raw, 12);

    // Get data range at offset 0x10 (relative sector within WAD)
    let data_rel_sector = r_u32(raw, 0x10);
    let data_size = r_u32(raw, 0x14);
    let data_off = (data_rel_sector as usize) * (SECTOR_SIZE as usize);

    println!("  WAD header size={}, data_rel_sector={}, data_sectors={}",
             wad_hdr_size, data_rel_sector, data_size);

    // Parse GcUyaLevelDataHeader at data_off
    let dh = &raw[data_off..];
    let data_ranges = wad::parse_gc_uya_data_header(dh);

    // Extract each data sub-range
    for &(name, range_off, range_size) in &data_ranges {
        if range_off < 0 || range_size <= 0 {
            continue;
        }
        let abs_off = data_off + range_off as usize;
        let abs_end = abs_off + range_size as usize;
        if abs_end > raw.len() {
            println!("    {}: offset out of bounds, skipping", name);
            continue;
        }
        let chunk = &raw[abs_off..abs_end];
        let name_safe = name.replace('[', "_").replace(']', "");

        // Check if WAD compressed
        let decompressed = if chunk.len() >= 3 && &chunk[..3] == b"WAD" {
            match wad::decompress_wad_lz(chunk) {
                Ok(d) => {
                    println!("    {}: {} bytes -> {} bytes decompressed", name_safe, range_size, d.len());
                    let comp_path = data_out.join(format!("{}_compressed.bin", name_safe));
                    let _ = std::fs::write(&comp_path, chunk);
                    d
                }
                Err(e) => {
                    println!("    {}: decompress error (saving raw): {}", name_safe, e);
                    chunk.to_vec()
                }
            }
        } else {
            println!("    {}: {} bytes (uncompressed)", name_safe, range_size);
            chunk.to_vec()
        };

        let out_path = data_out.join(format!("{}.bin", name_safe));
        std::fs::write(&out_path, &decompressed)
            .map_err(|e| format!("Write error: {}", e))?;
    }

    // Parse core_index
    let core_index_path = data_out.join("core_index.bin");
    if core_index_path.exists() {
        let ci_data = std::fs::read(&core_index_path)
            .map_err(|e| format!("Cannot read core_index: {}", e))?;
        parse_core_index(&ci_data, &data_out)?;
    }

    // Extract gameplay, sound_bank, occlusion from WAD header sector references
    for &(name, wad_off) in &[("gameplay", 0x20usize), ("sound_bank", 0x18), ("occlusion", 0x28)] {
        if wad_off + 8 > raw.len() { continue; }
        let sec = r_u32(raw, wad_off);
        let sz = r_u32(raw, wad_off + 4);
        if sec == 0 || sz == 0 { continue; }

        let byte_off = (sec as usize) * (SECTOR_SIZE as usize);
        let byte_sz = (sz as usize) * (SECTOR_SIZE as usize);
        if byte_off + byte_sz > raw.len() {
            println!("    {}: out of bounds, skipping", name);
            continue;
        }
        let chunk = &raw[byte_off..byte_off + byte_sz];

        let decompressed = if chunk.len() >= 3 && &chunk[..3] == b"WAD" {
            match wad::decompress_wad_lz(chunk) {
                Ok(d) => d,
                Err(_) => chunk.to_vec(),
            }
        } else {
            chunk.to_vec()
        };

        let out_path = level_out.join(format!("{}.bin", name));
        std::fs::write(&out_path, &decompressed)
            .map_err(|e| format!("Write error: {}", e))?;
        println!("    {}: {} sectors -> {} bytes", name, sz, decompressed.len());
    }

    println!("  LEVEL {:03}: done", level_num);
    Ok(())
}

/// Find a level's WAD entry in the TOC and return the file LBA and sector count.
struct LevelWadEntry {
    file_lba: u32,
    wad_sectors: u32,
}

fn find_level_wad_entry(iso_data: &[u8], level_num: u32) -> Result<LevelWadEntry, String> {
    const TOC_LBA: u32 = 1001;
    const TOC_SIZE: usize = 65536;

    let toc_start = (TOC_LBA as usize) * (SECTOR_SIZE as usize);
    if toc_start + 8 > iso_data.len() {
        return Err("TOC out of ISO bounds".into());
    }
    let toc_size = TOC_SIZE.min(iso_data.len() - toc_start);
    let toc = &iso_data[toc_start..toc_start + toc_size];

    // Skip global WAD headers using known sizes
    let known_sizes: [i32; 8] = [0x0648, 0x0048, 0x0BF0, 0x0C30, 0x0398, 0x2340, 0x03C8, 0x2AB0];
    let mut pos = 0;
    let mut header_idx = 0;
    while pos + 8 <= toc.len() && header_idx < known_sizes.len() {
        let hdr_sz = r_s32(toc, pos);
        if hdr_sz != known_sizes[header_idx] { break; }
        pos += hdr_sz as usize;
        header_idx += 1;
    }
    // Slide forward to find the start of the level table:
    // first position where all 6 consecutive u32 values are non-zero.
    while pos + 24 <= toc.len() {
        let mut all_nonzero = true;
        for i in 0..6 {
            if r_u32(toc, pos + i * 4) == 0 {
                all_nonzero = false;
                break;
            }
        }
        if all_nonzero { break; }
        pos += 4;
    }

    // Parse level table: 24-byte entries until a zero field is encountered
    let mut levels_found = 0u32;
    while pos + 24 <= toc.len() {
        // Check all 6 fields are non-zero (simple validation, matches Python)
        let mut all_nonzero = true;
        let mut vals = [0u32; 6];
        for i in 0..6 {
            let v = r_u32(toc, pos + i * 4);
            if v == 0 { all_nonzero = false; break; }
            vals[i] = v;
        }
        if !all_nonzero { break; }

        // vals[0]=audio_hdr, vals[1]=audio_sz, vals[2]=level_hdr, vals[3]=level_sz,
        // vals[4]=scene_hdr, vals[5]=scene_sz

        if levels_found == level_num {
            let hdr_lba = vals[2]; // level header LBA
            let wad_sectors = vals[3]; // level WAD size in sectors

            // Read per-level header at hdr_lba to get actual file LBA
            let hdr_off = (hdr_lba as usize) * (SECTOR_SIZE as usize);
            if hdr_off + 8 > iso_data.len() {
                return Err(format!("Level header LBA {} out of bounds", hdr_lba));
            }
            let file_lba = r_u32(iso_data, hdr_off + 4);
            if file_lba == 0 {
                return Err(format!("Level {} has zero file_lba", level_num));
            }

            return Ok(LevelWadEntry { file_lba, wad_sectors });
        }

        levels_found += 1;
        pos += 24;
    }

    Err(format!("Level {} not found in TOC", level_num))
}

fn parse_core_index(ci_data: &[u8], data_out: &Path) -> Result<(), String> {
    // Parse LevelCoreHeader
    let core_hdr = wad::parse_level_core_header(ci_data);

    // Save core_header.json
    let json_path = data_out.join("core_header.json");
    let json_str = serde_json::to_string_pretty(&core_hdr)
        .map_err(|e| format!("JSON error: {}", e))?;
    std::fs::write(&json_path, &json_str)
        .map_err(|e| format!("Write error: {}", e))?;

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
                let entries = wad::extract_class_entries_json(ci_data, offset, count, entry_sz);
                let tbl = json!({
                    "count": count,
                    "table_offset": offset,
                    "entries": entries,
                });
                let tbl_path = data_out.join(format!("{}.json", out_name));
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
