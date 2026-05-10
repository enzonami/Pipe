//! Audio WAD extraction — extract VAGp audio files
//! Ported from rac_audio_extractor.py
//!
//! Format: UyaLevelAudioWadHeader (0x08 + N × 8-byte SectorByteRange entries)
//! Audio data starts at various sector offsets, all files are VAGp (PS2 ADPCM).

use std::path::Path;
use crate::cli::*;
use std::fs;

pub fn run(scripts_dir: &Path, args: &AudioArgs) -> Result<(), String> {
    let level = args.level.unwrap_or(-1);
    let levels: Vec<i32> = if level < 0 {
        (0..14).collect()
    } else {
        vec![level]
    };

    let wad_dir = scripts_dir.join("extracted").join("WAD");
    let audio_out = scripts_dir.join("extracted").join("audio");
    fs::create_dir_all(&audio_out).map_err(|e| format!("Cannot create audio dir: {}", e))?;

    let mut total_vags = 0;
    let mut total_size: u64 = 0;

    for &level in &levels {
        let wad_path = wad_dir.join(format!("LEVEL{:03}_audio.wad", level));
        if !wad_path.exists() {
            // Try .bin extension
            let wad_path_bin = wad_dir.join(format!("LEVEL{:03}_audio.bin", level));
            if !wad_path_bin.exists() {
                println!("  LEVEL {:03}: AUDIO WAD not found", level);
                continue;
            }
            match process_audio_wad(&wad_path_bin, level, &audio_out) {
                Ok((vc, sz)) => { total_vags += vc; total_size += sz as u64; }
                Err(e) => println!("  LEVEL {:03}: error: {}", level, e),
            }
        } else {
            match process_audio_wad(&wad_path, level, &audio_out) {
                Ok((vc, sz)) => { total_vags += vc; total_size += sz as u64; }
                Err(e) => println!("  LEVEL {:03}: error: {}", level, e),
            }
        }
    }

    println!("Total: {} VAG files, {} bytes", total_vags, total_size);
    Ok(())
}

fn process_audio_wad(wad_path: &Path, level_num: i32, out_dir: &Path) -> Result<(usize, usize), String> {
    let audio = fs::read(wad_path)
        .map_err(|e| format!("Cannot read {}: {}", wad_path.display(), e))?;

    if audio.len() < 8 {
        return Err("Audio WAD too short".into());
    }

    let hdr_size = u32::from_le_bytes(audio[0..4].try_into().unwrap()) as usize;
    if hdr_size < 8 || hdr_size > audio.len() {
        return Err(format!("Invalid audio header size: {}", hdr_size));
    }

    let num_entries = (hdr_size - 8) / 8;

    let mut entries = Vec::new();
    let mut i = 0;
    while i < num_entries {
        let off = 0x08 + i * 8;
        if off + 8 > hdr_size {
            break;
        }
        let sec_off = u32::from_le_bytes(audio[off..off + 4].try_into().unwrap());
        let sz = u32::from_le_bytes(audio[off + 4..off + 8].try_into().unwrap());
        entries.push((sec_off, sz));
        i += 1;
    }

    let lvl_dir = out_dir.join(format!("LEVEL{:03}", level_num));
    fs::create_dir_all(&lvl_dir).map_err(|e| format!("Cannot create dir: {}", e))?;

    let mut vag_count = 0;
    let mut total_size = 0;

    for (idx, &(sec_off, sz)) in entries.iter().enumerate() {
        if sec_off == 0 || sz == 0 {
            continue;
        }
        let byte_off = (sec_off as usize) * 0x800;
        if byte_off + sz as usize > audio.len() {
            continue;
        }

        let chunk = &audio[byte_off..byte_off + sz as usize];

        // Validate VAGp header
        if chunk.len() < 4 || &chunk[0..4] != b"VAGp" {
            continue;
        }

        // Parse VAG header for naming
        let name_bytes = if chunk.len() >= 0x2C {
            let end = chunk[0x1C..0x2C].iter().position(|&b| b == 0).unwrap_or(16);
            String::from_utf8_lossy(&chunk[0x1C..0x1C + end]).to_string()
        } else {
            String::new()
        };
        let name = if name_bytes.is_empty() || name_bytes.trim().is_empty() {
            format!("track_{:03}", idx)
        } else {
            name_bytes.trim().to_string()
        };

        vag_count += 1;
        total_size += chunk.len();

        let fname = lvl_dir.join(format!("{:03}_{}.vag", idx, name));
        fs::write(&fname, chunk)
            .map_err(|e| format!("Cannot write {}: {}", fname.display(), e))?;
    }

    println!("  LEVEL {:03}: {} VAG files, {} bytes", level_num, vag_count, total_size);
    Ok((vag_count, total_size))
}
