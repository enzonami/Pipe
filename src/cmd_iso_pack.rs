//! ISO Packer — build a modified ISO from repacked WAD files
//!
//! Reads the original ISO, replaces WAD data with repacked versions
//! (padding to original size if smaller), updates TOC entries and
//! WAD headers, and writes a new ISO file.
//!
//! Usage: cargo run -- iso-pack <output.iso>

use std::path::{Path, PathBuf};
use crate::cli::*;
use crate::common::*;

pub fn run(scripts_dir: &Path, args: &IsoPackArgs) -> Result<(), String> {
    let iso_path = find_iso(scripts_dir)?;
    let out_path = args.output.clone()
        .unwrap_or_else(|| scripts_dir.join("extracted").join("repacked.iso").to_string_lossy().to_string());
    let repacked_dir = args.input_dir.clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| scripts_dir.join("extracted").join("repacked"));

    if !repacked_dir.exists() {
        return Err(format!("Repacked directory not found: {}", repacked_dir.display()));
    }

    let mut iso = std::fs::read(&iso_path)
        .map_err(|e| format!("read {}: {}", iso_path, e))?;
    let total_sectors = iso.len() / SECTOR_SIZE as usize;
    let toc_off = GC_UYA_DL_TOC_LBA as usize * SECTOR_SIZE as usize;

    println!("ISO: {} ({} sectors)", iso_path, total_sectors);
    println!("Output: {}", out_path);

    // ── Parse TOC ──
    let (level_entries, _level_table_toc_off) = parse_levels_from_toc(&iso, toc_off)?;
    println!("Found {} level entries in TOC", level_entries.len());

    // ── Apply replacements ──
    let mut replaced_count = 0u32;

    for le in &level_entries {
        for &(wad_type, (hdr_sector, tot_size)) in &[
            ("level", le.level_range),
            ("scene", le.scene_range),
            ("audio", le.audio_range),
        ] {
            let repacked_path = repacked_dir.join(format!("LEVEL{:03}_{}.wad", le.number, wad_type));
            if !repacked_path.exists() {
                continue;
            }

            let repacked_data = std::fs::read(&repacked_path)
                .map_err(|e| format!("read {}: {}", repacked_path.display(), e))?;

            // Read the 96-byte WAD header to find the actual data sector
            let hdr_off = hdr_sector as usize * SECTOR_SIZE as usize;
            if hdr_off + 12 > iso.len() {
                println!("    WARNING: truncated header for LEVEL{:03}_{}", le.number, wad_type);
                continue;
            }
            let wad_data_sector = r_u32(&iso, hdr_off + 4);
            let orig_size_sectors = tot_size;
            let wad_data_off = wad_data_sector as usize * SECTOR_SIZE as usize;
            let orig_size_bytes = orig_size_sectors as usize * SECTOR_SIZE as usize;

            if wad_data_off + orig_size_bytes > iso.len() {
                println!("    WARNING: WAD data extends beyond ISO for LEVEL{:03}_{}", le.number, wad_type);
                continue;
            }

            let new_sectors = (repacked_data.len() + SECTOR_SIZE as usize - 1) / SECTOR_SIZE as usize;

            if new_sectors > orig_size_sectors as usize {
                // Repacked WAD is larger — can't in-place replace without shifting
                println!("    LEVEL{:03}_{:<6}: repacked {}sect > orig {}sect, SKIPPING.",
                    le.number, format!("{}.wad", wad_type), new_sectors, orig_size_sectors);
                continue;
            }

            // Pad repacked data to fill the original sector span
            let mut padded = repacked_data;
            padded.resize(orig_size_bytes, 0);

            // Replace in the ISO
            iso[wad_data_off..wad_data_off + orig_size_bytes].copy_from_slice(&padded[..orig_size_bytes]);

            // Update WAD header's size_in_sectors (offset 8)
            // wad_sector stays the same (we write data at the same place)
            iso[hdr_off + 8..hdr_off + 12].copy_from_slice(&(new_sectors as u32).to_le_bytes());

            // Also update the data_rel_sector and data_size_sectors fields in the header
            // if the data section changed. Read these from the repacked WAD header.
            if padded.len() >= 24 {
                let new_data_rel_sector = r_u32(&padded, 0x10);
                let new_data_size_sectors = r_u32(&padded, 0x14);
                iso[hdr_off + 0x10..hdr_off + 0x14].copy_from_slice(&new_data_rel_sector.to_le_bytes());
                iso[hdr_off + 0x14..hdr_off + 0x18].copy_from_slice(&new_data_size_sectors.to_le_bytes());
            }

            // Also update the TOC entry size (at the level table in the TOC)
            // The TOC entry has: [audio_sector, audio_size, level_sector, level_size, scene_sector, scene_size]
            // The size is at offset 12 + 4 for level, etc.
            let toc_entry_off = toc_off + le.toc_offset;
            let size_field_off = match wad_type {
                "audio" => toc_entry_off + 4,
                "level" => toc_entry_off + 12,
                "scene" => toc_entry_off + 20,
                _ => unreachable!(),
            };
            if size_field_off + 4 <= iso.len() {
                iso[size_field_off..size_field_off + 4].copy_from_slice(&(new_sectors as u32).to_le_bytes());
            }

            replaced_count += 1;
            let delta = new_sectors as i64 - orig_size_sectors as i64;
            let sign = if delta >= 0 { "+" } else { "" };
            println!("    LEVEL{:03}_{:<6}: {}sect -> {}sect ({}{})",
                le.number, format!("{}.wad", wad_type), orig_size_sectors, new_sectors, sign, delta);
        }
    }

    if replaced_count == 0 {
        println!("\nNo repacked WADs found in {}", repacked_dir.display());
        println!("Run `cargo run -- wad-repack --all` first.");
        return Ok(());
    }

    // ── Write output ISO ──
    std::fs::write(&out_path, &iso)
        .map_err(|e| format!("write {}: {}", out_path, e))?;

    println!("\nRepacked ISO written to: {} ({} WAD(s) replaced)", out_path, replaced_count);
    println!("  Sectors: {} (unchanged)", total_sectors);

    Ok(())
}

fn find_iso(scripts_dir: &Path) -> Result<String, String> {
    let root = crate::common::project_root(scripts_dir);
    let candidates = [
        root.join("place-ISO-here/Ratchet & Clank - Up Your Arsenal.iso"),
        root.join("Ratchet & Clank - Up Your Arsenal.iso"),
    ];
    for p in &candidates {
        if p.exists() {
            return Ok(p.to_string_lossy().to_string());
        }
    }
    Err("No ISO found. Place 'Ratchet & Clank - Up Your Arsenal.iso' in the project root or place-ISO-here/".into())
}

/// Parse level entries from the TOC (matching cmd_wad_extract.rs logic)
struct LevelEntry {
    number: u32,
    /// Byte offset within TOC where this level's entry starts
    toc_offset: usize,
    audio_range: (u32, u32), // (header_sector, total_sectors)
    level_range: (u32, u32),
    scene_range: (u32, u32),
}

fn parse_levels_from_toc(iso_data: &[u8], toc_off: usize) -> Result<(Vec<LevelEntry>, usize), String> {
    if toc_off + 4 > iso_data.len() {
        return Err("ISO too small for TOC".into());
    }
    let toc_data = &iso_data[toc_off..];

    // Parse global WAD headers to skip past them
    let mut pos = 0usize;
    while pos + 8 <= toc_data.len() {
        let hdr_size = r_s32(toc_data, pos);
        if hdr_size <= 0 { break; }
        if pos + hdr_size as usize > toc_data.len() { break; }
        pos += hdr_size as usize;
    }

    // Scan forward from pos to find 6 consecutive non-zero u32s
    while pos + 24 <= toc_data.len() {
        if (0..6).all(|i| r_u32(toc_data, pos + i * 4) > 0) {
            break;
        }
        pos += 4;
    }

    if pos + 24 > toc_data.len() {
        return Err("No level table found in TOC".into());
    }

    // Parse level entries at 24-byte intervals
    let mut levels = Vec::new();
    while pos + 24 <= toc_data.len() {
        // Check first 3 is a valid range entry (non-zero)
        if (0..3).all(|i| {
            r_u32(toc_data, pos + i * 8) > 0 && r_u32(toc_data, pos + i * 8 + 4) > 0
        }) {
            let hdr_sectors = [
                r_u32(toc_data, pos),
                r_u32(toc_data, pos + 8),
                r_u32(toc_data, pos + 16),
            ];
            let sizes = [
                r_u32(toc_data, pos + 4),
                r_u32(toc_data, pos + 12),
                r_u32(toc_data, pos + 20),
            ];

            // Verify at least the first one has a valid wad_data_sector
            let hdr_off = hdr_sectors[0] as usize * SECTOR_SIZE as usize;
            if hdr_off + 8 > iso_data.len() || r_u32(iso_data, hdr_off + 4) == 0 {
                // First entry is invalid — may have overscanned
                if levels.is_empty() {
                    pos += 4;
                    continue;
                }
                break;
            }

            levels.push(LevelEntry {
                number: levels.len() as u32,
                toc_offset: pos,
                audio_range: (hdr_sectors[0], sizes[0]),
                level_range: (hdr_sectors[1], sizes[1]),
                scene_range: (hdr_sectors[2], sizes[2]),
            });
            pos += 24;
        } else {
            break;
        }
    }

    Ok((levels, pos))
}
