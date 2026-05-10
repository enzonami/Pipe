/// Collision mesh export — reads collision octree from core_data.bin
/// and writes OBJ + MTL files (ported from wrench-master's collision.cpp).
use std::path::Path;
use crate::cli::*;
use crate::common::{r_s32, r_u32};

/// Collision header: two s32 offsets (mesh, hero_groups)
struct CollHeader {
    mesh: u32,
    hero_groups: u32,
}

/// An octant in the collision tree (4×4×4 game units).
#[derive(Clone)]
struct Octant {
    displacement: [f32; 3],
    vertices: Vec<[f32; 3]>,
    faces: Vec<CollFace>,
}

#[derive(Clone)]
struct CollFace {
    v0: u8, v1: u8, v2: u8, v3: u8,
    is_quad: bool,
    material: u8, // collision type/material ID
}

pub fn run(scripts_dir: &Path, _args: &CollisionArgs) -> Result<(), String> {
    let unpacked = crate::common::unpacked_dir(scripts_dir);
    let out_dir = crate::common::extracted_dir(scripts_dir).join("collision");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("Cannot create output dir: {}", e))?;

    // Process all levels
    for level_num in 0..14 {
        if let Err(e) = process_level(&unpacked, &out_dir, level_num) {
            println!("  LEVEL {:03}: {}", level_num, e);
        }
    }
    Ok(())
}

fn process_level(unpacked: &Path, out_dir: &Path, level_num: u32) -> Result<(), String> {
    let data_dir = unpacked.join(format!("LEVEL{:03}", level_num)).join("data_wad");
    let ci_path = data_dir.join("core_index.bin");
    let cd_path = data_dir.join("core_data.bin");

    if !ci_path.exists() || !cd_path.exists() {
        return Err("core_index.bin or core_data.bin not found".into());
    }

    let ci_data = std::fs::read(&ci_path)
        .map_err(|e| format!("read core_index: {}", e))?;
    let core_data = std::fs::read(&cd_path)
        .map_err(|e| format!("read core_data: {}", e))?;

    // Read collision offset from core header (offset 0x14)
    let coll_offset = r_s32(&ci_data, 0x14) as usize;
    if coll_offset == 0 || coll_offset + 8 > core_data.len() {
        return Err("no collision data".into());
    }

    // Read collision header: mesh and hero offsets are RELATIVE to coll_offset
    let mesh_rel = r_u32(&core_data, coll_offset) as usize;
    let hero_rel = r_u32(&core_data, coll_offset + 4) as usize;

    if mesh_rel == 0 {
        return Err("invalid collision mesh offset".into());
    }

    let coll_mesh_off = coll_offset + mesh_rel;
    let hero_groups_off = if hero_rel > 0 { coll_offset + hero_rel } else { 0 };

    if coll_mesh_off >= core_data.len() {
        return Err("collision mesh offset out of bounds".into());
    }

    let mesh_end = if hero_groups_off > coll_mesh_off { hero_groups_off } else { core_data.len() };
    let mesh_data = &core_data[coll_mesh_off..mesh_end.min(core_data.len())];

    // Parse octree
    let octants = read_collision_mesh(mesh_data);
    let hero_groups = if hero_groups_off > 0 && hero_groups_off < core_data.len() {
        read_hero_collision_groups(&core_data[hero_groups_off..])
    } else {
        Vec::new()
    };

    // Write output
    let level_dir = out_dir.join(format!("LEVEL{:03}", level_num));
    std::fs::create_dir_all(&level_dir)
        .map_err(|e| format!("create dir: {}", e))?;

    write_collision_obj(&level_dir, "collision", &octants)?;
    if !hero_groups.is_empty() {
        write_hero_groups_obj(&level_dir, &hero_groups)?;
    }

    println!("  LEVEL {:03}: collision mesh exported ({} octants, {} hero groups)",
        level_num, count_octants(&octants), hero_groups.len());
    Ok(())
}

fn count_octants(octants: &CollisionOctants) -> usize {
    let mut count = 0;
    for yp in &octants.list {
        for xp in &yp.list {
            count += xp.list.len();
        }
    }
    count
}

// ── Octree reading ──

#[derive(Clone)]
struct CollisionList<T> {
    coord: i32,
    list: Vec<T>,
}

struct CollisionOctants {
    list: Vec<CollisionList<CollisionList<Octant>>>,
}

fn read_collision_mesh(data: &[u8]) -> CollisionOctants {
    let _z_coord = i16::from_le_bytes([data[0], data[1]]) as i32;
    let z_count = u16::from_le_bytes([data[2], data[3]]) as usize;

    let mut octants = CollisionOctants { list: Vec::new() };
    octants.list.resize(z_count, CollisionList { coord: 0, list: Vec::new() });

    // Read Z-level offsets (u16 * 4)
    if 4 + z_count * 2 > data.len() { return octants; }
    for z in 0..z_count {
        let z_off = u16::from_le_bytes([data[4 + z * 2], data[5 + z * 2]]) as usize * 4;
        if z_off == 0 || z_off + 4 > data.len() { continue; }

        let y_coord = i16::from_le_bytes([data[z_off], data[z_off + 1]]) as i32;
        let y_count = u16::from_le_bytes([data[z_off + 2], data[z_off + 3]]) as usize;

        octants.list[z].coord = y_coord;
        octants.list[z].list.resize(y_count, CollisionList { coord: 0, list: Vec::new() });

        // Read Y-level offsets (u32)
        let y_off_table = z_off + 4;
        for y in 0..y_count {
            let y_off = u32::from_le_bytes([
                data.get(y_off_table + y * 4).copied().unwrap_or(0),
                data.get(y_off_table + y * 4 + 1).copied().unwrap_or(0),
                data.get(y_off_table + y * 4 + 2).copied().unwrap_or(0),
                data.get(y_off_table + y * 4 + 3).copied().unwrap_or(0),
            ]) as usize;
            if y_off == 0 || y_off + 4 > data.len() { continue; }

            let x_coord = i16::from_le_bytes([data[y_off], data[y_off + 1]]) as i32;
            let x_count = u16::from_le_bytes([data[y_off + 2], data[y_off + 3]]) as usize;

            octants.list[z].list[y].coord = x_coord;
            octants.list[z].list[y].list.resize_with(x_count, || Octant {
                displacement: [0.0; 3],
                vertices: Vec::new(),
                faces: Vec::new(),
            });

            // Read X-level offsets (u32, top bits encode size)
            let x_off_table = y_off + 4;
            for x in 0..x_count {
                let raw = u32::from_le_bytes([
                    data.get(x_off_table + x * 4).copied().unwrap_or(0),
                    data.get(x_off_table + x * 4 + 1).copied().unwrap_or(0),
                    data.get(x_off_table + x * 4 + 2).copied().unwrap_or(0),
                    data.get(x_off_table + x * 4 + 3).copied().unwrap_or(0),
                ]);
                let oct_off = (raw >> 8) as usize;
                if oct_off == 0 || oct_off + 4 > data.len() { continue; }

                let face_count = u16::from_le_bytes([data[oct_off], data[oct_off + 1]]) as usize;
                let vertex_count = data[oct_off + 2] as usize;
                let quad_count = data[oct_off + 3] as usize;

                let mut oct = Octant {
                    displacement: [0.0; 3],
                    vertices: Vec::with_capacity(vertex_count),
                    faces: Vec::with_capacity(face_count),
                };

                let mut ofs = oct_off + 4;

                // Read vertices (packed in 32-bit: 10 bits x, 10 bits y, 12 bits z signed)
                for _ in 0..vertex_count {
                    if ofs + 4 > data.len() { break; }
                    let v = u32::from_le_bytes([data[ofs], data[ofs + 1], data[ofs + 2], data[ofs + 3]]);
                    let vx = (((v << 22) as i32) >> 22) as f32 / 16.0;
                    let vy = (((v << 12) as i32) >> 22) as f32 / 16.0;
                    let vz = (((v << 0) as i32) >> 20) as f32 / 64.0;
                    oct.vertices.push([vx, vy, vz]);
                    ofs += 4;
                }

                // Read faces (all quads first, then tris)
                for _ in 0..face_count {
                    if ofs + 4 > data.len() { break; }
                    let v0 = data[ofs];
                    let v1 = data[ofs + 1];
                    let v2 = data[ofs + 2];
                    let mat = data[ofs + 3];
                    oct.faces.push(CollFace { v0, v1, v2, v3: 0, is_quad: false, material: mat });
                    ofs += 4;
                }

                // Read quad v3 (overwrites the first quad_count faces)
                for q in 0..quad_count.min(face_count) {
                    if ofs >= data.len() { break; }
                    oct.faces[q].v3 = data[ofs];
                    oct.faces[q].is_quad = true;
                    ofs += 1;
                }

                octants.list[z].list[y].list[x] = oct;
            }
        }
    }

    // Compute displacements for each octant
    let z_base = octants.list.first().map(|l| l.coord).unwrap_or(0);
    for (zi, yp) in octants.list.iter_mut().enumerate() {
        let disp_z = (z_base + zi as i32) as f32 * 4.0 + 2.0;
        let y_base = yp.coord;
        for (yi, xp) in yp.list.iter_mut().enumerate() {
            let disp_y = (y_base + yi as i32) as f32 * 4.0 + 2.0;
            let x_base = xp.coord;
            for (xi, oct) in xp.list.iter_mut().enumerate() {
                let disp_x = (x_base + xi as i32) as f32 * 4.0 + 2.0;
                oct.displacement = [disp_x, disp_y, disp_z];
            }
        }
    }

    octants
}

// ── Hero collision groups ──

struct HeroGroup {
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[u8; 3]>,
}

fn read_hero_collision_groups(data: &[u8]) -> Vec<HeroGroup> {
    if data.len() < 4 { return Vec::new(); }
    let count = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if count <= 0 || count > 1000 { return Vec::new(); }

    let mut groups = Vec::new();
    let mut pos = 0x10; // skip count + padding

    for _ in 0..count {
        if pos + 16 > data.len() { break; }
        let _bsphere_x = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let _bsphere_y = u16::from_le_bytes([data[pos + 2], data[pos + 3]]);
        let _bsphere_z = u16::from_le_bytes([data[pos + 4], data[pos + 5]]);
        let _bsphere_r = u16::from_le_bytes([data[pos + 6], data[pos + 7]]);
        let tri_count = u16::from_le_bytes([data[pos + 8], data[pos + 9]]) as usize;
        let vert_count = u16::from_le_bytes([data[pos + 10], data[pos + 11]]) as usize;
        let data_off = u32::from_le_bytes([
            data[pos + 12], data[pos + 13], data[pos + 14], data[pos + 15],
        ]) as usize;
        pos += 16;

        let mut group = HeroGroup {
            vertices: Vec::with_capacity(vert_count),
            triangles: Vec::with_capacity(tri_count),
        };

        let mut dp = data_off;
        for _ in 0..vert_count {
            if dp + 8 > data.len() { break; }
            let vx = u16::from_le_bytes([data[dp], data[dp + 1]]) as f32 / 64.0;
            let vy = u16::from_le_bytes([data[dp + 2], data[dp + 3]]) as f32 / 64.0;
            let vz = u16::from_le_bytes([data[dp + 4], data[dp + 5]]) as f32 / 64.0;
            group.vertices.push([vx, vy, vz]);
            dp += 8;
        }

        for _ in 0..tri_count {
            if dp + 4 > data.len() { break; }
            group.triangles.push([data[dp], data[dp + 1], data[dp + 2]]);
            dp += 4;
        }

        groups.push(group);
    }

    groups
}

// ── OBJ writer ──

fn write_collision_obj(dir: &Path, name: &str, octants: &CollisionOctants) -> Result<(), String> {
    let mut obj = String::new();
    let mut mtl = String::new();
    let mut global_vertex_offset: usize = 1;

    obj.push_str(&format!("# Collision mesh: {}\n", name));
    obj.push_str(&format!("mtllib {}.mtl\n", name));
    obj.push_str(&format!("o {}\n", name));

    // Collect material IDs used
    let mut used_materials: Vec<u8> = Vec::new();
    for yp in &octants.list {
        for xp in &yp.list {
            for oct in &xp.list {
                for face in &oct.faces {
                    if !used_materials.contains(&face.material) {
                        used_materials.push(face.material);
                    }
                }
            }
        }
    }
    used_materials.sort();

    // Write MTL
    mtl.push_str(&format!("# Collision materials for {}\n", name));
    for &mat_id in &used_materials {
        let r = ((mat_id & 0x3) << 6) as f32 / 255.0;
        let g = ((mat_id & 0xc) << 4) as f32 / 255.0;
        let b = (mat_id & 0xf0) as f32 / 255.0;
        mtl.push_str(&format!("\nnewmtl col_{:02x}\n", mat_id));
        mtl.push_str(&format!("Kd {:.3} {:.3} {:.3}\n", r, g, b));
        mtl.push_str("Ka 0 0 0\n");
        mtl.push_str("Ks 0 0 0\n");
    }

    // Write vertices and faces grouped by material
    for &mat_id in &used_materials {
        obj.push_str(&format!("usemtl col_{:02x}\n", mat_id));

        // Collect all vertices for this material
        let mut verts: Vec<[f32; 3]> = Vec::new();
        let mut face_list: Vec<Vec<usize>> = Vec::new();

        for yp in &octants.list {
            for xp in &yp.list {
                for oct in &xp.list {
                    for face in &oct.faces {
                        if face.material != mat_id { continue; }
                        let disp = oct.displacement;

                        let add_vert = |v: &[f32; 3], verts: &mut Vec<[f32; 3]>| -> usize {
                            let wv = [v[0] + disp[0], v[1] + disp[1], v[2] + disp[2]];
                            // dedup
                            for (j, ev) in verts.iter().enumerate() {
                                if (ev[0] - wv[0]).abs() < 0.001
                                    && (ev[1] - wv[1]).abs() < 0.001
                                    && (ev[2] - wv[2]).abs() < 0.001
                                {
                                    return j;
                                }
                            }
                            let idx = verts.len();
                            verts.push(wv);
                            idx
                        };

                        let idx0 = add_vert(&oct.vertices[face.v0 as usize], &mut verts);
                        let idx1 = add_vert(&oct.vertices[face.v1 as usize], &mut verts);
                        let idx2 = add_vert(&oct.vertices[face.v2 as usize], &mut verts);

                        if face.is_quad {
                            let idx3 = add_vert(&oct.vertices[face.v3 as usize], &mut verts);
                            // Triangulate quad: (0,1,2) and (0,2,3)
                            // Reverse winding to match wrench-master
                            face_list.push(vec![idx2, idx1, idx0]);
                            face_list.push(vec![idx1, idx3, idx0]);
                        } else {
                            face_list.push(vec![idx2, idx1, idx0]);
                        }
                    }
                }
            }
        }

        // Write vertices
        for v in &verts {
            obj.push_str(&format!("v {:.6} {:.6} {:.6}\n", v[0], v[1], v[2]));
        }

        // Write faces
        for fv in &face_list {
            obj.push_str("f");
            for vi in fv {
                obj.push_str(&format!(" {}", vi + global_vertex_offset));
            }
            obj.push('\n');
        }

        global_vertex_offset += verts.len();
    }

    std::fs::write(&dir.join(format!("{}.obj", name)), &obj)
        .map_err(|e| format!("write obj: {}", e))?;
    std::fs::write(&dir.join(format!("{}.mtl", name)), &mtl)
        .map_err(|e| format!("write mtl: {}", e))?;

    Ok(())
}

fn write_hero_groups_obj(dir: &Path, groups: &[HeroGroup]) -> Result<(), String> {
    let mut obj = String::new();
    obj.push_str("# Hero collision groups\n");
    obj.push_str("mtllib hero_collision.mtl\n");

    let mut mtl = String::new();
    mtl.push_str("newmtl hero_group_collision\n");
    mtl.push_str("Kd 0 0 1\n");
    mtl.push_str("Ka 0 0 0\n");
    mtl.push_str("Ks 0 0 0\n");

    let mut voff = 1;
    for (i, group) in groups.iter().enumerate() {
        obj.push_str(&format!("o hero_collision_group_{}\n", i));
        obj.push_str("usemtl hero_group_collision\n");

        for v in &group.vertices {
            obj.push_str(&format!("v {:.6} {:.6} {:.6}\n", v[0], v[1], v[2]));
        }
        for tri in &group.triangles {
            obj.push_str(&format!("f {} {} {}\n",
                tri[2] as usize + voff,
                tri[1] as usize + voff,
                tri[0] as usize + voff));
        }
        voff += group.vertices.len();
    }

    std::fs::write(&dir.join("hero_collision.obj"), &obj)
        .map_err(|e| format!("write hero obj: {}", e))?;
    std::fs::write(&dir.join("hero_collision.mtl"), &mtl)
        .map_err(|e| format!("write hero mtl: {}", e))?;

    Ok(())
}
