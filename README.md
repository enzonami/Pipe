# rac_tools

Extraction and 3D viewer toolset for **Ratchet & Clank: Up Your Arsenal** (PS2).

## Quick Start

```bash
# Build
cargo build --release

# Full extraction pipeline (place ISO first)
cp /path/to/Ratchet\ \&\ Clank\ -\ Up\ Your\ Arsenal.iso ./place-ISO-here/
./target/release/rac_tools pipeline

# Launch 3D viewer
./target/release/rac_tools viewer
```

## Requirements

- **Rust** 2024 edition (MSRV: stable)
- **ISO file**: `Ratchet & Clank - Up Your Arsenal.iso` in `place-ISO-here/` or project root

## Usage

### Full Pipeline

Extracts everything in order — TOC, WADs, meshes, textures, audio, etc.:

```bash
./target/release/rac_tools pipeline
```

Runs: toc → wad-extract → wad-unpack → global-wad → core → texture → moby → tie → shrub → tfrag → stash → gsram → audio → bonus → armor → gadget → gadget-texture → gameplay → hud-texture → hud-layout → misc → scene → space-mesh-decode → space-mesh-export

Progress is tracked with marker files in `extracted/.step_*` — completed steps are skipped on re-run.

### 3D Viewer

Serves an interactive 3D viewer on `http://localhost:8000`:

```bash
# Default port 8000
./target/release/rac_tools viewer

# Custom port
./target/release/rac_tools viewer --port 3000

# Load a specific level at startup
./target/release/rac_tools viewer LEVEL03
```

### Individual Commands

| Command | Description |
|---------|-------------|
| `toc` | Parse ISO TOC and list WAD files |
| `wad-extract` | Extract WAD files from ISO |
| `wad-unpack` | Unpack WAD files into structured data |
| `wad-label` | Label/annotate WAD structure |
| `core` | Extract level core data |
| `texture` | Extract texture data |
| `moby` | Extract moby meshes |
| `tie` | Extract tie meshes |
| `shrub` | Extract shrub meshes |
| `tfrag` | Extract tfrag meshes |
| `stash` | Extract stash entries |
| `gsram` | Extract GSRAM vertex data |
| `audio` | Extract audio WADs |
| `bonus` | Extract bonus/demo data |
| `armor` / `armor-mesh` | Extract/decode armor data |
| `gadget` / `gadget-texture` | Extract gadgets and textures |
| `gameplay` | Extract gameplay data |
| `hud-texture` / `hud-layout` | HUD assets |
| `misc` | Extract misc data |
| `scene` | Assemble scene from extracted meshes |
| `space-mesh-decode` / `space-mesh-export` | Space mesh extraction |
| `global-wad` | Analyze global WAD structure |

## Output Structure

```
extracted/
├── .step_*              # Pipeline progress markers (auto-created)
├── WAD/                 # Extracted WAD files
│   └── GLOBAL/
├── unpacked/            # Unpacked structured data
│   ├── GLOBAL/
│   └── LEVEL*/
├── textures/            # Extracted textures (PNG)
│   ├── HUD/
│   └── LEVEL*/
├── meshes/              # Extracted mesh data
│   └── LEVEL*/
├── scenes/              # Assembled scenes
│   └── LEVEL*/
├── audio/               # Extracted audio files
└── ...                  # Other extracted assets
```

## Build

```bash
cargo build --release
```

The binary is optimized with LTO, `opt-level="z"`, and debug info stripped:
`./target/release/rac_tools` (~4MB).

## License

Internal tool — no license specified.
