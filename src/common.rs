//! Common shared helpers for all RAC tools

use std::path::{Path, PathBuf};

pub const SECTOR_SIZE: u32 = 0x800;
pub const TOC_LBA: u32 = 1001;
pub const LEVEL_COUNT: u32 = 14;
pub const GC_UYA_DL_TOC_LBA: u32 = 1001;
pub const TOC_MAX_SIZE: u32 = 0x200000;

/// Base paths (resolved from the project root)
pub fn project_root(project_dir: &Path) -> PathBuf {
    project_dir.to_path_buf()  // identity: scripts_dir IS the project root now
}

pub fn extracted_dir(scripts_dir: &Path) -> PathBuf {
    project_root(scripts_dir).join("extracted")
}

pub fn wad_dir(scripts_dir: &Path) -> PathBuf {
    extracted_dir(scripts_dir).join("WAD")
}

pub fn unpacked_dir(scripts_dir: &Path) -> PathBuf {
    extracted_dir(scripts_dir).join("unpacked")
}

pub fn meshes_dir(scripts_dir: &Path) -> PathBuf {
    extracted_dir(scripts_dir).join("meshes")
}

pub fn textures_dir(scripts_dir: &Path) -> PathBuf {
    extracted_dir(scripts_dir).join("textures")
}

pub fn scenes_dir(scripts_dir: &Path) -> PathBuf {
    extracted_dir(scripts_dir).join("scenes")
}

pub fn level_dir(scripts_dir: &Path, level_num: u32) -> PathBuf {
    unpacked_dir(scripts_dir).join(format!("LEVEL{:03}", level_num))
}

pub fn level_data_dir(scripts_dir: &Path, level_num: u32) -> PathBuf {
    level_dir(scripts_dir, level_num).join("data_wad")
}

/// Read little-endian values from bytes
#[inline]
pub fn r_u32(data: &[u8], off: usize) -> u32 {
    let buf: [u8; 4] = data[off..off + 4].try_into().unwrap();
    u32::from_le_bytes(buf)
}

#[inline]
pub fn r_s32(data: &[u8], off: usize) -> i32 {
    let buf: [u8; 4] = data[off..off + 4].try_into().unwrap();
    i32::from_le_bytes(buf)
}

#[inline]
pub fn r_u16(data: &[u8], off: usize) -> u16 {
    let buf: [u8; 2] = data[off..off + 2].try_into().unwrap();
    u16::from_le_bytes(buf)
}

#[inline]
pub fn r_s16(data: &[u8], off: usize) -> i16 {
    let buf: [u8; 2] = data[off..off + 2].try_into().unwrap();
    i16::from_le_bytes(buf)
}

#[inline]
pub fn r_u8(data: &[u8], off: usize) -> u8 {
    data[off]
}

#[inline]
pub fn r_f32_12(val: u16) -> f32 {
    // VU fixed-point 12.4
    if val & 0x8000 != 0 {
        -(((!val).wrapping_add(1) & 0xFFF) as f32) / 16.0
    } else {
        (val as f32) / 16.0
    }
}

#[inline]
pub fn r_f32_12_u16(val: u16) -> f32 {
    // unsigned VU fixed-point 12.4
    (val as f32) / 16.0
}

/// Read a sector (0x800 bytes) at a given LBA
pub fn read_sector(data: &[u8], lba: u32) -> &[u8] {
    let off = (lba as usize) * (SECTOR_SIZE as usize);
    &data[off..off + (SECTOR_SIZE as usize)]
}

/// Read arbitrary bytes at a given LBA and size
pub fn read_bytes(data: &[u8], lba: u32, size: u32) -> &[u8] {
    let off = (lba as usize) * (SECTOR_SIZE as usize);
    &data[off..off + (size as usize)]
}

/// Read bytes at an absolute offset
pub fn read_at(data: &[u8], offset: u32, size: u32) -> &[u8] {
    &data[offset as usize..offset as usize + size as usize]
}

/// Parse a sector range: 4-byte LBA + 4-byte count
pub fn parse_sector_range(data: &[u8], offset: usize) -> (u32, u32) {
    let lba = r_u32(data, offset);
    let count = r_u32(data, offset + 4);
    (lba, count)
}

/// Convert from little-endian u32 to the format used in Python's struct.unpack('I', ...)
/// This is the same as r_u32 above, included for API compatibility.
// pub use r_u32 as read_u32;
// pub use r_s32 as read_s32;

/// Write an OBJ file from mesh groups
pub fn write_obj(
    path: &Path,
    groups: &[MeshGroup],
    mtl_name: &str,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "mtllib {}.mtl", mtl_name)?;
    writeln!(f, "o rac_mesh")?;

    let mut global_vidx = 1u32;

    for group in groups {
        // Write vertices
        let base = global_vidx;
        for v in &group.vertices {
            if let (Some(u), Some(vt)) = (v.u, v.v) {
                writeln!(f, "v {:.6} {:.6} {:.6}", v.x, v.y, v.z)?;
                writeln!(f, "vt {:.6} {:.6}", u, vt)?;
            } else {
                writeln!(f, "v {:.6} {:.6} {:.6}", v.x, v.y, v.z)?;
            }
        }

        // Write faces
        writeln!(f, "usemtl {}", group.material)?;
        writeln!(f, "s off")?;
        for tri in &group.faces {
            if group.vertices[0].u.is_some() {
                writeln!(
                    f,
                    "f {}/{} {}/{} {}/{}",
                    base + tri[0], base + tri[0],
                    base + tri[1], base + tri[1],
                    base + tri[2], base + tri[2]
                )?;
            } else {
                writeln!(f, "f {} {} {}", base + tri[0], base + tri[1], base + tri[2])?;
            }
        }

        global_vidx += group.vertices.len() as u32;
    }

    Ok(())
}

/// Write an MTL file
pub fn write_mtl(path: &Path, groups: &[MeshGroup], base_dir: &Path) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    for group in groups {
        writeln!(f, "newmtl {}", group.material)?;
        writeln!(f, "Ka 0.6 0.6 0.6")?;
        writeln!(f, "Kd 0.8 0.8 0.8")?;
        writeln!(f, "Ks 0.2 0.2 0.2")?;
        writeln!(f, "Ns 10.0")?;
        writeln!(f, "d 1.0")?;
        writeln!(f, "illum 2")?;
        // Try to find a matching texture
        let tex_path = base_dir.join(format!("{}.png", group.material));
        if tex_path.exists() {
            writeln!(f, "map_Kd {}", tex_path.file_name().unwrap().to_string_lossy())?;
        }
    }
    Ok(())
}

/// Use types from types.rs
pub use crate::types::*;

/// Level dispatch helper: if level_num is >= LEVEL_COUNT, process all levels
pub fn level_dispatch<F>(level_num: i32, mut f: F) -> Result<(), String>
where
    F: FnMut(u32) -> Result<(), String>,
{
    if level_num < 0 || level_num >= LEVEL_COUNT as i32 {
        for lvl in 0..LEVEL_COUNT {
            f(lvl)?;
        }
    } else {
        f(level_num as u32)?;
    }
    Ok(())
}
