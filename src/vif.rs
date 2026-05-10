/// VIF (Vector InterFace) command parsing
/// Shared by moby, shrub, tfrag, and stash extractors

use crate::common::r_u32;
pub use crate::types::{VifCmd, VifCommand};

/// Decode a VIF code from a u32 at the given offset
pub fn read_vif_code(val: u32) -> (VifCmd, u16, u8) {
    let cmd_byte = (val & 0xFF) as u8;
    let imm = ((val >> 16) & 0xFFFF) as u16;
    let qwc = ((val >> 8) & 0xFF) as u8;
    let cmd = VifCmd::from_u8(cmd_byte).unwrap_or(VifCmd::Nop);
    (cmd, imm, qwc)
}

/// Unpack the VNVL format to get element size
pub fn unpack_element_size(vnvl_raw: u8) -> u8 {
    const VIF_V4_32: u8 = 0;
    const VIF_V3_32: u8 = 1;
    const VIF_V4_16: u8 = 2;
    const VIF_V3_16: u8 = 3;
    const VIF_V4_8: u8 = 4;
    const VIF_V3_8: u8 = 5;
    const VIF_V2_32: u8 = 8;
    const VIF_V2_16: u8 = 10;
    const VIF_V2_8: u8 = 12;
    const VIF_S3_32: u8 = 9;
    const VIF_S3_16: u8 = 11;
    const VIF_S3_8: u8 = 13;

    match vnvl_raw >> 4 {
        VIF_V4_32 | VIF_V3_32 | VIF_V2_32 | VIF_S3_32 => 4,
        VIF_V4_16 | VIF_V3_16 | VIF_V2_16 | VIF_S3_16 => 2,
        VIF_V4_8 | VIF_V3_8 | VIF_V2_8 | VIF_S3_8 => 1,
        _ => 4,
    }
}

/// Unpack qword count from VIF code for a given number of elements
pub fn unpack_qword_count(vnvl_raw: u8, num_values: u32) -> u32 {
    const VIF_S3_32: u8 = 0x90;

    if (vnvl_raw >> 4) == VIF_S3_32 {
        ((num_values + 3) / 4) + 1
    } else {
        (num_values + 3) / 4
    }
}

/// Read a VIF command list from data starting at base_ofs
pub fn read_vif_command_list(
    data: &[u8],
    base_ofs: usize,
    max_size: usize,
) -> Vec<VifCommand> {
    let mut commands = Vec::new();
    let mut offset = base_ofs;
    let end = (base_ofs + max_size).min(data.len());

    while offset + 4 <= end {
        let val = r_u32(data, offset);
        let (cmd, imm, qwc) = read_vif_code(val);

        if cmd == VifCmd::Nop || cmd == VifCmd::FlushA || cmd == VifCmd::FlushE {
            offset += 4;
            continue;
        }

        if cmd == VifCmd::DirDma || cmd == VifCmd::DirDmaIce {
            break;
        }

        let cmd_entry = VifCommand {
            cmd,
            imm,
            qword_count: qwc,
        };

        commands.push(cmd_entry);

        if cmd.is_unpack() {
            // Skip unpack data (qword_count * 16 bytes)
            let skip = (qwc as usize) * 16;
            offset += 4 + skip;
        } else {
            offset += 4;
        }
    }

    commands
}

/// Filter to keep only UNPACK commands
pub fn filter_vif_unpacks(commands: &[VifCommand]) -> Vec<&VifCommand> {
    commands.iter().filter(|c| c.cmd.is_unpack()).collect()
}

/// Read unpacked V4_8 data
pub fn read_unpack_v4_8(data: &[u8], cmd: &VifCommand) -> Vec<[u8; 4]> {
    let qwc = cmd.qword_count as usize;
    let mut result = Vec::with_capacity(qwc * 4);
    // Each qword = 16 bytes = 4 × V4_8 elements
    for i in 0..qwc {
        let base = 4 + i * 16; // skip VIF code qword
        for j in 0..4 {
            let off = base + j * 4;
            if off + 4 <= data.len() {
                result.push([data[off], data[off + 1], data[off + 2], data[off + 3]]);
            }
        }
    }
    result
}

/// Read unpacked V4_16 data
pub fn read_unpack_v4_16(data: &[u8], cmd: &VifCommand) -> Vec<[u16; 4]> {
    use crate::common::r_u16;
    let qwc = cmd.qword_count as usize;
    let mut result = Vec::with_capacity(qwc * 4);
    for i in 0..qwc {
        let base = 4 + i * 16;
        for j in 0..4 {
            let off = base + j * 8;
            if off + 8 <= data.len() {
                result.push([
                    r_u16(data, off),
                    r_u16(data, off + 2),
                    r_u16(data, off + 4),
                    r_u16(data, off + 6),
                ]);
            }
        }
    }
    result
}

/// Read unpacked V3_16 data
pub fn read_unpack_v3_16(data: &[u8], cmd: &VifCommand) -> Vec<[u16; 4]> {
    use crate::common::r_u16;
    let qwc = cmd.qword_count as usize;
    let mut result = Vec::with_capacity(qwc * 4);
    for i in 0..qwc {
        let base = 4 + i * 16;
        for j in 0..4 {
            let off = base + j * 8;
            if off + 8 <= data.len() {
                result.push([
                    r_u16(data, off),
                    r_u16(data, off + 2),
                    r_u16(data, off + 4),
                    0,
                ]);
            }
        }
    }
    result
}

/// Read unpacked V4_32 data
pub fn read_unpack_v4_32(data: &[u8], cmd: &VifCommand) -> Vec<[f32; 4]> {
    use crate::common::r_s32;
    let qwc = cmd.qword_count as usize;
    let mut result = Vec::with_capacity(qwc * 4);
    for i in 0..qwc {
        let base = 4 + i * 16;
        for j in 0..4 {
            let off = base + j * 16;
            if off + 16 <= data.len() {
                result.push([
                    r_s32(data, off) as f32,
                    r_s32(data, off + 4) as f32,
                    r_s32(data, off + 8) as f32,
                    r_s32(data, off + 12) as f32,
                ]);
            }
        }
    }
    result
}

/// Read unpacked V2_16 data
pub fn read_unpack_v2_16(data: &[u8], cmd: &VifCommand) -> Vec<[u16; 2]> {
    use crate::common::r_u16;
    let qwc = cmd.qword_count as usize;
    let mut result = Vec::with_capacity(qwc * 4);
    for i in 0..qwc {
        let base = 4 + i * 16;
        for j in 0..4 {
            let off = base + j * 8;
            if off + 4 <= data.len() {
                result.push([r_u16(data, off), r_u16(data, off + 2)]);
            }
        }
    }
    result
}

/// Parse VIF data for vertex positions from V3_32 or V4_32 unpacks
pub fn parse_vertex_positions(data: &[u8], unpacks: &[&VifCommand]) -> Vec<[f32; 3]> {
    let mut verts = Vec::new();
    for cmd in unpacks {
        if cmd.cmd == VifCmd::UnpackV4_32 {
            for v in read_unpack_v4_32(data, cmd) {
                verts.push([v[0], v[1], v[2]]);
            }
        } else if cmd.cmd == VifCmd::UnpackV3_32 {
            let vals = read_unpack_v4_32(data, cmd);
            for v in vals {
                verts.push([v[0], v[1], v[2]]);
            }
        }
    }
    verts
}

/// Convert s8 byte to signed value
#[inline]
pub fn to_s8(val: u8) -> i8 {
    val as i8
}
