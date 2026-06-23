//! Inode read/write, block & inode allocators, and the file/directory write
//! path (`write`, `create`, `mkdir`, `add_dirent`). All v2-only — v1 images
//! are treated as read-only because their 64-byte inode has no writable
//! timestamp fields.
use super::journal::{journal_commit, journal_log};
use super::{
    dirents_per_block, inodes_per_block, read_block, write_block, OnyfsStat, G_BUF, G_SB,
    G_VERSION, ONYFS_V1, ONYFS_V1_INODE_SIZE,
};
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    OnyfsDirent, OnyfsInode, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_DT_DIR, ONYFS_NAME_MAX,
};

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

pub unsafe fn read(ino: u32, buf: *mut u8, off: u32, len: u32) -> KResult<u32> {
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
    // inode.size is u64 in v2; cap to u32 for the VFS-facing API.
    let file_size = inode.size.min(u32::MAX as u64) as u32;
    let mut read_total: u32 = 0;
    let mut off = off;
    let mut remaining = len.min(file_size.saturating_sub(off));
    for &blk in inode.blocks.iter() {
        if remaining == 0 || blk == 0 {
            break;
        }
        {
            let pb = &raw mut G_BUF;
            read_block(blk, &mut *pb)
        }?;
        let chunk_off = (off % ONYFS_BLOCK_SIZE as u32) as usize;
        let chunk =
            (ONYFS_BLOCK_SIZE as u32 - off % ONYFS_BLOCK_SIZE as u32).min(remaining) as usize;
        core::ptr::copy_nonoverlapping(
            (*(&raw const G_BUF)).as_ptr().add(chunk_off),
            buf.add(read_total as usize),
            chunk,
        );
        read_total += chunk as u32;
        off += chunk as u32;
        remaining -= chunk as u32;
    }
    Ok(read_total)
}

// ════════════════════════════════════════════════════════════════════════════
// Write support — alloc_data_block, alloc_inode, write, create, mkdir.
// All v2-only (v1 has no writable inode fields). Each logical operation is
// wrapped in a journal transaction (journal_log + journal_commit).
// ════════════════════════════════════════════════════════════════════════════

/// Allocate a free data block by scanning the data bitmap. Marks the bit as
/// used and returns the block number (`data_blocks_start + bit_index`).
pub(super) unsafe fn alloc_data_block() -> KResult<u32> {
    let bm_blk = (*(&raw const G_SB)).data_bitmap_start;
    let pb = &raw mut G_BUF;
    read_block(bm_blk, &mut *pb)?;
    // Each byte has 8 bits. Scan for a 0 bit.
    for byte_idx in 0..ONYFS_BLOCK_SIZE {
        if (*pb)[byte_idx] == 0xFF {
            continue;
        }
        for bit in 0..8u32 {
            if (*pb)[byte_idx] & (1 << bit) == 0 {
                (*pb)[byte_idx] |= 1 << bit;
                let bit_index = (byte_idx as u32) * 8 + bit;
                journal_log(bm_blk, &*pb)?;
                write_block(bm_blk, &*pb)?;
                return Ok((*(&raw const G_SB)).data_blocks_start + bit_index);
            }
        }
    }
    Err(Errno::NoSpace)
}

/// Allocate a free inode by scanning the inode bitmap (block 1). Marks the
/// bit as used and returns the 1-based inode number (`bit_index + 1`).
pub(super) unsafe fn alloc_inode() -> KResult<u32> {
    // The inode bitmap lives at block 1 in both v1 and v2 layouts.
    const INODE_BITMAP_BLK: u32 = 1;
    let pb = &raw mut G_BUF;
    read_block(INODE_BITMAP_BLK, &mut *pb)?;
    for byte_idx in 0..ONYFS_BLOCK_SIZE {
        if (*pb)[byte_idx] == 0xFF {
            continue;
        }
        for bit in 0..8u32 {
            if (*pb)[byte_idx] & (1 << bit) == 0 {
                (*pb)[byte_idx] |= 1 << bit;
                let bit_index = (byte_idx as u32) * 8 + bit;
                journal_log(INODE_BITMAP_BLK, &*pb)?;
                write_block(INODE_BITMAP_BLK, &*pb)?;
                return Ok(bit_index + 1); // 1-based
            }
        }
    }
    Err(Errno::NoSpace)
}

/// Helper: add a dirent to a directory's first data block. Allocates the
/// directory data block if it does not yet exist. Logs the block to the
/// journal before writing. Used by `create` and `mkdir`.
pub(super) unsafe fn add_dirent(
    dir_ino: u32,
    name: &[u8],
    target_ino: u32,
    dtype: u8,
) -> KResult<()> {
    let mut dir_inode = OnyfsInode {
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
    read_inode(dir_ino, &mut dir_inode)?;
    let mut dir_blk = dir_inode.blocks[0];
    if dir_blk == 0 {
        // Directory has no data block yet — allocate one and zero it.
        dir_blk = alloc_data_block()?;
        dir_inode.blocks[0] = dir_blk;
        let pb = &raw mut G_BUF;
        for b in (*pb).iter_mut() {
            *b = 0;
        }
        journal_log(dir_blk, &*pb)?;
        write_block(dir_blk, &*pb)?;
        write_inode(dir_ino, &dir_inode)?;
    }
    let dpb = dirents_per_block();
    let entry_size = match *(&raw const G_VERSION) {
        ONYFS_V1 => super::ONYFS_V1_DIRENT_SIZE,
        _ => OnyfsDirent::SIZE,
    };
    let pb = &raw mut G_BUF;
    read_block(dir_blk, &mut *pb)?;
    for i in 0..dpb {
        let off = i * entry_size;
        if off + entry_size > ONYFS_BLOCK_SIZE {
            break;
        }
        // The inode field is at offset `entry_size - 8` (last 4 bytes of name
        // area + 4-byte inode). For v2 40-byte dirent, inode is at off+32.
        // For v1 36-byte dirent, inode is at off+32 as well. Both layouts
        // store the inode number at offset 32 within the dirent.
        let inode_off = off + 32;
        let existing = u32::from_le_bytes([
            (*pb)[inode_off],
            (*pb)[inode_off + 1],
            (*pb)[inode_off + 2],
            (*pb)[inode_off + 3],
        ]);
        if existing != 0 {
            continue;
        }
        // Empty slot — fill it in.
        let mut name_buf = [0u8; ONYFS_NAME_MAX];
        let n = name.len().min(ONYFS_NAME_MAX);
        for j in 0..n {
            name_buf[j] = name[j];
        }
        for j in 0..ONYFS_NAME_MAX {
            (*pb)[off + j] = name_buf[j];
        }
        let ino_bytes = target_ino.to_le_bytes();
        (*pb)[inode_off] = ino_bytes[0];
        (*pb)[inode_off + 1] = ino_bytes[1];
        (*pb)[inode_off + 2] = ino_bytes[2];
        (*pb)[inode_off + 3] = ino_bytes[3];
        if *(&raw const G_VERSION) != ONYFS_V1 {
            // v2 dirent: dtype at off+36, name_len at off+37.
            (*pb)[off + 36] = dtype;
            (*pb)[off + 37] = n as u8;
        }
        journal_log(dir_blk, &*pb)?;
        write_block(dir_blk, &*pb)?;
        return Ok(());
    }
    Err(Errno::NoSpace)
}

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

/// Create a new directory. Returns the new inode number. Like `create()` but
/// with `mode = ONYFS_DT_DIR`, and the new directory is given its own data
/// block pre-populated with the conventional "." and ".." entries.
pub unsafe fn mkdir(dir_ino: u32, name: &[u8]) -> KResult<u32> {
    if *(&raw const G_VERSION) == ONYFS_V1 {
        return Err(Errno::NoSys);
    }
    if name.is_empty() || name.len() > ONYFS_NAME_MAX {
        return Err(Errno::Inval);
    }
    let new_ino = alloc_inode()?;
    let now = *(&raw const timer::G_JIFFIES);
    let mut inode = OnyfsInode {
        mode: ONYFS_DT_DIR,
        size: 0,
        uid: 0,
        gid: 0,
        nlink: 2,
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
    // Allocate the new directory's own data block and seed it with "."/"..".
    let dir_blk = alloc_data_block()?;
    inode.blocks[0] = dir_blk;
    {
        let pb = &raw mut G_BUF;
        for b in (*pb).iter_mut() {
            *b = 0;
        }
        let mut dot_name = [0u8; ONYFS_NAME_MAX];
        dot_name[0] = b'.';
        let mut dotdot_name = [0u8; ONYFS_NAME_MAX];
        dotdot_name[0] = b'.';
        dotdot_name[1] = b'.';
        let dot = OnyfsDirent {
            name: dot_name,
            inode: new_ino,
            dtype: 4,
            name_len: 1,
            reserved: [0, 0],
        };
        let dotdot = OnyfsDirent {
            name: dotdot_name,
            inode: dir_ino,
            dtype: 4,
            name_len: 2,
            reserved: [0, 0],
        };
        let db1 = dot.to_bytes();
        let db2 = dotdot.to_bytes();
        for j in 0..OnyfsDirent::SIZE {
            (*pb)[j] = db1[j];
            (*pb)[OnyfsDirent::SIZE + j] = db2[j];
        }
        journal_log(dir_blk, &*pb)?;
        write_block(dir_blk, &*pb)?;
    }
    write_inode(new_ino, &inode)?;
    add_dirent(dir_ino, name, new_ino, /*dtype=*/ 4)?;
    journal_commit()?;
    Ok(new_ino)
}
