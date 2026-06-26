//! File write path — `write` (grow a file with new data) and `create` (new
//! regular file). v2-only. Each operation is wrapped in a journal transaction.
use super::alloc::{add_dirent, alloc_data_block, alloc_inode};
use super::inode::{read_inode, write_inode};
use super::journal::{journal_commit, journal_log};
use super::{read_block, write_block, G_BUF, G_VERSION, ONYFS_V1};
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{OnyfsInode, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_NAME_MAX};

/// Write data to a file at a given offset. Grows the file if needed.
/// Allocates new data blocks lazily for any block touched by the write that
/// is not yet mapped. Indirect blocks are not supported (MVP). The inode's
/// mtime and size are bumped as needed. The whole operation is wrapped in a
/// single journal transaction.
pub unsafe fn write(ino: u32, buf: *const u8, off: u32, len: u32) -> KResult<u32> {
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
    let mut written: u32 = 0;
    let mut cur_off = off;
    let mut remaining = len;
    while remaining > 0 {
        let blk_idx = (cur_off / ONYFS_BLOCK_SIZE as u32) as usize;
        if blk_idx >= ONYFS_DIRECT_BLKS {
            break; // MVP: no indirect-block support.
        }
        let mut blk = inode.blocks[blk_idx];
        if blk == 0 {
            // Newly touched block — allocate, zero, journal, write.
            blk = alloc_data_block()?;
            inode.blocks[blk_idx] = blk;
            let pb = &raw mut G_BUF;
            for b in (*pb).iter_mut() {
                *b = 0;
            }
            journal_log(blk, &*pb)?;
            write_block(blk, &*pb)?;
        }
        let chunk_off = (cur_off % ONYFS_BLOCK_SIZE as u32) as usize;
        let chunk =
            (ONYFS_BLOCK_SIZE as u32 - cur_off % ONYFS_BLOCK_SIZE as u32).min(remaining) as usize;
        {
            let pb = &raw mut G_BUF;
            read_block(blk, &mut *pb)?;
            core::ptr::copy_nonoverlapping(
                buf.add(written as usize),
                (*pb).as_mut_ptr().add(chunk_off),
                chunk,
            );
            journal_log(blk, &*pb)?;
            write_block(blk, &*pb)?;
        }
        written += chunk as u32;
        cur_off += chunk as u32;
        remaining -= chunk as u32;
    }
    let end = off.wrapping_add(written);
    if (end as u64) > inode.size {
        inode.size = end as u64;
    }
    inode.mtime = *(&raw const timer::G_JIFFIES);
    write_inode(ino, &inode)?;
    journal_commit()?;
    Ok(written)
}

/// Create a new regular file in a directory. Returns the new inode number.
/// The new inode is initialized with `mode`, size 0, no blocks, and current
/// timestamps. A dirent pointing to it is added to the parent directory's
/// first data block.
pub unsafe fn create(dir_ino: u32, name: &[u8], mode: u32) -> KResult<u32> {
    if *(&raw const G_VERSION) == ONYFS_V1 {
        return Err(Errno::NoSys);
    }
    if name.is_empty() || name.len() > ONYFS_NAME_MAX {
        return Err(Errno::Inval);
    }
    let new_ino = alloc_inode()?;
    let now = *(&raw const timer::G_JIFFIES);
    let inode = OnyfsInode {
        mode,
        size: 0,
        uid: 0,
        gid: 0,
        nlink: 1,
        blocks: [0; ONYFS_DIRECT_BLKS],
        indirect: 0,
        double_indirect: 0,
        crtime: now,
        mtime: now,
        atime: now,
        ctime: now,
        flags: 0,
        reserved: 0,
    };
    write_inode(new_ino, &inode)?;
    add_dirent(dir_ino, name, new_ino, /*dtype=*/ 8)?;
    journal_commit()?;
    Ok(new_ino)
}

pub unsafe fn truncate(ino: u32) -> KResult<()> {
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
    inode.size = 0;
    inode.blocks = [0; ONYFS_DIRECT_BLKS];
    inode.indirect = 0;
    inode.double_indirect = 0;
    write_inode(ino, &inode)?;
    journal_commit()?;
    Ok(())
}
