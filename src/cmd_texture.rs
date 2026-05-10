/// Texture decoder - decode GS textures from unpacked level data to PNG
/// Ported from rac_texture_decoder.py
use std::path::Path;
use crate::cli::*;
use crate::common::*;
use crate::wad;
use image::{RgbaImage, Rgba};

pub fn run(scripts_dir: &Path, args: &TextureArgs) -> Result<(), String> {
    let base = crate::common::unpacked_dir(scripts_dir);
    let out_dir = crate::common::textures_dir(scripts_dir);

    let level_filter = args.level.unwrap_or(-1);
    let category = args.category.as_deref();

    level_dispatch(level_filter, |level_num| {
        process_level(&base, &out_dir, level_num, category)
    })
}

fn process_level(base: &Path, out_dir: &Path, level_num: u32, category: Option<&str>) -> Result<(), String> {
    let lvl = format!("LEVEL{:03}", level_num);
    let data_dir = base.join(&lvl).join("data_wad");

    // Required files
    let ci_path = data_dir.join("core_index.bin");
    let decomp_path = data_dir.join("core_data.bin");
    let gs_path = data_dir.join("gs_ram.bin");

    if !ci_path.exists() || !decomp_path.exists() || !gs_path.exists() {
        return Err(format!("{}: missing core_index.bin, core_data.bin, or gs_ram.bin", lvl));
    }

    let index = std::fs::read(&ci_path)
        .map_err(|e| format!("Cannot read core_index.bin: {}", e))?;
    let decomp = std::fs::read(&decomp_path)
        .map_err(|e| format!("Cannot read core_data.bin: {}", e))?;
    let gs_ram = std::fs::read(&gs_path)
        .map_err(|e| format!("Cannot read gs_ram.bin: {}", e))?;

    // Parse core header fields
    let core_hdr = wad::parse_level_core_header(&index);
    let hdr_map = core_hdr.as_object().ok_or("Header not an object")?;

    let textures_base_offset = get_s32_field(hdr_map, "textures_base_offset") as usize;
    let stash_count = get_s32_field(hdr_map, "moby_gs_stash_count") as usize;

    // Get GS RAM ArrayRange
    let gs_ram_cnt = get_ar_count(hdr_map, "gs_ram") as usize;
    let gs_ram_off = get_ar_offset(hdr_map, "gs_ram") as usize;

    // Read GS RAM entries
    let _gs_entry_sz: usize = 0x10;
    let total_gs_entries = gs_ram_cnt + stash_count;
    let mut gs_entries: Vec<(i32, i32, i32, i32, i32)> = Vec::new(); // psm, w, h, addr, offset
    for i in 0..total_gs_entries {
        let entry_sz = if i < gs_ram_cnt { 0x10 } else { 0x14 };
        let eo = gs_ram_off + i * 0x10; // GS RAM entries are always at 0x10 stride
        if eo + entry_sz > index.len() { break; }
        gs_entries.push((
            r_s32(&index, eo), // psm
            r_s32(&index, eo + 4), // w
            r_s32(&index, eo + 8), // h
            r_s32(&index, eo + 12), // addr
            if entry_sz > 0x10 { r_s32(&index, eo + 16) } else { 0 }, // offset
        ));
    }

    let moby_stash_addr = if stash_count > 0 && gs_ram_cnt < gs_entries.len() {
        gs_entries[gs_ram_cnt].3
    } else {
        -1
    };

    // Categories: (name, offset_field, count_field)
    let categories: [(&str, &str, &str); 6] = [
        ("tfrag", "tfrag_textures", "tfrag_textures"),
        ("moby", "moby_textures", "moby_textures"),
        ("tie", "tie_textures", "tie_textures"),
        ("shrub", "shrub_textures", "shrub_textures"),
        ("part", "part_textures", "part_textures"),
        ("fx", "fx_textures", "fx_textures"),
    ];

    let out_base = out_dir.join(&lvl);
    let mut total_decoded = 0;

    for &(cat_name, _off_field, cnt_field) in &categories {
        let cat_cnt = get_ar_count(hdr_map, cnt_field) as usize;
        let cat_off = get_ar_offset(hdr_map, cnt_field) as usize;
        
        if cat_cnt == 0 || cat_off == 0 { continue; }
        if let Some(filter) = category {
            if cat_name != filter { continue; }
        }

        let cat_out = out_base.join(cat_name);
        std::fs::create_dir_all(&cat_out)
            .map_err(|e| format!("Cannot create dir: {}", e))?;

        for i in 0..cat_cnt {
            let eo = cat_off + i * TEXTURE_ENTRY_SZ;
            if eo + TEXTURE_ENTRY_SZ > index.len() { break; }

            // Read TextureEntry (from rac_texture_decoder.py)
            // struct { i32 data_off; i16 w; i16 h; i16 type; i16 pal; i16 mip; i16 pad; }
            let data_off = r_s32(&index, eo);
            let w = r_s16(&index, eo + 4) as i32;
            let h = r_s16(&index, eo + 6) as i32;
            let typ = r_s16(&index, eo + 8) as i32;
            let pal = r_s16(&index, eo + 10) as i32;
            let _mip = r_s16(&index, eo + 12) as i32;

            if w <= 0 || h <= 0 || w > 1024 || h > 1024 { continue; }

            let pixel_size = (w * h) as usize;

            // Determine data source
            let (src_data, src_base) = if typ == 3 || typ == 1 || typ == 2 {
                (&decomp[..], textures_base_offset)
            } else if typ == 0 {
                if moby_stash_addr >= 0 {
                    (&gs_ram[..], moby_stash_addr as usize)
                } else {
                    continue;
                }
            } else {
                continue;
            };

            let data_start = src_base.wrapping_add(data_off as usize);
            if data_start + pixel_size > src_data.len() { continue; }

            let raw_pixels = &src_data[data_start..data_start + pixel_size];

            // Read palette from GS RAM
            let pal_addr = (if pal > 0 { pal as usize } else { 0 }) * 0x100;
            if pal_addr + 1024 > gs_ram.len() { continue; }
            let pal_raw = &gs_ram[pal_addr..pal_addr + 1024];

            // Parse palette: GS stores 32-bit colors as BGRA in little-endian
            let mut pal_rgba = Vec::with_capacity(256);
            for c in 0..256 {
                let po = c * 4;
                let b = pal_raw[po];
                let g = pal_raw[po + 1];
                let r = pal_raw[po + 2];
                let a = pal_raw[po + 3];
                pal_rgba.push([r, g, b, a]);
            }

            // Apply GS palette bit-swap
            pal_rgba = unswizzle_palette_v2(&pal_rgba);
            // Scale alpha (PS2 alpha 0-128 -> 0-255)
            for c in &mut pal_rgba {
                let a = c[3] as u32;
                c[3] = (a * 2).min(255) as u8;
            }

            // Pixel data is linear row-major (for IDTEX8)
            let pixels = raw_pixels.to_vec();

            // Write PNG
            let fname = format!("{}_{:03}_w{}_h{}.png", cat_name, i, w, h);
            let fpath = cat_out.join(&fname);
            write_png(&fpath, &pixels, &pal_rgba, w as u32, h as u32)?;
            total_decoded += 1;

            if i < 2 {
                println!("    {}: data_off=0x{:X}, pal=0x{:X}, type={}", fname, data_off, pal, typ);
            }
        }
    }

    if total_decoded > 0 {
        println!("  {}: {} textures decoded to {}", lvl, total_decoded, out_base.display());
    }
    Ok(())
}

const TEXTURE_ENTRY_SZ: usize = 0x10;

/// GS palette unswizzle ported from Python's map_palette_index
fn unswizzle_palette_v2(pal: &[[u8; 4]]) -> Vec<[u8; 4]> {
    let n = pal.len().min(256);
    let mut result = pal.to_vec();
    for i in 0..n {
        // swap middle two bits (CSM=1 mode)
        let swap = if ((i & 16) >> 1) != (i & 8) { i ^ 0b00011000 } else { i };
        if swap < n {
            result[swap] = pal[i];
        }
    }
    result
}

fn write_png(path: &Path, pixels: &[u8], pal: &[[u8; 4]], w: u32, h: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            if idx < pixels.len() {
                let pal_idx = pixels[idx] as usize;
                if pal_idx < pal.len() {
                    let c = pal[pal_idx];
                    img.put_pixel(x, y, Rgba([c[0], c[1], c[2], c[3]]));
                }
            }
        }
    }
    img.save(path).map_err(|e| format!("save png: {}", e))?;
    Ok(())
}

fn get_s32_field(map: &serde_json::Map<String, serde_json::Value>, name: &str) -> i32 {
    map.get(name).and_then(|v| v.as_i64()).unwrap_or(0) as i32
}

fn get_ar_count(map: &serde_json::Map<String, serde_json::Value>, name: &str) -> i32 {
    map.get(name)
        .and_then(|v| v.get("count"))
        .and_then(|c| c.as_i64())
        .unwrap_or(0) as i32
}

fn get_ar_offset(map: &serde_json::Map<String, serde_json::Value>, name: &str) -> i32 {
    map.get(name)
        .and_then(|v| v.get("offset"))
        .and_then(|o| o.as_i64())
        .unwrap_or(0) as i32
}
