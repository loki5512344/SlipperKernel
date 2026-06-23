#![allow(unused_imports)]

use crate::parser::{le32, le64};

// ════════════════════════════════════════════════════════════════════════════
// OnyxExec segment — 48 bytes in v2 (added compressed_size field).
// ════════════════════════════════════════════════════════════════════════════

pub const ONX_MAX_SEGS: usize = 256; // v2: up to 256 segments

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnxSegment {
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub offset: u32,
    pub flags: u32,
    pub align: u32,
    pub reserved: u32,
    pub compressed_size: u32, // v2: if 0, not compressed; else compressed size
}

impl OnxSegment {
    pub const SIZE_V1: usize = 40;
    pub const SIZE_V2: usize = 48;

    pub fn from_bytes_v1(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE_V1 {
            return None;
        }
        Some(Self {
            vaddr: le64(&buf[0..8]),
            filesz: le64(&buf[8..16]),
            memsz: le64(&buf[16..24]),
            offset: le32(&buf[24..28]),
            flags: le32(&buf[28..32]),
            align: le32(&buf[32..36]),
            reserved: le32(&buf[36..40]),
            compressed_size: 0,
        })
    }

    pub fn from_bytes_v2(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE_V2 {
            return None;
        }
        Some(Self {
            vaddr: le64(&buf[0..8]),
            filesz: le64(&buf[8..16]),
            memsz: le64(&buf[16..24]),
            offset: le32(&buf[24..28]),
            flags: le32(&buf[28..32]),
            align: le32(&buf[32..36]),
            reserved: le32(&buf[36..40]),
            compressed_size: le32(&buf[40..44]),
        })
    }

    pub fn to_bytes_v1(&self) -> [u8; 40] {
        let mut b = [0u8; 40];
        b[0..8].copy_from_slice(&self.vaddr.to_le_bytes());
        b[8..16].copy_from_slice(&self.filesz.to_le_bytes());
        b[16..24].copy_from_slice(&self.memsz.to_le_bytes());
        b[24..28].copy_from_slice(&self.offset.to_le_bytes());
        b[28..32].copy_from_slice(&self.flags.to_le_bytes());
        b[32..36].copy_from_slice(&self.align.to_le_bytes());
        b[36..40].copy_from_slice(&self.reserved.to_le_bytes());
        b
    }

    pub fn to_bytes_v2(&self) -> [u8; 48] {
        let mut b = [0u8; 48];
        b[0..8].copy_from_slice(&self.vaddr.to_le_bytes());
        b[8..16].copy_from_slice(&self.filesz.to_le_bytes());
        b[16..24].copy_from_slice(&self.memsz.to_le_bytes());
        b[24..28].copy_from_slice(&self.offset.to_le_bytes());
        b[28..32].copy_from_slice(&self.flags.to_le_bytes());
        b[32..36].copy_from_slice(&self.align.to_le_bytes());
        b[36..40].copy_from_slice(&self.reserved.to_le_bytes());
        b[40..44].copy_from_slice(&self.compressed_size.to_le_bytes());
        b
    }
}
