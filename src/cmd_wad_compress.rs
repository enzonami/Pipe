/// Standalone WAD LZ compression
use std::path::Path;
use crate::cli::*;
use crate::wad::compress_wad_lz;
use std::fs;

pub fn run(_scripts_dir: &Path, args: &WadCompressArgs) -> Result<(), String> {
    let input_path = Path::new(&args.input);
    let data = fs::read(input_path).map_err(|e| format!("read {}: {}", input_path.display(), e))?;

    let output_path = match &args.output {
        Some(p) => Path::new(p).to_path_buf(),
        None => input_path.with_extension("lz"),
    };

    println!("Compressing {} ({} bytes)...", input_path.display(), data.len());
    let compressed = compress_wad_lz(&data);
    fs::write(&output_path, &compressed)
        .map_err(|e| format!("write {}: {}", output_path.display(), e))?;
    let ratio = if data.len() > 0 {
        (compressed.len() as f64 / data.len() as f64) * 100.0
    } else {
        0.0
    };
    println!("Wrote {} bytes to {} ({:.1}% of original)", compressed.len(), output_path.display(), ratio);
    Ok(())
}
