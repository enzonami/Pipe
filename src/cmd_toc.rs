//! TOC Parser - parse ISO Table of Contents

use std::path::Path;
use crate::cli::*;
use crate::common::*;

pub fn run(scripts_dir: &Path, args: &TocArgs) -> Result<(), String> {
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

    let iso_size = iso_data.len();
    let total_sectors = iso_size / (SECTOR_SIZE as usize);
    println!("ISO: {}", iso_path);
    println!("Size: {} bytes ({} sectors)", iso_size, total_sectors);
    println!();

    println!("Reading TOC at LBA {} (offset {:#X})...",
        GC_UYA_DL_TOC_LBA, GC_UYA_DL_TOC_LBA * SECTOR_SIZE);
    let toc_lba = GC_UYA_DL_TOC_LBA * SECTOR_SIZE;
    let toc_size = TOC_MAX_SIZE.min((iso_size - toc_lba as usize) as u32);
    let toc_data = &iso_data[toc_lba as usize..toc_lba as usize + toc_size as usize];

    let first_word = r_u32(toc_data, 0);
    println!("First 4 bytes at TOC LBA: 0x{:08X} ({})", first_word, first_word);
    print_hex(toc_data, 0, 128, "First 128 bytes hex");
    println!();

    // Parse global WAD headers
    println!("=== Global WAD Headers ===");
    let (global_wads, level_offset) = parse_global_wad_headers(&iso_data, toc_data, 0, total_sectors);
    println!("\nFound {} global WAD(s)", global_wads.len());
    println!("Level table starts at TOC offset: {:#X} ({} bytes)", level_offset, level_offset);
    println!();

    // Scan for level table
    println!("=== Scanning for Level Table ===");
    let scan_starts: [i64; 7] = [
        level_offset as i64,
        level_offset as i64 - 4,
        level_offset as i64 + 4,
        level_offset as i64 - 8,
        level_offset as i64 + 8,
        level_offset as i64 - 16,
        level_offset as i64 + 16,
    ];

    for &scan_start in &scan_starts {
        if scan_start < 0 || (scan_start as usize) + 24 > toc_data.len() {
            continue;
        }
        let levels = scan_for_level_table(&iso_data, toc_data, scan_start as usize, total_sectors);
        if !levels.is_empty() {
            let mut levels = levels;
            levels.sort_by_key(|l| l.number);

            println!("\nFound {} level(s) at TOC offset {:#X}:", levels.len(), scan_start);
            for lvl in &levels {
                let ranges_str: Vec<String> = lvl.ranges.iter().enumerate()
                    .map(|(i, r)| format!("R{}: sector={}, size={}sect", i, r.offset, r.size))
                    .collect();
                println!("  Level {:3}: {}", lvl.number, ranges_str.join(" | "));
            }

            println!("\n=== Parsing Level Headers ===");
            let levels = parse_level_headers(&iso_data, levels, total_sectors);
            for lvl in &levels {
                let names = ["AUDIO", "LEVEL", "SCENE"];
                for (i, rng) in lvl.ranges.iter().enumerate() {
                    let hdr_str = rng.header_size.map_or("N/A".into(), |v| format!("{:#X}", v));
                    let sec_str = rng.file_sector.map_or("N/A".into(), |v| format!("{}", v));
                    let id_str = rng.file_id.map_or("N/A".into(), |v| format!("{}", v));
                    println!(
                        "  Level {:3} {:<8} hdr={} file_sec={} id={} (header_at={}, size={})",
                        lvl.number, names[i], hdr_str, sec_str, id_str, rng.offset, rng.size
                    );
                }
            }
            return Ok(());
        }
    }

    println!("No level table found at expected locations.");
    Ok(())
}

fn print_hex(data: &[u8], offset: usize, len: usize, label: &str) {
    print!("{}: ", label);
    for i in 0..len.min(data.len().saturating_sub(offset)) {
        print!("{:02X}", data[offset + i]);
    }
    println!();
}

#[derive(Debug)]
struct GlobalWad {
    name: String,
    wtype: String,
    header_size: i32,
    sector: u32,
    game: String,
    size_sectors: Option<u32>,
}

#[derive(Debug)]
struct LevelRange {
    offset: u32,
    size: u32,
    header_size: Option<i32>,
    file_sector: Option<u32>,
    file_id: Option<u32>,
}

#[derive(Debug)]
struct Level {
    number: u32,
    ranges: Vec<LevelRange>,
}

fn identify_wad(header_bytes: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    if header_bytes.len() < 4 {
        return (None, None, None);
    }
    let hdr_size = r_s32(header_bytes, 0) as u32;

    use std::collections::HashMap;
    let uya: HashMap<u32, (&str, &str)> = [
        (0x0648, ("MPEG", "mpeg")),
        (0x0048, ("MISC", "misc")),
        (0x0bf0, ("BONUS", "bonus")),
        (0x0c30, ("SPACE", "space")),
        (0x0398, ("ARMOR", "armor")),
        (0x2340, ("AUDIO", "audio")),
        (0x03c8, ("GADGET", "gadget")),
        (0x2ab0, ("HUD", "hud")),
    ].iter().cloned().collect();

    if let Some(&(name, wtype)) = uya.get(&hdr_size) {
        return (Some("UYA".into()), Some(name.into()), Some(wtype.into()));
    }

    (None, None, None)
}

fn parse_global_wad_headers(
    iso_data: &[u8],
    toc_data: &[u8],
    start_offset: usize,
    _total_sectors: usize,
) -> (Vec<GlobalWad>, usize) {
    let mut global_wads = Vec::new();
    let mut pos = start_offset;

    while pos + 8 <= toc_data.len() {
        let hdr_size = r_s32(toc_data, pos);
        if hdr_size <= 0 {
            break;
        }

        let (game, name, wtype) = identify_wad(&toc_data[pos..]);
        if game.is_none() {
            break;
        }

        let game = game.unwrap();
        let name = name.unwrap();
        let wtype = wtype.unwrap();

        if pos + hdr_size as usize > toc_data.len() {
            println!("  [WARNING] Truncated global WAD header for {}", name);
            break;
        }

        let sector = r_u32(toc_data, pos + 4);
        let size_sectors = if hdr_size >= 12 {
            Some(r_u32(toc_data, pos + 8))
        } else {
            None
        };

        println!(
            "  Global WAD: {:<8} type={:<8} header_size=0x:{:04} sector={}",
            name, wtype, hdr_size, sector
        );
        if let Some(sz) = size_sectors {
            println!("             size_in_header={}", sz);
        }

        let sector_off = (sector as usize) * (SECTOR_SIZE as usize);
        if sector_off + 12 <= iso_data.len() {
            let sector_data = &iso_data[sector_off..];
            if sector_data.len() >= 4 && &sector_data[..3] == b"WAD" {
                let ver = sector_data[3];
                let data_off = r_u32(sector_data, 4);
                let total_sz = r_u32(sector_data, 8);
                println!("             WAD magic OK  ver={} data_offset={:#X} total_size={}", ver, data_off, total_sz);
            }
        }

        global_wads.push(GlobalWad {
            name,
            wtype,
            header_size: hdr_size,
            sector,
            game,
            size_sectors,
        });

        pos += hdr_size as usize;
    }

    (global_wads, pos)
}

fn scan_for_level_table(
    _iso_data: &[u8],
    toc_data: &[u8],
    start_offset: usize,
    _total_sectors: usize,
) -> Vec<Level> {
    let mut levels = Vec::new();
    let mut pos = start_offset;

    // Scan forward to find first valid entry (6 consecutive non-zero u32s)
    while pos + 24 <= toc_data.len() {
        if (0..6).all(|i| r_u32(toc_data, pos + i * 4) > 0) {
            break;
        }
        pos += 4;
    }

    // Parse level entries at 24-byte intervals
    while pos + 24 <= toc_data.len() && (0..6).all(|i| r_u32(toc_data, pos + i * 4) > 0) {
        let mut ranges = Vec::new();
        for i in 0..3 {
            let off = r_u32(toc_data, pos + i * 8);
            let sz = r_u32(toc_data, pos + i * 8 + 4);
            ranges.push(LevelRange {
                offset: off,
                size: sz,
                header_size: None,
                file_sector: None,
                file_id: None,
            });
        }
        let level_num = levels.len() as u32;
        levels.push(Level { number: level_num, ranges });
        pos += 24;
    }

    levels
}

fn parse_level_headers(
    iso_data: &[u8],
    levels: Vec<Level>,
    _total_sectors: usize,
) -> Vec<Level> {
    levels.into_iter().map(|mut lvl| {
        for rng in &mut lvl.ranges {
            let hdr_off = (rng.offset as usize) * (SECTOR_SIZE as usize);
            if hdr_off + 8 <= iso_data.len() {
                let hdr_sz = r_s32(iso_data, hdr_off);
                let sector = r_u32(iso_data, hdr_off + 4);
                rng.header_size = Some(hdr_sz);
                rng.file_sector = Some(sector);

                if hdr_sz >= 0x0C {
                    rng.file_id = Some(r_u32(iso_data, hdr_off + 8));
                }

                let file_off = (sector as usize) * (SECTOR_SIZE as usize);
                if file_off + 12 <= iso_data.len() {
                    let fdata = &iso_data[file_off..];
                    if fdata.len() >= 4 && &fdata[..3] == b"WAD" {
                        let ver = fdata[3];
                        let data_off = r_u32(fdata, 4);
                        let total_sz = r_u32(fdata, 8);
                        println!(
                            "            -> WAD magic OK ver={} data_off={:#X} total_sz={}",
                            ver, data_off, total_sz
                        );
                    }
                }
            }
        }
        lvl
    }).collect()
}
