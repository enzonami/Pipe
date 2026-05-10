use std::path::Path;
use crate::cli::*;

pub fn run(_scripts_dir: &Path, _args: &HudLayoutArgs) -> Result<(), String> {
    println!("cmd_hud_layout");
    Ok(())
}
