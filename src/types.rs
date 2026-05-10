/// Shared types for RAC tooling

/// VIF command codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VifCmd {
    Nop = 0x00,
    Stcycl = 0x01,
    Offset = 0x02,
    Base = 0x03,
    Itops = 0x04,
    Stmod = 0x05,
    UsrDir = 0x06,
    MscalF = 0x07,
    MscntF = 0x08,
    Unk09 = 0x09,
    Unk0A = 0x0A,
    Unk0B = 0x0B,
    Unk0C = 0x0C,
    Unk0D = 0x0D,
    Unk0E = 0x0E,
    Unk0F = 0x0F,
    StMask = 0x20,
    StRow = 0x30,
    StColR = 0x31,
    StColC = 0x32,
    MscPath = 0x38,
    MskPath3 = 0x39,
    StCyCl = 0x4A,
    FlushE = 0x50,
    FlushA = 0x51,
    MscCal = 0x58,
    MscCnt = 0x59,
    MscCat = 0x5A,
    DirDma = 0x60,
    DirDmaIce = 0x61,
    UnpackV4_32 = 0x68,
    UnpackV3_32 = 0x69,
    UnpackV4_16 = 0x6A,
    UnpackV3_16 = 0x6B,
    UnpackV4_8 = 0x6C,
    UnpackV3_8 = 0x6D,
    UnpackV2_32 = 0x70,
    UnpackV2_16 = 0x72,
    UnpackV2_8 = 0x74,
    UnpackS3_32 = 0x79,
    UnpackS3_16 = 0x7B,
    UnpackS3_8 = 0x7D,
    UnpackV4_5 = 0x6E,  // 4x5-bit packed format (metal moby packets)
}

impl VifCmd {
    pub fn from_u8(val: u8) -> Option<Self> {
        use VifCmd::*;
        Some(match val {
            0x00 => Nop,
            0x01 => Stcycl,
            0x02 => Offset,
            0x03 => Base,
            0x04 => Itops,
            0x05 => Stmod,
            0x06 => UsrDir,
            0x07 => MscalF,
            0x08 => MscntF,
            0x09 => Unk09,
            0x0A => Unk0A,
            0x0B => Unk0B,
            0x0C => Unk0C,
            0x0D => Unk0D,
            0x0E => Unk0E,
            0x0F => Unk0F,
            0x20 => StMask,
            0x30 => StRow,
            0x31 => StColR,
            0x32 => StColC,
            0x38 => MscPath,
            0x39 => MskPath3,
            0x4A => StCyCl,
            0x50 => FlushE,
            0x51 => FlushA,
            0x58 => MscCal,
            0x59 => MscCnt,
            0x5A => MscCat,
            0x60 => DirDma,
            0x61 => DirDmaIce,
            0x68 => UnpackV4_32,
            0x69 => UnpackV3_32,
            0x6A => UnpackV4_16,
            0x6B => UnpackV3_16,
            0x6C => UnpackV4_8,
            0x6D => UnpackV3_8,
            0x70 => UnpackV2_32,
            0x72 => UnpackV2_16,
            0x74 => UnpackV2_8,
            0x79 => UnpackS3_32,
            0x7B => UnpackS3_16,
            0x7D => UnpackS3_8,
            0x6E => UnpackV4_5,
            _ => return None,
        })
    }

    pub fn is_unpack(self) -> bool {
        matches!(
            self,
            VifCmd::UnpackV4_32
                | VifCmd::UnpackV3_32
                | VifCmd::UnpackV4_16
                | VifCmd::UnpackV3_16
                | VifCmd::UnpackV4_8
                | VifCmd::UnpackV3_8
                | VifCmd::UnpackV2_32
                | VifCmd::UnpackV2_16
                | VifCmd::UnpackV2_8
                | VifCmd::UnpackS3_32
                | VifCmd::UnpackS3_16
                | VifCmd::UnpackS3_8
                | VifCmd::UnpackV4_5
        )
    }
}

/// A parsed VIF command
#[derive(Debug, Clone)]
pub struct VifCommand {
    pub cmd: VifCmd,
    pub imm: u16,      // immediate value
    pub qword_count: u8,
}

/// A vertex with position and optional UV/normal
#[derive(Debug, Clone, Default)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub u: Option<f32>,
    pub v: Option<f32>,
}

/// A mesh group (material + vertices + faces)
#[derive(Debug, Clone)]
pub struct MeshGroup {
    pub material: String,
    pub vertices: Vec<Vertex>,
    pub faces: Vec<[u32; 3]>,   // triangle indices
}

/// Texture entry from GS memory
#[derive(Debug, Clone)]
pub struct TextureEntry {
    pub tbp: u32,     // texture base pointer
    pub tbw: u32,     // texture buffer width
    pub psm: u32,     // pixel storage format
    pub tw: u32,      // texture width (log2)
    pub th: u32,      // texture height (log2)
    pub tcc: u32,     // texture color component
    pub mag: u32,
    pub min: u32,
    pub mip: u32,
    pub addr: u32,
}

/// Core-level data header
#[derive(Debug, Clone)]
pub struct LevelCoreHeader {
    pub tie_count: u32,
    pub tie_data_ofs: u32,
    pub tie_data_size: u32,
    pub moby_count: u32,
    pub moby_data_ofs: u32,
    pub moby_data_size: u32,
    pub shrub_count: u32,
    pub shrub_data_ofs: u32,
    pub shrub_data_size: u32,
    pub tfrag_count: u32,
    pub tfrag_data_ofs: u32,
    pub tfrag_data_size: u32,
}
