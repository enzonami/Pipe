# rac_tools

Extraction, repack, and 3D viewer toolset for **Ratchet & Clank: Up Your Arsenal** (PS2).

## Quick Start

```bash
# Build
cargo build --release

# Place ISO in project root, then run full pipeline
./rac-pipeline.sh

# Launch 3D viewer
./rac-viewer.sh
```

## Requirements

- **Rust** 2024 edition (MSRV: stable)
- **ISO file**: `Ratchet & Clank - Up Your Arsenal.iso` in the project root

## Usage

### Full Pipeline

Extracts everything in order — TOC, WADs, meshes, textures, audio, collision, etc.:

```bash
./rac-pipeline.sh
# or directly:
./target/debug/rac_tools pipeline
```

**29 steps** across 8 phases:
1. **ISO Extraction**: toc → wad-extract
2. **WAD Processing**: wad-unpack → wad-label
3. **Global WAD Assets**: global-wad → hud-texture → bonus → misc
4. **Level Core Data**: core → texture → gsram → audio
5. **Mesh Extraction**: moby → tie → shrub → tfrag → stash
6. **Special Data**: armor → armor-mesh → gadget → gadget-texture → gameplay → **collision** → hud-layout
7. **Repack & Export**: wad-repack → iso-pack
8. **Scene Assembly**: scene → space-mesh-decode → space-mesh-export

### Repack & Build ISO

Recompresses unpacked data and rebuilds all WADs, then injects them into a new ISO:

```bash
./rac-repack.sh            # Recompress and rebuild all level WADs
./rac-iso-pack.sh          # Build repacked ISO
```

Output: `extracted/repacked.iso`

### 3D Viewer

Serves an interactive 3D viewer on `http://localhost:8000`:

```bash
./rac-viewer.sh
# or with custom port:
./target/debug/rac_tools viewer --port 3000
```

The viewer also has a **Tools** tab for running repack and ISO pack from the browser.

### Individual Commands

| Command | Description |
|---------|-------------|
| `pipeline` | Run full extraction (all 29 steps) |
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
| **`collision`** | Export collision mesh as OBJ |
| `gameplay` | Extract gameplay data |
| `hud-texture` / `hud-layout` | HUD assets |
| `misc` | Extract misc data |
| `scene` | Assemble scene from extracted meshes |
| `space-mesh-decode` / `space-mesh-export` | Space mesh extraction |
| `global-wad` | Analyze global WAD structure |
| `viewer` | Start 3D viewer (HTTP server) |
| `decompress` | Decompress a WAD LZ file (standalone) |
| `wad-compress` | Compress a file into WAD LZ format (standalone) |
| `wad-repack` | Repack WAD files from unpacked data |
| `iso-pack` | Pack repacked WADs into a new ISO |
| `texture-pack` | Pack a PNG image into PIF8 format |

### Shell Scripts

| Script | Runs |
|--------|------|
| `rac-pipeline.sh` | Full extraction pipeline (all 29 steps) |
| `rac-repack.sh` | Recompress unpacked data and rebuild WADs |
| `rac-iso-pack.sh` | Build repacked ISO from rebuilt WADs |
| `rac-viewer.sh` | Launch 3D web viewer on port 8000 |

## Output Structure

```
extracted/
├── WAD/                 # Extracted WAD files
│   └── GLOBAL/
├── unpacked/            # Unpacked structured data
│   ├── GLOBAL/
│   └── LEVEL*/
├── repacked/            # Rebuilt WAD files (wad-repack output)
│   └── LEVEL*_level.wad
├── repacked.iso         # Repacked ISO image (iso-pack output)
├── textures/            # Extracted textures (PNG)
│   ├── HUD/
│   └── LEVEL*/
├── meshes/              # Extracted mesh data
│   └── LEVEL*/
├── collision/           # Collision mesh OBJ exports
│   └── LEVEL*/
├── scenes/              # Assembled scenes
│   └── LEVEL*/
├── audio/               # Extracted audio files
├── armor/               # Armor mesh data
├── bonus/               # Bonus/demo data
├── core/                # Level core data
├── gadget_icons/        # Gadget icon textures
└── ...                  # Other extracted assets
```

## Build

```bash
cargo build --release
```

The binary is optimized with LTO, `opt-level="z"`, and debug info stripped:
`./target/release/rac_tools` (~4MB).

Note: `wad-repack`, `iso-pack`, and `viewer` are currently only in the debug build.
Use `cargo build --bin rac_tools` (debug) for those commands.

## License

Internal tool — no license specified.
