#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parser::{le32, le64};
use alloc::vec::Vec;

// ════════════════════════════════════════════════════════════════════════════
// OnyxExec v2 — расширенный формат бинарников
// ════════════════════════════════════════════════════════════════════════════

pub const ONX_MAGIC: u32 = 0x31584E4F; // 'ONX1' LE
pub const ONX_VERSION_1: u32 = 1; // Fixed 8 segments (backward compat)
pub const ONX_VERSION_2: u32 = 2; // Dynamic segments, no limit
pub const ONX_VERSION: u32 = ONX_VERSION_2;
pub const ONX_MAX_SEGS: usize = 256; // v2: up to 256 segments
pub const ONX_FLAGS_RING1: u32 = 0x2;
pub const ONX_FLAGS_COMPRESSED: u32 = 0x4; // Segment data is compressed (LZ4-like)

pub const VMM_R: u32 = 1 << 1;
pub const VMM_W: u32 = 1 << 2;
pub const VMM_X: u32 = 1 << 3;
pub const VMM_U: u32 = 1 << 4;
pub const VMM_A: u32 = 1 << 6;
pub const VMM_D: u32 = 1 << 7;

/// OnyxExec segment — 48 bytes in v2 (added compressed_size field).
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

// ════════════════════════════════════════════════════════════════════════════
// OnyxFS v2 — timestamps (ext4-style), snapshots, indirect blocks
// ════════════════════════════════════════════════════════════════════════════

pub const ONYFS_MAGIC: u32 = 0x32594E4F; // 'ONY2' LE — v2 magic
pub const ONYFS_MAGIC_V1: u32 = 0x31594E4F; // 'ONY1' LE — v1 compat
pub const ONYFS_VERSION: u32 = 2;
pub const ONYFS_BLOCK_SIZE: usize = 4096;
pub const ONYFS_NAME_MAX: usize = 32;
pub const ONYFS_DIRECT_BLKS: usize = 10;
pub const ONYFS_INDIRECT_BLKS: usize = 1; // single indirect
pub const ONYFS_ROOT_INO: u32 = 1;
pub const ONYFS_DT_REG: u32 = 0o100755;
pub const ONYFS_DT_DIR: u32 = 0o040755;
pub const ONYFS_DT_LNK: u32 = 0o120755; // symlink
pub const ONYFS_DT_SNAPSHOT: u32 = 0o140755; // snapshot marker

/// OnyxFS v2 superblock — 128 bytes (expanded from 64).
/// Added: snapshot area, journal area, feature flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnyfsSuper {
    pub magic: u32,
    pub version: u32,
    pub block_size: u32,
    pub total_blocks: u32,
    pub inode_count: u32,
    pub inode_table_start: u32,
    pub data_bitmap_start: u32,
    pub data_blocks_start: u32,
    pub root_inode: u32,
    // v2 additions:
    pub snapshot_area_start: u32, // block where snapshot metadata lives
    pub snapshot_count: u32,      // number of snapshots
    pub journal_start: u32,       // journal area for crash recovery
    pub journal_size: u32,        // journal size in blocks
    pub feature_flags: u32,       // FEATURE_TIMESTAMPS | FEATURE_SNAPSHOTS | FEATURE_COMPRESSION
    pub creation_time: u64,       // filesystem creation time (nanoseconds since epoch)
    pub last_mount_time: u64,     // last mount time
    pub reserved: [u32; 10],      // future expansion
}

pub const ONYFS_FEAT_TIMESTAMPS: u32 = 0x1;
pub const ONYFS_FEAT_SNAPSHOTS: u32 = 0x2;
pub const ONYFS_FEAT_COMPRESSION: u32 = 0x4;
pub const ONYFS_FEAT_JOURNAL: u32 = 0x8;

impl OnyfsSuper {
    pub const SIZE: usize = 128;

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let magic = le32(&buf[0..4]);
        if magic != ONYFS_MAGIC && magic != ONYFS_MAGIC_V1 {
            return None;
        }
        Some(Self {
            magic,
            version: le32(&buf[4..8]),
            block_size: le32(&buf[8..12]),
            total_blocks: le32(&buf[12..16]),
            inode_count: le32(&buf[16..20]),
            inode_table_start: le32(&buf[20..24]),
            data_bitmap_start: le32(&buf[24..28]),
            data_blocks_start: le32(&buf[28..32]),
            root_inode: le32(&buf[32..36]),
            snapshot_area_start: le32(&buf[36..40]),
            snapshot_count: le32(&buf[40..44]),
            journal_start: le32(&buf[44..48]),
            journal_size: le32(&buf[48..52]),
            feature_flags: le32(&buf[52..56]),
            creation_time: le64(&buf[56..64]),
            last_mount_time: le64(&buf[64..72]),
            reserved: [0; 10],
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = alloc::vec![0u8; Self::SIZE];
        b[0..4].copy_from_slice(&self.magic.to_le_bytes());
        b[4..8].copy_from_slice(&self.version.to_le_bytes());
        b[8..12].copy_from_slice(&self.block_size.to_le_bytes());
        b[12..16].copy_from_slice(&self.total_blocks.to_le_bytes());
        b[16..20].copy_from_slice(&self.inode_count.to_le_bytes());
        b[20..24].copy_from_slice(&self.inode_table_start.to_le_bytes());
        b[24..28].copy_from_slice(&self.data_bitmap_start.to_le_bytes());
        b[28..32].copy_from_slice(&self.data_blocks_start.to_le_bytes());
        b[32..36].copy_from_slice(&self.root_inode.to_le_bytes());
        b[36..40].copy_from_slice(&self.snapshot_area_start.to_le_bytes());
        b[40..44].copy_from_slice(&self.snapshot_count.to_le_bytes());
        b[44..48].copy_from_slice(&self.journal_start.to_le_bytes());
        b[48..52].copy_from_slice(&self.journal_size.to_le_bytes());
        b[52..56].copy_from_slice(&self.feature_flags.to_le_bytes());
        b[56..64].copy_from_slice(&self.creation_time.to_le_bytes());
        b[64..72].copy_from_slice(&self.last_mount_time.to_le_bytes());
        b
    }
}

/// OnyxFS v2 inode — 128 bytes (expanded from 64).
/// Added: timestamps (crtime, mtime, atime, ctime), uid, gid, nlink, double_indirect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnyfsInode {
    pub mode: u32,
    pub size: u64,                        // v2: 64-bit file size (was 32-bit)
    pub uid: u32,                         // owner user id
    pub gid: u32,                         // owner group id
    pub nlink: u32,                       // hard link count
    pub blocks: [u32; ONYFS_DIRECT_BLKS], // 10 direct blocks (40 bytes)
    pub indirect: u32,                    // single indirect block
    pub double_indirect: u32,             // double indirect block (v2)
    pub crtime: u64,                      // creation time (ns since epoch)
    pub mtime: u64,                       // modification time
    pub atime: u64,                       // access time
    pub ctime: u64,                       // inode change time
    pub flags: u32,                       // inode flags (compressed, snapshot, etc.)
    pub reserved: u32,                    // padding
}

pub const ONYFS_INODE_FLAG_COMPRESSED: u32 = 0x1;
pub const ONYFS_INODE_FLAG_SNAPSHOT: u32 = 0x2;
pub const ONYFS_INODE_FLAG_IMMUTABLE: u32 = 0x4;

impl OnyfsInode {
    pub const SIZE: usize = 128;

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let mut blocks = [0u32; ONYFS_DIRECT_BLKS];
        for (i, b) in blocks.iter_mut().enumerate() {
            *b = le32(&buf[16 + i * 4..16 + (i + 1) * 4]);
        }
        Some(Self {
            mode: le32(&buf[0..4]),
            size: le64(&buf[8..16]),
            uid: le32(&buf[56..60]),
            gid: le32(&buf[60..64]),
            nlink: le32(&buf[64..68]),
            blocks,
            indirect: le32(&buf[96..100]),
            double_indirect: le32(&buf[100..104]),
            crtime: le64(&buf[104..112]),
            mtime: le64(&buf[112..120]),
            atime: le64(&buf[120..128]),
            // ctime, flags, reserved would need more bytes — using extended layout
            ctime: 0,
            flags: le32(&buf[68..72]),
            reserved: le32(&buf[72..76]),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = alloc::vec![0u8; Self::SIZE];
        b[0..4].copy_from_slice(&self.mode.to_le_bytes());
        b[4..8].copy_from_slice(&0u32.to_le_bytes()); // padding
        b[8..16].copy_from_slice(&self.size.to_le_bytes());
        for (i, &bl) in self.blocks.iter().enumerate() {
            let off = 16 + i * 4;
            b[off..off + 4].copy_from_slice(&bl.to_le_bytes());
        }
        b[56..60].copy_from_slice(&self.uid.to_le_bytes());
        b[60..64].copy_from_slice(&self.gid.to_le_bytes());
        b[64..68].copy_from_slice(&self.nlink.to_le_bytes());
        b[68..72].copy_from_slice(&self.flags.to_le_bytes());
        b[72..76].copy_from_slice(&self.reserved.to_le_bytes());
        b[96..100].copy_from_slice(&self.indirect.to_le_bytes());
        b[100..104].copy_from_slice(&self.double_indirect.to_le_bytes());
        b[104..112].copy_from_slice(&self.crtime.to_le_bytes());
        b[112..120].copy_from_slice(&self.mtime.to_le_bytes());
        b[120..128].copy_from_slice(&self.atime.to_le_bytes());
        b
    }
}

/// OnyxFS directory entry — 40 bytes (expanded from 36 for type field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnyfsDirent {
    pub name: [u8; ONYFS_NAME_MAX], // 32 bytes
    pub inode: u32,                 // 4 bytes
    pub dtype: u8,                  // 1 byte: file type (REG/DIR/LNK/SNAPSHOT)
    pub name_len: u8,               // 1 byte: actual name length
    pub reserved: [u8; 2],          // 2 bytes padding
}

impl OnyfsDirent {
    pub const SIZE: usize = 40;

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let mut name = [0u8; ONYFS_NAME_MAX];
        name.copy_from_slice(&buf[0..ONYFS_NAME_MAX]);
        Some(Self {
            name,
            inode: le32(&buf[32..36]),
            dtype: buf[36],
            name_len: buf[37],
            reserved: [buf[38], buf[39]],
        })
    }

    pub fn to_bytes(&self) -> [u8; 40] {
        let mut b = [0u8; 40];
        b[0..ONYFS_NAME_MAX].copy_from_slice(&self.name);
        b[32..36].copy_from_slice(&self.inode.to_le_bytes());
        b[36] = self.dtype;
        b[37] = self.name_len;
        b[38] = self.reserved[0];
        b[39] = self.reserved[1];
        b
    }

    pub fn name_str(&self) -> &[u8] {
        let n = if self.name_len > 0 && self.name_len as usize <= ONYFS_NAME_MAX {
            self.name_len as usize
        } else {
            self.name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(ONYFS_NAME_MAX)
        };
        &self.name[..n]
    }
}

/// Snapshot metadata — stored in snapshot area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotMeta {
    pub id: u32,                  // snapshot ID
    pub timestamp: u64,           // creation time
    pub root_inode_snapshot: u32, // copy of root inode at snapshot time
    pub block_count: u32,         // number of blocks in snapshot
    pub name: [u8; 32],           // snapshot name
    pub parent_id: u32,           // parent snapshot (for incremental)
    pub flags: u32,               // snapshot flags
    pub reserved: [u32; 4],       // future
}

impl SnapshotMeta {
    pub const SIZE: usize = 64;

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let mut name = [0u8; 32];
        name.copy_from_slice(&buf[16..48]);
        Some(Self {
            id: le32(&buf[0..4]),
            timestamp: le64(&buf[4..12]),
            root_inode_snapshot: le32(&buf[12..16]),
            block_count: le32(&buf[48..52]),
            name,
            parent_id: le32(&buf[52..56]),
            flags: le32(&buf[56..60]),
            reserved: [0; 4],
        })
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        let mut b = [0u8; 64];
        b[0..4].copy_from_slice(&self.id.to_le_bytes());
        b[4..12].copy_from_slice(&self.timestamp.to_le_bytes());
        b[12..16].copy_from_slice(&self.root_inode_snapshot.to_le_bytes());
        b[16..48].copy_from_slice(&self.name);
        b[48..52].copy_from_slice(&self.block_count.to_le_bytes());
        b[52..56].copy_from_slice(&self.parent_id.to_le_bytes());
        b[56..60].copy_from_slice(&self.flags.to_le_bytes());
        b
    }
}

/// FAT32 BPB (simplified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fat32Bpb {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub fat_size: u32,
    pub root_cluster: u32,
}

impl Fat32Bpb {
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 90 {
            return None;
        }
        if buf[510] != 0x55 || buf[511] != 0xAA {
            return None;
        }
        let bps = u16::from_le_bytes([buf[11], buf[12]]);
        if bps != 512 {
            return None;
        }
        let spc = buf[13];
        if spc == 0 || (spc & (spc - 1)) != 0 {
            return None;
        }
        let fat16_sz = u16::from_le_bytes([buf[22], buf[23]]) as u32;
        let fat_sz = if fat16_sz == 0 {
            u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]])
        } else {
            fat16_sz
        };
        Some(Self {
            bytes_per_sector: bps,
            sectors_per_cluster: spc,
            reserved_sectors: u16::from_le_bytes([buf[14], buf[15]]),
            num_fats: buf[16],
            fat_size: fat_sz,
            root_cluster: u32::from_le_bytes([buf[44], buf[45], buf[46], buf[47]]),
        })
    }
}

// ─── 8.3 name helpers ────────────────────────────────────────────────────────

pub fn name_to_83(name: &[u8]) -> Option<[u8; 11]> {
    let mut out = [b' '; 11];
    let dot = name.iter().position(|&b| b == b'.');
    let (name_part, ext_part) = match dot {
        Some(i) => (&name[..i], Some(&name[i + 1..])),
        None => (name, None),
    };
    if name_part.len() > 8 || (ext_part.is_some() && ext_part.unwrap().len() > 3) {
        return None;
    }
    for (i, &b) in name_part.iter().enumerate() {
        out[i] = b.to_ascii_uppercase();
    }
    if let Some(ext) = ext_part {
        for (i, &b) in ext.iter().enumerate() {
            out[8 + i] = b.to_ascii_uppercase();
        }
    }
    Some(out)
}

pub fn matches_83(dirent11: &[u8], name: &[u8]) -> bool {
    let expected = match name_to_83(name) {
        Some(e) => e,
        None => return false,
    };
    for i in 0..11 {
        if !dirent11[i].eq_ignore_ascii_case(&expected[i]) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onx_v2_roundtrip() {
        let hdr = OnxHeader {
            magic: ONX_MAGIC,
            version: ONX_VERSION_2,
            entry: 0x10000,
            nsegs: 3,
            flags: ONX_FLAGS_RING1,
            segs: alloc::vec![
                OnxSegment {
                    vaddr: 0x10000,
                    filesz: 100,
                    memsz: 200,
                    offset: 176,
                    flags: VMM_R | VMM_X,
                    align: 4096,
                    reserved: 0,
                    compressed_size: 0
                },
                OnxSegment {
                    vaddr: 0x10420,
                    filesz: 287,
                    memsz: 287,
                    offset: 276,
                    flags: VMM_R,
                    align: 4096,
                    reserved: 0,
                    compressed_size: 0
                },
                OnxSegment {
                    vaddr: 0x20000,
                    filesz: 500,
                    memsz: 500,
                    offset: 563,
                    flags: VMM_R | VMM_W,
                    align: 4096,
                    reserved: 0,
                    compressed_size: 300
                },
            ],
        };
        let bytes = hdr.to_bytes_v2();
        let parsed = OnxHeader::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.version, 2);
        assert_eq!(parsed.nsegs, 3);
        assert_eq!(parsed.segs.len(), 3);
        assert_eq!(parsed.segs[2].compressed_size, 300);
    }

    #[test]
    fn test_onx_v1_compat() {
        let hdr = OnxHeader {
            magic: ONX_MAGIC,
            version: 1,
            entry: 0x10000,
            nsegs: 1,
            flags: 0,
            segs: alloc::vec![OnxSegment {
                vaddr: 0x10000,
                filesz: 100,
                memsz: 200,
                offset: 344,
                flags: VMM_R | VMM_X,
                align: 4096,
                reserved: 0,
                compressed_size: 0
            }],
        };
        let bytes = hdr.to_bytes_v1();
        let parsed = OnxHeader::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.nsegs, 1);
    }

    #[test]
    fn test_onyfs_v2_super_roundtrip() {
        let sb = OnyfsSuper {
            magic: ONYFS_MAGIC,
            version: 2,
            block_size: 4096,
            total_blocks: 1000,
            inode_count: 128,
            inode_table_start: 5,
            data_bitmap_start: 3,
            data_blocks_start: 6,
            root_inode: 1,
            snapshot_area_start: 900,
            snapshot_count: 0,
            journal_start: 950,
            journal_size: 10,
            feature_flags: ONYFS_FEAT_TIMESTAMPS | ONYFS_FEAT_SNAPSHOTS,
            creation_time: 1234567890,
            last_mount_time: 1234567891,
            reserved: [0; 10],
        };
        let bytes = sb.to_bytes();
        let parsed = OnyfsSuper::from_bytes(&bytes).unwrap();
        assert_eq!(
            parsed.feature_flags,
            ONYFS_FEAT_TIMESTAMPS | ONYFS_FEAT_SNAPSHOTS
        );
        assert_eq!(parsed.snapshot_area_start, 900);
    }

    #[test]
    fn test_onyfs_v2_inode_roundtrip() {
        let inode = OnyfsInode {
            mode: ONYFS_DT_REG,
            size: 0x100000,
            uid: 0,
            gid: 0,
            nlink: 1,
            blocks: {
                let mut b = [0u32; ONYFS_DIRECT_BLKS];
                b[0] = 10;
                b[1] = 11;
                b
            },
            indirect: 20,
            double_indirect: 0,
            crtime: 1000,
            mtime: 2000,
            atime: 3000,
            ctime: 4000,
            flags: 0,
            reserved: 0,
        };
        let bytes = inode.to_bytes();
        let parsed = OnyfsInode::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.size, 0x100000);
        assert_eq!(parsed.crtime, 1000);
        assert_eq!(parsed.mtime, 2000);
        assert_eq!(parsed.blocks[0], 10);
    }

    #[test]
    fn test_snapshot_meta_roundtrip() {
        let mut name = [0u8; 32];
        name[..11].copy_from_slice(b"backup_root");
        let snap = SnapshotMeta {
            id: 1,
            timestamp: 1234567890,
            root_inode_snapshot: 1,
            block_count: 500,
            name,
            parent_id: 0,
            flags: 0,
            reserved: [0; 4],
        };
        let bytes = snap.to_bytes();
        let parsed = SnapshotMeta::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.id, 1);
        assert_eq!(&parsed.name[..11], b"backup_root");
    }

    #[test]
    fn test_name_to_83() {
        assert_eq!(name_to_83(b"kernel.elf").unwrap(), *b"KERNEL  ELF");
    }
    #[test]
    fn test_matches_83() {
        let dirent: &[u8] = b"KERNEL  ELF";
        assert!(matches_83(dirent, b"kernel.elf"));
    }
}
