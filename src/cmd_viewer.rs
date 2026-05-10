//! Web UI server — pipeline dashboard, scene viewer, asset browser
//! Serves at http://localhost:{port}

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::path::Path;
use std::time::Instant;
use std::convert::Infallible;
use tokio::sync::broadcast;
use axum::{
    Router, extract::State, response::{Html, Sse, Json as AxJson, IntoResponse, sse::Event},
    routing::{get, post},
};
use axum::http::StatusCode;
use serde::Serialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt as _;
use tower_http::services::ServeDir;
use crate::cli::ViewerArgs;

// ── Shared state ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    scripts_dir: String,
    pipeline_tx: broadcast::Sender<PipelineEvent>,
    pipeline_running: Arc<AtomicUsize>,
    tools_tx: broadcast::Sender<ToolEvent>,
    tool_running: Arc<AtomicUsize>,
}

#[derive(Clone, Serialize)]
struct PipelineEvent {
    step: usize,
    total: usize,
    name: String,
    phase: String,
    status: String,
    message: String,
    elapsed_ms: u64,
}

#[derive(Clone, Serialize)]
struct ToolEvent {
    kind: String,
    status: String,
    message: String,
    current: usize,
    total: usize,
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn has_extracted_wads(scripts_dir: &Path) -> bool {
    let wad_dir = scripts_dir.join("extracted").join("WAD");
    wad_dir.exists() && std::fs::read_dir(&wad_dir).ok()
        .map(|e| e.flatten().count() > 5)
        .unwrap_or(false)
}

fn has_unpacked_data(scripts_dir: &Path) -> bool {
    let unpacked = scripts_dir.join("extracted").join("unpacked");
    unpacked.exists() && std::fs::read_dir(&unpacked).ok()
        .map(|e| e.flatten().count() > 5)
        .unwrap_or(false)
}

fn has_meshes(scripts_dir: &Path) -> bool {
    let meshes = scripts_dir.join("extracted").join("meshes");
    meshes.exists() && std::fs::read_dir(&meshes).ok()
        .map(|e| e.flatten().count() > 10)
        .unwrap_or(false)
}

fn has_global_wad_data(scripts_dir: &Path) -> bool {
    let global = scripts_dir.join("extracted").join("unpacked").join("GLOBAL");
    global.exists() && std::fs::read_dir(&global).ok()
        .map(|e| e.flatten().count() >= 4)
        .unwrap_or(false)
}

fn has_scenes(scripts_dir: &Path) -> bool {
    let scenes = scripts_dir.join("extracted").join("scenes");
    scenes.exists() && std::fs::read_dir(&scenes).ok()
        .map(|e| e.flatten().count() >= 5)
        .unwrap_or(false)
}

// ── Routes ─────────────────────────────────────────────────────────────────

async fn index() -> Html<&'static str> {
    Html(HTML)
}

async fn status_api(State(state): State<AppState>) -> AxJson<serde_json::Value> {
    let sd = Path::new(&state.scripts_dir);
    AxJson(serde_json::json!({
        "extracted": has_extracted_wads(sd),
        "unpacked": has_unpacked_data(sd),
        "global_wad": has_global_wad_data(sd),
        "meshes": has_meshes(sd),
        "scenes": has_scenes(sd),
        "pipeline_running": state.pipeline_running.load(Ordering::SeqCst) > 0,
    }))
}

async fn scenes_api(State(state): State<AppState>) -> AxJson<serde_json::Value> {
    let scenes_dir = Path::new(&state.scripts_dir).join("extracted").join("scenes");
    let mut scenes = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&scenes_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let mut file_count = 0u64;
            let mut total_size = 0u64;
            let path = entry.path();
            if path.is_dir() {
                if let Ok(files) = std::fs::read_dir(&path) {
                    for f in files.flatten() {
                        file_count += 1;
                        if let Ok(meta) = f.metadata() {
                            total_size += meta.len();
                        }
                    }
                }
                scenes.push(serde_json::json!({
                    "name": name, "files": file_count, "size": total_size,
                }));
            }
        }
    }
    AxJson(serde_json::json!(scenes))
}

async fn asset_tree_api(State(state): State<AppState>) -> AxJson<serde_json::Value> {
    let extracted = Path::new(&state.scripts_dir).join("extracted");
    AxJson(build_tree(&extracted))
}

fn build_tree(dir: &Path) -> serde_json::Value {
    let mut entries = Vec::new();
    if let Ok(readdir) = std::fs::read_dir(dir) {
        for entry in readdir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            if path.is_dir() {
                let mut file_count = 0u64;
                let mut total_size = 0u64;
                count_files(&path, &mut file_count, &mut total_size);
                let children = build_tree(&path);
                entries.push(serde_json::json!({
                    "name": name, "type": "dir",
                    "files": file_count, "size": total_size,
                    "children": children,
                }));
            } else {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
                entries.push(serde_json::json!({
                    "name": name, "type": "file", "size": size, "ext": ext,
                }));
            }
        }
    }
    serde_json::json!(entries)
}

fn count_files(dir: &Path, files: &mut u64, size: &mut u64) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count_files(&path, files, size);
            } else {
                *files += 1;
                if let Ok(meta) = entry.metadata() {
                    *size += meta.len();
                }
            }
        }
    }
}

async fn pipeline_start(State(state): State<AppState>) -> impl IntoResponse {
    if state.pipeline_running.load(Ordering::SeqCst) > 0 {
        return (StatusCode::CONFLICT, "Pipeline already running".to_string());
    }
    state.pipeline_running.store(1, Ordering::SeqCst);

    let scripts_dir = state.scripts_dir.clone();
    let tx = state.pipeline_tx.clone();
    let running = state.pipeline_running.clone();

    // Run pipeline on a dedicated thread (not tokio blocking pool)
    // so long-running sync operations don't stall the SSE stream
    std::thread::spawn(move || {
        let sd = Path::new(&scripts_dir);
        let total = crate::cmd_pipeline::STEPS.len();

        for (i, step) in crate::cmd_pipeline::STEPS.iter().enumerate() {
            // Check if we can skip this step
            let step_name = step.name;
            let skip = should_skip(step_name, sd);

            if skip {
                let _ = tx.send(PipelineEvent {
                    step: i + 1, total, name: step_name.to_string(), phase: step.phase.to_string(),
                    status: "skipped".to_string(), message: "Already completed".to_string(), elapsed_ms: 0,
                });
                // Still emit a small delay so the SSE stream doesn't race ahead
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }

            // Emit "running"
            let _ = tx.send(PipelineEvent {
                step: i + 1, total, name: step_name.to_string(), phase: step.phase.to_string(),
                status: "running".to_string(), message: String::new(), elapsed_ms: 0,
            });

            let start = Instant::now();
            match (step.action)(sd) {
                Ok(()) => {
                    // Write marker file so skip detection is reliable
                    let marker = sd.join("extracted").join(format!(".step_{}", step_name));
                    let _ = std::fs::write(&marker, b"done");
                    let ms = start.elapsed().as_millis() as u64;
                    let _ = tx.send(PipelineEvent {
                        step: i + 1, total, name: step_name.to_string(), phase: step.phase.to_string(),
                        status: "passed".to_string(), message: String::new(), elapsed_ms: ms,
                    });
                }
                Err(e) => {
                    let ms = start.elapsed().as_millis() as u64;
                    let _ = tx.send(PipelineEvent {
                        step: i + 1, total, name: step_name.to_string(), phase: step.phase.to_string(),
                        status: "failed".to_string(), message: e, elapsed_ms: ms,
                    });
                }
            }
        }

        running.store(0, Ordering::SeqCst);
        let _ = tx.send(PipelineEvent {
            step: 0, total: 0, name: String::new(), phase: String::new(),
            status: "done".to_string(), message: "Pipeline complete".to_string(), elapsed_ms: 0,
        });
    });

    (StatusCode::OK, "Pipeline started".to_string())
}

fn should_skip(name: &str, sd: &Path) -> bool {
    let extracted = sd.join("extracted");
    // Prefer marker files — most reliable
    let marker = extracted.join(format!(".step_{}", name));
    if marker.exists() {
        return true;
    }
    // Fallback directory-based checks for existing extractions without markers
    match name {
        "toc" => extracted.join("toc.json").exists(),
        "wad-extract" => {
            let wad = extracted.join("WAD");
            wad.exists() && std::fs::read_dir(&wad).ok()
                .map(|e| e.flatten().count() > 0)
                .unwrap_or(false)
        }
        "wad-unpack" => {
            let unpacked = extracted.join("unpacked");
            unpacked.exists() && std::fs::read_dir(&unpacked).ok()
                .map(|e| e.flatten().count() > 3)
                .unwrap_or(false)
        }
        "wad-label" => has_unpacked_data(sd),
        "global-wad" => has_global_wad_data(sd),
        "hud-texture" => {
            let hud = extracted.join("unpacked").join("HUD");
            hud.exists() && std::fs::read_dir(&hud).ok()
                .map(|e| e.flatten().count() > 0)
                .unwrap_or(false)
        }
        "bonus" => {
            let bonus = extracted.join("unpacked").join("BONUS");
            bonus.exists() && std::fs::read_dir(&bonus).ok()
                .map(|e| e.flatten().count() > 0)
                .unwrap_or(false)
        }
        "misc" => {
            let misc = extracted.join("unpacked").join("MISC");
            misc.exists() && std::fs::read_dir(&misc).ok()
                .map(|e| e.flatten().count() > 0)
                .unwrap_or(false)
        }
        "moby" => {
            // moby runs first for all levels — per-level subdirs indicate it ran
            let meshes = extracted.join("meshes");
            meshes.exists() && std::fs::read_dir(&meshes).ok()
                .map(|e| e.flatten().count() >= 14)
                .unwrap_or(false)
        }
        "tie" | "shrub" | "tfrag" | "stash" => {
            // These are always false without markers — they'd be falsely
            // triggered by moby's output otherwise. Marker is the only
            // reliable check.
            false
        }
        "hud-layout" => {
            let layouts = extracted.join("hud_layouts");
            layouts.exists() && std::fs::read_dir(&layouts).ok()
                .map(|e| e.flatten().count() >= 14)
                .unwrap_or(false)
        }
        "collision" => {
            let col = extracted.join("collision");
            col.exists() && std::fs::read_dir(&col).ok()
                .map(|e| e.flatten().count() >= 14)
                .unwrap_or(false)
        }
        "scene" => has_scenes(sd),
        "space-mesh-decode" | "space-mesh-export" => extracted.join("scenes").exists(),
        _ => false,
    }
}

async fn repack_api(State(state): State<AppState>) -> impl IntoResponse {
    if state.tool_running.load(Ordering::SeqCst) > 0 {
        return (StatusCode::CONFLICT, "Tool already running".to_string());
    }
    state.tool_running.store(1, Ordering::SeqCst);

    let scripts_dir = state.scripts_dir.clone();
    let tx = state.tools_tx.clone();
    let running = state.tool_running.clone();

    std::thread::spawn(move || {
        let sd = Path::new(&scripts_dir);
        let _ = tx.send(ToolEvent {
            kind: "repack".into(), status: "running".into(),
            message: "Repacking all levels...".into(), current: 0, total: 1,
        });
        let start = Instant::now();
        match crate::cmd_wad_repack::run(sd, &crate::cli::WadRepackArgs { level: None, all: true }) {
            Ok(()) => {
                let elapsed = start.elapsed();
                let _ = tx.send(ToolEvent {
                    kind: "repack".into(), status: "done".into(),
                    message: format!("Done in {}.{:01}s", elapsed.as_secs(), elapsed.subsec_millis() / 100),
                    current: 1, total: 1,
                });
            }
            Err(e) => {
                let _ = tx.send(ToolEvent {
                    kind: "repack".into(), status: "error".into(),
                    message: e, current: 0, total: 0,
                });
            }
        }
        running.store(0, Ordering::SeqCst);
    });

    (StatusCode::OK, "Repack started".to_string())
}

async fn iso_pack_api(State(state): State<AppState>) -> impl IntoResponse {
    if state.tool_running.load(Ordering::SeqCst) > 0 {
        return (StatusCode::CONFLICT, "Tool already running".to_string());
    }
    state.tool_running.store(1, Ordering::SeqCst);

    let scripts_dir = state.scripts_dir.clone();
    let tx = state.tools_tx.clone();
    let running = state.tool_running.clone();

    std::thread::spawn(move || {
        let sd = Path::new(&scripts_dir);
        let _ = tx.send(ToolEvent {
            kind: "iso-pack".into(), status: "running".into(),
            message: "Building repacked ISO...".into(), current: 0, total: 1,
        });
        let start = Instant::now();
        match crate::cmd_iso_pack::run(sd, &crate::cli::IsoPackArgs { output: None, input_dir: None }) {
            Ok(()) => {
                let elapsed = start.elapsed();
                let _ = tx.send(ToolEvent {
                    kind: "iso-pack".into(), status: "done".into(),
                    message: format!("Done in {}.{:01}s", elapsed.as_secs(), elapsed.subsec_millis() / 100),
                    current: 1, total: 1,
                });
            }
            Err(e) => {
                let _ = tx.send(ToolEvent {
                    kind: "iso-pack".into(), status: "error".into(),
                    message: e, current: 0, total: 0,
                });
            }
        }
        running.store(0, Ordering::SeqCst);
    });

    (StatusCode::OK, "ISO pack started".to_string())
}

async fn tools_events(State(state): State<AppState>) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tools_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        match result {
            Ok(ev) => {
                let json = serde_json::to_string(&ev).unwrap_or_default();
                Some(Ok(Event::default().data(json)))
            }
            Err(_) => None,
        }
    });
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new())
}

async fn pipeline_events(State(state): State<AppState>) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.pipeline_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        match result {
            Ok(ev) => {
                let json = serde_json::to_string(&ev).unwrap_or_default();
                Some(Ok(Event::default().data(json)))
            }
            Err(_) => None,
        }
    });
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new())
}

// ── Public entry point ─────────────────────────────────────────────────────

pub fn run(scripts_dir: &Path, args: &ViewerArgs) -> Result<(), String> {
    let (tx, _rx) = broadcast::channel(128);
    let (tools_tx, _tools_rx) = broadcast::channel(128);
    let state = AppState {
        scripts_dir: scripts_dir.to_string_lossy().to_string(),
        pipeline_tx: tx,
        pipeline_running: Arc::new(AtomicUsize::new(0)),
        tools_tx,
        tool_running: Arc::new(AtomicUsize::new(0)),
    };

    let port = args.port;
    let app = Router::new()
        .route("/", get(index))
        .route("/api/scenes", get(scenes_api))
        .route("/api/assets", get(asset_tree_api))
        .route("/api/status", get(status_api))
        .route("/api/pipeline/start", post(pipeline_start))
        .route("/api/pipeline/events", get(pipeline_events))

        .route("/api/tools/events", get(tools_events))
        .route("/api/tools/repack", post(repack_api))
        .route("/api/tools/iso-pack", post(iso_pack_api))
        .nest_service("/extracted", ServeDir::new(scripts_dir.join("extracted")))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    println!("╔══════════════════════════════════════════╗");
    println!("║   R&C: UYA Extraction Dashboard         ║");
    println!("║   http://localhost:{:<4}                   ║", port);
    println!("╚══════════════════════════════════════════╝");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .map_err(|e| format!("tokio: {}", e))?;

    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(&addr).await
            .map_err(|e| format!("bind: {}", e))?;
        axum::serve(listener, app).await
            .map_err(|e| format!("serve: {}", e))
    })
}

// ── Embedded HTML ──────────────────────────────────────────────────────────

const HTML: &str = r###"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>R&C: UYA — Extraction Dashboard</title>
<style>
* { margin:0; padding:0; box-sizing:border-box; }
:root { --bg:#0d1117; --bg2:#161b22; --bg3:#1c2333; --fg:#e6edf3; --fg2:#8b949e; --accent:#58a6ff; --green:#3fb950; --red:#f85149; --yellow:#d29922; --orange:#d77337; --border:#30363d; --radius:8px; }
body { font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif; background:var(--bg); color:var(--fg); min-height:100vh; }
.header { background:var(--bg2); border-bottom:1px solid var(--border); padding:16px 24px; display:flex; align-items:center; gap:16px; flex-wrap:wrap; }
.header h1 { font-size:20px; font-weight:600; }
.header .sub { color:var(--fg2); font-size:13px; }
.header .badge { font-size:11px; padding:3px 10px; border-radius:10px; background:var(--bg3); border:1px solid var(--border); }
.tabs { display:flex; gap:0; background:var(--bg2); border-bottom:1px solid var(--border); padding:0 24px; }
.tab { padding:12px 20px; cursor:pointer; color:var(--fg2); font-size:14px; border-bottom:2px solid transparent; transition:all .15s; }
.tab:hover { color:var(--fg); background:var(--bg3); }
.tab.active { color:var(--accent); border-bottom-color:var(--accent); }
.content { padding:24px; max-width:1200px; margin:0 auto; display:none; }
.content.active { display:block; }
.btn { display:inline-flex; align-items:center; gap:8px; padding:10px 20px; border-radius:var(--radius); border:1px solid var(--border); cursor:pointer; font-size:14px; font-weight:500; transition:all .15s; background:var(--bg3); color:var(--fg); }
.btn:hover { background:var(--bg2); border-color:var(--fg2); }
.btn-primary { background:#238636; border-color:#2ea043; color:#fff; }
.btn-primary:hover { background:#2ea043; }
.btn-primary:disabled { opacity:.5; cursor:not-allowed; }
.stats-row { display:grid; grid-template-columns:repeat(auto-fit,minmax(200px,1fr)); gap:16px; margin-bottom:24px; }
.stat-card { background:var(--bg2); border:1px solid var(--border); border-radius:var(--radius); padding:16px; text-align:center; }
.stat-card .val { font-size:28px; font-weight:700; }
.stat-card .lbl { font-size:12px; color:var(--fg2); margin-top:4px; }
.step-list { display:flex; flex-direction:column; gap:4px; }
.step-row { display:flex; align-items:center; gap:12px; padding:8px 12px; background:var(--bg2); border:1px solid var(--border); border-radius:var(--radius); font-size:13px; transition:all .2s; }
.step-row .tag { color:var(--fg2); min-width:48px; }
.step-row .name { flex:1; font-weight:500; }
.step-row .phase { color:var(--fg2); font-size:11px; min-width:80px; }
.step-row .time { color:var(--fg2); font-size:11px; min-width:60px; text-align:right; }
.step-row .status { min-width:20px; text-align:center; }
.step-row.pending { opacity:.5; }
.step-row.running { border-color:var(--accent); background:var(--bg3); box-shadow:0 0 8px rgba(88,166,255,.15); }
.step-row.passed { border-color:var(--green); }
.step-row.failed { border-color:var(--red); }
.step-row.skipped { border-color:var(--yellow); opacity:.7; }
.spin { display:inline-block; width:14px; height:14px; border:2px solid var(--accent); border-top-color:transparent; border-radius:50%; animation:spin .6s linear infinite; }
@keyframes spin { to { transform:rotate(360deg); } }
.tree { font-size:13px; }
.tree-item { padding:4px 8px; cursor:pointer; border-radius:4px; display:flex; align-items:center; gap:8px; }
.tree-item:hover { background:var(--bg3); }
.tree-item .icon { color:var(--fg2); min-width:16px; }
.tree-item .sz { color:var(--fg2); font-size:11px; margin-left:auto; }
.tree-children { padding-left:24px; display:none; }
.tree-children.open { display:block; }
.scene-grid { display:grid; grid-template-columns:repeat(auto-fill,minmax(220px,1fr)); gap:12px; }
.scene-card { background:var(--bg2); border:1px solid var(--border); border-radius:var(--radius); padding:16px; }
.scene-card .name { font-weight:600; font-size:14px; }
.scene-card .meta { font-size:12px; color:var(--fg2); margin-top:4px; }
.mt-2 { margin-top:12px; }
.gap-2 { gap:8px; }
.flex { display:flex; }
.items-center { align-items:center; }
.wrap { flex-wrap:wrap; }
.error-msg { color:var(--red); font-size:13px; margin-top:4px; }
.skip-msg { color:var(--yellow); font-size:13px; margin-top:4px; }
.progress-bar { width:100%; height:4px; background:var(--bg3); border-radius:2px; overflow:hidden; margin-top:12px; }
.progress-bar .fill { height:100%; background:var(--accent); transition:width .3s; border-radius:2px; }
@media(max-width:600px){ .tabs{padding:0 12px;} .tab{padding:10px 12px;font-size:13px;} .content{padding:12px;} }
</style>
</head>
<body>
<div class="header">
  <h1>⚙ R&amp;C: Up Your Arsenal</h1>
  <span class="sub">Extraction Dashboard</span>
  <span class="badge" id="status-badge">checking...</span>
</div>
<div class="tabs" id="tabs">
  <div class="tab active" data-tab="pipeline">Pipeline</div>
  <div class="tab" data-tab="scenes">Scenes</div>
  <div class="tab" data-tab="assets">Assets</div>
  <div class="tab" data-tab="tools">Tools</div>
</div>

<div class="content active" id="tab-pipeline">
  <div class="stats-row" id="pipeline-stats">
    <div class="stat-card"><div class="val" id="stat-total">0</div><div class="lbl">Total Steps</div></div>
    <div class="stat-card"><div class="val" id="stat-passed" style="color:var(--green)">0</div><div class="lbl">Passed</div></div>
    <div class="stat-card"><div class="val" id="stat-failed" style="color:var(--red)">0</div><div class="lbl">Failed</div></div>
    <div class="stat-card"><div class="val" id="stat-time" style="color:var(--fg2)">&mdash;</div><div class="lbl">Duration</div></div>
  </div>
  <div id="pipeline-progress" class="progress-bar" style="display:none"><div class="fill" id="progress-fill" style="width:0%"></div></div>
  <div class="flex gap-2 items-center wrap mt-2">
    <button class="btn btn-primary" id="btn-run" onclick="startPipeline()">&#9654; Run Full Pipeline</button>
    <span id="pipeline-status" style="font-size:13px;color:var(--fg2)">Loading...</span>
  </div>
  <div class="step-list mt-2" id="step-list"></div>
</div>

<div class="content" id="tab-scenes">
  <div class="flex gap-2 items-center" style="margin-bottom:12px">
    <span style="font-size:15px;font-weight:600">Extracted Scenes</span>
    <span id="scene-count" style="font-size:13px;color:var(--fg2)"></span>
  </div>
  <div class="scene-grid" id="scene-grid"></div>
</div>

<div class="content" id="tab-assets">
  <div class="flex gap-2 items-center" style="margin-bottom:12px">
    <span style="font-size:15px;font-weight:600">Extracted Files</span>
    <span id="asset-summary" style="font-size:13px;color:var(--fg2)"></span>
  </div>
  <div class="tree" id="asset-tree"></div>
</div>

<div class="content" id="tab-tools">
  <div style="background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius);padding:16px;max-width:600px">
    <label style="font-size:13px;color:var(--fg2);display:block;margin-bottom:6px">WAD Repack</label>
    <p style="font-size:12px;color:var(--fg2);margin-bottom:10px">Recompresses unpacked data and rebuilds all level WADs.</p>
    <div class="flex gap-2 items-center wrap">
      <button class="btn btn-primary" id="btn-repack" onclick="startRepack()">&#9654; Repack All</button>
      <span id="repack-status" style="font-size:13px;color:var(--fg2)"></span>
    </div>
    <div id="repack-result" style="margin-top:10px;display:none;font-size:13px"></div>
  </div>

  <div style="background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius);padding:16px;max-width:600px;margin-top:16px">
    <label style="font-size:13px;color:var(--fg2);display:block;margin-bottom:6px">ISO Pack</label>
    <p style="font-size:12px;color:var(--fg2);margin-bottom:10px">Injects repacked WADs into the original ISO. Run Repack All first.</p>
    <div class="flex gap-2 items-center wrap">
      <button class="btn btn-primary" id="btn-iso-pack" onclick="startIsoPack()">&#9654; Build Repacked ISO</button>
      <span id="iso-pack-status" style="font-size:13px;color:var(--fg2)"></span>
    </div>
    <div id="iso-pack-result" style="margin-top:10px;display:none;font-size:13px"></div>
  </div>
</div>

<script>
// ── Tab switching ──
document.querySelectorAll('.tab').forEach(tab => {
  tab.addEventListener('click', () => {
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    document.querySelectorAll('.content').forEach(c => c.classList.remove('active'));
    tab.classList.add('active');
    document.getElementById('tab-' + tab.dataset.tab).classList.add('active');
    if (tab.dataset.tab === 'scenes') loadScenes();
    if (tab.dataset.tab === 'assets') loadAssets();
  });
});

// ── Pipeline ──
const PIPELINE = [
  {name:"toc",phase:"ISO Extraction"},{name:"wad-extract",phase:"ISO Extraction"},
  {name:"wad-unpack",phase:"WAD Processing"},{name:"wad-label",phase:"WAD Processing"},
  {name:"global-wad",phase:"Global WAD Assets"},{name:"hud-texture",phase:"Global WAD Assets"},
  {name:"bonus",phase:"Global WAD Assets"},{name:"misc",phase:"Global WAD Assets"},
  {name:"core",phase:"Level Core Data"},{name:"texture",phase:"Level Core Data"},
  {name:"gsram",phase:"Level Core Data"},{name:"audio",phase:"Level Core Data"},
  {name:"moby",phase:"Mesh Extraction"},{name:"tie",phase:"Mesh Extraction"},
  {name:"shrub",phase:"Mesh Extraction"},{name:"tfrag",phase:"Mesh Extraction"},
  {name:"stash",phase:"Mesh Extraction"},{name:"armor",phase:"Special Data"},
  {name:"armor-mesh",phase:"Special Data"},{name:"gadget",phase:"Special Data"},
  {name:"gadget-texture",phase:"Special Data"},{name:"gameplay",phase:"Special Data"},
  {name:"collision",phase:"Special Data"},{name:"hud-layout",phase:"Special Data"},
  {name:"wad-repack",phase:"Repack & Export"},
  {name:"iso-pack",phase:"Repack & Export"},
  {name:"scene",phase:"Scene Assembly"},
  {name:"space-mesh-decode",phase:"Scene Assembly"},{name:"space-mesh-export",phase:"Scene Assembly"},
];

let stepStates = {};
let startTime = null;
let timerInterval = null;

function renderSteps() {
  const list = document.getElementById('step-list');
  list.innerHTML = '';
  const total = PIPELINE.length;
  PIPELINE.forEach((s, i) => {
    const st = stepStates[i] || {status:'pending',msg:'',time:0};
    const row = document.createElement('div');
    row.className = 'step-row ' + st.status;
    row.id = 'step-' + i;
    const elapsed = st.time ? (st.time/1000).toFixed(1) + 's' : '';
    row.innerHTML = [
      '<span class="tag">[' + (i+1) + '/' + total + ']</span>',
      '<span class="name">' + s.name + '</span>',
      '<span class="phase">' + s.phase + '</span>',
      '<span class="time">' + elapsed + '</span>',
      '<span class="status">' + statusIcon(st.status) + '</span>',
    ].join('');
    if (st.msg && st.status === 'failed') {
      const msg = document.createElement('div');
      msg.className = 'error-msg';
      msg.textContent = st.msg;
      row.appendChild(msg);
    }
    if (st.msg && st.status === 'skipped') {
      const msg = document.createElement('div');
      msg.className = 'skip-msg';
      msg.textContent = '\u2192 ' + st.msg;
      row.appendChild(msg);
    }
    list.appendChild(row);
  });
  updateStats();
}

function statusIcon(s) {
  if (s === 'running') return '<span class="spin"></span>';
  if (s === 'passed') return '<span style="color:var(--green)">&#10003;</span>';
  if (s === 'failed') return '<span style="color:var(--red)">&#10007;</span>';
  if (s === 'skipped') return '<span style="color:var(--yellow)">&#8594;</span>';
  return '<span style="color:var(--fg2)">&#9679;</span>';
}

function updateStats() {
  let passed = 0, failed = 0, running = 0, skipped = 0, pending = 0;
  Object.values(stepStates).forEach(s => {
    if (s.status === 'passed') passed++;
    else if (s.status === 'failed') failed++;
    else if (s.status === 'running') running++;
    else if (s.status === 'skipped') skipped++;
    else pending++;
  });
  document.getElementById('stat-total').textContent = PIPELINE.length;
  document.getElementById('stat-passed').textContent = passed + skipped;
  document.getElementById('stat-failed').textContent = failed;
  if (startTime) {
    const d = Date.now() - startTime;
    const m = Math.floor(d / 60000);
    const s = Math.floor((d % 60000) / 1000);
    document.getElementById('stat-time').textContent = m + 'm' + s.toString().padStart(2,'0') + 's';
  }
  // Progress bar
  const done = passed + failed + skipped;
  const pct = PIPELINE.length > 0 ? Math.round(done / PIPELINE.length * 100) : 0;
  const bar = document.getElementById('pipeline-progress');
  const fill = document.getElementById('progress-fill');
  if (done > 0 || running > 0) { bar.style.display = 'block'; fill.style.width = pct + '%'; }
}

async function checkStatus() {
  try {
    const resp = await fetch('/api/status');
    const st = await resp.json();
    const parts = [];
    if (st.extracted) parts.push('WADs extracted');
    if (st.unpacked) parts.push('unpacked');
    if (st.meshes) parts.push('meshes');
    if (st.scenes) parts.push('scenes ready');
    const badge = document.getElementById('status-badge');
    if (parts.length > 0) {
      badge.textContent = '\u2713 ' + parts.join(', ');
    } else {
      badge.textContent = 'ready';
    }
  } catch(e) {}
}

async function startPipeline() {
  if (document.getElementById('btn-run').disabled) return;
  document.getElementById('btn-run').disabled = true;
  document.getElementById('pipeline-status').textContent = 'Starting...';
  stepStates = {};
  startTime = Date.now();
  timerInterval = setInterval(updateStats, 1000);
  renderSteps();

  // Connect EventSource FIRST and wait for it to open before starting pipeline
  const evtSource = new EventSource('/api/pipeline/events');
  try {
    await new Promise((resolve, reject) => {
      evtSource.onopen = resolve;
      evtSource.onerror = () => reject(new Error('SSE connection failed'));
      setTimeout(() => resolve(), 2000);
    });
  } catch(_) {
    clearInterval(timerInterval);
    timerInterval = null;
    /* continue even if SSE fails */
  }
  evtSource.onmessage = (e) => {
    let data;
    try { data = JSON.parse(e.data); } catch(ex) { return; }
    if (data.status === 'done') {
      evtSource.close();
      clearInterval(timerInterval);
      timerInterval = null;
      document.getElementById('btn-run').disabled = false;
      document.getElementById('pipeline-status').textContent = 'Complete';
      return;
    }
    const idx = data.step - 1;
    stepStates[idx] = { status: data.status, msg: data.message || '', time: data.elapsed_ms };
    const row = document.getElementById('step-' + idx);
    if (row) {
      row.className = 'step-row ' + data.status;
      const statusSpan = row.querySelector('.status');
      if (statusSpan) statusSpan.innerHTML = statusIcon(data.status);
      const timeSpan = row.querySelector('.time');
      if (timeSpan && data.elapsed_ms) timeSpan.textContent = (data.elapsed_ms/1000).toFixed(1) + 's';
      // Clear old messages
      row.querySelectorAll('.error-msg, .skip-msg').forEach(el => el.remove());
      if (data.message && data.status === 'failed') {
        const msg = document.createElement('div');
        msg.className = 'error-msg';
        msg.textContent = data.message;
        row.appendChild(msg);
      }
      if (data.message && data.status === 'skipped') {
        const msg = document.createElement('div');
        msg.className = 'skip-msg';
        msg.textContent = '\u2192 ' + data.message;
        row.appendChild(msg);
      }
    }
    updateStats();
  };
  evtSource.onerror = () => {
    evtSource.close();
    clearInterval(timerInterval);
    timerInterval = null;
    document.getElementById('btn-run').disabled = false;
    document.getElementById('pipeline-status').textContent = 'Connection lost';
  };

  const resp = await fetch('/api/pipeline/start', { method: 'POST' });
  if (!resp.ok) {
    evtSource.close();
    clearInterval(timerInterval);
    timerInterval = null;
    document.getElementById('btn-run').disabled = false;
    document.getElementById('pipeline-status').textContent = 'Error: ' + await resp.text();
  }
}

// ── Scenes ──
async function loadScenes() {
  const resp = await fetch('/api/scenes');
  const scenes = await resp.json();
  document.getElementById('scene-count').textContent = '(' + scenes.length + ' total)';
  const grid = document.getElementById('scene-grid');
  grid.innerHTML = '';
  if (scenes.length === 0) {
    grid.innerHTML = '<div style="color:var(--fg2);padding:24px;text-align:center">No scenes extracted yet. Run the pipeline first.</div>';
    return;
  }
  scenes.forEach(s => {
    const card = document.createElement('div');
    card.className = 'scene-card';
    const size = s.size > 1048576 ? (s.size/1048576).toFixed(1)+'MB' : s.size > 1024 ? (s.size/1024).toFixed(1)+'KB' : s.size+'B';
    card.innerHTML = '<div class="name">' + s.name + '</div><div class="meta">' + s.files + ' files &middot; ' + size + '</div>';
    grid.appendChild(card);
  });
}

// ── Assets ──
async function loadAssets() {
  const resp = await fetch('/api/assets');
  const tree = await resp.json();
  const container = document.getElementById('asset-tree');
  container.innerHTML = '';
  let totalFiles = 0, totalSize = 0;
  function countAll(items) {
    items.forEach(i => {
      if (i.type === 'file') { totalFiles++; totalSize += i.size; }
      else if (i.children) countAll(i.children);
    });
  }
  countAll(tree);
  const sz = totalSize > 1048576 ? (totalSize/1048576).toFixed(1)+'MB' : totalSize > 1024 ? (totalSize/1024).toFixed(1)+'KB' : totalSize+'B';
  document.getElementById('asset-summary').textContent = '(' + totalFiles + ' files &middot; ' + sz + ')';
  renderTree(tree, container);
}

function renderTree(items, parent) {
  items.sort((a,b) => {
    if (a.type !== b.type) return a.type === 'dir' ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
  items.forEach(item => {
    const div = document.createElement('div');
    if (item.type === 'dir') {
      div.className = 'tree-item';
      const sz = item.size > 1048576 ? (item.size/1048576).toFixed(1)+'MB' : item.size > 1024 ? (item.size/1024).toFixed(1)+'KB' : item.size+'B';
      div.innerHTML = '<span class="icon">&#128193;</span><span>' + item.name + '</span><span class="sz">' + item.files + ' files &middot; ' + sz + '</span>';
      div.onclick = () => {
        const child = div.nextElementSibling;
        if (child) child.classList.toggle('open');
      };
      parent.appendChild(div);
      const children = document.createElement('div');
      children.className = 'tree-children';
      if (item.children) renderTree(item.children, children);
      parent.appendChild(children);
    } else {
      div.className = 'tree-item';
      const sz = item.size > 1048576 ? (item.size/1048576).toFixed(1)+'MB' : item.size > 1024 ? (item.size/1024).toFixed(1)+'KB' : item.size+'B';
      const icons = {obj:'&#128512;',mtl:'&#127912;',png:'&#128444;',bin:'&#128230;',json:'&#128203;',txt:'&#128196;',bmp:'&#128444;'};
      const icon = icons[item.ext] || '&#128196;';
      div.innerHTML = '<span class="icon">' + icon + '</span><span>' + item.name + '</span><span class="sz">' + sz + '</span>';
      parent.appendChild(div);
    }
  });
}

// ── Tools ──
async function startRepack() {
  const btn = document.getElementById('btn-repack');
  const status = document.getElementById('repack-status');
  const result = document.getElementById('repack-result');
  btn.disabled = true;
  status.textContent = 'Starting...';
  result.style.display = 'none';

  const evtSource = new EventSource('/api/tools/events');
  evtSource.onmessage = (e) => {
    let data;
    try { data = JSON.parse(e.data); } catch(ex) { return; }
    if (data.kind !== 'repack') return;
    if (data.status === 'running') {
      status.textContent = data.message;
    } else if (data.status === 'done') {
      evtSource.close();
      status.textContent = data.message;
      result.innerHTML = '<span style="color:var(--green)">' + data.message + '</span>';
      result.style.display = 'block';
      btn.disabled = false;
    } else if (data.status === 'error') {
      evtSource.close();
      status.textContent = 'Error';
      result.innerHTML = '<span style="color:var(--red)">' + data.message + '</span>';
      result.style.display = 'block';
      btn.disabled = false;
    }
  };
  evtSource.onerror = () => { evtSource.close(); btn.disabled = false; status.textContent = 'Connection lost'; };

  const resp = await fetch('/api/tools/repack', { method: 'POST' });
  if (!resp.ok) {
    evtSource.close();
    status.textContent = 'Error: ' + await resp.text();
    btn.disabled = false;
  }
}

async function startIsoPack() {
  const btn = document.getElementById('btn-iso-pack');
  const status = document.getElementById('iso-pack-status');
  const result = document.getElementById('iso-pack-result');
  btn.disabled = true;
  status.textContent = 'Starting...';
  result.style.display = 'none';

  const evtSource = new EventSource('/api/tools/events');
  evtSource.onmessage = (e) => {
    let data;
    try { data = JSON.parse(e.data); } catch(ex) { return; }
    if (data.kind !== 'iso-pack') return;
    if (data.status === 'running') {
      status.textContent = data.message;
    } else if (data.status === 'done') {
      evtSource.close();
      status.textContent = data.message;
      result.innerHTML = '<span style="color:var(--green)">' + data.message + '</span>';
      result.style.display = 'block';
      btn.disabled = false;
    } else if (data.status === 'error') {
      evtSource.close();
      status.textContent = 'Error';
      result.innerHTML = '<span style="color:var(--red)">' + data.message + '</span>';
      result.style.display = 'block';
      btn.disabled = false;
    }
  };
  evtSource.onerror = () => { evtSource.close(); btn.disabled = false; status.textContent = 'Connection lost'; };

  const resp = await fetch('/api/tools/iso-pack', { method: 'POST' });
  if (!resp.ok) {
    evtSource.close();
    status.textContent = 'Error: ' + await resp.text();
    btn.disabled = false;
  }
}

// ── Init ──
checkStatus();
renderSteps();
</script>
</body>
</html>"###;
