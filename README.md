# rac_tools

Extraction, repack, and 3D viewer toolset for **Ratchet & Clank: Up Your Arsenal** (PS2, NTSC-U).

Written in Rust. Parses the ISO image's TOC, extracts and decompresses WAD archives, decodes GS textures (PIF8/2FIP), reconstructs 3D meshes from VIF command streams, assembles full scenes, and can repack modified data back into a playable ISO.

## Quick Start

```bash
# Build (release with LTO, stripped)
cargo build --release

# Place ISO in project root, then run full pipeline
./rac-pipeline.sh

# Launch 3D viewer
./rac-viewer.sh
```

## Requirements

- **Rust** 2024 edition (MSRV: stable — tested with 1.85+)
- **ISO file**: `Ratchet & Clank - Up Your Arsenal.iso` in `place-ISO-here/` or project root
- **PCSX2** (optional, for emulator launch from viewer)

## Build

```bash
cargo build --release
```

Produces `./target/release/rac_tools` (~4MB, LTO + `opt-level="z"` + stripped).

All 34 commands are available in both debug and release builds. Debug build (`cargo build`) adds debug symbols and disables LTO.

## Usage

### Full Pipeline

Runs all extraction steps in dependency order:

```bash
./rac-pipeline.sh
# or directly:
cargo run --release -- pipeline
```

**29 steps across 8 phases:**

| Phase | Steps |
|-------|-------|
| **ISO Extraction** | `toc` → `wad-extract` |
| **WAD Processing** | `wad-unpack` → `wad-label` |
| **Global WAD Assets** | `global-wad` → `hud-texture` → `bonus` → `misc` |
| **Level Core Data** | `core` → `texture` → `gsram` → `audio` |
| **Mesh Extraction** | `moby` → `tie` → `shrub` → `tfrag` → `stash` |
| **Special Data** | `armor` → `armor-mesh` → `gadget` → `gadget-texture` → `gameplay` → `collision` → `hud-layout` |
| **Repack & Export** | `wad-repack` → `iso-pack` |
| **Scene Assembly** | `scene` → `space-mesh-decode` → `space-mesh-export` |

The pipeline uses `.step_*` marker files in `extracted/` to track progress. Re-running skips completed steps unless the marker is removed.

### Repack & Build ISO

Recompresses unpacked data, rebuilds level WADs with correct headers, and injects them into a copy of the original ISO (in-place sector replacement, padded if smaller):

```bash
./rac-repack.sh            # Recompress and rebuild all level WADs
./rac-iso-pack.sh          # Build repacked ISO
```

Output: `extracted/repacked.iso`

### 3D Viewer

Serves a web dashboard on `http://localhost:8000` with pipeline control, scene browser, asset tree, repack/ISO tools, and PCSX2 launcher:

```bash
./rac-viewer.sh
# or with custom port:
cargo run --release -- viewer --port 3000
```

## All Commands

| Command | Args | Description |
|---------|------|-------------|
| `pipeline` | *(none)* | Run full 29-step extraction pipeline |
| `toc` | `[iso_path]` | Parse ISO TOC at LBA 1001, list global WADs and level table |
| `wad-extract` | `[iso_path]` `--extract` | Extract WAD files from ISO (global + per-level audio/level/scene) |
| `wad-unpack` | `[level]` | Unpack WAD into structured sections (overlay, core, GS RAM, HUD, gameplay, etc.) |
| `wad-label` | `[level]` | Print WAD header info, decompression stats, core header fields |
| `core` | `[level]` | Extract level core data: `core_header.json`, moby/tie/shrub class entries |
| `texture` | `[level]` `[category]` | Decode GS textures → PNG (categories: tfrag, moby, tie, shrub, part, fx) |
| `moby` | `[level]` `[class]` | Extract moby meshes → OBJ+MTL (VIF parsing, triangle strips, per-class) |
| `tie` | `[level]` `[class]` | Extract tie meshes → OBJ (LOD packets, GIF tags, per-texture materials) |
| `shrub` | `[level]` `[class]` | Extract shrub meshes → OBJ (quad strips, billboard, UV normalization) |
| `tfrag` | `[level]` `[tfrag_index]` `--lod` | Extract tfrag terrain meshes → OBJ (LOD0/1/2, ADGIF material markers) |
| `stash` | `[level]` | Extract GS stash geometry (5 auto-detected formats, VIF parsing, dedup verts) |
| `gsram` | `[level]` | Dump GS RAM entries → JSON with hex dumps |
| `audio` | `[level]` | Extract VAG audio tracks → `.vag` files (validates VAGp magic, names from header) |
| `bonus` | *(none)* | Extract bonus WAD: credits, demo menu, cheat/skill images (PIF8/RGBA) |
| `armor` | *(none)* | Extract armor WAD: 29 armors, 6 wrenches, 21 MP armors, 2 Clank textures |
| `armor-mesh` | `--all` `[path]` | Armor mesh decoder (acknowledges execution) |
| `gadget` | *(none)* | Extract gadget WAD: 24 groups × 5 entries each, LZ + SBlk decoding |
| `gadget-texture` | *(none)* | Decode gadget textures → PNG icons + JSON (pal4/pal8, GS swizzle) |
| `collision` | *(none)* | Export collision mesh → OBJ (octree parsing, hero collision groups) |
| `gameplay` | `[level]` | Parse gameplay.bin → JSON (38 blocks: cameras, lights, instances, paths, etc.) |
| `hud-texture` | *(none)* | Decode HUD textures → PNG (2FIP format, 256-color palette, font atlas blocks) |
| `hud-layout` | *(none)* | HUD layout analysis (acknowledges execution) |
| `misc` | *(none)* | Extract MISC global WAD: 6 sub-files (2FIP, WAD LZ, PS2D, structured data) |
| `scene` | `[level]` `--no-tie/shrub/moby/tfrag` | Assemble scene.obj from meshes + gameplay transforms (weighted normals) |
| `space-mesh-decode` | `[block_index]` `--input` | Decode SPACE mesh blocks (magic 0x000009D8) → JSON analysis |
| `space-mesh-export` | `[block_index]` `--input` `--output` | Export SPACE meshes → OBJ (vertex clusters, dedup by raw bytes) |
| `global-wad` | *(none)* | Analyze SPACE/HUD/BONUS/MISC WADs: scan/decompress/classify blocks |
| `viewer` | `[level]` `--port` | Start web dashboard + 3D viewer (axum server) |
| `decompress` | `<input>` `[output]` | Standalone WAD LZ decompress (detects zlib/WAD LZ, output defaults to `.bin`) |
| `wad-compress` | `<input>` `[output]` | Standalone WAD LZ compress (output defaults to `.lz`) |
| `wad-repack` | `--level/-l N` `--all` | Rebuild level WAD from unpacked data (recompress + reassemble) |
| `iso-pack` | `[output]` `--input-dir` | Inject repacked WADs into ISO, update TOC + headers |
| `texture-pack` | `<input>` `[output]` | Pack PNG → PIF8 (256-color NeuQuant quantize, GS palette swizzle) |

**Legend:** `<required>` `[optional]` — `level` = 0–13 (-1 = all levels)

### Shell Scripts

| Script | Runs | Equivalent Command |
|--------|------|-------------------|
| `rac-pipeline.sh` | Full extraction pipeline | `cargo run --release -- pipeline` |
| `rac-repack.sh` | Rebuild all WADs | `cargo run --release -- wad-repack --all` |
| `rac-iso-pack.sh` | Build repacked ISO | `cargo run --release -- iso-pack` |
| `rac-viewer.sh` | Launch 3D viewer (port 8000) | `cargo run --release -- viewer --port 8000` |

All scripts `cd` to the project root and exec `./target/release/rac_tools`.

## Output Structure

```
extracted/
├── WAD/                     # Extracted WAD files from ISO
│   ├── GLOBAL/              #   Global WADs (SPACE, HUD, BONUS, MISC, ARMOR, AUDIO, GADGET)
│   ├── LEVEL*_level.wad     #   Level data WADs
│   ├── LEVEL*_scene.wad     #   Level scene WADs
│   └── LEVEL*_audio.wad     #   Level audio WADs
├── unpacked/                # WAD-LZ decompressed structured data
│   ├── GLOBAL/              #   Global WAD block data
│   │   ├── SPACE/           #     Decompressed SPACE blocks
│   │   ├── HUD/             #     Decompressed HUD blocks
│   │   ├── BONUS/           #     Decompressed BONUS blocks
│   │   └── MISC/            #     Decompressed MISC blocks
│   └── LEVEL*/              #   Per-level data
│       ├── data_wad/        #     11 section .bin files (overlay, core, gs_ram, hud, etc.)
│       └── gameplay.bin     #     Gameplay data (or .dec if decompressed)
├── repacked/                # Rebuilt WAD files (wad-repack output)
├── repacked.iso             # Repacked ISO image (iso-pack output)
├── toc.json                 # Table of Contents parsed from ISO
├── textures/                # Decoded GS textures (PNG)
│   ├── HUD/                 #   HUD textures
│   └── LEVEL*/              #   Per-level textures
│       ├── tfrag/
│       ├── moby/
│       ├── tie/
│       ├── shrub/
│       ├── part/
│       └── fx/
├── meshes/                  # Extracted 3D meshes (OBJ + MTL)
│   ├── LEVEL*/              #   Per-level meshes
│   │   ├── moby_*.obj
│   │   ├── tie_*.obj
│   │   ├── shrub_*.obj
│   │   ├── tfrag_*.obj
│   │   └── stash_*.obj
│   └── SPACE/               #   Space mesh OBJ exports
├── scenes/                  # Assembled full scenes (OBJ + MTL)
│   └── LEVEL*/              #   Per-level scene.obj + scene.mtl
│       └── textures/        #     Referenced textures for scene
├── collision/               # Collision mesh exports
│   └── LEVEL*/              #   Per-level collision.obj
├── audio/                   # Extracted VAG audio tracks
│   └── LEVEL*/              #   Per-level .vag files
├── armor/                   # Armor data
│   ├── armors/
│   ├── wrenches/
│   ├── multiplayer_armors/
│   └── clank_textures/
├── bonus/                   # Bonus/demo data
│   ├── credits_text/
│   ├── credits_images/
│   ├── demo_menu/
│   ├── demo_exit/
│   ├── cheat_images/
│   ├── skill_images/
│   └── trophy_image/
├── core/                    # Level core data
│   └── LEVEL*/              #   Per-level core_header.json
├── gadget_icons/            # Gadget icon textures (g00-g12.png)
├── hud_layouts/             # HUD layout analysis output
├── gameplay/                # Gameplay JSON dumps
├── gs_ram/                  # GS RAM entry dumps
└── .step_*                  # Pipeline step marker files
```

## Technical Details

### WAD Format

Three compression variants handled:

| Type | Magic | Compression | Detection |
|------|-------|-------------|-----------|
| Standard WAD | `WAD\0` / `WAD\x01` | zlib on data block | First 4 bytes |
| WAD LZ | `WAD` + 13-byte hdr | Custom LZ77 | Header + size check |
| Raw zlib | `0x78` prefix | zlib stream | First byte |

**WAD LZ** uses a hash-chain based LZ77 with three match types:

| Type | Max Distance | Max Length | Encoding |
|------|-------------|------------|----------|
| Little | 2,048 | 7 | 2 bytes |
| Medium | 16,384 | 288 | 3 bytes |
| Far | 32,767 | 264 | 3 bytes (bit3=1 encoding is **broken** in PS2 decompressor, limited to `0x7FFF`) |

### Texture Formats

| Format | Description |
|--------|-------------|
| **2FIP/PIF2** | PS2 palettized texture: 32-byte header + 256×4 palette (1024B) + index data. Magic `2FIP`. |
| **PIF8** | 8-bit palettized (format 0x13). Per-pixel byte index into 256-entry RGBA palette. |
| **GS 4-bit** | Swizzled 4-bit palettized (format 0x94). Two pixels per byte, GS 8×8 block swizzle. |
| **SBlk** | Gadget texture block format. |
| **Raw RGBA** | Simple w×h header + RGBA pixel data. |

**GS Memory Swizzle:**
- Pixel indices: 8×8 block swizzle (`map_pixel_index_rac4`) — pixels laid out in 8×8 tiles within each 64-pixel page
- Palette: CSM=1 mode swaps bits 3 and 4 (`map_palette_index`) — palette entries stored in swizzled order
- Alpha: PS2 stores 0–128, scaled to 0–255 on extraction (`multiply_alphas`)

### VIF (Vector InterFace) Parsing

Commands are decoded from 32-bit VIF codes in the VU1 microprogram data. Supported UNPACK formats: V4_32, V3_32, V4_16, V3_16, V4_8, V2_16, V4_5. Mobies use a custom parser (`read_vif_command_list_moby`) that handles 0x52 RAW_DATA and a "7-vertex-ahead" quirk in triangle strip reconstruction.

### Pipeline Tracking

The pipeline writes `.step_<name>` marker files to `extracted/` after each successful step. The viewer and pipeline runner check for these markers (plus fallback directory heuristics) to skip already-completed steps. Removing a marker file forces that step to re-run.

### `debug_diff.rs`

A standalone binary (`src/debug_diff.rs`) for testing WAD LZ compression round-trip accuracy. Reads a `.bin` file, compresses with `compress_wad_lz`, decompresses with `decompress_wad_lz`, and reports the first byte difference (if any). Not a command — run directly with `cargo run --bin debug_diff`.

## License

Internal tool — no license specified.
