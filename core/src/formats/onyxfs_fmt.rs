use crate::parser::{le32, le64};
use alloc::vec::Vec;
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
