//! SPACE Mesh Exporter — finds vertex clusters and exports to OBJ format.

use std::path::Path;
use crate::cli::*;
use crate::common::r_u32;

const VERTEX_STRIDE: usize = 0x20;
const MIN_CLUSTER: usize = 160;

/// Validate that bytes at offset form a plausible f32 vertex position
fn is_valid_vertex(data: &[u8], off: usize) -> bool {
    if off + 12 > data.len() { return false; }
    let b: [u8; 4] = data[off..off+4].try_into().unwrap_or([0;4]);
    let x = f32::from_le_bytes(b);
    let b: [u8; 4] = data[off+4..off+8].try_into().unwrap_or([0;4]);
    let y = f32::from_le_bytes(b);
    let b: [u8; 4] = data[off+8..off+12].try_into().unwrap_or([0;4]);
    let z = f32::from_le_bytes(b);
    x > -10000.0 && x < 10000.0
        && y > -10000.0 && y < 10000.0
        && z > -10000.0 && z < 10000.0
        && !(x.abs() < 0.001 && y.abs() < 0.001 && z.abs() < 0.001)
}

fn read_f32_at(data: &[u8], off: usize) -> f32 {
    let b: [u8; 4] = data[off..off+4].try_into().unwrap_or([0;4]);
    f32::from_le_bytes(b)
}

/// Extract vertex clusters from sections 1-3 (skipping section 0 float descriptors).
fn extract_vertex_clusters(data: &[u8], section_offsets: &[u32]) -> Vec<(usize, usize)> {
    let mut scan_regions: Vec<(usize, usize)> = Vec::new();
    for i in 1..section_offsets.len().min(4) {
        let start = section_offsets[i] as usize;
        let end = if i + 1 < section_offsets.len() {
            section_offsets[i + 1] as usize
        } else {
            data.len()
        };
        scan_regions.push((start, end.min(data.len())));
    }

    let mut clusters = Vec::new();
    for &(rstart, rend) in &scan_regions {
        let region = &data[rstart..rend];
        let mut in_cluster = false;
        let mut cluster_start = 0usize;

        for off in (0..rend - rstart - 12).step_by(VERTEX_STRIDE) {
            let valid = is_valid_vertex(region, off);
            if valid && !in_cluster {
                cluster_start = rstart + off;
                in_cluster = true;
            } else if !valid && in_cluster {
                let size = (rstart + off) - cluster_start;
                if size >= MIN_CLUSTER {
                    clusters.push((cluster_start, size));
                }
                in_cluster = false;
            }
        }
        if in_cluster {
            let size = (rstart + region.len()) - cluster_start;
            if size >= MIN_CLUSTER {
                clusters.push((cluster_start, size));
            }
        }
    }
    clusters
}

/// Write OBJ file with vertex positions
fn write_obj(path: &Path, vertices: &[(f32, f32, f32)]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "# SPACE MESH export")?;
    writeln!(f, "# {} vertices", vertices.len())?;
    writeln!(f, "o space_mesh")?;
    for (x, y, z) in vertices {
        writeln!(f, "v {:.6} {:.6} {:.6}", x, y, z)?;
    }
    Ok(())
}

/// Read block header, return (magic_ok, section_count, section_offsets)
fn parse_block_header(data: &[u8]) -> Option<(Vec<u32>, u32)> {
    if data.len() < 0x14 { return None; }
    let magic = r_u32(data, 0);
    if magic != 0x000009D8 { return None; }
    let sect_cnt = r_u32(data, 0x0C);
    if sect_cnt > 16 { return None; }
    let mut offsets = Vec::new();
    for i in 0..sect_cnt as usize {
        offsets.push(r_u32(data, 0x10 + i * 4));
    }
    Some((offsets, sect_cnt))
}

pub fn run(scripts_dir: &Path, args: &SpaceMeshExportArgs) -> Result<(), String> {
    let out_dir = if let Some(ref out) = args.output {
        Path::new(out).to_path_buf()
    } else {
        crate::common::meshes_dir(scripts_dir).join("SPACE")
    };
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("Failed to create output dir: {}", e))?;

    if let Some(ref input_path) = args.input {
        // Single file mode
        let data = std::fs::read(input_path)
            .map_err(|e| format!("Failed to read {}: {}", input_path, e))?;
        if let Some((offsets, _sect_cnt)) = parse_block_header(&data) {
            let clusters = extract_vertex_clusters(&data, &offsets);
            let all_verts = export_vertices(&data, &clusters);
            if !all_verts.is_empty() {
                let fname = "space_mesh_single.obj";
                write_obj(&out_dir.join(fname), &all_verts)
                    .map_err(|e| format!("Failed to write OBJ: {}", e))?;
                println!("  {} vertices -> {}", all_verts.len(), fname);
            }
        }
        return Ok(());
    }

    // Directory mode
    let space_dir = crate::common::unpacked_dir(scripts_dir).join("GLOBAL").join("SPACE");
    if !space_dir.exists() {
        return Err(format!("SPACE directory not found: {}", space_dir.display()));
    }

    let mut entries: Vec<_> = std::fs::read_dir(&space_dir)
        .map_err(|e| format!("Failed to read {}: {}", space_dir.display(), e))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with("block_") && name.ends_with(".bin")
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut total_exported = 0;
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let data = std::fs::read(entry.path())
            .map_err(|e| format!("Failed to read {}: {}", name, e))?;

        // Check magic (first 2 bytes)
        if data.len() < 2 || data[0] != 0xD8 || data[1] != 0x09 {
            continue;
        }

        // Extract block index
        let block_idx = name.strip_prefix("block_")
            .and_then(|s| s.split('_').next())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        // Filter by block index if specified
        if let Some(target) = args.block_index {
            if block_idx != target { continue; }
        }

        let Some((offsets, _sect_cnt)) = parse_block_header(&data) else { continue; };
        let clusters = extract_vertex_clusters(&data, &offsets);
        let all_verts = export_vertices(&data, &clusters);

        if !all_verts.is_empty() {
            let fname = format!("space_mesh_{:03}_{}v.obj", block_idx, all_verts.len());
            let fpath = out_dir.join(&fname);
            write_obj(&fpath, &all_verts)
                .map_err(|e| format!("Failed to write OBJ: {}", e))?;
            total_exported += 1;
            println!("  [{:3}] {} vertices -> {}", block_idx, all_verts.len(), fname);
        } else {
            println!("  [{:3}] no vertex clusters found", block_idx);
        }
    }

    println!("\nDone. {} OBJ files -> {}", total_exported, out_dir.display());
    Ok(())
}

fn export_vertices(data: &[u8], clusters: &[(usize, usize)]) -> Vec<(f32, f32, f32)> {
    use std::collections::HashSet;
    let mut all_verts = Vec::new();
    let mut seen = HashSet::new();

    for &(start, size) in clusters {
        let mut off = start;
        while off + VERTEX_STRIDE <= start + size {
            let key = &data[off..off + 12];
            if seen.insert(key.to_vec()) {
                let x = read_f32_at(data, off);
                let y = read_f32_at(data, off + 4);
                let z = read_f32_at(data, off + 8);
                all_verts.push((x, y, z));
            }
            off += VERTEX_STRIDE;
        }
    }
    all_verts
}
