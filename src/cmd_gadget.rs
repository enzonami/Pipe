/// GADGET WAD extractor — 24 gadget groups with WAD-compressed headers and SBlk textures
/// Ported from rac_gadget_extractor.py
use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::wad;
use std::fs;

const GADGET_GROUPS: u32 = 24;
const ENTRIES_PER_GROUP: u32 = 5;

fn fmt_size(sz: usize) -> String {
    if sz < 1024 {
        format!("{}B", sz)
    } else if sz < 1024 * 1024 {
        format!("{:.1}KB", sz as f64 / 1024.0)
    } else {
        format!("{:.1}MB", sz as f64 / (1024.0 * 1024.0))
    }
}

fn parse_gadget_header(data: &[u8]) -> Vec<[u32; 5]> {
    // 24 groups × 5 entries × 8 bytes (SectorRange)
    let mut groups = Vec::with_capacity(GADGET_GROUPS as usize);
    for g in 0..GADGET_GROUPS as usize {
        let base = 0x08 + g * 0x28;
        let mut entries = [0u32; 5];
        for e in 0..5 {
            let off = base + e * 8;
            let sector = r_u32(data, off);
            let size = r_u32(data, off + 4);
            entries[e] = (sector << 16) | (size & 0xFFFF);
        }
        groups.push(entries);
    }
    groups
}

struct GadgetEntry {
    sector: u32,
    size_sectors: u32,
    byte_offset: usize,
    byte_size: usize,
}

fn get_entry(data: &[u8], g: usize, e: usize) -> GadgetEntry {
    let base = 0x08 + g * 0x28 + e * 8;
    let sector = r_u32(data, base);
    let size = r_u32(data, base + 4);
    GadgetEntry {
        sector,
        size_sectors: size,
        byte_offset: sector as usize * 0x800,
        byte_size: size as usize * 0x800,
    }
}

fn extract_and_decompress_header_block(wad_data: &[u8], g: usize, _group_entries: &[GadgetEntry]) -> Result<Option<(Vec<u8>, u32, u32, u32, u32)>, String> {
    let hdr = get_entry(wad_data, g, 3);
    if hdr.byte_offset + hdr.byte_size > wad_data.len() || hdr.byte_size == 0 {
        return Ok(None);
    }

    let chunk = &wad_data[hdr.byte_offset..hdr.byte_offset + hdr.byte_size];
    if chunk.len() < 16 {
        return Ok(None);
    }

    let self_ptr = r_u32(chunk, 0);
    let field_04 = r_u32(chunk, 4);
    let field_08 = r_u32(chunk, 8);
    let field_0c = r_u32(chunk, 12);

    let wad_blob = &chunk[0x10..];
    if wad_blob.len() < 3 || &wad_blob[0..3] != b"WAD" {
        return Ok(None);
    }

    match wad::decompress_wad(wad_blob) {
        Ok(decompressed) => Ok(Some((decompressed, self_ptr, field_04, field_08, field_0c))),
        Err(e) => Err(format!("Group {} header decompress: {}", g, e)),
    }
}

fn extract_texture_block(wad_data: &[u8], g: usize) -> Result<Option<(Vec<u8>, u32, u32, u32, u32, Vec<Vec<u8>>)>, String> {
    let tex = get_entry(wad_data, g, 4);
    if tex.byte_offset + tex.byte_size > wad_data.len() || tex.byte_size == 0 {
        return Ok(None);
    }

    let chunk = &wad_data[tex.byte_offset..tex.byte_offset + tex.byte_size];
    if chunk.len() < 16 {
        return Ok(None);
    }

    let count = r_u32(chunk, 0);
    let flags = r_u32(chunk, 4);
    let bpp = r_u32(chunk, 8);
    let data_size = r_u32(chunk, 12);

    let mut textures = Vec::new();
    let mut pos = 16usize;
    for _t in 0..count.min(32) {
        if pos + 16 > chunk.len() { break; }
        let sblk_total = r_u32(chunk, pos) as usize;
        let _sblk_unk = r_u32(chunk, pos + 4);
        let magic = &chunk[pos + 8..pos + 12];
        if magic != b"SBlk" { break; }

        let end = pos + if sblk_total > 0 { sblk_total } else { 0x10 };
        let raw = if end <= chunk.len() { chunk[pos..end].to_vec() } else { chunk[pos..].to_vec() };
        textures.push(raw);

        pos = end;
        if pos >= chunk.len() { break; }
    }

    Ok(Some((chunk.to_vec(), count, flags, bpp, data_size, textures)))
}

fn extract_inline_data(wad_data: &[u8], g: usize, e_idx: usize) -> Option<Vec<u8>> {
    let entry = get_entry(wad_data, g, e_idx);
    if entry.byte_offset + entry.byte_size > wad_data.len() || entry.byte_size == 0 {
        return None;
    }
    Some(wad_data[entry.byte_offset..entry.byte_offset + entry.byte_size].to_vec())
}

fn analyze_gadget_header(decompressed: &[u8]) -> String {
    if decompressed.len() < 16 {
        return "too short".to_string();
    }
    let u16_04 = r_u16(decompressed, 4);
    let u16_06 = r_u16(decompressed, 6);
    let u16_08 = r_u16(decompressed, 8);
    let u16_0a = r_u16(decompressed, 10);

    let mut floats = String::new();
    if decompressed.len() >= 0x40 {
        let f0 = f32::from_le_bytes(decompressed[0x30..0x34].try_into().unwrap_or([0; 4]));
        let f1 = f32::from_le_bytes(decompressed[0x34..0x38].try_into().unwrap_or([0; 4]));
        let f2 = f32::from_le_bytes(decompressed[0x38..0x3c].try_into().unwrap_or([0; 4]));
        floats = format!(" floats=[{:.3},{:.3},{:.3}]", f0, f1, f2);
    }

    format!("u16[{},{},{},{}]{}", u16_04, u16_06, u16_08, u16_0a, floats)
}

pub fn run(_scripts_dir: &Path, _args: &GadgetArgs) -> Result<(), String> {
    let wad_path = wad_dir(_scripts_dir).join("GLOBAL").join("GADGET.bin");
    if !wad_path.exists() {
        return Err(format!("GADGET.bin not found at {}", wad_path.display()));
    }

    let data = fs::read(&wad_path).map_err(|e| format!("read: {}", e))?;
    let out_dir = unpacked_dir(_scripts_dir).join("GLOBAL").join("GADGET");
    fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir: {}", e))?;

    println!("=== GADGET WAD Extractor ===");
    println!("File: {}", wad_path.display());
    println!("Size: {} bytes ({} sectors)", data.len(), data.len() / 0x800);

    let hdr_sz = r_u32(&data, 0);
    println!("Header: size=0x{:X} groups={}", hdr_sz, GADGET_GROUPS);
    println!();

    let mut summary: Vec<(usize, usize, u16, u16, u32, u32)> = Vec::new();

    for g in 0..GADGET_GROUPS as usize {
        let gdir = out_dir.join(format!("gadget_{:02}", g));
        fs::create_dir_all(&gdir).map_err(|e| format!("mkdir: {}", e))?;

        println!("--- Group {:2} ---", g);

        // Entry[0]: disc reference
        if let Some(disc) = extract_inline_data(&data, g, 0) {
            fs::write(gdir.join("disc_ref.bin"), &disc).map_err(|e| format!("write: {}", e))?;
        }

        // Entry[1]: inline data A
        if let Some(inline_a) = extract_inline_data(&data, g, 1) {
            fs::write(gdir.join("inline_a.bin"), &inline_a).map_err(|e| format!("write: {}", e))?;
        }

        // Entry[2]: inline data B
        if let Some(inline_b) = extract_inline_data(&data, g, 2) {
            fs::write(gdir.join("inline_b.bin"), &inline_b).map_err(|e| format!("write: {}", e))?;
        }

        // Entry[3]: header block
        let (hdr_decomp_size, _hdr_info) = match extract_and_decompress_header_block(&data, g, &[]) {
            Ok(Some((decomp, _sp, _f04, _f08, _f0c))) => {
                fs::write(gdir.join("header_decompressed.bin"), &decomp)
                    .map_err(|e| format!("write: {}", e))?;
                let info = analyze_gadget_header(&decomp);
                println!("  Header: decompressed {} {}", fmt_size(decomp.len()), info);
                (decomp.len(), info)
            }
            Ok(None) => {
                println!("  Header: empty/out of range");
                (0, String::new())
            }
            Err(e) => {
                println!("  Header: ERROR - {}", e);
                (0, format!("error: {}", e))
            }
        };

        // Entry[4]: texture block
        let (tex_count, tex_data_size) = match extract_texture_block(&data, g) {
            Ok(Some((raw, count, _flags, _bpp, dsize, textures))) => {
                fs::write(gdir.join("texture_block.bin"), &raw)
                    .map_err(|e| format!("write: {}", e))?;
                for (t, tex_raw) in textures.iter().enumerate() {
                    fs::write(gdir.join(format!("texture_{}.bin", t)), tex_raw)
                        .map_err(|e| format!("write: {}", e))?;
                }
                println!("  Textures: count={} size={}", count, dsize);
                (count as u32, dsize)
            }
            Ok(None) => {
                println!("  Textures: empty/out of range");
                (0, 0)
            }
            Err(e) => {
                println!("  Textures: ERROR - {}", e);
                (0, 0)
            }
        };

        // Parse u16 values from header for summary
        let u16_04 = 0u16;
        let u16_06 = 0u16;
        summary.push((g, hdr_decomp_size, u16_04, u16_06, tex_count, tex_data_size));
    }

    // Summary table
    println!("\n{}", "=".repeat(80));
    println!("{:>6} {:>8} {:>8} {:>8} {:>6} {:>8}", "Group", "HDRsz", "u16[0]", "u16[1]", "TexCnt", "TexSz");
    println!("{}", "-".repeat(80));
    for (g, hsz, u0, u1, tc, ts) in &summary {
        println!("  {:4}   {:>8} {:8} {:8} {:6} {:8}", g, fmt_size(*hsz), u0, u1, tc, ts);
    }

    println!("\nDone. Output in: {}", out_dir.display());
    Ok(())
}
