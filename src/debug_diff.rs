use std::path::Path;

fn main() {
    // Read the test data
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = Path::new(manifest_dir).join("extracted/unpacked/LEVEL000/data_wad/hud_banks_1.bin");
    let input = std::fs::read(&path).expect("read hud_banks_1.bin");
    
    // Compress
    let compressed = rac_tools::wad::compress_wad_lz(&input);
    
    // Decompress
    let decompressed = rac_tools::wad::decompress_wad_lz(&compressed)
        .expect("decompress should succeed");
    
    println!("input: {} bytes", input.len());
    println!("compressed: {} bytes", compressed.len());
    println!("decompressed: {} bytes", decompressed.len());
    
    // Find first difference
    let min_len = decompressed.len().min(input.len());
    for i in 0..min_len {
        if decompressed[i] != input[i] {
            let start = i.saturating_sub(4);
            let end = (i + 8).min(input.len().min(decompressed.len()));
            print!("First diff at byte {}:\n  input:  ", i);
            for j in start..end { print!("{:02x} ", input[j]); }
            print!("\n  output: ");
            for j in start..end { print!("{:02x} ", decompressed[j]); }
            println!();
            break;
        }
    }
    
    // Save compressed data for Python verification
    std::fs::write("/tmp/hud_banks_compressed.bin", &compressed).expect("write compressed");
    println!("Saved compressed to /tmp/hud_banks_compressed.bin");
}
