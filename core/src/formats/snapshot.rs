use crate::parser::{le32, le64};
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
