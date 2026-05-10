/// Standalone WAD LZ decompression
use std::path::Path;
use crate::cli::*;
use crate::wad::decompress_wad;
use std::fs;

pub fn run(_scripts_dir: &Path, args: &DecompressArgs) -> Result<(), String> {
    let input_path = Path::new(&args.input);
    let data = fs::read(input_path).map_err(|e| format!("read {}: {}", input_path.display(), e))?;

    let output_path = match &args.output {
        Some(p) => Path::new(p).to_path_buf(),
        None => input_path.with_extension("bin"),
    };

    println!("Decompressing {} ({} bytes)...", input_path.display(), data.len());
    let decompressed = decompress_wad(&data)?;
    fs::write(&output_path, &decompressed)
        .map_err(|e| format!("write {}: {}", output_path.display(), e))?;
    println!("Wrote {} bytes to {}", decompressed.len(), output_path.display());
    Ok(())
}
