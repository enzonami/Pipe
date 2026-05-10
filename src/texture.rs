/// Texture decoding: GS swizzle, palette handling, PNG output
/// Based on rac_texture_decoder.py

use crate::common::r_u32;
use image::{RgbaImage, Rgba};
use std::path::Path;

/// Map pixel index for RAC4 format — matches wrench's algorithm exactly
pub fn map_pixel_index_rac4(i: u32, width: u32) -> u32 {
    let s = i / (width * 2);
    let r = if (s % 2) == 0 {
        s * 2
    } else {
        (s - 1) * 2 + 1
    };
    let q = (i % (width * 2)) / 32;
    let m = i % 4;
    let n = (i / 4) % 4;
    let o = i % 2;
    let mut p = (i / 16) % 2;
    if ((s / 2) % 2) == 1 {
        p = 1 - p;
    }
    let m = if o == 0 {
        (m + p) % 4
    } else {
        (m + 4 - p) % 4
    };
    let x = n + ((m + q * 4) * 4);
    let y = r + (o * 2);
    (x % width) + (y * width)
}

/// Map palette index for GS palette swizzle (CSM=1 mode — swap bits 3 and 4)
/// Matches wrench-master's map_palette_index and Python's implementation.
pub fn map_palette_index(index: u32) -> u32 {
    // Swap middle two bits: bit 3 (0x08) and bit 4 (0x10)
    if ((index & 16) >> 1) != (index & 8) {
        index ^ 0b00011000
    } else {
        index
    }
}

/// Unswizzle pixels from GS memory (4-bit nibble-packed, PS2 GS swizzle)
pub fn unswizzle_pixels(pixels: &[u8], w: u32, h: u32, palette: &[[u8; 4]]) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let swizzled = map_pixel_index_rac4(y * w + x, w);
            let src_idx = (swizzled / 2) as usize;
            let shift = if swizzled % 2 == 0 { 0 } else { 4 };
            if src_idx < pixels.len() {
                let pal_idx = ((pixels[src_idx] >> shift) & 0x0F) as usize;
                if pal_idx < palette.len() {
                    out[(y * w + x) as usize] = palette[pal_idx];
                }
            }
        }
    }
    out
}

/// Unswizzle palette data
pub fn unswizzle_palette(pal: &[u8]) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; 256];
    for i in 0..256 {
        let map = map_palette_index(i as u32) as usize;
        if map < 256 {
            let off = i * 4;
            if off + 4 <= pal.len() {
                out[map] = [pal[off], pal[off + 1], pal[off + 2], pal[off + 3]];
            }
        }
    }
    out
}

/// Scale PS2 alpha values (0-128) to standard alpha (0-255).
/// Does NOT premultiply RGB — matches Python's multiply_alphas and cmd_texture.rs behavior.
pub fn multiply_alphas(colors: &mut [[u8; 4]]) {
    for c in colors.iter_mut() {
        c[3] = ((c[3] as u32) * 2).min(255) as u8;
    }
}

/// Write a PNG file from pixels and palette
pub fn write_png(path: &Path, pixels: &[u8], pal: &[[u8; 4]], w: u32, h: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }

    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let src_idx = (y * w + x) as usize;
            if src_idx < pixels.len() {
                let pal_idx = pixels[src_idx] as usize;
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

/// Read a texture entry (16 bytes)
pub fn read_texture_entry(data: &[u8], offset: usize) -> (u32, u32, u32, u32, u32, u32, u32, u32) {
    (
        r_u32(data, offset),      // tbp
        r_u32(data, offset + 4),  // tbw
        r_u32(data, offset + 8) & 0x3F,  // psm
        (r_u32(data, offset + 8) >> 26) & 0x0F, // tw
        (r_u32(data, offset + 8) >> 30) & 0x03, // th (low bits)
        0, // tcc placeholder
        0, // mag placeholder
        0, // min placeholder
    )
}

/// Read a GS RAM entry (texture data block)
pub fn read_gs_ram_entry(data: &[u8], offset: usize) -> &[u8] {
    let size = r_u32(data, offset + 4) as usize;
    let start = r_u32(data, offset) as usize;
    if start + size <= data.len() {
        &data[start..start + size]
    } else {
        &[]
    }
}

/// Decode a 4-bit palettized texture
pub fn decode_pal4(pixels: &[u8], w: u32, h: u32, pal: &[[u8; 4]]) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let src_idx = ((y * w + x) / 2) as usize;
            let shift = if x % 2 == 0 { 0 } else { 4 };
            if src_idx < pixels.len() {
                let pal_idx = ((pixels[src_idx] >> shift) & 0x0F) as usize;
                if pal_idx < pal.len() {
                    out[(y * w + x) as usize] = pal[pal_idx];
                }
            }
        }
    }
    out
}

/// Decode an 8-bit palettized texture
pub fn decode_pal8(pixels: &[u8], w: u32, h: u32, pal: &[[u8; 4]]) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; (w * h) as usize];
    for i in 0..(w * h) as usize {
        if i < pixels.len() {
            let pal_idx = pixels[i] as usize;
            if pal_idx < pal.len() {
                out[i] = pal[pal_idx];
            }
        }
    }
    out
}

/// PS2 GS 8-bit texture unswizzle — uses map_pixel_index_rac4 (GS 8x8 block swizzle)
/// Converts GS-swizzled pixel data to linear row-major order
pub fn unswizzle_pif8(pixels: &[u8], w: u32, h: u32) -> Vec<u8> {
    let area = (w * h) as usize;
    let mut out = vec![0u8; area];
    for i in 0..area {
        let map = map_pixel_index_rac4(i as u32, w) as usize;
        if map < area {
            out[i] = pixels[map];
        }
    }
    out
}

/// Decode a PIF8 (2FIP) texture. Returns (width, height, RGBA_bytes) or None.
pub fn decode_pif8(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if data.len() < 16 { return None; }
    let magic = &data[0..4];
    if magic != b"2FIP" && magic != b"PIF2" { return None; }

    let w = r_u32(data, 8);
    let h = r_u32(data, 12);
    if w == 0 || h == 0 || w > 1024 || h > 1024 { return None; }

    // Standard header is 0x20 bytes based on Wrench source
    // Only try smaller headers if 0x20 doesn't fit (matching Python logic)
    let hdr_size = {
        let mut sz = 0x20;
        let idx_start = sz + 1024;
        if idx_start + (w * h) as usize > data.len() {
            for try_hdr in [0x10, 0x20, 0x30, 0x40] {
                if try_hdr + 1024 + (w * h) as usize <= data.len() {
                    sz = try_hdr;
                    break;
                }
            }
        }
        sz
    };

    let pal_start = hdr_size as usize;
    let idx_start = pal_start + 1024;
    let area = (w * h) as usize;
    if idx_start + area > data.len() { return None; }

    // Read palette (RGBA, PS2 format: alpha 0-128)
    let mut palette = Vec::with_capacity(256);
    for i in 0..256 {
        let pofs = pal_start + i * 4;
        if pofs + 4 > data.len() { break; }
        let r = data[pofs];
        let g = data[pofs + 1];
        let b = data[pofs + 2];
        let a = data[pofs + 3];
        palette.push([r, g, b, a]);
    }

    // Apply GS palette bit-swap (CSM=1) then scale alpha — matches wrench-master
    let pal_bytes: Vec<u8> = palette.iter().flat_map(|c| c.iter().copied()).collect();
    palette = unswizzle_palette(&pal_bytes);
    multiply_alphas(&mut palette);

    // Read pixel indices — PIF8 pixel data for UYA is linear row-major, not GS-swizzled
    let indices = &data[idx_start..idx_start + area];

    // Map indices through palette to RGBA
    let mut rgba = vec![0u8; area * 4];
    for i in 0..area {
        let idx = indices[i] as usize;
        if idx < palette.len() {
            rgba[i * 4..i * 4 + 4].copy_from_slice(&palette[idx]);
        }
    }

    Some((w, h, rgba))
}

/// Write raw RGBA bytes as PNG
pub fn write_raw_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| "Failed to create image from raw RGBA".to_string())?;
    img.save(path).map_err(|e| format!("save png: {}", e))?;
    Ok(())
}

/// Decode raw RGBA texture with (w,h) header. Returns (width, height, RGBA_bytes) or None.
pub fn decode_raw_rgba(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if data.len() < 8 { return None; }
    let w = r_u32(data, 0);
    let h = r_u32(data, 4);
    if w == 0 || h == 0 || w > 2048 || h > 2048 { return None; }
    let expected = (w as usize) * (h as usize) * 4;
    if data.len() < 8 + expected { return None; }
    Some((w, h, data[8..8 + expected].to_vec()))
}
