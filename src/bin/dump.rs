/// Debug: dump original data around the first diff position
use std::fs;

fn main() {
    let original = fs::read(
        "/home/enzonami/Downloads/Ratchet & Clank - Up Your Arsenal/scripts/rac_tools/extracted/unpacked/LEVEL000/data_wad/hud_banks_1.bin"
    ).expect("read original");
    
    // Show 64 bytes centered around 50155
    let center = 50155usize;
    let start = center.saturating_sub(32);
    let end = (center + 32).min(original.len());
    
    eprintln!("Original data around byte {}:", center);
    eprint!("  offset={}: ", start);
    for i in start..end {
        eprint!("{:02x} ", original[i]);
        if (i - start + 1) % 16 == 0 { eprintln!("\n  {:>9}: ", i+1); }
    }
    eprintln!();
    
    // Also check if there's a match-pattern
    eprintln!("\n4-byte groupings at {}: ", center);
    for i in center.saturating_sub(16)..center + 16 {
        eprint!("{:02x} ", original[i]);
    }
    eprintln!();
    
    // Check for repeating patterns
    eprintln!("\nChecking for repeating patterns around {}:", center);
    for dist in [4, 8, 12, 16, 100, 200] {
        let pos = center;
        let lookback = pos.saturating_sub(dist);
        let match_len = (original.len() - pos).min(16);
        let mut matching = true;
        for j in 0..match_len {
            if original[lookback + j] != original[pos + j] {
                matching = false;
                break;
            }
        }
        eprintln!("  dist={}: at pos {}, lookback to {}: match_len={} {}", 
            dist, pos, lookback, if matching { match_len.to_string() } else { "0".to_string() },
            if matching { "FULL" } else { "no match" });
    }
}
