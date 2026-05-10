/// Texture packing — encode PNG → PIF8 (8-bit palettized, GS-ready format).
/// Matches wrench-master's pack_pif() in texture_asset.cpp exactly.
use std::path::Path;
use crate::cli::*;
use color_quant::NeuQuant;

pub fn run(_scripts_dir: &Path, args: &TexturePackArgs) -> Result<(), String> {
    let input = Path::new(&args.input);
    if !input.exists() {
        return Err(format!("input file not found: {}", args.input));
    }

    let output = if let Some(ref out) = args.output {
        Path::new(out).to_path_buf()
    } else {
        let mut p = input.with_extension("pif8");
        if p == Path::new(&args.input).with_extension("") {
            p = input.join(".pif8");
        }
        p
    };

    // Load PNG
    let img = image::open(input)
        .map_err(|e| format!("failed to open image: {}", e))?
        .into_rgba8();

    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || w > 1024 || h > 1024 {
        return Err(format!("invalid dimensions: {}x{}", w, h));
    }

    let pixels = img.into_raw(); // [R,G,B,A, R,G,B,A, ...]
    let pixel_count = (w * h) as usize;

    // Quantize to 256 colors using NeuQuant
    let nq = NeuQuant::new(10, 256, &pixels);
    let cmap = nq.color_map_rgba(); // 256 * 4 = 1024 bytes

    // Build linear palette (256 RGBA entries)
    let mut palette = vec![[0u8; 4]; 256];
    for i in 0..256 {
        palette[i] = [
            cmap[i * 4],
            cmap[i * 4 + 1],
            cmap[i * 4 + 2],
            cmap[i * 4 + 3],
        ];
    }

    // Refine alpha: for each palette index, average alpha of matching input pixels
    let mut alpha_sum = vec![0u64; 256];
    let mut alpha_count = vec![0u64; 256];
    for i in 0..pixel_count {
        let idx = nq.index_of(&pixels[i * 4..i * 4 + 4]);
        if idx < 256 {
            alpha_sum[idx] += pixels[i * 4 + 3] as u64;
            alpha_count[idx] += 1;
        }
    }
    for i in 0..256 {
        if alpha_count[i] > 0 {
            palette[i][3] = (alpha_sum[i] / alpha_count[i]) as u8;
        }
    }

    // Divide alphas by 2 (PS2 stores alpha 0-128, matching wrench's divide_alphas())
    for i in 0..256 {
        palette[i][3] /= 2;
    }

    // Swizzle palette for file output (GS CSM=1 swap bits 3↔4)
    // Wrench: pack path calls swizzle_palette() which maps linear→file order
    // Our unswizzle_palette: file→linear by out[map(i)] = in[i]
    // So swizzle (linear→file): out[i] = in[map(i)]
    let mut file_pal = vec![[0u8; 4]; 256];
    for j in 0..256 {
        let src = crate::texture::map_palette_index(j as u32) as usize;
        file_pal[j] = palette[src];
    }

    // Map each pixel to nearest palette index
    let mut indices = Vec::with_capacity(pixel_count);
    for i in 0..pixel_count {
        let idx = nq.index_of(&pixels[i * 4..i * 4 + 4]) as u8;
        indices.push(idx);
    }

    // ── Write PIF8 file (matching PifHeader struct exactly) ──
    // struct PifHeader {
    //     0x00: char magic[4];     // "2FIP"
    //     0x04: s32 file_size;
    //     0x08: s32 width;
    //     0x0C: s32 height;
    //     0x10: s32 format;        // 0x13 = pal8, 0x94 = pal4
    //     0x14: s32 clut_format;
    //     0x18: s32 clut_order;
    //     0x1C: s32 mip_levels;
    // };
    // Total header: 0x20 bytes

    let palette_bytes: Vec<u8> = file_pal.iter().flat_map(|c| c.iter().copied()).collect();
    let total_size = 0x20 + palette_bytes.len() + indices.len();

    let mut out = Vec::with_capacity(total_size);

    // Header — write placeholder then patch at end (same as wrench)
    out.extend_from_slice(b"2FIP");              // magic
    out.extend_from_slice(&[0; 4]);              // file_size (placeholder)
    out.extend_from_slice(&(w as i32).to_le_bytes()); // width
    out.extend_from_slice(&(h as i32).to_le_bytes()); // height
    out.extend_from_slice(&(0x13i32).to_le_bytes());  // format = 0x13 (PIF8)
    out.extend_from_slice(&0i32.to_le_bytes());       // clut_format
    out.extend_from_slice(&0i32.to_le_bytes());       // clut_order
    out.extend_from_slice(&1i32.to_le_bytes());       // mip_levels = 1

    // Palette (256 × 4 bytes = 1024 bytes, in file/swizzled order)
    out.extend_from_slice(&palette_bytes);

    // Pixel indices (1 byte per pixel)
    out.extend_from_slice(&indices);

    // Patch file_size at offset 4
    let size = out.len() as i32;
    out[4..8].copy_from_slice(&size.to_le_bytes());

    std::fs::write(&output, &out)
        .map_err(|e| format!("failed to write output: {}", e))?;

    println!("  Packed {}x{} → {} ({} bytes)", w, h, output.display(), out.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texture;

    #[test]
    fn roundtrip_png_pif8_png() {
        // Pack an in-memory test image
        let w = 32u32;
        let h = 32u32;
        // Create a simple checkerboard pattern
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let bright = ((x ^ y) & 8) != 0;
                rgba.push(if bright { 255 } else { 32 }); // R
                rgba.push(if bright { 64 } else { 16 });  // G
                rgba.push(if bright { 128 } else { 48 }); // B
                rgba.push(if bright { 255 } else { 128 }); // A
            }
        }

        // Save as PNG in-memory
        let img = image::RgbaImage::from_raw(w, h, rgba.clone()).unwrap();
        let mut png_bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .expect("write png to memory");

        // Pack to PIF8
        let pixels = img.into_raw();
        let pixel_count = (w * h) as usize;

        let nq = NeuQuant::new(10, 256, &pixels);
        let cmap = nq.color_map_rgba();

        let mut palette = vec![[0u8; 4]; 256];
        for i in 0..256 {
            palette[i] = [cmap[i * 4], cmap[i * 4 + 1], cmap[i * 4 + 2], cmap[i * 4 + 3]];
        }

        // Refine alpha
        let mut alpha_sum = vec![0u64; 256];
        let mut alpha_count = vec![0u64; 256];
        for i in 0..pixel_count {
            let idx = nq.index_of(&pixels[i * 4..i * 4 + 4]);
            if idx < 256 {
                alpha_sum[idx] += pixels[i * 4 + 3] as u64;
                alpha_count[idx] += 1;
            }
        }
        for i in 0..256 {
            if alpha_count[i] > 0 {
                palette[i][3] = (alpha_sum[i] / alpha_count[i]) as u8;
            }
        }
        for i in 0..256 {
            palette[i][3] /= 2;
        }

        // Swizzle palette for file
        let mut file_pal = vec![[0u8; 4]; 256];
        for j in 0..256 {
            let src = crate::texture::map_palette_index(j as u32) as usize;
            file_pal[j] = palette[src];
        }

        // Map pixels
        let mut indices = Vec::with_capacity(pixel_count);
        for i in 0..pixel_count {
            let idx = nq.index_of(&pixels[i * 4..i * 4 + 4]) as u8;
            indices.push(idx);
        }

        // Write PIF8 in-memory
        let palette_bytes: Vec<u8> = file_pal.iter().flat_map(|c| c.iter().copied()).collect();
        let total_size = 0x20 + palette_bytes.len() + indices.len();
        let mut pif8 = Vec::with_capacity(total_size);
        pif8.extend_from_slice(b"2FIP");
        pif8.extend_from_slice(&[0; 4]);
        pif8.extend_from_slice(&(w as i32).to_le_bytes());
        pif8.extend_from_slice(&(h as i32).to_le_bytes());
        pif8.extend_from_slice(&0x13i32.to_le_bytes());
        pif8.extend_from_slice(&0i32.to_le_bytes());
        pif8.extend_from_slice(&0i32.to_le_bytes());
        pif8.extend_from_slice(&1i32.to_le_bytes());
        pif8.extend_from_slice(&palette_bytes);
        pif8.extend_from_slice(&indices);
        let size = pif8.len() as i32;
        pif8[4..8].copy_from_slice(&size.to_le_bytes());

        // Decode PIF8
        let decoded = texture::decode_pif8(&pif8)
            .expect("decode_pif8 should succeed");

        let (dw, dh, decoded_rgba) = decoded;
        assert_eq!(dw, w, "width mismatch");
        assert_eq!(dh, h, "height mismatch");

        // Verify decoded RGBA is valid (non-zero, proper size)
        assert_eq!(decoded_rgba.len(), (w * h * 4) as usize, "decoded data size");

        // Write decoded back to PNG
        let decoded_path = Path::new("/tmp/test_roundtrip_decode.png");
        texture::write_raw_png(decoded_path, w, h, &decoded_rgba)
            .expect("write decoded PNG");

        // Verify image has content (not all transparent/black)
        let has_content = decoded_rgba.chunks(4).any(|p| p[0] > 0 || p[1] > 0 || p[2] > 0);
        assert!(has_content, "decoded image should have visible content");

        println!("  ✓ Round-trip test passed: {}x{} → PIF8 ({} bytes) → decoded PNG", w, h, pif8.len());
    }
}
