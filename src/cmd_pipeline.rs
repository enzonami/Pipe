//! Full extraction pipeline — runs all commands in correct dependency order.
//! Single entry point for users: `cargo run -- pipeline`

use std::path::Path;
use std::time::Instant;

use crate::cli::*;

// ── Step tracking ──────────────────────────────────────────────────────────

pub struct Step {
    pub name: &'static str,
    pub phase: &'static str,
    pub action: fn(&Path) -> Result<(), String>,
}

// ── Colored helpers (ANSI, no deps) ────────────────────────────────────────

fn green(s: &str) -> String  { format!("\x1b[32m{s}\x1b[0m") }
fn red(s: &str) -> String    { format!("\x1b[31m{s}\x1b[0m") }
fn yellow(s: &str) -> String { format!("\x1b[33m{s}\x1b[0m") }
fn cyan(s: &str) -> String   { format!("\x1b[36m{s}\x1b[0m") }
fn bold(s: &str) -> String   { format!("\x1b[1m{s}\x1b[0m") }
fn dim(s: &str) -> String    { format!("\x1b[2m{s}\x1b[0m") }

fn step_tag(i: usize, total: usize) -> String {
    format!("[{}/{}]", i, total)
}

fn elapsed(inst: Instant) -> String {
    let d = inst.elapsed();
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}.{:01}s", secs, d.subsec_millis() / 100)
    } else {
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

fn fmt_size(sz: u64) -> String {
    if sz < 1024 {
        format!("{}B", sz)
    } else if sz < 1024 * 1024 {
        format!("{:.1}KB", sz as f64 / 1024.0)
    } else {
        format!("{:.1}MB", sz as f64 / (1024.0 * 1024.0))
    }
}

fn separator(title: &str) {
    let w = 78;
    let pad = (w - title.len() - 2).max(2);
    let l = pad / 2;
    let r = pad - l;
    println!("\n{} {} {}\n", "═".repeat(l), bold(title), "═".repeat(r));
}

// ── Wrapper actions ────────────────────────────────────────────────────────

fn run_toc(sd: &Path) -> Result<(), String>       { crate::cmd_toc::run(sd, &TocArgs { iso_path: None }) }
fn run_wad_extract(sd: &Path) -> Result<(), String> {
    crate::cmd_wad_extract::run(sd, &WadExtractArgs { iso_path: None, extract: false })
}
fn run_wad_unpack(sd: &Path) -> Result<(), String> { crate::cmd_wad_unpack::run(sd, &WadUnpackArgs { level: Some(-1) }) }
fn run_wad_label(sd: &Path) -> Result<(), String>  { crate::cmd_wad_label::run(sd, &WadLabelArgs { level: Some(-1) }) }
fn run_global_wad(sd: &Path) -> Result<(), String> { crate::cmd_global_wad::run(sd, &GlobalWadArgs {}) }
fn run_hud_texture(sd: &Path) -> Result<(), String>{ crate::cmd_hud_texture::run(sd, &HudTextureArgs {}) }
fn run_bonus(sd: &Path) -> Result<(), String>      { crate::cmd_bonus::run(sd, &BonusArgs {}) }
fn run_misc(sd: &Path) -> Result<(), String>       { crate::cmd_misc::run(sd, &MiscArgs {}) }
fn run_core(sd: &Path) -> Result<(), String>       { crate::cmd_core::run(sd, &CoreArgs { level: Some(-1) }) }
fn run_gsram(sd: &Path) -> Result<(), String>      { crate::cmd_gsram::run(sd, &GsramArgs { level: Some(-1) }) }
fn run_audio(sd: &Path) -> Result<(), String>      { crate::cmd_audio::run(sd, &AudioArgs { level: Some(-1) }) }

fn run_texture(sd: &Path) -> Result<(), String> {
    crate::cmd_texture::run(sd, &TextureArgs { level: Some(-1), category: None })
}

fn run_moby(sd: &Path) -> Result<(), String> {
    crate::cmd_moby::run(sd, &MobyArgs { level: Some(-1), class: None })
}

fn run_tie(sd: &Path) -> Result<(), String> {
    crate::cmd_tie::run(sd, &TieArgs { level: Some(-1), class: None })
}

fn run_shrub(sd: &Path) -> Result<(), String> {
    crate::cmd_shrub::run(sd, &ShrubArgs { level: Some(-1), class: None })
}

fn run_tfrag(sd: &Path) -> Result<(), String> {
    crate::cmd_tfrag::run(sd, &TfragArgs { level: Some(-1), tfrag_index: None, lod: 0 })
}

fn run_stash(sd: &Path) -> Result<(), String> {
    crate::cmd_stash::run(sd, &StashArgs { level: Some(-1) })
}

fn run_armor(sd: &Path) -> Result<(), String>       { crate::cmd_armor::run(sd, &ArmorArgs {}) }
fn run_armor_mesh(sd: &Path) -> Result<(), String>  {
    crate::cmd_armor_mesh::run(sd, &ArmorMeshArgs { all: false, path: None })
}
fn run_gadget(sd: &Path) -> Result<(), String>      { crate::cmd_gadget::run(sd, &GadgetArgs {}) }
fn run_gadget_texture(sd: &Path) -> Result<(), String>{
    crate::cmd_gadget_texture::run(sd, &GadgetTextureArgs {})
}
fn run_gameplay(sd: &Path) -> Result<(), String>    { crate::cmd_gameplay::run(sd, &GameplayArgs { level: Some(-1) }) }
fn run_collision(sd: &Path) -> Result<(), String>  { crate::cmd_collision::run(sd, &CollisionArgs {}) }

fn run_scene(sd: &Path) -> Result<(), String> {
    crate::cmd_scene::run(sd, &SceneArgs { level: Some(-1), no_tie: false, no_shrub: false, no_moby: false, no_tfrag: false })
}

fn run_space_decode(sd: &Path) -> Result<(), String>{
    crate::cmd_space_mesh_decode::run(sd, &SpaceMeshDecodeArgs { block_index: None, input: None })
}
fn run_space_export(sd: &Path) -> Result<(), String>{
    crate::cmd_space_mesh_export::run(sd, &SpaceMeshExportArgs { block_index: None, input: None, output: None })
}
fn run_wad_repack(sd: &Path) -> Result<(), String> {
    crate::cmd_wad_repack::run(sd, &WadRepackArgs { level: None, all: true })
}

fn run_iso_pack(sd: &Path) -> Result<(), String> {
    crate::cmd_iso_pack::run(sd, &IsoPackArgs { output: None, input_dir: None })
}

fn run_hud_layout(sd: &Path) -> Result<(), String>  {
    crate::cmd_hud_layout::run(sd, &HudLayoutArgs {})
}

// ── Pipeline definition ────────────────────────────────────────────────────

pub const STEPS: &[Step] = &[
    // Phase 1: ISO → WAD extraction
    Step { name: "toc",              phase: "ISO Extraction",     action: run_toc },
    Step { name: "wad-extract",      phase: "ISO Extraction",     action: run_wad_extract },
    // Phase 2: WAD unpacking & labeling
    Step { name: "wad-unpack",       phase: "WAD Processing",     action: run_wad_unpack },
    Step { name: "wad-label",        phase: "WAD Processing",     action: run_wad_label },
    // Phase 3: Global WAD assets
    Step { name: "global-wad",       phase: "Global WAD Assets",  action: run_global_wad },
    Step { name: "hud-texture",      phase: "Global WAD Assets",  action: run_hud_texture },
    Step { name: "bonus",            phase: "Global WAD Assets",  action: run_bonus },
    Step { name: "misc",             phase: "Global WAD Assets",  action: run_misc },
    // Phase 4: Level core data
    Step { name: "core",             phase: "Level Core Data",    action: run_core },
    Step { name: "texture",          phase: "Level Core Data",    action: run_texture },
    Step { name: "gsram",            phase: "Level Core Data",    action: run_gsram },
    Step { name: "audio",            phase: "Level Core Data",    action: run_audio },
    // Phase 5: Mesh extraction
    Step { name: "moby",             phase: "Mesh Extraction",    action: run_moby },
    Step { name: "tie",              phase: "Mesh Extraction",    action: run_tie },
    Step { name: "shrub",            phase: "Mesh Extraction",    action: run_shrub },
    Step { name: "tfrag",            phase: "Mesh Extraction",    action: run_tfrag },
    Step { name: "stash",            phase: "Mesh Extraction",    action: run_stash },
    // Phase 6: Special data
    Step { name: "armor",            phase: "Special Data",       action: run_armor },
    Step { name: "armor-mesh",       phase: "Special Data",       action: run_armor_mesh },
    Step { name: "gadget",           phase: "Special Data",       action: run_gadget },
    Step { name: "gadget-texture",   phase: "Special Data",       action: run_gadget_texture },
    Step { name: "gameplay",         phase: "Special Data",       action: run_gameplay },
    Step { name: "collision",        phase: "Special Data",       action: run_collision },
    Step { name: "hud-layout",       phase: "Special Data",       action: run_hud_layout },
    // Phase 7: Repack & ISO build
    Step { name: "wad-repack",       phase: "Repack & Export",   action: run_wad_repack },
    Step { name: "iso-pack",         phase: "Repack & Export",   action: run_iso_pack },
    // Phase 8: Final assembly
    Step { name: "scene",            phase: "Scene Assembly",     action: run_scene },
    Step { name: "space-mesh-decode",phase: "Scene Assembly",     action: run_space_decode },
    Step { name: "space-mesh-export",phase: "Scene Assembly",     action: run_space_export },
];

// ── Public entry point ─────────────────────────────────────────────────────

pub fn run(scripts_dir: &Path, _args: &PipelineArgs) -> Result<(), String> {
    let total = STEPS.len();

    // Header
    println!();
    println!("{}", bold("  ╔══════════════════════════════════════════════════════════╗"));
    println!("{}", bold("  ║    Ratchet & Clank: Up Your Arsenal — Full Pipeline     ║"));
    println!("{}", bold("  ╚══════════════════════════════════════════════════════════╝"));
    println!();
    println!("  {} {} steps across {} phases", bold(&total.to_string()), dim("total"), bold("7"));
    println!();
    println!("  {}  {:30} {:>10} {:>10}", dim("Phase"), dim("Name"), dim("Status"), dim("Time"));
    let hr = "─".repeat(58);
    println!("  {}", dim(&hr));

    let mut phase_start = Instant::now();
    let mut current_phase = String::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let skipped = 0usize;
    let overall_start = Instant::now();

    for (i, step) in STEPS.iter().enumerate() {
        // Phase header
        if step.phase != current_phase {
            if !current_phase.is_empty() {
                let phase_time = elapsed(phase_start);
                let phase_line = format!("  ─ {} ─", phase_time);
                println!("  {}", dim(&phase_line));
            }
            current_phase = step.phase.to_string();
            phase_start = Instant::now();
            separator(step.phase);
            println!();
        }

        let tag = step_tag(i + 1, total);
        let step_start = Instant::now();
        print!("  {}  {:30} ", dim(&tag), step.name);
        // Flush so user sees progress before the command runs
        use std::io::Write;
        std::io::stdout().flush().ok();

        match (step.action)(scripts_dir) {
            Ok(()) => {
                let t = elapsed(step_start);
                println!("  {}  {}", green("✓ PASS"), dim(&t));
                passed += 1;
            }
            Err(e) => {
                let t = elapsed(step_start);
                println!("  {}  {}", red("✗ FAIL"), dim(&t));
                println!("    {} {}", dim("│"), red(&e));
                failed += 1;
            }
        }
    }

    // Final footer
    let total_time = elapsed(overall_start);
    println!("\n{}", "═".repeat(80));
    println!("  {} of {} steps — {} / {} / {}",
        if failed == 0 { green("ALL PASSED") } else { red("SOME FAILED") },
        total,
        green(&format!("{} passed", passed)),
        red(&format!("{} failed", failed)),
        dim(&format!("{} skipped", skipped)),
    );
    println!("  {} {} {}", dim("Total time:"), bold(&total_time), dim(&fmt_size(
        dir_size(&scripts_dir.join("extracted")).unwrap_or(0)
    )));
    println!();

    if failed > 0 {
        Err(format!("Pipeline completed with {} failed step(s). See output above.", failed))
    } else {
        Ok(())
    }
}

fn dir_size(path: &Path) -> Option<u64> {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = entry.metadata().ok()?;
            if meta.is_dir() {
                total += dir_size(&entry.path()).unwrap_or(0);
            } else {
                total += meta.len();
            }
        }
    }
    Some(total)
}
