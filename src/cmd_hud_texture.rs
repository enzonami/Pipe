/// HUD texture decoder — reads 2FIP format textures from HUD WAD blocks
/// Ported from rac_hud_texture_decode.py
use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::texture::{write_png, unswizzle_palette, multiply_alphas};
use std::fs;

fn read_2fip_header(data: &[u8]) -> Option<(u32, u32, Vec<u8>, Vec<u8>)> {
    if data.len() < 32 { return None; }
    if &data[0..4] != b"2FIP" { return None; }
    let w = r_u32(data, 8);
    let h = r_u32(data, 0x0C);
    if w == 0 || h == 0 || w > 1024 || h > 1024 { return None; }
    let px_off = 0x20 + 1024; // header(32) + palette(1024)
    let expected = px_off + (w * h) as usize;
    if data.len() < expected { return None; }
    let pal_raw = data[0x20..0x20 + 1024].to_vec();
    let pixels = data[px_off..px_off + (w * h) as usize].to_vec();
    Some((w, h, pal_raw, pixels))
}

fn parse_palette(pal_raw: &[u8]) -> Vec<[u8; 4]> {
    let mut pal = Vec::with_capacity(256);
    for c in 0..256 {
        let off = c * 4;
        if off + 4 <= pal_raw.len() {
            pal.push([pal_raw[off], pal_raw[off + 1], pal_raw[off + 2], pal_raw[off + 3]]);
        }
    }
    let pal_raw: Vec<u8> = pal.iter().flat_map(|c| c.iter().copied()).collect();
    pal = unswizzle_palette(&pal_raw);
    multiply_alphas(&mut pal);
    pal
}

pub fn run(_scripts_dir: &Path, _args: &HudTextureArgs) -> Result<(), String> {
    let hud_dir = unpacked_dir(_scripts_dir).join("GLOBAL").join("HUD");
    let out_dir = extracted_dir(_scripts_dir).join("textures").join("HUD");
    fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir: {}", e))?;

    println!("=== HUD Texture Decode ===\n");

    if !hud_dir.exists() {
        return Err(format!("HUD dir not found at {}", hud_dir.display()));
    }

    let mut entries: Vec<(u32, String)> = Vec::new();
    for entry in fs::read_dir(&hud_dir).map_err(|e| format!("read_dir: {}", e))? {
        let entry = entry.map_err(|e| format!("entry: {}", e))?;
        let fname = entry.file_name().to_string_lossy().to_string();
        if fname.starts_with("block_") && fname.ends_with(".bin") {
            let parts: Vec<&str> = fname.split('_').collect();
            if parts.len() >= 2 {
                if let Ok(idx) = parts[1].parse::<u32>() {
                    entries.push((idx, fname));
                }
            }
        }
    }

    entries.sort_by_key(|e| e.0);
    // Deduplicate by index
    entries.dedup_by_key(|e| e.0);

    let font_atlas_blocks = [226u32, 252];
    let mut total_scanned = 0;
    let mut decoded = 0;
    let mut errors = 0;
    let mut non_2fip = 0;
    let mut font_atlas_decoded = 0;

    for &(idx, ref fname) in &entries {
        let fpath = hud_dir.join(fname);
        let data = fs::read(&fpath).map_err(|e| format!("read {}: {}", fpath.display(), e))?;
        total_scanned += 1;

        // Check for font atlas (tag 0x20) with embedded 2FIP textures
        let tag = r_u32(&data, 0);
        if tag == 0x20 && font_atlas_blocks.contains(&idx) {
            if data.len() < 0x1C { continue; }
            let off_c = r_u32(&data, 0x10) as usize;
            let off_d = r_u32(&data, 0x14) as usize;
            let off_e = r_u32(&data, 0x18) as usize;
            let tex_offsets = [off_c, off_d, off_e];

            for (ti, &to) in tex_offsets.iter().enumerate() {
                if to + 32 > data.len() { continue; }
                let tdata = &data[to..];
                if let Some((w, h, pal_raw, pixels)) = read_2fip_header(tdata) {
                    let pal_rgba = parse_palette(&pal_raw);
                    // Font atlas 2FIP pixel data is also linear row-major
                    let fname_out = format!("hud_{:03}_font_pg{}_{}x{}.png", idx, ti, w, h);
                    let fpath_out = out_dir.join(&fname_out);
                    write_png(&fpath_out, &pixels, &pal_rgba, w, h)?;
                    font_atlas_decoded += 1;
                    println!("  [{:3}] font atlas page {}: {}x{} decoded", idx, ti, w, h);
                }
            }
            continue;
        }

        let hdr = read_2fip_header(&data);
        let hdr = match hdr {
            Some(h) => h,
            None => {
                non_2fip += 1;
                continue;
            }
        };

        let (w, h, pal_raw, pixels) = hdr;

        // Parse palette
        let pal_rgba = parse_palette(&pal_raw);

        // 2FIP pixel data is linear row-major — no GS unswizzle needed
        let indices = &pixels;
        let fname_out = format!("hud_{:03}_{}x{}.png", idx, w, h);
        let fpath_out = out_dir.join(&fname_out);
        match write_png(&fpath_out, &indices, &pal_rgba, w, h) {
            Ok(()) => {
                decoded += 1;
                if idx % 100 == 0 {
                    println!("  [{:3}] {}x{} decoded", idx, w, h);
                }
            }
            Err(e) => {
                println!("  [{:3}] PNG write error: {}", idx, e);
                errors += 1;
            }
        }
    }

    println!("\nSummary:");
    println!("  Total blocks scanned:      {}", total_scanned);
    println!("  2FIP textures decoded:     {}", decoded);
    println!("  Font atlas textures:       {}", font_atlas_decoded);
    println!("  Non-2FIP blocks:           {}", non_2fip);
    println!("  Errors:                    {}", errors);
    println!("\nOutput: {}/", out_dir.display());

    Ok(())
}
