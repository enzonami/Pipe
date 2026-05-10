/// Global WAD analyzer — scans SPACE/HUD/BONUS/MISC WADs for WAD LZ blocks, decompresses, classifies
/// Ported from rac_global_wad_analyzer.py
use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::wad;
use std::fs;

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

fn classify_block(dec: &[u8]) -> &'static str {
    if dec.len() < 4 { return "EMPTY"; }
    if dec.len() >= 2 && dec[0] == 0xd8 && dec[1] == 0x09 { return "SPACE_MESH"; }
    if dec.len() >= 4 && &dec[0..4] == b"\x89PNG" { return "PNG"; }
    if dec.len() >= 4 && &dec[0..4] == b"SBlk" { return "SBLK"; }
    if dec.len() >= 4 && &dec[0..4] == b"TEX\x00" { return "TEX"; }
    if dec.len() >= 4 && &dec[0..4] == b"2FIP" { return "PIF8_TEXTURE"; }
    if dec.len() >= 2 && dec[0] == 0 && dec[1] == 0 { return "ZERO_HEADER"; }
    if dec.len() >= 2 && &dec[0..2] == b"BM" { return "BMP"; }
    if dec.len() >= 4 && &dec[0..4] == b"RIFF" { return "RIFF"; }
    if dec.len() >= 3 && (&dec[0..3] == b"\xff\xf8\xe0" || &dec[0..3] == b"\xff\xf9\xe0") { return "MPEG_AUDIO"; }
    // Check for text content
    let text_end = dec.len().min(64);
    let text = &dec[..text_end];
    if let Ok(s) = std::str::from_utf8(text) {
        if s.chars().any(|c| c.is_ascii_alphabetic()) &&
           s.chars().all(|c| c.is_ascii() && (c.is_ascii_graphic() || c.is_ascii_whitespace())) {
            return "TEXT";
        }
    }
    "DATA"
}

fn analyze_wad(name: &str, filepath: &Path, out_dir: &Path) -> Result<(), String> {
    println!("\n{}", "=".repeat(60));
    println!("  {} WAD Analysis", name);
    println!("{}", "=".repeat(60));

    let raw = fs::read(filepath).map_err(|e| format!("read: {}", e))?;
    println!("File: {}", filepath.display());
    println!("Size: {} bytes ({:.1} MB)", raw.len(), raw.len() as f64 / 1048576.0);

    // Parse minimal header
    let hdr_sz = r_u32(&raw, 0);
    let field_4 = r_u32(&raw, 4);
    let field_8 = r_u32(&raw, 8);
    println!("Header: hdr_size=0x{:X} ({})  field_4={}  field_8={}", hdr_sz, hdr_sz, field_4, field_8);

    // Scan for WAD blocks
    let blocks = scan_wad_blocks(&raw);
    println!("WAD LZ blocks: {}", blocks.len());

    // Decompress and classify
    let wad_out_dir = out_dir.join(name);
    fs::create_dir_all(&wad_out_dir).map_err(|e| format!("mkdir: {}", e))?;

    let mut classified: std::collections::HashMap<String, (usize, usize, usize)> = std::collections::HashMap::new();
    let mut block_details: Vec<serde_json::Value> = Vec::new();
    let mut total_comp = 0usize;
    let mut total_dec = 0usize;
    let mut errors = 0;

    for (i, &(offset, total_size, ref _tag)) in blocks.iter().enumerate() {
        let chunk = &raw[offset..offset + total_size];
        total_comp += total_size;

        match wad::decompress_wad(chunk) {
            Ok(dec) => {
                let fmt = classify_block(&dec);
                total_dec += dec.len();

                let fname = format!("block_{:03}_0x{:08X}.bin", i, offset);
                fs::write(wad_out_dir.join(&fname), &dec).map_err(|e| format!("write: {}", e))?;

                let u32_0 = if dec.len() >= 4 { r_u32(&dec, 0) } else { 0 };
                block_details.push(serde_json::json!({
                    "index": i, "offset": offset, "compressed": total_size,
                    "decompressed": dec.len(), "format": fmt, "u32_0": u32_0,
                }));

                let entry = classified.entry(fmt.to_string()).or_insert((0, 0, 0));
                entry.0 += 1;
                entry.1 += total_size;
                entry.2 += dec.len();
            }
            Err(_) => {
                errors += 1;
                let entry = classified.entry("ERROR".to_string()).or_insert((0, 0, 0));
                entry.0 += 1;
                entry.1 += total_size;
                block_details.push(serde_json::json!({
                    "index": i, "offset": offset, "compressed": total_size, "error": "decompress failed",
                }));
            }
        }
    }

    // Print classification summary
    println!("\nClassification ({} blocks, {} errors):", blocks.len(), errors);
    let mut sorted: Vec<_> = classified.into_iter().collect();
    sorted.sort_by(|a, b| b.1.0.cmp(&a.1.0));
    for (fmt, (count, comp, dec)) in &sorted {
        let ratio = if *comp > 0 {
            format!("{:.1}x", *dec as f64 / *comp as f64)
        } else {
            "-".to_string()
        };
        println!("  {:16}: {:4} blocks  {:>9} -> {:>9} ({})", fmt, count, comp, dec, ratio);
    }

    // Analyze sub-table in header
    if hdr_sz >= 12 {
        println!("\nSub-table (non-zero entries in header):");
        let mut found = 0;
        for i in 0..512 {
            let off = 12 + i * 8;
            if off + 8 > raw.len() { break; }
            let sec = r_u32(&raw, off);
            let cnt = r_u32(&raw, off + 4);
            if sec > 0 {
                println!("  [{:3}] sector={:6} count={:5}  -> bytes 0x{:X}", i, sec, cnt, sec as usize * 0x800);
                found += 1;
                if found >= 50 {
                    println!("  ... ({} total entries in header)", i);
                    break;
                }
            }
        }
    }

    // Save detailed report
    let report = serde_json::json!({
        "name": name,
        "file_size": raw.len(),
        "header": {"hdr_size": hdr_sz, "field_4": field_4, "field_8": field_8},
        "blocks_found": blocks.len(),
        "errors": errors,
        "total_compressed": total_comp,
        "total_decompressed": total_dec,
        "classification": sorted.iter().map(|(fmt, (count, comp, dec))| {
            (fmt.clone(), serde_json::json!({"count": count, "compressed": comp, "decompressed": dec}))
        }).collect::<serde_json::Map<_, _>>(),
        "blocks": block_details,
    });

    let report_path = wad_out_dir.join(format!("{}_analysis.json", name.to_lowercase()));
    let json_str = serde_json::to_string_pretty(&report).map_err(|e| format!("json: {}", e))?;
    fs::write(&report_path, &json_str).map_err(|e| format!("write: {}", e))?;
    println!("\nReport: {}", report_path.display());

    Ok(())
}

pub fn run(_scripts_dir: &Path, _args: &GlobalWadArgs) -> Result<(), String> {
    println!("Global WAD Analyzer");
    println!("==================\n");

    let wad_global_dir = wad_dir(_scripts_dir).join("GLOBAL");
    let out_dir = unpacked_dir(_scripts_dir).join("GLOBAL");

    let wads = [
        ("SPACE", wad_global_dir.join("SPACE.bin")),
        ("HUD", wad_global_dir.join("HUD.bin")),
        ("BONUS", wad_global_dir.join("BONUS.bin")),
        ("MISC", wad_global_dir.join("MISC.bin")),
    ];

    let mut results: Vec<(String, usize, usize, usize, usize, usize)> = Vec::new();

    for &(name, ref path) in &wads {
        if path.exists() {
            analyze_wad(name, path, &out_dir)?;
        } else {
            println!("SKIP: {} not found", path.display());
        }

        // Re-read analysis for summary
        let report_path = out_dir.join(name).join(format!("{}_analysis.json", name.to_lowercase()));
        if report_path.exists() {
            if let Ok(report_str) = fs::read_to_string(&report_path) {
                if let Ok(report) = serde_json::from_str::<serde_json::Value>(&report_str) {
                    let file_size = report["file_size"].as_u64().unwrap_or(0) as usize;
                    let blocks = report["blocks_found"].as_u64().unwrap_or(0) as usize;
                    let errors = report["errors"].as_u64().unwrap_or(0) as usize;
                    let comp = report["total_compressed"].as_u64().unwrap_or(0) as usize;
                    let dec = report["total_decompressed"].as_u64().unwrap_or(0) as usize;
                    results.push((name.to_string(), file_size, blocks, errors, comp, dec));
                }
            }
        }
    }

    // Summary table
    println!("\n{}", "=".repeat(60));
    println!("  Summary");
    println!("{}", "=".repeat(60));
    println!("{:8} {:>10} {:>8} {:>8} {:>12} {:>14}", "WAD", "Size", "Blocks", "Errors", "Compressed", "Decompressed");
    println!("{}", "-".repeat(60));
    for (name, file_size, blocks, errors, comp, dec) in &results {
        println!("{:8} {:>10} {:>8} {:>8} {:>12} {:>14}", name, file_size, blocks, errors, comp, dec);
    }

    println!("\nDone.");
    Ok(())
}
