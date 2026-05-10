//! CLI dispatch using clap

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rac_tools", about = "Ratchet & Clank: Up Your Arsenal data extraction tools")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Parse ISO TOC and list WAD files
    Toc(TocArgs),
    /// Extract WAD files from ISO
    WadExtract(WadExtractArgs),
    /// Unpack WAD files into structured data
    WadUnpack(WadUnpackArgs),
    /// Label/annotate WAD structure
    WadLabel(WadLabelArgs),
    /// Extract level core data
    Core(CoreArgs),
    /// Extract texture data
    Texture(TextureArgs),
    /// Extract moby meshes
    Moby(MobyArgs),
    /// Extract tie meshes
    Tie(TieArgs),
    /// Extract shrub meshes
    Shrub(ShrubArgs),
    /// Extract tfrag meshes
    Tfrag(TfragArgs),
    /// Extract stash entries
    Stash(StashArgs),
    /// Extract GSRAM vertex data
    Gsram(GsramArgs),
    /// Extract audio WADs
    Audio(AudioArgs),
    /// Extract bonus/demo data
    Bonus(BonusArgs),
    /// Extract armor data
    Armor(ArmorArgs),
    /// Decode armor meshes
    ArmorMesh(ArmorMeshArgs),
    /// Extract gadget data
    Gadget(GadgetArgs),
    /// Decode gadget textures
    GadgetTexture(GadgetTextureArgs),
    /// Export collision mesh as OBJ
    Collision(CollisionArgs),
    /// Extract gameplay data
    Gameplay(GameplayArgs),
    /// Extract HUD textures
    HudTexture(HudTextureArgs),
    /// Analyze HUD layout
    HudLayout(HudLayoutArgs),
    /// Extract misc data
    Misc(MiscArgs),
    /// Assemble scene from extracted meshes
    Scene(SceneArgs),
    /// Extract space mesh data (decode)
    SpaceMeshDecode(SpaceMeshDecodeArgs),
    /// Export space mesh data
    SpaceMeshExport(SpaceMeshExportArgs),
    /// Analyze global WAD structure
    GlobalWad(GlobalWadArgs),
    /// Start 3D viewer (HTTP server)
    Viewer(ViewerArgs),
    /// Run full extraction pipeline (all steps in order)
    Pipeline(PipelineArgs),
    /// Decompress a WAD LZ compressed file (standalone)
    Decompress(DecompressArgs),
    /// Compress a file into WAD LZ format (standalone)
    WadCompress(WadCompressArgs),
    /// Repack WAD files from unpacked data
    WadRepack(WadRepackArgs),
    /// Pack repacked WADs into a new ISO
    IsoPack(IsoPackArgs),
    /// Pack a PNG image into PIF8 format
    TexturePack(TexturePackArgs),
}

#[derive(clap::Args)]
pub struct TocArgs {
    /// Path to ISO file
    pub iso_path: Option<String>,
}

#[derive(clap::Args)]
pub struct WadExtractArgs {
    /// Path to ISO file
    pub iso_path: Option<String>,
    /// Extract only (don't unpack)
    #[arg(long)]
    pub extract: bool,
}

#[derive(clap::Args)]
pub struct WadUnpackArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
}

#[derive(clap::Args)]
pub struct WadLabelArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
}

#[derive(clap::Args)]
pub struct CoreArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
}

#[derive(clap::Args)]
pub struct TextureArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
    /// Texture category filter (e.g., "mobs", "tie", "shrub")
    pub category: Option<String>,
}

#[derive(clap::Args)]
pub struct MobyArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
    /// Target moby class index
    pub class: Option<i32>,
}

#[derive(clap::Args)]
pub struct TieArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
    /// Target tie class index
    pub class: Option<i32>,
}

#[derive(clap::Args)]
pub struct ShrubArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
    /// Target shrub class index
    pub class: Option<i32>,
}

#[derive(clap::Args)]
pub struct TfragArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
    /// Target tfrag index
    pub tfrag_index: Option<i32>,
    /// LOD level
    #[arg(long, default_value = "0")]
    pub lod: u32,
}

#[derive(clap::Args)]
pub struct StashArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
}

#[derive(clap::Args)]
pub struct GsramArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
}

#[derive(clap::Args)]
pub struct AudioArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
}

#[derive(clap::Args)]
pub struct BonusArgs;

#[derive(clap::Args)]
pub struct ArmorArgs;

#[derive(clap::Args)]
pub struct ArmorMeshArgs {
    /// Process all armor meshes
    #[arg(long)]
    pub all: bool,
    /// Specific mesh path
    pub path: Option<String>,
}

#[derive(clap::Args)]
pub struct GadgetArgs;

#[derive(clap::Args)]
pub struct GadgetTextureArgs;

#[derive(clap::Args)]
pub struct GameplayArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
}

#[derive(clap::Args)]
pub struct HudTextureArgs;

#[derive(clap::Args)]
pub struct CollisionArgs;


#[derive(clap::Args)]
pub struct HudLayoutArgs;

#[derive(clap::Args)]
pub struct MiscArgs;

#[derive(clap::Args)]
pub struct SceneArgs {
    /// Level number (0-13) or -1 for all
    pub level: Option<i32>,
    /// Skip tie meshes
    #[arg(long)]
    pub no_tie: bool,
    /// Skip shrub meshes
    #[arg(long)]
    pub no_shrub: bool,
    /// Skip moby meshes
    #[arg(long)]
    pub no_moby: bool,
    /// Skip tfrag meshes
    #[arg(long)]
    pub no_tfrag: bool,
}

#[derive(clap::Args)]
pub struct SpaceMeshDecodeArgs {
    /// Optional block index (decodes a specific block)
    pub block_index: Option<u32>,
    /// Optional path to a specific block file
    #[arg(long)]
    pub input: Option<String>,
}

#[derive(clap::Args)]
pub struct SpaceMeshExportArgs {
    /// Optional block index (exports a specific block only)
    pub block_index: Option<u32>,
    /// Optional path to a specific block file
    #[arg(long)]
    pub input: Option<String>,
    /// Output directory for OBJ files
    #[arg(long, default_value = "extracted/meshes/SPACE")]
    pub output: Option<String>,
}

#[derive(clap::Args)]
pub struct GlobalWadArgs;

#[derive(clap::Args)]
pub struct ViewerArgs {
    /// Default level to load
    pub level: Option<i32>,
    /// HTTP port
    #[arg(long, default_value = "8000")]
    pub port: u16,
}

#[derive(clap::Args)]
pub struct PipelineArgs;

#[derive(clap::Args)]
pub struct DecompressArgs {
    /// Path to compressed WAD LZ file
    pub input: String,
    /// Optional output path (defaults to input.bin)
    pub output: Option<String>,
}

#[derive(clap::Args)]
pub struct WadCompressArgs {
    /// Path to input file to compress
    pub input: String,
    /// Optional output path (defaults to input.lz)
    pub output: Option<String>,
}

#[derive(clap::Args)]
pub struct WadRepackArgs {
    /// Level number to repack (0-13)
    #[arg(long, short)]
    pub level: Option<i32>,
    /// Repack all levels
    #[arg(long)]
    pub all: bool,
}

#[derive(clap::Args)]
pub struct IsoPackArgs {
    /// Path to output ISO file
    pub output: Option<String>,
    /// Input directory with repacked WADs (default: extracted/repacked/)
    #[arg(long)]
    pub input_dir: Option<String>,
}

#[derive(clap::Args)]
pub struct TexturePackArgs {
    /// Input PNG file path
    pub input: String,
    /// Optional output PIF8 file path (defaults to input with .pif8 extension)
    pub output: Option<String>,
}
