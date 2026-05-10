//! WAD Extractor - extract WAD files from ISO

use std::path::Path;
use crate::cli::*;
use crate::common::*;

pub fn run(scripts_dir: &Path, args: &WadExtractArgs) -> Result<(), String> {
    let iso_path = args.iso_path.clone().unwrap_or_else(|| {
        let root = project_root(scripts_dir);
        let candidates = [
            root.join("place-ISO-here/Ratchet & Clank - Up Your Arsenal.iso"),
            root.join("Ratchet & Clank - Up Your Arsenal.iso"),
        ];
        for p in &candidates {
            if p.exists() {
                return p.to_string_lossy().to_string();
            }
        }
        candidates[0].to_string_lossy().to_string()
    });

    let iso_data = std::fs::read(&iso_path)
        .map_err(|e| format!("Cannot read ISO '{}': {}", iso_path, e))?;

    let out_dir = wad_dir(scripts_dir);
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("Cannot create output dir: {}", e))?;

    let toc_lba = GC_UYA_DL_TOC_LBA * SECTOR_SIZE;
    let toc_size = TOC_MAX_SIZE.min((iso_data.len() - toc_lba as usize) as u32);
    let toc_data = &iso_data[toc_lba as usize..toc_lba as usize + toc_size as usize];

    let (global_wads, level_offset) = scan_global_headers(toc_data);
    let levels = find_levels(&iso_data, toc_data, level_offset);

    if levels.is_empty() {
        return Err("No level table found in TOC".into());
    }

    // Create GLOBAL directory for global WADs
    let global_dir = out_dir.join("GLOBAL");
    std::fs::create_dir_all(&global_dir)
        .map_err(|e| format!("Cannot create GLOBAL dir: {}", e))?;

    // Compute global WAD sizes by finding the next file start after each WAD
    let mut file_starts: Vec<(usize, u32, &str)> = Vec::new();
    for gw in &global_wads {
        file_starts.push(((gw.sector as usize) * (SECTOR_SIZE as usize), gw.sector, &gw.name));
    }
    for lvl in &levels {
        for rng in &lvl.ranges {
            if let Some(sec) = rng.file_sector {
                file_starts.push(((sec as usize) * (SECTOR_SIZE as usize), sec, ""));
            }
        }
    }
    file_starts.sort_by_key(|&(off, _, _)| off);

    for gw in &global_wads {
        let gw_off = (gw.sector as usize) * (SECTOR_SIZE as usize);
        let next_off = file_starts.iter()
            .find(|&&(off, _, _)| off > gw_off)
            .map(|&(off, _, _)| off)
            .unwrap_or(iso_data.len());
        let file_size = next_off - gw_off;
        let end = (gw_off + file_size).min(iso_data.len());
        let data = &iso_data[gw_off..end];

        let gw_path = global_dir.join(format!("{}.bin", gw.name));
        std::fs::write(&gw_path, data)
            .map_err(|e| format!("Failed to write {}: {}", gw_path.display(), e))?;
        println!("  Global {} -> {}", gw.name, gw_path.display());
    }

    println!("Extracting level WAD files to: {}", out_dir.display());
    let names = ["audio", "level", "scene"];
    for lvl in &levels {
        for (i, name) in names.iter().enumerate() {
            if i >= lvl.ranges.len() { continue; }
            let rng = &lvl.ranges[i];
            let sector = match rng.file_sector { Some(s) => s, None => continue };

            let wad_path = out_dir.join(format!("LEVEL{:03}_{}.wad", lvl.number, name));
            let sector_off = (sector as usize) * (SECTOR_SIZE as usize);
            let wad_size = (rng.size as usize) * (SECTOR_SIZE as usize);
            let end = (sector_off + wad_size).min(iso_data.len());

            let wad_data = &iso_data[sector_off..end];
            std::fs::write(&wad_path, wad_data)
                .map_err(|e| format!("Failed to write {}: {}", wad_path.display(), e))?;
            println!("  Extracted {} -> {}", name, wad_path.display());
        }
    }

    Ok(())
}

struct WadHdr { name: String, sector: u32, hdr_size: i32 }
struct LvlRange { offset: u32, size: u32, file_sector: Option<u32> }
struct LvlEntry { number: u32, ranges: Vec<LvlRange> }

fn scan_global_headers(toc_data: &[u8]) -> (Vec<WadHdr>, usize) {
    // Known global WAD header sizes for UYA
    let known_sizes: [i32; 8] = [0x0648, 0x0048, 0x0BF0, 0x0C30, 0x0398, 0x2340, 0x03C8, 0x2AB0];
    let known_names: [&str; 8] = ["MPEG", "MISC", "BONUS", "SPACE", "ARMOR", "AUDIO", "GADGET", "HUD"];

    let mut wads = Vec::new();
    let mut pos = 0;
    let mut idx = 0;
    while pos + 8 <= toc_data.len() && idx < known_sizes.len() {
        let hdr_size = r_s32(toc_data, pos);
        if hdr_size != known_sizes[idx] { break; }
        let sector = r_u32(toc_data, pos + 4);
        wads.push(WadHdr { name: known_names[idx].to_string(), sector, hdr_size });
        pos += hdr_size as usize;
        idx += 1;
    }
    (wads, pos)
}

fn find_levels(iso_data: &[u8], toc_data: &[u8], start: usize) -> Vec<LvlEntry> {
    let mut levels = Vec::new();
    let mut pos = start;

    // Scan forward to find first valid entry (6 consecutive non-zero u32s)
    while pos + 24 <= toc_data.len() {
        if (0..6).all(|i| r_u32(toc_data, pos + i * 4) > 0) {
            break;
        }
        pos += 4;
    }

    // Parse level entries at 24-byte intervals
    while pos + 24 <= toc_data.len() {
        let all_nonzero = |i: usize| -> bool {
            r_u32(toc_data, pos + i * 8) > 0 && r_u32(toc_data, pos + i * 8 + 4) > 0
        };

        if (0..3).all(|i| all_nonzero(i)) {
            let mut ranges = Vec::new();
            for i in 0..3 {
                let off = r_u32(toc_data, pos + i * 8);
                let sz = r_u32(toc_data, pos + i * 8 + 4);
                let hdr_off = (off as usize) * (SECTOR_SIZE as usize);
                let file_sector = if hdr_off + 8 <= iso_data.len() {
                    Some(r_u32(iso_data, hdr_off + 4))
                } else {
                    None
                };
                ranges.push(LvlRange { offset: off, size: sz, file_sector });
            }
            let num = levels.len() as u32;
            levels.push(LvlEntry { number: num, ranges });
            pos += 24;
        } else {
            break;
        }
    }

    levels
}
