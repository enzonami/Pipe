/// Scene Assembler - combine meshes with instance transforms into a complete scene
use std::collections::BTreeMap;
use std::path::Path;
use std::fs;
use crate::cli::*;
use crate::common::*;

pub fn run(scripts_dir: &Path, args: &SceneArgs) -> Result<(), String> {
    let level_filter = args.level.unwrap_or(-1);
    let flags = SceneFlags {
        tie: !args.no_tie,
        shrub: !args.no_shrub,
        moby: !args.no_moby,
        tfrag: !args.no_tfrag,
    };

    level_dispatch(level_filter, |level_num| {
        process_level(scripts_dir, level_num, &flags)
    })
}

struct SceneFlags {
    tie: bool,
    shrub: bool,
    moby: bool,
    tfrag: bool,
}

#[derive(Clone)]
struct FaceVert {
    v: usize,
    vt: usize,
}

type Tri = [FaceVert; 3];

fn process_level(scripts_dir: &Path, level_num: u32, flags: &SceneFlags) -> Result<(), String> {
    let unpacked = unpacked_dir(scripts_dir);
    let mesh_dir = meshes_dir(scripts_dir).join(format!("LEVEL{:03}", level_num));
    let gameplay_path = unpacked.join(format!("LEVEL{:03}", level_num)).join("gameplay_extracted.json");
    let out_dir = scenes_dir(scripts_dir).join(format!("LEVEL{:03}", level_num));
    fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir: {}", e))?;

    if !gameplay_path.exists() {
        return Err(format!("gameplay_extracted.json not found for LEVEL {:03}", level_num));
    }

    let gameplay: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&gameplay_path).map_err(|e| format!("read gameplay: {}", e))?
    ).map_err(|e| format!("parse gameplay: {}", e))?;

    let mut scene_verts: Vec<[f64; 3]> = Vec::new();
    let mut scene_uvs: Vec<[f64; 2]> = Vec::new();
    let mut scene_faces: Vec<(String, Vec<Tri>)> = Vec::new();
    let mut global_mtl_registry: BTreeMap<String, String> = BTreeMap::new(); // abs_path -> mat_name
    let mut mtl_entries: Vec<(String, String)> = Vec::new(); // mat_name -> rel_path
    let mut instance_count = 0;

    let get_global_mat = |abs_tex: &str, mtl_entries: &mut Vec<(String, String)>, reg: &mut BTreeMap<String, String>| -> String {
        if let Some(name) = reg.get(abs_tex) {
            return name.clone();
        }
        let name = format!("tex_{}", reg.len());
        let rel = rel_path(abs_tex, &out_dir);
        reg.insert(abs_tex.to_string(), name.clone());
        mtl_entries.push((name.clone(), rel));
        name
    };

    // ── Tfrag meshes ──
    if flags.tfrag && mesh_dir.exists() {
        let mut entries: Vec<_> = fs::read_dir(&mesh_dir).map_err(|e| format!("read mesh dir: {}", e))?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let n = e.file_name();
                let s = n.to_string_lossy();
                s.starts_with("tfrag_") && s.ends_with(".obj")
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in &entries {
            let obj_path = entry.path();
            if let Some(data) = read_obj_textured(&obj_path) {
                if data.verts.is_empty() { continue; }
                let mtl_path = obj_path.with_extension("mtl");
                let tex_map = read_mtl_textures(&mtl_path);
                let mut uvs = data.uvs;
                if uvs.len() < data.verts.len() {
                    uvs.resize(data.verts.len(), [0.0, 0.0]);
                }
                let base_v = scene_verts.len();
                let base_u = scene_uvs.len();
                scene_verts.extend(data.verts);
                scene_uvs.extend(uvs);

                for (mat_name, tri_list) in &data.mat_groups {
                    let global_mat = if let Some(tex_rel) = tex_map.get(mat_name.as_str()) {
                        let abs_path = mesh_dir.join(tex_rel);
                        get_global_mat(&abs_path.to_string_lossy(), &mut mtl_entries, &mut global_mtl_registry)
                    } else {
                        let fallback = format!("tfrag_{}", mat_name);
                        mtl_entries.push((fallback.clone(), String::new()));
                        fallback
                    };
                    let tris: Vec<Tri> = tri_list.iter().map(|tri| {
                        [
                            FaceVert { v: base_v + tri[0], vt: base_u + tri[0] },
                            FaceVert { v: base_v + tri[1], vt: base_u + tri[1] },
                            FaceVert { v: base_v + tri[2], vt: base_u + tri[2] },
                        ]
                    }).collect();
                    scene_faces.push((global_mat, tris));
                }
            }
        }
        println!("  Tfrags: {} fragments", entries.len());
    }

    // ── Tie instances ──
    if flags.tie {
        if let Some(ties) = gameplay["tie_instances"]["instances"].as_array() {
            for inst in ties {
                let o_class = inst["o_class"].as_i64().unwrap_or(0);
                let m = matrix_from_value(inst);
                let obj_path = mesh_dir.join(format!("tie_{}.obj", o_class));
                let mtl_path = obj_path.with_extension("mtl");

                if let Some(data) = read_obj_textured(&obj_path) {
                    if data.verts.is_empty() { continue; }
                    let tex_map = read_mtl_textures(&mtl_path);
                    let mut uvs = data.uvs;
                    if uvs.len() < data.verts.len() {
                        uvs.resize(data.verts.len(), [0.0, 0.0]);
                    }
                    let xformed: Vec<_> = data.verts.iter().map(|v| transform_vert(&m, *v)).collect();
                    let base_v = scene_verts.len();
                    let base_u = scene_uvs.len();
                    scene_verts.extend(xformed);
                    scene_uvs.extend(uvs);

                    for (mat_name, tri_list) in &data.mat_groups {
                        let global_mat = if let Some(tex_rel) = tex_map.get(mat_name.as_str()) {
                            let abs_path = mesh_dir.join(tex_rel);
                            get_global_mat(&abs_path.to_string_lossy(), &mut mtl_entries, &mut global_mtl_registry)
                        } else {
                            let fallback = format!("tie_{}", mat_name);
                            mtl_entries.push((fallback.clone(), String::new()));
                            fallback
                        };
                        let tris: Vec<Tri> = tri_list.iter().map(|tri| {
                            [
                                FaceVert { v: base_v + tri[0], vt: base_u + tri[0] },
                                FaceVert { v: base_v + tri[1], vt: base_u + tri[1] },
                                FaceVert { v: base_v + tri[2], vt: base_u + tri[2] },
                            ]
                        }).collect();
                        scene_faces.push((global_mat, tris));
                    }
                } else if let Some(verts) = read_obj_verts(&obj_path) {
                    let xformed: Vec<_> = verts.iter().map(|v| transform_vert(&m, *v)).collect();
                    let base = scene_verts.len();
                    let vc = xformed.len();
                    scene_verts.extend(xformed);
                    let mut tris = Vec::new();
                    for k in 2..vc {
                        if k % 2 == 0 {
                            tris.push([FaceVert { v: base + k - 2, vt: base + k - 2 },
                                       FaceVert { v: base + k - 1, vt: base + k - 1 },
                                       FaceVert { v: base + k, vt: base + k }]);
                        } else {
                            tris.push([FaceVert { v: base + k - 1, vt: base + k - 1 },
                                       FaceVert { v: base + k - 2, vt: base + k - 2 },
                                       FaceVert { v: base + k, vt: base + k }]);
                        }
                    }
                    scene_faces.push(("tie_default".into(), tris));
                }
                instance_count += 1;
            }
            println!("  Ties: {} instances", ties.len());
        }
    }

    // ── Shrub instances ──
    if flags.shrub {
        if let Some(shrubs) = gameplay["shrub_instances"]["instances"].as_array() {
            for inst in shrubs {
                let o_class = inst["o_class"].as_i64().unwrap_or(0);
                let m = matrix_from_value(inst);
                let obj_path = mesh_dir.join(format!("shrub_{}.obj", o_class));
                let mtl_path = obj_path.with_extension("mtl");

                if let Some(data) = read_obj_textured(&obj_path) {
                    if data.verts.is_empty() { continue; }
                    let tex_map = read_mtl_textures(&mtl_path);
                    let mut uvs = data.uvs;
                    if uvs.len() < data.verts.len() {
                        uvs.resize(data.verts.len(), [0.0, 0.0]);
                    }
                    let xformed: Vec<_> = data.verts.iter().map(|v| transform_vert(&m, *v)).collect();
                    let base_v = scene_verts.len();
                    let base_u = scene_uvs.len();
                    scene_verts.extend(xformed);
                    scene_uvs.extend(uvs);

                    for (mat_name, tri_list) in &data.mat_groups {
                        let global_mat = if let Some(tex_rel) = tex_map.get(mat_name.as_str()) {
                            let abs_path = mesh_dir.join(tex_rel);
                            get_global_mat(&abs_path.to_string_lossy(), &mut mtl_entries, &mut global_mtl_registry)
                        } else {
                            let fallback = format!("shrub_{}", mat_name);
                            mtl_entries.push((fallback.clone(), String::new()));
                            fallback
                        };
                        let tris: Vec<Tri> = tri_list.iter().map(|tri| {
                            [
                                FaceVert { v: base_v + tri[0], vt: base_u + tri[0] },
                                FaceVert { v: base_v + tri[1], vt: base_u + tri[1] },
                                FaceVert { v: base_v + tri[2], vt: base_u + tri[2] },
                            ]
                        }).collect();
                        scene_faces.push((global_mat, tris));
                    }
                }
                instance_count += 1;
            }
            println!("  Shrubs: {} instances", shrubs.len());
        }
    }

    // ── Moby instances ──
    if flags.moby {
        // Load moby texture map
        let moby_classes_path = unpacked.join(format!("LEVEL{:03}", level_num)).join("data_wad").join("moby_classes.json");
        let moby_tex_map: BTreeMap<i64, Vec<i64>> = if moby_classes_path.exists() {
            let mc: serde_json::Value = serde_json::from_str(
                &fs::read_to_string(&moby_classes_path).unwrap_or_default()
            ).unwrap_or_default();
            mc["entries"].as_array().map(|arr| {
                arr.iter().filter_map(|e| {
                    let cls = e["o_class"].as_i64()?;
                    let tex_hex = e["tex"].as_str()?;
                    let indices: Vec<i64> = (0..tex_hex.len()).step_by(2)
                        .filter_map(|i| i64::from_str_radix(&tex_hex[i..i+2], 16).ok())
                        .filter(|&v| v != 0xFF)
                        .collect();
                    Some((cls, indices))
                }).collect()
            }).unwrap_or_default()
        } else { BTreeMap::new() };

        if let Some(mobies) = gameplay["moby_instances"]["instances"].as_array() {
            for inst in mobies {
                let o_class = inst["o_class"].as_i64().unwrap_or(0);
                if o_class == 0 { continue; }

                let pos = &inst["position"];
                let rot = &inst["rotation"];
                let scale = inst["scale"].as_f64().unwrap_or(1.0);
                let tex_indices = moby_tex_map.get(&o_class).cloned().unwrap_or_default();
                let has_texture = !tex_indices.is_empty();

                let obj_path = mesh_dir.join(format!("class_{}.obj", o_class));
                let _mtl_path = obj_path.with_extension("mtl");

                if has_texture {
                    if let Some(data) = read_obj_textured(&obj_path) {
                        if data.verts.is_empty() { continue; }
                        let mut uvs = data.uvs;
                        if uvs.len() < data.verts.len() {
                            uvs.resize(data.verts.len(), [0.0, 0.0]);
                        }

                        let mut m = rotation_matrix(rot);
                        m[12] = pos["x"].as_f64().unwrap_or(0.0);
                        m[13] = pos["y"].as_f64().unwrap_or(0.0);
                        m[14] = pos["z"].as_f64().unwrap_or(0.0);
                        scale_mat(&mut m, scale);

                        let tex_idx = tex_indices[0];
                        let tex_name = format!("moby_{:03}", tex_idx);

                        let tex_dir = textures_dir(scripts_dir).join(format!("LEVEL{:03}", level_num)).join("moby");
                        let tex_file = if tex_dir.exists() {
                            fs::read_dir(&tex_dir).ok().and_then(|rd| {
                                rd.filter_map(|e| e.ok())
                                    .find(|e| e.file_name().to_string_lossy().starts_with(&tex_name))
                                    .map(|e| e.path())
                            })
                        } else { None };

                        let global_mat = if let Some(ref tf) = tex_file {
                            get_global_mat(&tf.to_string_lossy(), &mut mtl_entries, &mut global_mtl_registry)
                        } else {
                            let fallback = format!("moby_class_{}", o_class);
                            mtl_entries.push((fallback.clone(), String::new()));
                            fallback
                        };

                        let xformed: Vec<_> = data.verts.iter().map(|v| transform_vert(&m, *v)).collect();
                        let base_v = scene_verts.len();
                        let base_u = scene_uvs.len();
                        scene_verts.extend(xformed);
                        scene_uvs.extend(uvs);

                        for (_mat_name, tri_list) in &data.mat_groups {
                            let tris: Vec<Tri> = tri_list.iter().map(|tri| {
                                [
                                    FaceVert { v: base_v + tri[0], vt: base_u + tri[0] },
                                    FaceVert { v: base_v + tri[1], vt: base_u + tri[1] },
                                    FaceVert { v: base_v + tri[2], vt: base_u + tri[2] },
                                ]
                            }).collect();
                            scene_faces.push((global_mat.clone(), tris));
                        }
                        instance_count += 1;
                        continue;
                    }
                }

                // Untextured moby
                if let Some((verts, idxs)) = read_obj_mesh(&obj_path) {
                    let mut m = rotation_matrix(rot);
                    m[12] = pos["x"].as_f64().unwrap_or(0.0);
                    m[13] = pos["y"].as_f64().unwrap_or(0.0);
                    m[14] = pos["z"].as_f64().unwrap_or(0.0);
                    scale_mat(&mut m, scale);

                    let xformed: Vec<_> = verts.iter().map(|v| transform_vert(&m, *v)).collect();
                    let base = scene_verts.len();
                    scene_verts.extend(xformed);

                    let tris: Vec<Tri> = idxs.iter().map(|&(i0, i1, i2)| {
                        [
                            FaceVert { v: base + i0, vt: base + i0 },
                            FaceVert { v: base + i1, vt: base + i1 },
                            FaceVert { v: base + i2, vt: base + i2 },
                        ]
                    }).collect();
                    scene_faces.push(("moby_default".into(), tris));
                }
                instance_count += 1;
            }
            println!("  Mobies: {} instances", mobies.len());
        }
    }

    // ── Write output ──
    if !scene_verts.is_empty() {
        let has_uvs = !scene_uvs.is_empty();
        let normals = compute_normals(&scene_verts, &scene_faces);
        write_scene_output(&out_dir, &scene_verts, &scene_uvs, &normals, &scene_faces, &mtl_entries, level_num, has_uvs);
        println!("  Wrote scene ({} verts, {} uvs, {} instances)", scene_verts.len(), scene_uvs.len(), instance_count);
    }

    Ok(())
}

// ── OBJ reading ──

#[derive(Default)]
struct TexturedObj {
    verts: Vec<[f64; 3]>,
    uvs: Vec<[f64; 2]>,
    mat_groups: Vec<(String, Vec<[usize; 3]>)>,
}

fn read_obj_verts(path: &Path) -> Option<Vec<[f64; 3]>> {
    let content = fs::read_to_string(path).ok()?;
    let verts: Vec<[f64; 3]> = content.lines()
        .filter(|l| l.starts_with("v "))
        .filter_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            if parts.len() >= 4 {
                Some([
                    parts[1].parse::<f64>().ok()?,
                    parts[2].parse::<f64>().ok()?,
                    parts[3].parse::<f64>().ok()?,
                ])
            } else { None }
        }).collect();
    if verts.is_empty() { None } else { Some(verts) }
}

fn read_obj_mesh(path: &Path) -> Option<(Vec<[f64; 3]>, Vec<(usize, usize, usize)>)> {
    let content = fs::read_to_string(path).ok()?;
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    for line in content.lines() {
        if line.starts_with("v ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                verts.push([
                    parts[1].parse::<f64>().ok()?,
                    parts[2].parse::<f64>().ok()?,
                    parts[3].parse::<f64>().ok()?,
                ]);
            }
        } else if line.starts_with("f ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let i0 = parts[1].split('/').next()?.parse::<usize>().ok()? - 1;
                let i1 = parts[2].split('/').next()?.parse::<usize>().ok()? - 1;
                let i2 = parts[3].split('/').next()?.parse::<usize>().ok()? - 1;
                tris.push((i0, i1, i2));
                if parts.len() >= 5 {
                    let i3 = parts[4].split('/').next()?.parse::<usize>().ok()? - 1;
                    tris.push((i0, i2, i3));
                }
            }
        }
    }
    if verts.is_empty() { None } else { Some((verts, tris)) }
}

fn read_obj_textured(path: &Path) -> Option<TexturedObj> {
    let content = fs::read_to_string(path).ok()?;
    let mut result = TexturedObj::default();
    let mut current_mat = "default".to_string();
    let mut raw_tris: Vec<(String, Vec<[usize; 3]>)> = Vec::new();

    for line in content.lines() {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') { continue; }
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.is_empty() { continue; }
        match parts[0] {
            "v" if parts.len() >= 4 => {
                result.verts.push([
                    parts[1].parse::<f64>().ok()?,
                    parts[2].parse::<f64>().ok()?,
                    parts[3].parse::<f64>().ok()?,
                ]);
            }
            "vt" if parts.len() >= 3 => {
                result.uvs.push([
                    parts[1].parse::<f64>().ok()?,
                    parts[2].parse::<f64>().ok()?,
                ]);
            }
            "usemtl" if parts.len() >= 2 => {
                current_mat = parts[1].to_string();
            }
            "f" if parts.len() >= 4 => {
                let idxs: Vec<usize> = parts[1..parts.len().min(5)].iter()
                    .filter_map(|p| p.split('/').next()?.parse::<usize>().ok())
                    .map(|i| i - 1)
                    .collect();
                if idxs.len() >= 3 {
                    raw_tris.push((current_mat.clone(), vec![[idxs[0], idxs[1], idxs[2]]]));
                    if idxs.len() >= 4 {
                        raw_tris.push((current_mat.clone(), vec![[idxs[0], idxs[2], idxs[3]]]));
                    }
                }
            }
            _ => {}
        }
    }

    // Group tris by material
    let mut mat_groups: BTreeMap<String, Vec<[usize; 3]>> = BTreeMap::new();
    for (mat, tri_list) in &raw_tris {
        mat_groups.entry(mat.clone()).or_default().extend(tri_list.clone());
    }
    result.mat_groups = mat_groups.into_iter().collect();

    if result.verts.is_empty() { None } else { Some(result) }
}

fn read_mtl_textures(path: &Path) -> BTreeMap<String, String> {
    let content = match fs::read_to_string(path) { Ok(c) => c, _ => return BTreeMap::new() };
    let mut map = BTreeMap::new();
    let mut current_mat = String::new();
    for line in content.lines() {
        let s = line.trim();
        if s.starts_with("newmtl ") {
            current_mat = s[7..].trim().to_string();
        } else if s.starts_with("map_Kd ") && !current_mat.is_empty() {
            map.insert(current_mat.clone(), s[7..].trim().to_string());
        }
    }
    map
}

// ── Transform helpers ──

fn transform_vert(m: &[f64; 16], v: [f64; 3]) -> [f64; 3] {
    let x = m[0]*v[0] + m[4]*v[1] + m[8]*v[2]  + m[12];
    let y = m[1]*v[0] + m[5]*v[1] + m[9]*v[2]  + m[13];
    let z = m[2]*v[0] + m[6]*v[1] + m[10]*v[2] + m[14];
    let w = m[3]*v[0] + m[7]*v[1] + m[11]*v[2] + m[15];
    if w.abs() > 1e-6 && (w - 1.0).abs() > 1e-6 {
        [x/w, y/w, z/w]
    } else {
        [x, y, z]
    }
}

fn rotation_matrix(rot: &serde_json::Value) -> [f64; 16] {
    let rx = rot["x"].as_f64().unwrap_or(0.0);
    let ry = rot["y"].as_f64().unwrap_or(0.0);
    let rz = rot["z"].as_f64().unwrap_or(0.0);
    let cx = rx.cos();
    let sx = rx.sin();
    let cy = ry.cos();
    let sy = ry.sin();
    let cz = rz.cos();
    let sz = rz.sin();
    [
        cy * cz,              cy * sz,              -sy,        0.0,
        sx * sy * cz - cx * sz, sx * sy * sz + cx * cz, sx * cy, 0.0,
        cx * sy * cz + sx * sz, cx * sy * sz - sx * cz, cx * cy, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ]
}

fn scale_mat(m: &mut [f64; 16], s: f64) {
    if s != 1.0 {
        m[0] *= s; m[1] *= s; m[2] *= s;
        m[4] *= s; m[5] *= s; m[6] *= s;
        m[8] *= s; m[9] *= s; m[10] *= s;
    }
}

fn matrix_from_value(v: &serde_json::Value) -> [f64; 16] {
    if let Some(mat) = v["matrix"].as_array() {
        if mat.len() >= 16 {
            let mut m = [0.0; 16];
            for (i, val) in mat.iter().enumerate().take(16) {
                m[i] = val.as_f64().unwrap_or(0.0);
            }
            return m;
        }
    }
    let pos = &v["position"];
    let rot = &v["rotation"];
    let mut m = rotation_matrix(rot);
    m[12] = pos["x"].as_f64().unwrap_or(0.0);
    m[13] = pos["y"].as_f64().unwrap_or(0.0);
    m[14] = pos["z"].as_f64().unwrap_or(0.0);
    if let Some(s) = v["scale"].as_f64() {
        scale_mat(&mut m, s);
    }
    m
}

// ── Normal computation ──

fn compute_normals(verts: &[[f64; 3]], faces: &[(String, Vec<Tri>)]) -> Vec<[f64; 3]> {
    let mut normals = vec![[0.0; 3]; verts.len()];
    for (_, tri_list) in faces {
        for tri in tri_list {
            let v0 = verts[tri[0].v];
            let v1 = verts[tri[1].v];
            let v2 = verts[tri[2].v];
            let e1 = [v1[0]-v0[0], v1[1]-v0[1], v1[2]-v0[2]];
            let e2 = [v2[0]-v0[0], v2[1]-v0[1], v2[2]-v0[2]];
            let fn_ = [e1[1]*e2[2] - e1[2]*e2[1],
                       e1[2]*e2[0] - e1[0]*e2[2],
                       e1[0]*e2[1] - e1[1]*e2[0]];
            if fn_[0] == 0.0 && fn_[1] == 0.0 && fn_[2] == 0.0 { continue; }
            for idx in 0..3 {
                let i = tri[idx].v;
                normals[i][0] += fn_[0];
                normals[i][1] += fn_[1];
                normals[i][2] += fn_[2];
            }
        }
    }
    for n in normals.iter_mut() {
        let len = (n[0]*n[0] + n[1]*n[1] + n[2]*n[2]).sqrt();
        if len > 1e-8 {
            n[0] /= len; n[1] /= len; n[2] /= len;
        } else {
            *n = [0.0, 0.0, 1.0];
        }
    }
    normals
}

// ── Output writing ──

fn rel_path(abs_path: &str, base: &Path) -> String {
    let abs = Path::new(abs_path);
    let rel = abs.strip_prefix(base).unwrap_or(abs);
    rel.to_string_lossy().to_string()
}

fn write_scene_output(
    out_dir: &Path, verts: &[[f64; 3]], uvs: &[[f64; 2]],
    normals: &[[f64; 3]], faces: &[(String, Vec<Tri>)],
    mtl_entries: &[(String, String)], level_num: u32, has_uvs: bool,
) {
    // Write MTL
    let mtl_path = out_dir.join("scene.mtl");
    let mut mtl = String::new();
    mtl.push_str(&format!("# Combined scene materials - LEVEL{:03}\n", level_num));
    mtl.push_str(&format!("# {} unique textures\n", mtl_entries.len()));
    for (mat_name, tex_rel) in mtl_entries {
        mtl.push_str(&format!("\nnewmtl {}\n", mat_name));
        mtl.push_str("Ka 0.8 0.8 0.8\nKd 0.8 0.8 0.8\nKs 0.0 0.0 0.0\nd 1.0\nillum 1\n");
        if !tex_rel.is_empty() {
            mtl.push_str(&format!("map_Kd {}\n", tex_rel));
        }
    }
    let _ = fs::write(&mtl_path, mtl);

    // Write OBJ
    let obj_path = out_dir.join("scene.obj");
    let mut obj = String::new();
    obj.push_str(&format!("# Combined scene: {} total vertices\n", verts.len()));
    obj.push_str("mtllib scene.mtl\no scene\n");

    for v in verts {
        obj.push_str(&format!("v {:.6} {:.6} {:.6}\n", v[0], v[1], v[2]));
    }
    for n in normals {
        obj.push_str(&format!("vn {:.6} {:.6} {:.6}\n", n[0], n[1], n[2]));
    }
    if has_uvs {
        for uv in uvs {
            obj.push_str(&format!("vt {:.6} {:.6}\n", uv[0], uv[1]));
        }
    }

    obj.push_str("s off\n");
    for (mat_name, tri_list) in faces {
        obj.push_str(&format!("usemtl {}\n", mat_name));
        let is_textured = has_uvs && (mat_name.starts_with("tex_") || mat_name.starts_with("moby_"));
        for tri in tri_list {
            if is_textured {
                obj.push_str(&format!("f {}/{}/{} {}/{}/{} {}/{}/{}\n",
                    tri[0].v+1, tri[0].vt+1, tri[0].v+1,
                    tri[1].v+1, tri[1].vt+1, tri[1].v+1,
                    tri[2].v+1, tri[2].vt+1, tri[2].v+1));
            } else {
                obj.push_str(&format!("f {}//{} {}//{} {}//{}\n",
                    tri[0].v+1, tri[0].v+1,
                    tri[1].v+1, tri[1].v+1,
                    tri[2].v+1, tri[2].v+1));
            }
        }
    }
    let _ = fs::write(&obj_path, obj);
    println!("  Wrote OBJ: {} verts, {} uvs, {} normals, {} face groups",
        verts.len(), uvs.len(), normals.len(), faces.len());
}
