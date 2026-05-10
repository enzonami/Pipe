/// Standalone WAD LZ decompressor for cross-verification
/// Based on Python's rac_core_extractor.py decompress_wad()
use std::fs;

fn r_u32(data: &[u8], offset: usize) -> u32 {
    data[offset] as u32 | (data[offset+1] as u32) << 8 | (data[offset+2] as u32) << 16 | (data[offset+3] as u32) << 24
}

fn decompress_wad_lz(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 0x10 {
        return Err("Data too small for WAD header".into());
    }
    if &data[0..3] != b"WAD" {
        return Err("Bad WAD LZ magic".into());
    }

    let compressed_size = r_u32(data, 3) as usize;
    let end = compressed_size.min(data.len());
    let mut ptr = 0x10;
    let mut dest = Vec::new();

    while ptr < end {
        let flag_byte = data[ptr];
        ptr += 1;

        if flag_byte < 0x10 {
            let literal_size = if flag_byte != 0 {
                flag_byte as usize + 3
            } else {
                if ptr >= end { return Err("Unexpected EOF in literal".into()); }
                let sz = data[ptr] as usize + 18;
                ptr += 1;
                sz
            };
            if ptr + literal_size > end {
                return Err(format!("Literal exceeds data: {} > {}", ptr + literal_size, end));
            }
            dest.extend_from_slice(&data[ptr..ptr + literal_size]);
            ptr += literal_size;
            if ptr < end && data[ptr] < 0x10 {
                return Err("Double literal".into());
            }
        } else {
            let (lookback_offset, match_size) = if flag_byte < 0x20 {
                let mut match_size = (flag_byte & 7) as usize;
                if match_size == 0 {
                    if ptr >= end { return Err("Unexpected EOF in far match size".into()); }
                    match_size = data[ptr] as usize + 7;
                    ptr += 1;
                }
                if ptr + 2 > end { return Err("Unexpected EOF in far match offset".into()); }
                let b0 = data[ptr];
                let b1 = data[ptr + 1];
                ptr += 2;

                let lo = dest.len().wrapping_sub(
                    ((flag_byte & 8) as usize) * 0x800 + (b1 as usize) * 0x40 + (b0 as usize >> 2)
                );

                if lo != dest.len() {
                    (lo.wrapping_sub(0x4000), match_size + 2)
                } else if match_size != 1 {
                    while (ptr - 0x10) % 0x1000 != 0 {
                        ptr += 1;
                    }
                    continue;
                } else {
                    (0, 1)
                }
            } else if flag_byte < 0x40 {
                let mut match_size = (flag_byte & 0x1f) as usize;
                if match_size == 0 {
                    if ptr >= end { return Err("Unexpected EOF in med match size".into()); }
                    match_size = data[ptr] as usize + 0x1f;
                    ptr += 1;
                }
                match_size += 2;
                if ptr + 2 > end { return Err("Unexpected EOF in med match offset".into()); }
                let b1 = data[ptr];
                let b2 = data[ptr + 1];
                ptr += 2;
                let lo = dest.len().wrapping_sub((b2 as usize) * 0x40 + (b1 as usize >> 2) + 1);
                (lo, match_size)
            } else {
                if ptr >= end { return Err("Unexpected EOF in little match".into()); }
                let b1 = data[ptr];
                ptr += 1;
                let lo = dest.len().wrapping_sub((b1 as usize) * 8 + ((flag_byte >> 2) & 7) as usize + 1);
                let ms = ((flag_byte >> 5) as usize) + 1;
                (lo, ms)
            };

            if match_size != 1 {
                if lookback_offset >= dest.len() {
                    return Err(format!("Match offset {} out of bounds (len={})", lookback_offset, dest.len()));
                }
                for i in 0..match_size {
                    dest.push(dest[lookback_offset + i]);
                }
            }

            let little_literal_size = (data[ptr.wrapping_sub(2)] & 3) as usize;
            if little_literal_size > 0 {
                if ptr + little_literal_size > end {
                    return Err("Little literal exceeds data".into());
                }
                dest.extend_from_slice(&data[ptr..ptr + little_literal_size]);
                ptr += little_literal_size;
            }
        }
    }

    Ok(dest)
}

fn main() {
    let compressed = fs::read("/tmp/hud_banks_compressed.bin")
        .expect("read compressed data");
    
    match decompress_wad_lz(&compressed) {
        Ok(decompressed) => {
            println!("Standalone decompress: {} bytes", decompressed.len());
            
            let original = fs::read(
                "/home/enzonami/Downloads/Ratchet & Clank - Up Your Arsenal/scripts/rac_tools/extracted/unpacked/LEVEL000/data_wad/hud_banks_1.bin"
            ).expect("read original");
            
            println!("Original: {} bytes", original.len());
            
            let min_len = decompressed.len().min(original.len());
            for i in 0..min_len {
                if decompressed[i] != original[i] {
                    let start = i.saturating_sub(4);
                    let end = (i + 12).min(original.len());
                    print!("First diff at byte {}:\n  orig: ", i);
                    for j in start..end { print!("{:02x} ", original[j]); }
                    print!("\n  dec:  ");
                    for j in start..end { print!("{:02x} ", decompressed[j]); }
                    println!();
                    break;
                }
            }
            if decompressed.len() != original.len() {
                println!("SIZE MISMATCH: {} vs {}", decompressed.len(), original.len());
            }
        }
        Err(e) => println!("Decompress error: {}", e),
    }
}
