/// ARMOR WAD extractor — armor/wrench/multiplayer meshes and PIF8 textures
/// Ported from rac_armor_extractor.py
use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::texture::{decode_pif8, write_raw_png};
use std::fs;

fn parse_sec_range(data: &[u8], offset: usize) -> (usize, usize) {
    let sector = r_u32(data, offset) as usize;
    let count = r_u32(data, offset + 4) as usize;
    (sector * 0x800, count * 0x800)
}

fn extract_textures(data: &[u8], out_dir: &Path, prefix: &str, tex_boff: usize, tex_bsz: usize) -> Result<usize, String> {
    let tex_data = &data[tex_boff..tex_boff + tex_bsz];
    let tex_dir = out_dir.join("textures");
    fs::create_dir_all(&tex_dir).map_err(|e| format!("mkdir: {}", e))?;

    let mut count = 0;
    let mut idx = 0;
    let mut pos = 0;
    while pos + 16 <= tex_data.len() {
        let magic = &tex_data[pos..pos + 4];
        if magic == b"2FIP" || magic == b"PIF2" {
            if let Some((w, h, rgba)) = decode_pif8(&tex_data[pos..]) {
                let png_path = tex_dir.join(format!("{}_tex_{:02}_w{}_h{}.png", prefix, idx, w, h));
                write_raw_png(&png_path, w, h, &rgba)?;
                count += 1;
                idx += 1;
                let tex_size = 0x20 + 256 * 4 + (w as usize) * (h as usize);
                pos += tex_size;
                continue;
            }
        }
        pos += 16;
    }

    if count == 0 {
        let raw_path = tex_dir.join(format!("{}_textures.bin", prefix));
        fs::write(&raw_path, tex_data).map_err(|e| format!("write: {}", e))?;
    }
    Ok(count)
}

fn save_binary(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    fs::write(path, data).map_err(|e| format!("write: {}", e))
}

fn process_slots(
    data: &[u8],
    slot_base: usize,
    count: usize,
    out_dir: &Path,
    name_fmt: &str,
    prefix_fmt: &str,
) -> Result<(usize, usize), String> {
    let mut total_slots = 0;
    let mut total_tex = 0;
    for i in 0..count {
        let hdr_ofs = slot_base + i * 16;
        let (mesh_boff, mesh_bsz) = parse_sec_range(data, hdr_ofs);
        let (tex_boff, tex_bsz) = parse_sec_range(data, hdr_ofs + 8);

        if mesh_bsz == 0 && tex_bsz == 0 {
            continue;
        }

        let slot_dir = out_dir.join(format!("{}_{:02}", name_fmt, i));
        fs::create_dir_all(&slot_dir).map_err(|e| format!("mkdir: {}", e))?;

        if mesh_bsz > 0 {
            save_binary(&slot_dir.join("mesh.bin"), &data[mesh_boff..mesh_boff + mesh_bsz])?;
        }

        let tex_count = if tex_bsz > 0 {
            extract_textures(data, &slot_dir, &format!("{}{}", prefix_fmt, i), tex_boff, tex_bsz)?
        } else {
            0
        };

        total_slots += 1;
        total_tex += tex_count;
        println!("  [{:2}] mesh={:>7}  tex={:>7} ({} decoded)", i, mesh_bsz, tex_bsz, tex_count);
    }
    Ok((total_slots, total_tex))
}

pub fn run(_scripts_dir: &Path, _args: &ArmorArgs) -> Result<(), String> {
    let armor_path = wad_dir(_scripts_dir).join("GLOBAL").join("ARMOR.bin");
    if !armor_path.exists() {
        return Err(format!("ARMOR WAD not found at {}", armor_path.display()));
    }

    let data = fs::read(&armor_path).map_err(|e| format!("read: {}", e))?;
    let armor_out = extracted_dir(_scripts_dir).join("armor");
    println!("ARMOR WAD: {} bytes ({:.1} MB)", data.len(), data.len() as f64 / 1048576.0);

    let mut results: Vec<(&str, (usize, usize))> = Vec::new();

    // ── armors[29] ──
    println!("\n=== Armors (29 slots) ===");
    let armor_dir = armor_out.join("armors");
    let (ta, tta) = process_slots(&data, 0x008, 29, &armor_dir, "armor", "armor")?;
    results.push(("armors", (ta, tta)));

    // ── wrenches[6] ──
    println!("\n=== Wrenches (6 slots) ===");
    let wrench_dir = armor_out.join("wrenches");
    let (tw, ttw) = process_slots(&data, 0x1d8, 6, &wrench_dir, "wrench", "wrench")?;
    results.push(("wrenches", (tw, ttw)));

    // ── multiplayer_armors[21] ──
    println!("\n=== Multiplayer Armors (21 slots) ===");
    let mp_dir = armor_out.join("multiplayer_armors");
    let (tmp, ttmp) = process_slots(&data, 0x238, 21, &mp_dir, "mp_armor", "mp")?;
    results.push(("multiplayer_armors", (tmp, ttmp)));

    // ── clank_textures[2] ──
    println!("\n=== Clank Textures (2 slots) ===");
    let clank_dir = armor_out.join("clank_textures");
    let mut total_clank = 0;
    for i in 0..2 {
        let (tex_boff, tex_bsz) = parse_sec_range(&data, 0x388 + i * 8);
        if tex_bsz > 0 {
            let tex_out = clank_dir.join(format!("clank_tex_{}", i));
            fs::create_dir_all(&tex_out).map_err(|e| format!("mkdir: {}", e))?;
            let tc = extract_textures(&data, &tex_out, &format!("clank{}", i), tex_boff, tex_bsz)?;
            total_clank += tc;
            println!("  [{}] tex={:>7} ({} decoded)", i, tex_bsz, tc);
        }
    }
    results.push(("clank_textures", (total_clank, 0)));

    // ── Summary ──
    println!("\n=== Summary ===");
    for (k, (slots, tex)) in &results {
        println!("  {}: {} slots, {} textures", k, slots, tex);
    }
    println!("\nOutput: {}/", armor_out.display());

    Ok(())
}
