/// MISC Global WAD extractor — 6 hardcoded sub-files (2FIP, WAD LZ, PS2D, etc.)
/// Ported from rac_misc_extractor.py
use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::wad;
use std::fs;

const SUB_FILES: &[(u32, u32, &str)] = &[
    (1, 9, "2FIP_texture"),
    (10, 235, "WAD_LZ_data"),
    (245, 83, "PS2D_format"),
    (328, 1071, "structured_data_A"),
    (1399, 190, "structured_data_B"),
    (1589, 784, "WAD_LZ_bundle"),
];

fn scan_wad_blocks(data: &[u8]) -> Vec<(usize, usize, String)> {
    let mut blocks = Vec::new();
    let mut pos = 0;
    while pos < data.len() - 16 {
        if &data[pos..pos + 3] == b"WAD" {
            let total_size = r_u32(data, pos + 3) as usize;
            if total_size >= 16 && pos + total_size <= data.len() {
                let tag_bytes = &data[pos + 7..pos + 16];
                let end = tag_bytes.iter().position(|&b| b == 0).unwrap_or(tag_bytes.len());
                let tag = if end > 0 {
                    String::from_utf8_lossy(&tag_bytes[..end]).to_string()
                } else {
                    hex::encode(tag_bytes)
                };
                blocks.push((pos, total_size, tag));
                pos += 12;
            } else {
                pos += 1;
            }
        } else {
            pos += 1;
        }
    }
    blocks
}

fn examine_2fip(data: &[u8], _sub_dir: &Path) {
    if data.len() >= 4 && &data[0..4] == b"2FIP" {
        let w = r_u32(data, 0x08);
        let h = r_u32(data, 0x0C);
        let tex_id = r_u32(data, 0x04);
        println!("    2FIP Texture: id=0x{:X} ({}), {}x{}, PIF8", tex_id, tex_id, w, h);
    }
}

fn extract_sub_file(raw: &[u8], sec: u32, cnt: u32, name: &str, out_dir: &Path) -> Result<usize, String> {
    let byte_off = sec as usize * 0x800;
    let byte_sz = cnt as usize * 0x800;
    let sub_data = &raw[byte_off..byte_off + byte_sz.min(raw.len() - byte_off)];

    let sub_dir = out_dir.join(name);
    fs::create_dir_all(&sub_dir).map_err(|e| format!("mkdir: {}", e))?;

    // Save raw sub-file
    let raw_path = sub_dir.join(format!("raw_{}.bin", name));
    fs::write(&raw_path, sub_data).map_err(|e| format!("write: {}", e))?;
    println!("  Saved raw: {} ({} bytes)", raw_path.display(), sub_data.len());

    // Detect and handle WAD LZ blocks
    let wad_blocks = scan_wad_blocks(sub_data);
    let mut block_count = 0;
    if !wad_blocks.is_empty() {
        println!("  Found {} WAD LZ blocks", wad_blocks.len());
        for (i, (offset, total_size, _tag)) in wad_blocks.iter().enumerate() {
            let chunk = &sub_data[*offset..*offset + *total_size];
            match wad::decompress_wad(chunk) {
                Ok(dec) => {
                    let wb_name = format!("wad_block_{:03}.bin", i);
                    let wb_path = sub_dir.join(&wb_name);
                    fs::write(&wb_path, &dec).map_err(|e| format!("write: {}", e))?;
                    println!("    Block {}: offset=0x{:X} -> {} bytes", i, offset, dec.len());
                    block_count += 1;

                    // Check for 2FIP in decompressed blocks
                    if dec.len() >= 4 && &dec[0..4] == b"2FIP" {
                        let w = r_u32(&dec, 0x08);
                        let h = r_u32(&dec, 0x0C);
                        let tex_id = r_u32(&dec, 0x04);
                        println!("      -> 2FIP: id=0x{:X} {}x{}", tex_id, w, h);
                    }
                }
                Err(e) => {
                    println!("    Block {}: ERROR - {}", i, e);
                }
            }
        }
    }

    // Post-process based on type
    if name == "2FIP_texture" {
        examine_2fip(sub_data, &sub_dir);
    } else if name == "PS2D_format" {
        if let Some(ps2d_off) = sub_data.windows(4).position(|w| w == b"PS2D") {
            println!("  PS2D signature at offset 0x{:X}", ps2d_off);
            if ps2d_off + 32 <= sub_data.len() {
                println!("  PS2D header: {}", hex::encode(&sub_data[ps2d_off..ps2d_off + 32]));
            }
            let ps2d_path = sub_dir.join("ps2d_data.bin");
            fs::write(&ps2d_path, sub_data).map_err(|e| format!("write: {}", e))?;
        }
    }

    Ok(block_count)
}

pub fn run(_scripts_dir: &Path, _args: &MiscArgs) -> Result<(), String> {
    let wad_path = wad_dir(_scripts_dir).join("GLOBAL").join("MISC.bin");
    if !wad_path.exists() {
        return Err(format!("MISC.bin not found at {}", wad_path.display()));
    }

    let raw = fs::read(&wad_path).map_err(|e| format!("read: {}", e))?;
    let out_dir = unpacked_dir(_scripts_dir).join("GLOBAL").join("MISC");
    fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir: {}", e))?;

    println!("MISC Global WAD Extractor");
    println!("{}", "=".repeat(50));
    println!("File size: {} bytes ({:.2} MB)", raw.len(), raw.len() as f64 / 1048576.0);
    println!("Total sectors: {}", raw.len() / 0x800);
    println!();

    for (i, &(sec, cnt, name)) in SUB_FILES.iter().enumerate() {
        let boff = sec as usize * 0x800;
        let bsz = cnt as usize * 0x800;
        println!("[{}] {}: sectors {}-{} ({} sectors)", i, name, sec, sec + cnt - 1, cnt);
        println!("    offset=0x{:X}-0x{:X} ({} bytes)", boff, boff + bsz, bsz);

        extract_sub_file(&raw, sec, cnt, name, &out_dir)?;
        println!();
    }

    let total_size: usize = SUB_FILES.iter().map(|&(_, cnt, _)| cnt as usize * 0x800).sum();
    println!("Extracted {} sub-files: {} bytes total", SUB_FILES.len(), total_size);
    println!("Output: {}", out_dir.display());
    println!("Done.");

    Ok(())
}
