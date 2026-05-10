/// BONUS WAD extractor — credits text/images, demo images, cheat/skill/trophy PIF8
/// Ported from rac_bonus_extractor.py
use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::texture::{decode_pif8, decode_raw_rgba, write_raw_png};
use crate::wad;
use std::fs;

fn parse_sec_range(data: &[u8], offset: usize) -> (usize, usize) {
    let sector = r_u32(data, offset) as usize;
    let count = r_u32(data, offset + 4) as usize;
    (sector * 0x800, count * 0x800)
}

fn save_binary(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    fs::write(path, data).map_err(|e| format!("write: {}", e))
}

fn decode_credits_text(data: &[u8], out_dir: &Path) -> Result<usize, String> {
    fs::create_dir_all(out_dir).map_err(|e| format!("mkdir: {}", e))?;
    save_binary(&out_dir.join("raw.bin"), data)?;

    let mut entries: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut pos = 0;
    while pos + 8 <= data.len().min(4096) {
        let text_off = r_u32(data, pos) as usize;
        let text_sz = r_u32(data, pos + 4) as usize;
        if text_off == 0 && text_sz == 0 { break; }
        if text_off > data.len() || text_off + text_sz > data.len() { break; }
        if text_sz > 0x10000 { break; }

        let text_bytes = &data[text_off..text_off + text_sz];
        let mut strings = Vec::new();
        let mut s_start = 0;
        for i in 0..text_bytes.len() {
            if text_bytes[i] == 0 {
                if i > s_start {
                    if let Ok(s) = std::str::from_utf8(&text_bytes[s_start..i]) {
                        strings.push(s.to_string());
                    } else {
                        strings.push(format!("{:?}", &text_bytes[s_start..i]));
                    }
                }
                s_start = i + 1;
            }
        }
        entries.push((text_off, text_sz, strings));
        pos += 8;
    }

    if !entries.is_empty() {
        let mut text = String::new();
        for (i, (off, sz, strings)) in entries.iter().enumerate() {
            text.push_str(&format!("--- Entry {} (offset=0x{:x}, size={}) ---\n", i, off, sz));
            for s in strings {
                if s.chars().all(|c| {
                    let u = c as u32;
                    u == 0x0a || u == 0x0d || u == 0x09 || (0x20..0x7f).contains(&u)
                }) {
                    text.push_str(s);
                    text.push('\n');
                }
            }
            text.push('\n');
        }
        fs::write(out_dir.join("text.txt"), text).map_err(|e| format!("write: {}", e))?;
    }

    Ok(entries.len())
}

fn extract_demo_images(data: &[u8], out_dir: &Path, prefix: &str) -> Result<usize, String> {
    fs::create_dir_all(out_dir).map_err(|e| format!("mkdir: {}", e))?;

    let count = r_u32(data, 0) as usize;
    let count = if count > 1000 { 0 } else { count };

    let mut offsets = Vec::new();
    for i in 0..count {
        let off = r_u32(data, 4 + i * 4) as usize;
        if off == 0xffffffff || off == 0 { break; }
        if off > 0 && off < data.len() {
            offsets.push(off);
        }
    }

    if offsets.is_empty() {
        save_binary(&out_dir.join(format!("{}_raw.bin", prefix)), data)?;
        return Ok(0);
    }

    let mut extracted = 0;
    for (i, &off) in offsets.iter().enumerate() {
        if off >= data.len() { continue; }
        let remaining = data.len() - off;
        if remaining < 16 { continue; }

        let chunk = &data[off..];
        if chunk.len() >= 3 && &chunk[0..3] == b"WAD" {
            match wad::decompress_wad(chunk) {
                Ok(decompressed) if !decompressed.is_empty() => {
                    if let Some((w, h, rgba)) = decode_raw_rgba(&decompressed) {
                        write_raw_png(&out_dir.join(format!("{}_{:03}.png", prefix, i)), w, h, &rgba)?;
                        extracted += 1;
                        continue;
                    }
                    if let Some((w, h, rgba)) = decode_pif8(&decompressed) {
                        write_raw_png(&out_dir.join(format!("{}_{:03}.png", prefix, i)), w, h, &rgba)?;
                        extracted += 1;
                        continue;
                    }
                    save_binary(&out_dir.join(format!("{}_{:03}.bin", prefix, i)), &decompressed)?;
                    extracted += 1;
                }
                _ => {
                    let size = remaining.min(0x100000);
                    save_binary(&out_dir.join(format!("{}_{:03}.wad", prefix, i)), &chunk[..size])?;
                    extracted += 1;
                }
            }
        } else {
            let size = remaining.min(0x100000);
            let sub_data = &chunk[..size];
            if let Some((w, h, rgba)) = decode_raw_rgba(sub_data) {
                write_raw_png(&out_dir.join(format!("{}_{:03}.png", prefix, i)), w, h, &rgba)?;
                extracted += 1;
                continue;
            }
            save_binary(&out_dir.join(format!("{}_{:03}.bin", prefix, i)), sub_data)?;
            extracted += 1;
        }
    }
    Ok(extracted)
}

pub fn run(_scripts_dir: &Path, _args: &BonusArgs) -> Result<(), String> {
    let extracted = extracted_dir(_scripts_dir);
    let bonus_path = wad_dir(_scripts_dir).join("GLOBAL").join("BONUS.bin");
    if !bonus_path.exists() {
        return Err(format!("BONUS WAD not found at {}", bonus_path.display()));
    }

    let data = fs::read(&bonus_path).map_err(|e| format!("read: {}", e))?;
    let bonus_out = extracted.join("bonus");
    fs::create_dir_all(&bonus_out).map_err(|e| format!("mkdir: {}", e))?;
    println!("BONUS WAD: {} bytes ({:.1} MB)", data.len(), data.len() as f64 / 1048576.0);

    let mut results: Vec<(&str, usize)> = Vec::new();

    // ── credits_text[6] ──
    println!("\n=== credits_text ===");
    let text_dir = bonus_out.join("credits_text");
    let mut total_text = 0;
    for i in 0..6 {
        let (boff, bsz) = parse_sec_range(&data, 0x5c0 + i * 8);
        if bsz > 0 {
            let out_dir = text_dir.join(format!("entry_{}", i));
            let n = decode_credits_text(&data[boff..boff + bsz], &out_dir)?;
            total_text += 1;
            println!("  [{}] {} bytes -> {} text entries", i, bsz, n);
        }
    }
    results.push(("credits_text", total_text));

    // ── credits_images[13] ──
    println!("\n=== credits_images (RGBA 512x416) ===");
    let img_dir = bonus_out.join("credits_images");
    let mut total_imgs = 0;
    for i in 0..13 {
        let (boff, bsz) = parse_sec_range(&data, 0x5f0 + i * 8);
        if bsz > 0 {
            if let Some((w, h, rgba)) = decode_raw_rgba(&data[boff..boff + bsz]) {
                let png_path = img_dir.join(format!("credits_{:02}_w{}_h{}.png", i, w, h));
                write_raw_png(&png_path, w, h, &rgba)?;
                total_imgs += 1;
                println!("  [{}] {} bytes -> {}x{} PNG", i, bsz, w, h);
            } else {
                save_binary(&img_dir.join(format!("credits_{:02}.bin", i)), &data[boff..boff + bsz])?;
                total_imgs += 1;
                println!("  [{}] {} bytes -> raw (failed decode)", i, bsz);
            }
        }
    }
    results.push(("credits_images", total_imgs));

    // ── demo_menu[6] ──
    println!("\n=== demo_menu (sub-images) ===");
    let demo_menu_dir = bonus_out.join("demo_menu");
    let mut total_demo = 0;
    for i in 0..6 {
        let (boff, bsz) = parse_sec_range(&data, 0x9f0 + i * 8);
        if bsz > 0 {
            let out_dir = demo_menu_dir.join(format!("menu_{}", i));
            let n = extract_demo_images(&data[boff..boff + bsz], &out_dir, &format!("menu{}", i))?;
            total_demo += n;
            println!("  [{}] {} bytes -> {} sub-images", i, bsz, n);
        }
    }
    results.push(("demo_menu", total_demo));

    // ── demo_exit[6] ──
    println!("\n=== demo_exit (sub-images) ===");
    let demo_exit_dir = bonus_out.join("demo_exit");
    let mut total_exit = 0;
    for i in 0..6 {
        let (boff, bsz) = parse_sec_range(&data, 0xa20 + i * 8);
        if bsz > 0 {
            let out_dir = demo_exit_dir.join(format!("exit_{}", i));
            let n = extract_demo_images(&data[boff..boff + bsz], &out_dir, &format!("exit{}", i))?;
            total_exit += n;
            println!("  [{}] {} bytes -> {} sub-images", i, bsz, n);
        }
    }
    results.push(("demo_exit", total_exit));

    // ── cheat_images[20] ──
    println!("\n=== cheat_images (PIF8) ===");
    let cheat_dir = bonus_out.join("cheat_images");
    let mut total_cheat = 0;
    for i in 0..20 {
        let (boff, bsz) = parse_sec_range(&data, 0xa50 + i * 8);
        if bsz > 0 {
            if let Some((w, h, rgba)) = decode_pif8(&data[boff..boff + bsz]) {
                let png_path = cheat_dir.join(format!("cheat_{:02}_w{}_h{}.png", i, w, h));
                write_raw_png(&png_path, w, h, &rgba)?;
                total_cheat += 1;
                println!("  [{}] {} bytes -> {}x{} PNG", i, bsz, w, h);
            } else {
                save_binary(&cheat_dir.join(format!("cheat_{:02}.bin", i)), &data[boff..boff + bsz])?;
                total_cheat += 1;
                println!("  [{}] {} bytes -> raw (failed decode)", i, bsz);
            }
        }
    }
    results.push(("cheat_images", total_cheat));

    // ── skill_images[31] ──
    println!("\n=== skill_images (PIF8) ===");
    let skill_dir = bonus_out.join("skill_images");
    let mut total_skill = 0;
    for i in 0..31 {
        let (boff, bsz) = parse_sec_range(&data, 0xaf0 + i * 8);
        if bsz > 0 {
            if let Some((w, h, rgba)) = decode_pif8(&data[boff..boff + bsz]) {
                let png_path = skill_dir.join(format!("skill_{:02}_w{}_h{}.png", i, w, h));
                write_raw_png(&png_path, w, h, &rgba)?;
                total_skill += 1;
                println!("  [{}] {} bytes -> {}x{} PNG", i, bsz, w, h);
            } else {
                save_binary(&skill_dir.join(format!("skill_{:02}.bin", i)), &data[boff..boff + bsz])?;
                total_skill += 1;
                println!("  [{}] {} bytes -> raw (failed decode)", i, bsz);
            }
        }
    }
    results.push(("skill_images", total_skill));

    // ── trophy_image ──
    println!("\n=== trophy_image (PIF8) ===");
    let trophy_dir = bonus_out.join("trophy_image");
    let (boff, bsz) = parse_sec_range(&data, 0xbe8);
    let trophy_result;
    if bsz > 0 {
        if let Some((w, h, rgba)) = decode_pif8(&data[boff..boff + bsz]) {
            let png_path = trophy_dir.join(format!("trophy_w{}_h{}.png", w, h));
            write_raw_png(&png_path, w, h, &rgba)?;
            trophy_result = 1;
            println!("  {} bytes -> {}x{} PNG", bsz, w, h);
        } else {
            save_binary(&trophy_dir.join("trophy.bin"), &data[boff..boff + bsz])?;
            trophy_result = 1;
            println!("  {} bytes -> raw (failed decode)", bsz);
        }
    } else {
        trophy_result = 0;
    }
    results.push(("trophy_image", trophy_result));

    // ── Summary ──
    println!("\n=== Summary ===");
    for (k, v) in &results {
        println!("  {}: {}", k, v);
    }
    println!("\nOutput: {}/", bonus_out.display());

    Ok(())
}
