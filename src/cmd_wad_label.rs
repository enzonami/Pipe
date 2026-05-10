/// WAD Labeler - label/annotate WAD structure
use std::path::Path;
use crate::cli::*;
use crate::wad;
use crate::common::*;

pub fn run(scripts_dir: &Path, args: &WadLabelArgs) -> Result<(), String> {
    let wad_dir = crate::common::wad_dir(scripts_dir);
    let level_filter = args.level.unwrap_or(-1);

    level_dispatch(level_filter, |level_num| {
        label_level(&wad_dir, level_num)
    })
}

fn label_level(wad_dir: &Path, level_num: u32) -> Result<(), String> {
    for name in &["level", "audio", "scene"] {
        let wad_path = wad_dir.join(format!("LEVEL{:03}_{}.wad", level_num, name));
        if !wad_path.exists() {
            continue;
        }
        let data = std::fs::read(&wad_path)
            .map_err(|e| format!("Cannot read {}: {}", wad_path.display(), e))?;

        println!("--- {} ---", wad_path.display());
        println!("  Size: {} bytes", data.len());

        if data.len() >= 4 && &data[..3] == b"WAD" {
            let ver = data[3];
            let hdr_size: usize = if ver == 0 { 0x20 } else { 0x10 };
            let block_ofs = r_u32(&data, hdr_size);
            let block_sz = r_u32(&data, hdr_size + 4);
            println!("  WAD ver={} header_size=0x{:X} block_offset=0x{:X} block_size=0x{:X}",
                     ver, hdr_size, block_ofs, block_sz);
        } else {
            println!("  No WAD magic - raw data?");
        }

        // Try to decompress first section
        if wad::is_wad_compressed(&data) {
            match wad::decompress_wad(&data) {
                Ok(decompressed) => {
                    println!("  Decompressed: {} bytes -> {} bytes", data.len(), decompressed.len());
                    label_decompressed(&decompressed, name);
                }
                Err(e) => println!("  Decompress error: {}", e),
            }
        }
    }

    Ok(())
}

fn label_decompressed(data: &[u8], name: &str) {
    if data.len() < 0x30 { return; }

    // Parse level core header
    let core = wad::parse_level_core_header(&data);
    let class_names = ["tie", "moby", "shrub", "tfrag"];
    println!("  Core header ({})", name);
    for (i, cname) in class_names.iter().enumerate() {
        let idx = i * 3;
        let count = core[idx].as_i64().unwrap_or(0);
        let ofs = core[idx + 1].as_i64().unwrap_or(0);
        let size = core[idx + 2].as_i64().unwrap_or(0);
        println!("    {}: count={} offset=0x{:X} size=0x{:X}",
                 cname, count, ofs, size);
    }

    // Check for data header
    if data.len() >= 0x28 {
        let fields: Vec<String> = (0..10)
            .map(|i| format!("0x{:X}", r_u32(&data, i * 4)))
            .collect();
        println!("  Data header: {}", fields.join(", "));
    }
}
