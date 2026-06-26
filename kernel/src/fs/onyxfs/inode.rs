//! Inode read/write, stat, and update_mtime. All v2-only for write paths —
//! v1 images are treated as read-only because their 64-byte inode has no
//! writable timestamp fields.
use super::journal::{journal_commit, journal_log};
use super::{
    inodes_per_block, read_block, write_block, OnyfsStat, G_BUF, G_SB, G_VERSION, ONYFS_V1,
    ONYFS_V1_INODE_SIZE,
};
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{OnyfsInode, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS};

/// Read an inode by number into the v2 `OnyfsInode` struct.
/// Works for both v1 (64-byte) and v2 (128-byte) on-disk layouts;
/// v1 fields are upcast (size u32 → u64, timestamps zeroed).
pub(super) unsafe fn read_inode(ino: u32, out: &mut OnyfsInode) -> KResult<()> {
    let ipb = inodes_per_block();
    let idx = (ino as usize).saturating_sub(1);
    let blk = (*(&raw const G_SB)).inode_table_start as usize + idx / ipb;
    let slot = idx % ipb;
    {
        let pb = &raw mut G_BUF;
        read_block(blk as u32, &mut *pb)
    }?;
    let buf_view: &[u8] = &(*(&raw const G_BUF));
    *out = match *(&raw const G_VERSION) {
        ONYFS_V1 => {
            // v1 64-byte inode layout:
            //   mode(0..4), size_u32(4..8), blocks[10](8..48),
            //   indirect(48..52), reserved[3](52..64)
            let off = slot * ONYFS_V1_INODE_SIZE;
            if off + ONYFS_V1_INODE_SIZE > ONYFS_BLOCK_SIZE {
                return Err(Errno::Inval);
            }
            let s = &buf_view[off..off + ONYFS_V1_INODE_SIZE];
            let mut blocks = [0u32; ONYFS_DIRECT_BLKS];
            for (i, b) in blocks.iter_mut().enumerate() {
                let o = 8 + i * 4;
                *b = u32::from_le_bytes([s[o], s[o + 1], s[o + 2], s[o + 3]]);
            }
            let mode = u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
            let size_u32 = u32::from_le_bytes([s[4], s[5], s[6], s[7]]);
            let indirect = u32::from_le_bytes([s[48], s[49], s[50], s[51]]);
            OnyfsInode {
                mode,
                size: size_u32 as u64,
                uid: 0,
                gid: 0,
                nlink: 0,
                blocks,
                indirect,
                double_indirect: 0,
                crtime: 0,
                mtime: 0,
                atime: 0,
                ctime: 0,
                flags: 0,
                reserved: 0,
            }
        }
        _ => {
            // v2 128-byte inode — parsed via the canonical `from_bytes`.
            let off = slot * OnyfsInode::SIZE;
            if off + OnyfsInode::SIZE > ONYFS_BLOCK_SIZE {
                return Err(Errno::Inval);
            }
            OnyfsInode::from_bytes(&buf_view[off..off + OnyfsInode::SIZE]).ok_or(Errno::Io)?
        }
    };
    Ok(())
}

/// Write an inode back to disk. v2 only (v1 has no writable metadata fields
/// beyond what `read` already covers, and is treated as read-only here).
/// Logs the inode-table block to the journal before writing.
pub(super) unsafe fn write_inode(ino: u32, inode: &OnyfsInode) -> KResult<()> {
    if *(&raw const G_VERSION) == ONYFS_V1 {
        return Err(Errno::NoSys);
    }
    let ipb = inodes_per_block();
    let idx = (ino as usize).saturating_sub(1);
    let blk = (*(&raw const G_SB)).inode_table_start + (idx / ipb) as u32;
    let slot = idx % ipb;
    {
        let pb = &raw mut G_BUF;
        read_block(blk, &mut *pb)
    }?;
    let bytes = inode.to_bytes();
    let off = slot * OnyfsInode::SIZE;
    let pb = &raw mut G_BUF;
    for i in 0..OnyfsInode::SIZE {
        (*pb)[off + i] = bytes[i];
    }
    journal_log(blk, &*pb)?;
    write_block(blk, &*pb)
}

/// Update the mtime of an inode to the current timer jiffies.
/// Intended for future write support — bumps the modification timestamp
/// and persists the inode. No-op on v1 filesystems (no timestamp fields).
pub unsafe fn update_mtime(ino: u32) -> KResult<()> {
    let mut inode = OnyfsInode {
        mode: 0,
        size: 0,
        uid: 0,
        gid: 0,
        nlink: 0,
        blocks: [0; ONYFS_DIRECT_BLKS],
        indirect: 0,
        double_indirect: 0,
        crtime: 0,
        mtime: 0,
        atime: 0,
        ctime: 0,
        flags: 0,
        reserved: 0,
    };
    read_inode(ino, &mut inode)?;
    inode.mtime = *(&raw const timer::G_JIFFIES);
    write_inode(ino, &inode)
}

pub unsafe fn set_timestamps(ino: u32, mtime: u64, atime: u64) -> KResult<()> {
    if *(&raw const G_VERSION) == ONYFS_V1 {
        return Err(Errno::NoSys);
    }
    let mut inode = OnyfsInode {
        mode: 0,
        size: 0,
        uid: 0,
        gid: 0,
        nlink: 0,
        blocks: [0; ONYFS_DIRECT_BLKS],
        indirect: 0,
        double_indirect: 0,
        crtime: 0,
        mtime: 0,
        atime: 0,
        ctime: 0,
        flags: 0,
        reserved: 0,
    };
    read_inode(ino, &mut inode)?;
    inode.mtime = mtime;
    inode.atime = atime;
    write_inode(ino, &inode)
}

pub unsafe fn stat(ino: u32, out: &mut OnyfsStat) -> KResult<()> {
    let mut inode = OnyfsInode {
        mode: 0,
        size: 0,
        uid: 0,
        gid: 0,
        nlink: 0,
        blocks: [0; ONYFS_DIRECT_BLKS],
        indirect: 0,
        double_indirect: 0,
        crtime: 0,
        mtime: 0,
        atime: 0,
        ctime: 0,
        flags: 0,
        reserved: 0,
    };
    read_inode(ino, &mut inode)?;
    out.ino = ino;
    out.size = inode.size;
    out.mode = inode.mode;
    out.mtime = inode.mtime;
    out.atime = inode.atime;
    out.ctime = inode.ctime;
    Ok(())
}
