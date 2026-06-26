use super::super::journal::journal_log;
use super::super::{
    inodes_per_block, read_block, write_block, G_BUF, G_SB, G_VERSION, ONYFS_V1,
};
use super::read::read_inode;
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{OnyfsInode, ONYFS_DIRECT_BLKS};

pub unsafe fn write_inode(ino: u32, inode: &OnyfsInode) -> KResult<()> {
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
