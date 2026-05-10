//! WAD Repacker — rebuild .WAD files from unpacked data sections
//!
//! Takes the decompressed .bin files in data_wad/, recompresses each
//! with WAD LZ, and rebuilds the level WAD file with correct headers.
//! Scene and audio WADs are preserved unchanged (passthrough).
//!
//! Usage: cargo run -- wad-repack --level 0   (single level)
//!        cargo run -- wad-repack --all         (all levels)

use std::path::{Path, PathBuf};
use crate::cli::*;
use crate::common::*;
use crate::wad;

pub fn run(scripts_dir: &Path, args: &WadRepackArgs) -> Result<(), String> {
    let wad_dir = crate::common::wad_dir(scripts_dir);
    let _unpacked = crate::common::unpacked_dir(scripts_dir);

    let levels: Vec<u32> = if args.all {
        (0..LEVEL_COUNT).collect()
    } else if let Some(lvl) = args.level {
        if lvl >= LEVEL_COUNT as i32 {
            return Err(format!("Level {} out of range (0-{})", lvl, LEVEL_COUNT - 1));
        }
        vec![lvl as u32]
    } else {
        return Err("Specify --level N or --all".into());
    };

    // Output directory for repacked WADs
    let out_dir = scripts_dir.join("extracted").join("repacked");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("Cannot create output dir: {}", e))?;

    for &level_num in &levels {
        let orig_wad = wad_dir.join(format!("LEVEL{:03}_level.wad", level_num));
        let data_wad_dir = level_data_dir(scripts_dir, level_num);
        let level_out_dir = level_dir(scripts_dir, level_num);

        if !orig_wad.exists() {
            println!("  LEVEL {:03}: level WAD not found, skipping", level_num);
            continue;
        }
        if !data_wad_dir.exists() {
            println!("  LEVEL {:03}: data_wad directory not found, skipping", level_num);
            continue;
        }

        let result = repack_level_wad(&orig_wad, &data_wad_dir, &level_out_dir, &out_dir, level_num);
        match result {
            Ok(path) => println!("  LEVEL {:03}: repacked -> {}", level_num, path.display()),
            Err(e) => println!("  LEVEL {:03}: error: {}", level_num, e),
        }
    }

    Ok(())
}

fn repack_level_wad(
    orig_wad: &Path,
    data_wad_dir: &Path,
    _level_out_dir: &Path,
    out_dir: &Path,
    level_num: u32,
) -> Result<PathBuf, String> {
    let raw = std::fs::read(orig_wad)
        .map_err(|e| format!("read {}: {}", orig_wad.display(), e))?;

    // ── Parse original WAD header (96 bytes) ──
    // Fields preserved: header_size, level_number, reverb
    // Fields we update: data_rel_sector, data_size_sectors
    // Fields passthrough: sound_bank, gameplay, occlusion sectors
    if raw.len() < 96 {
        return Err("WAD too small for 96-byte header".into());
    }
    let _header_size = r_s32(&raw, 0); // 96
    let _wad_sector = r_u32(&raw, 4);  // 0
    let level_number = r_s32(&raw, 8); // should match level_num
    let reverb = r_s32(&raw, 12);
    let _orig_data_rel_sector = r_u32(&raw, 0x10);
    let _orig_data_sectors = r_u32(&raw, 0x14);
    let sound_bank_sector = r_u32(&raw, 0x18);
    let sound_bank_sectors = r_u32(&raw, 0x1C);
    let gameplay_sector = r_u32(&raw, 0x20);
    let gameplay_sectors = r_u32(&raw, 0x24);
    let occlusion_sector = r_u32(&raw, 0x28);
    let occlusion_sectors = r_u32(&raw, 0x2C);
    // Padding at 0x30-0x5F is zeros

    println!("    Level {} (reverb={}), sound_bank={}+{}, gameplay={}+{}, occlusion={}+{}",
        level_number, reverb,
        sound_bank_sector, sound_bank_sectors,
        gameplay_sector, gameplay_sectors,
        occlusion_sector, occlusion_sectors);

    // ── Data entries (11 × 8 bytes) ──
    let data_entry_names: [&str; 11] = [
        "overlay", "core_index", "gs_ram", "hud_header",
        "hud_banks_0", "hud_banks_1", "hud_banks_2",
        "hud_banks_3", "hud_banks_4", "core_data",
        "transition_textures",
    ];

    // Read decompressed .bin files, recompress them
    struct DataEntry {
        name: String,
        decompressed: Vec<u8>,
        compressed: Vec<u8>,
        orig_offset: i32,
        orig_size: i32,
    }

    let mut entries: Vec<DataEntry> = Vec::new();
    let mut _has_transition = false;

    for &name in &data_entry_names {
        let name_safe = name.replace('[', "_").replace(']', "");
        let bin_path = data_wad_dir.join(format!("{}.bin", name_safe));

        // transition_textures might not exist (offset=-1)
        let (orig_offset, orig_size) = if let Some((i, _)) = data_entry_names.iter().enumerate()
            .find(|&(_, &n)| n == name)
        {
            let _dh_off = /* data_rel_sector * 0x800 */ 0; // handled below
            let entry_off = _orig_data_rel_sector as usize * SECTOR_SIZE as usize + i * 8;
            if entry_off + 8 <= raw.len() {
                (r_s32(&raw, entry_off), r_s32(&raw, entry_off + 4))
            } else {
                (-1, -1)
            }
        } else {
            (-1, -1)
        };

        // Read decompressed file
        let decompressed = if bin_path.exists() {
            std::fs::read(&bin_path)
                .map_err(|e| format!("read {}: {}", bin_path.display(), e))?
        } else if name == "transition_textures" || (orig_offset == -1 && orig_size <= 0) {
            // transition_textures often has offset=-1 (empty), or file doesn't exist
            if name == "transition_textures" {
                _has_transition = true;
            }
            Vec::new()
        } else {
            return Err(format!("Missing required file: {}", bin_path.display()));
        };

        let compressed = if !decompressed.is_empty() {
            wad::compress_wad_lz(&decompressed)
        } else {
            Vec::new() // empty sections: no compression
        };

        entries.push(DataEntry {
            name: name_safe,
            decompressed,
            compressed,
            orig_offset,
            orig_size,
        });
    }

    // ── Build new data header and sections ──
    // Strategy: place all compressed data sequentially after header + sound_bank,
    // then compute data_rel_sector to point there.
    // We preserve sound_bank, gameplay, occlusion sector references from original.

    // Data header is 88 bytes (11 × 8), padded to sector boundary
    const DATA_HEADER_SIZE: usize = 88;

    // Compute size of original sectors we're preserving (sound_bank alone)
    let sound_bank_end = if sound_bank_sector > 0 && sound_bank_sectors > 0 {
        (sound_bank_sector + sound_bank_sectors) as usize
    } else {
        1
    };

    // The new WAD layout:
    //   Sector 0: 96-byte WAD header
    //   Sectors 1..sound_bank_end: sound_bank data (preserved from original)
    //   Sector sound_bank_end: data header (88 bytes + padding)
    //   After data header: compressed data sections, each padded to 0x800 boundary
    //   Then: gameplay, occlusion (at original sectors, copied from original)
    let data_rel_sector = sound_bank_end as u32;

    // Build data header: [offset, size] pairs (byte offsets from data section start)
    const PADDED: bool = true;

    // Collect compressed data, compute offsets
    #[derive(Clone)]
    struct SectionLayout {
        name: String,
        byte_offset: u32,
        byte_size: u32,
        data: Vec<u8>,
    }

    let mut sections: Vec<SectionLayout> = Vec::new();
    let current_off = DATA_HEADER_SIZE as u32;

    // Pad data header to sector boundary
    let data_header_end = align_up(current_off as usize, SECTOR_SIZE as usize) as u32;

    // First entry in data header describes the transition_textures or overlay
    // Actually: offset and size are relative to "data section start"
    // The data section starts at (data_rel_sector * 0x800)
    // The data header is at the beginning of the data section (padded to 0x800)
    // Entries point to bytes within the data section

    // Build data section: start at data_rel_sector * 0x800
    let _data_start = data_rel_sector as usize * SECTOR_SIZE as usize;
    let mut data_section = vec![0u8; DATA_HEADER_SIZE];
    // Pad to sector boundary after header
    data_section.resize(data_header_end as usize, 0);

    // Now add each compressed section
    let mut entry_offset = data_header_end;
    for entry in &entries {
        if entry.compressed.is_empty() {
            // Empty section: offset=-1, size=0
            continue;
        }
            // Align to 16 bytes (PS2 alignment)
        while entry_offset % 16 != 0 {
            entry_offset += 1;
        }

        sections.push(SectionLayout {
            name: entry.name.clone(),
            byte_offset: entry_offset,
            byte_size: entry.compressed.len() as u32,
            data: entry.compressed.clone(),
        });

        entry_offset += entry.compressed.len() as u32;
    }

    // Pad data section to sector boundary
    let total_data_size = align_up(entry_offset as usize, SECTOR_SIZE as usize) as u32;
    data_section.resize(total_data_size as usize, 0);

    // Place compressed data into data_section
    for sec in &sections {
        let abs_off = sec.byte_offset as usize;
        if abs_off + sec.data.len() <= data_section.len() {
            data_section[abs_off..abs_off + sec.data.len()].copy_from_slice(&sec.data);
        }
    }

    // ── Build data header entries ──
    // Each entry: (i32 byte_offset, i32 byte_size) relative to data section start
    let mut data_header_bytes = vec![0u8; DATA_HEADER_SIZE];
    for (i, entry) in entries.iter().enumerate() {
        let off = i * 8;
        let (rel_off, rel_size) = if entry.compressed.is_empty() {
            // Preserve original if empty (e.g., transition_textures = -1, 0)
            // or set to (-1, 0) for empty
            if entry.name == "transition_textures" {
                (-1i32, 0i32)
            } else {
                (0i32, 0i32)
            }
        } else {
            // Find this entry in sections
            let sec = sections.iter().find(|s| s.name == entry.name)
                .ok_or_else(|| format!("Missing section {}", entry.name))?;
            (sec.byte_offset as i32, sec.byte_size as i32)
        };

        data_header_bytes[off..off + 4].copy_from_slice(&rel_off.to_le_bytes());
        data_header_bytes[off + 4..off + 8].copy_from_slice(&rel_size.to_le_bytes());
    }

    // Copy data header into data_section
    for (i, &b) in data_header_bytes.iter().enumerate() {
        data_section[i] = b;
    }

    // ── Build the final WAD ──
    let mut new_wad = Vec::new();

    // Copy header sector (sector 0) — we rebuild it
    let mut header = vec![0u8; SECTOR_SIZE as usize];

    // Preserve original header values
    header[0..4].copy_from_slice(&96i32.to_le_bytes()); // header_size
    header[4..8].copy_from_slice(&0u32.to_le_bytes());  // wad_sector
    header[8..12].copy_from_slice(&level_number.to_le_bytes());
    header[12..16].copy_from_slice(&reverb.to_le_bytes());
    // data_rel_sector — updated
    header[16..20].copy_from_slice(&data_rel_sector.to_le_bytes());
    // data_size_sectors — updated
    let data_size_sectors = (total_data_size / SECTOR_SIZE) + if total_data_size % SECTOR_SIZE != 0 { 1 } else { 0 };
    header[20..24].copy_from_slice(&data_size_sectors.to_le_bytes());
    // sound_bank — passthrough
    header[24..28].copy_from_slice(&sound_bank_sector.to_le_bytes());
    header[28..32].copy_from_slice(&sound_bank_sectors.to_le_bytes());
    // gameplay — passthrough
    header[32..36].copy_from_slice(&gameplay_sector.to_le_bytes());
    header[36..40].copy_from_slice(&gameplay_sectors.to_le_bytes());
    // occlusion — passthrough
    header[40..44].copy_from_slice(&occlusion_sector.to_le_bytes());
    header[44..48].copy_from_slice(&occlusion_sectors.to_le_bytes());

    new_wad.extend_from_slice(&header);

    // Copy sound_bank data from original
    if sound_bank_sector > 0 && sound_bank_sectors > 0 {
        let sb_start = sound_bank_sector as usize * SECTOR_SIZE as usize;
        let sb_size = sound_bank_sectors as usize * SECTOR_SIZE as usize;
        if sb_start + sb_size <= raw.len() {
            new_wad.extend_from_slice(&raw[sb_start..sb_start + sb_size]);
        } else {
            // Pad with zeros if original is truncated
            let available = raw.len().saturating_sub(sb_start);
            new_wad.extend_from_slice(&raw[sb_start..sb_start + available]);
            new_wad.resize(sb_start + sb_size, 0);
        }
    }

    // Copy data section (header + compressed data)
    new_wad.extend_from_slice(&data_section);

    // Copy gameplay from original
    if gameplay_sector > 0 && gameplay_sectors > 0 {
        let gp_start = gameplay_sector as usize * SECTOR_SIZE as usize;
        let gp_size = gameplay_sectors as usize * SECTOR_SIZE as usize;
        if gp_start + gp_size <= raw.len() {
            new_wad.extend_from_slice(&raw[gp_start..gp_start + gp_size]);
        } else {
            let available = raw.len().saturating_sub(gp_start);
            new_wad.extend_from_slice(&raw[gp_start..gp_start + available]);
            new_wad.resize(new_wad.len() + (gp_size - available), 0);
        }
    }

    // Copy occlusion from original
    if occlusion_sector > 0 && occlusion_sectors > 0 {
        let oc_start = occlusion_sector as usize * SECTOR_SIZE as usize;
        let oc_size = occlusion_sectors as usize * SECTOR_SIZE as usize;
        if oc_start + oc_size <= raw.len() {
            new_wad.extend_from_slice(&raw[oc_start..oc_start + oc_size]);
        } else {
            let available = raw.len().saturating_sub(oc_start);
            new_wad.extend_from_slice(&raw[oc_start..oc_start + available]);
            new_wad.resize(new_wad.len() + (oc_size - available), 0);
        }
    }

    // Pad final WAD to sector boundary
    let final_pad = new_wad.len() % SECTOR_SIZE as usize;
    if final_pad != 0 {
        new_wad.resize(new_wad.len() + (SECTOR_SIZE as usize - final_pad), 0);
    }

    // Write output
    let out_path = out_dir.join(format!("LEVEL{:03}_level.wad", level_num));
    std::fs::write(&out_path, &new_wad)
        .map_err(|e| format!("write {}: {}", out_path.display(), e))?;

    // Also copy scene and audio WADs (passthrough)
    for ext in &["scene", "audio"] {
        let src = orig_wad.parent().unwrap_or(Path::new("."))
            .join(format!("LEVEL{:03}_{}.wad", level_num, ext));
        if src.exists() {
            let dst = out_dir.join(format!("LEVEL{:03}_{}.wad", level_num, ext));
            std::fs::copy(&src, &dst)
                .map_err(|e| format!("copy {}: {}", src.display(), e))?;
        }
    }

    println!("    Repacked: {} -> {} sectors", orig_wad.display(), new_wad.len() / SECTOR_SIZE as usize);
    Ok(out_path)
}

/// Align a value up to the next multiple of alignment
fn align_up(val: usize, align: usize) -> usize {
    (val + align - 1) / align * align
}
