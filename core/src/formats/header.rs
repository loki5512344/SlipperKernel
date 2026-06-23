#![allow(unused_imports)]

use crate::parser::{le32, le64};
use alloc::vec::Vec;

use super::segment::{OnxSegment, ONX_MAX_SEGS};

// ════════════════════════════════════════════════════════════════════════════
// OnyxExec v2 — расширенный формат бинарников
// ════════════════════════════════════════════════════════════════════════════

pub const ONX_MAGIC: u32 = 0x31584E4F; // 'ONX1' LE
pub const ONX_VERSION_1: u32 = 1; // Fixed 8 segments (backward compat)
pub const ONX_VERSION_2: u32 = 2; // Dynamic segments, no limit
pub const ONX_VERSION: u32 = ONX_VERSION_2;
pub const ONX_FLAGS_RING1: u32 = 0x2;
pub const ONX_FLAGS_COMPRESSED: u32 = 0x4; // Segment data is compressed (LZ4-like)

pub const VMM_R: u32 = 1 << 1;
pub const VMM_W: u32 = 1 << 2;
pub const VMM_X: u32 = 1 << 3;
pub const VMM_U: u32 = 1 << 4;
pub const VMM_A: u32 = 1 << 6;
pub const VMM_D: u32 = 1 << 7;

/// OnyxExec header — v1: 344 bytes (fixed 8 segs), v2: 32 bytes + dynamic segs.
#[derive(Debug, Clone)]
pub struct OnxHeader {
    pub magic: u32,
    pub version: u32,
    pub entry: u64,
    pub nsegs: u32,
    pub flags: u32,
    pub segs: Vec<OnxSegment>,
}

impl OnxHeader {
    /// v1 header size: 24 + 8*40 = 344 bytes.
    pub const V1_HEADER_SIZE: usize = 344;
    /// v2 header size: 32 bytes (fixed part) + nsegs * 48.
    pub const V2_HEADER_SIZE: usize = 32;

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 24 {
            return None;
        }
        let magic = le32(&buf[0..4]);
        if magic != ONX_MAGIC {
            return None;
        }
        let version = le32(&buf[4..8]);
        let entry = le64(&buf[8..16]);
        let nsegs = le32(&buf[16..20]);
        let flags = le32(&buf[20..24]);

        match version {
            1 => {
                // v1: fixed 8 segments, each 40 bytes, starting at offset 24.
                if buf.len() < Self::V1_HEADER_SIZE {
                    return None;
                }
                if nsegs as usize > 8 {
                    return None;
                }
                let mut segs = Vec::with_capacity(nsegs as usize);
                for i in 0..nsegs as usize {
                    let off = 24 + i * OnxSegment::SIZE_V1;
                    segs.push(OnxSegment::from_bytes_v1(
                        &buf[off..off + OnxSegment::SIZE_V1],
                    )?);
                }
                Some(Self {
                    magic,
                    version,
                    entry,
                    nsegs,
                    flags,
                    segs,
                })
            }
            2 => {
                // v2: 8 more bytes in header (total 32), then nsegs * 48 bytes.
                if buf.len() < Self::V2_HEADER_SIZE {
                    return None;
                }
                if nsegs as usize > ONX_MAX_SEGS {
                    return None;
                }
                let seg_table_offset = Self::V2_HEADER_SIZE;
                let seg_table_size = nsegs as usize * OnxSegment::SIZE_V2;
                if buf.len() < seg_table_offset + seg_table_size {
                    return None;
                }
                let mut segs = Vec::with_capacity(nsegs as usize);
                for i in 0..nsegs as usize {
                    let off = seg_table_offset + i * OnxSegment::SIZE_V2;
                    segs.push(OnxSegment::from_bytes_v2(
                        &buf[off..off + OnxSegment::SIZE_V2],
                    )?);
                }
                Some(Self {
                    magic,
                    version,
                    entry,
                    nsegs,
                    flags,
                    segs,
                })
            }
            _ => None,
        }
    }

    pub fn to_bytes_v1(&self) -> Vec<u8> {
        let mut b = alloc::vec![0u8; Self::V1_HEADER_SIZE];
        b[0..4].copy_from_slice(&self.magic.to_le_bytes());
        b[4..8].copy_from_slice(&1u32.to_le_bytes()); // version 1
        b[8..16].copy_from_slice(&self.entry.to_le_bytes());
        b[16..20].copy_from_slice(&self.nsegs.to_le_bytes());
        b[20..24].copy_from_slice(&self.flags.to_le_bytes());
        for (i, s) in self.segs.iter().enumerate().take(8) {
            let off = 24 + i * OnxSegment::SIZE_V1;
            b[off..off + OnxSegment::SIZE_V1].copy_from_slice(&s.to_bytes_v1());
        }
        b
    }

    pub fn to_bytes_v2(&self) -> Vec<u8> {
        let total = Self::V2_HEADER_SIZE + self.nsegs as usize * OnxSegment::SIZE_V2;
        let mut b = alloc::vec![0u8; total];
        b[0..4].copy_from_slice(&self.magic.to_le_bytes());
        b[4..8].copy_from_slice(&2u32.to_le_bytes()); // version 2
        b[8..16].copy_from_slice(&self.entry.to_le_bytes());
        b[16..20].copy_from_slice(&self.nsegs.to_le_bytes());
        b[20..24].copy_from_slice(&self.flags.to_le_bytes());
        // bytes 24..32 reserved in v2 header
        for (i, s) in self.segs.iter().enumerate() {
            let off = Self::V2_HEADER_SIZE + i * OnxSegment::SIZE_V2;
            b[off..off + OnxSegment::SIZE_V2].copy_from_slice(&s.to_bytes_v2());
        }
        b
    }
}
