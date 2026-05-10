#![allow(dead_code)]

mod cli;
mod common;
mod types;
mod vif;
mod wad;
pub mod texture;

mod cmd_toc;
mod cmd_wad_extract;
mod cmd_wad_unpack;
mod cmd_wad_label;
mod cmd_core;
mod cmd_texture;
mod cmd_moby;
mod cmd_tie;
mod cmd_shrub;
mod cmd_tfrag;
mod cmd_stash;
mod cmd_gsram;
mod cmd_audio;
mod cmd_bonus;
mod cmd_armor;
mod cmd_armor_mesh;
mod cmd_gadget;
mod cmd_gadget_texture;
mod cmd_collision;
mod cmd_gameplay;
mod cmd_hud_texture;
mod cmd_hud_layout;
mod cmd_misc;
mod cmd_scene;
mod cmd_space_mesh_decode;
mod cmd_space_mesh_export;
mod cmd_global_wad;
mod cmd_viewer;
mod cmd_pipeline;
mod cmd_decompress;
mod cmd_wad_compress;
mod cmd_wad_repack;
mod cmd_iso_pack;
mod cmd_texture_pack;

use clap::Parser;
use cli::Cli;
use std::path::Path;

/// Get the base directory for data extraction.
/// Points to the current working directory (expected to be rac_tools/)
/// so extracted data goes to ./extracted/ and WADs go to ./extracted/WAD/
fn scripts_dir() -> &'static Path {
    Path::new(".")
}

fn main() {
    let cli = Cli::parse();
    let script_dir = scripts_dir();
    let result = match &cli.command {
        cli::Commands::Toc(args) => cmd_toc::run(script_dir, args),
        cli::Commands::WadExtract(args) => cmd_wad_extract::run(script_dir, args),
        cli::Commands::WadUnpack(args) => cmd_wad_unpack::run(script_dir, args),
        cli::Commands::WadLabel(args) => cmd_wad_label::run(script_dir, args),
        cli::Commands::Core(args) => cmd_core::run(script_dir, args),
        cli::Commands::Texture(args) => cmd_texture::run(script_dir, args),
        cli::Commands::Moby(args) => cmd_moby::run(script_dir, args),
        cli::Commands::Tie(args) => cmd_tie::run(script_dir, args),
        cli::Commands::Shrub(args) => cmd_shrub::run(script_dir, args),
        cli::Commands::Tfrag(args) => cmd_tfrag::run(script_dir, args),
        cli::Commands::Stash(args) => cmd_stash::run(script_dir, args),
        cli::Commands::Gsram(args) => cmd_gsram::run(script_dir, args),
        cli::Commands::Audio(args) => cmd_audio::run(script_dir, args),
        cli::Commands::Bonus(args) => cmd_bonus::run(script_dir, args),
        cli::Commands::Armor(args) => cmd_armor::run(script_dir, args),
        cli::Commands::ArmorMesh(args) => cmd_armor_mesh::run(script_dir, args),
        cli::Commands::Gadget(args) => cmd_gadget::run(script_dir, args),
        cli::Commands::GadgetTexture(args) => cmd_gadget_texture::run(script_dir, args),
        cli::Commands::Collision(args) => cmd_collision::run(script_dir, args),
        cli::Commands::Gameplay(args) => cmd_gameplay::run(script_dir, args),
        cli::Commands::HudTexture(args) => cmd_hud_texture::run(script_dir, args),
        cli::Commands::HudLayout(args) => cmd_hud_layout::run(script_dir, args),
        cli::Commands::Misc(args) => cmd_misc::run(script_dir, args),
        cli::Commands::Scene(args) => cmd_scene::run(script_dir, args),
        cli::Commands::SpaceMeshDecode(args) => cmd_space_mesh_decode::run(script_dir, args),
        cli::Commands::SpaceMeshExport(args) => cmd_space_mesh_export::run(script_dir, args),
        cli::Commands::GlobalWad(args) => cmd_global_wad::run(script_dir, args),
        cli::Commands::Viewer(args) => cmd_viewer::run(script_dir, args),
        cli::Commands::Pipeline(args) => cmd_pipeline::run(script_dir, args),
        cli::Commands::Decompress(args) => cmd_decompress::run(script_dir, args),
        cli::Commands::WadCompress(args) => cmd_wad_compress::run(script_dir, args),
        cli::Commands::WadRepack(args) => cmd_wad_repack::run(script_dir, args),
        cli::Commands::IsoPack(args) => cmd_iso_pack::run(script_dir, args),
        cli::Commands::TexturePack(args) => cmd_texture_pack::run(script_dir, args),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
