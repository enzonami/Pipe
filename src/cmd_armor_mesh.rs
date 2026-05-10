use std::path::Path;
use crate::cli::*;

pub fn run(_scripts_dir: &Path, _args: &ArmorMeshArgs) -> Result<(), String> {
    println!("cmd_armor_mesh");
    Ok(())
}
