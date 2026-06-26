//! Block & inode allocators and the `add_dirent` helper used by `create`
//! and `mkdir`. All v2-only (v1 has no writable inode fields). Each logical
//! operation is wrapped in a journal transaction (journal_log + journal_commit).
use super::journal::journal_log;
use super::{
    dirents_per_block, read_block, write_block, G_BUF, G_SB, G_VERSION, ONYFS_V1,
    ONYFS_V1_DIRENT_SIZE,
};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    OnyfsDirent, OnyfsInode, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_NAME_MAX,
};

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
    super::inode::read_inode(dir_ino, &mut dir_inode)?;
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
        super::inode::write_inode(dir_ino, &dir_inode)?;
    }
    let dpb = dirents_per_block();
    let entry_size = match *(&raw const G_VERSION) {
        ONYFS_V1 => ONYFS_V1_DIRENT_SIZE,
        _ => OnyfsDirent::SIZE,
    };
    let pb = &raw mut G_BUF;
    read_block(dir_blk, &mut *pb)?;

    // First pass: check if an entry with the same name already exists.
    // If so, overwrite its inode number (effectively replaces the file).
    for i in 0..dpb {
        let off = i * entry_size;
        if off + entry_size > ONYFS_BLOCK_SIZE {
            break;
        }
        let inode_off = off + 32;
        let existing = u32::from_le_bytes([
            (*pb)[inode_off],
            (*pb)[inode_off + 1],
            (*pb)[inode_off + 2],
            (*pb)[inode_off + 3],
        ]);
        if existing == 0 {
            continue;
        }
        // Check if name matches.
        let existing_name = &(&*pb)[off..off + ONYFS_NAME_MAX];
        let mut match_len = 0;
        while match_len < name.len() && match_len < ONYFS_NAME_MAX {
            if existing_name[match_len] != name[match_len] {
                break;
            }
            match_len += 1;
        }
        if match_len == name.len() && (match_len >= ONYFS_NAME_MAX || existing_name[match_len] == 0) {
            // Found existing entry — overwrite inode number.
            let ino_bytes = target_ino.to_le_bytes();
            (*pb)[inode_off] = ino_bytes[0];
            (*pb)[inode_off + 1] = ino_bytes[1];
            (*pb)[inode_off + 2] = ino_bytes[2];
            (*pb)[inode_off + 3] = ino_bytes[3];
            if *(&raw const G_VERSION) != ONYFS_V1 {
                (*pb)[off + 36] = dtype;
            }
            journal_log(dir_blk, &*pb)?;
            write_block(dir_blk, &*pb)?;
            return Ok(());
        }
    }

    // Second pass: find an empty slot.
    for i in 0..dpb {
        let off = i * entry_size;
        if off + entry_size > ONYFS_BLOCK_SIZE {
            break;
        }
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
            (*pb)[off + 36] = dtype;
            (*pb)[off + 37] = n as u8;
        }
        journal_log(dir_blk, &*pb)?;
        write_block(dir_blk, &*pb)?;
        return Ok(());
    }
    Err(Errno::NoSpace)
}

pub(super) unsafe fn remove_dirent(dir_ino: u32, name: &[u8]) -> KResult<()> {
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
    super::inode::read_inode(dir_ino, &mut dir_inode)?;
    let dir_blk = dir_inode.blocks[0];
    if dir_blk == 0 {
        return Err(Errno::NoEnt);
    }
    let dpb = dirents_per_block();
    let entry_size = match *(&raw const G_VERSION) {
        ONYFS_V1 => ONYFS_V1_DIRENT_SIZE,
        _ => OnyfsDirent::SIZE,
    };
    let pb = &raw mut G_BUF;
    read_block(dir_blk, &mut *pb)?;

    for i in 0..dpb {
        let off = i * entry_size;
        if off + entry_size > ONYFS_BLOCK_SIZE {
            break;
        }
        let inode_off = off + 32;
        let existing = u32::from_le_bytes([
            (*pb)[inode_off],
            (*pb)[inode_off + 1],
            (*pb)[inode_off + 2],
            (*pb)[inode_off + 3],
        ]);
        if existing == 0 {
            continue;
        }
        let existing_name = &(&*pb)[off..off + ONYFS_NAME_MAX];
        let mut match_len = 0;
        while match_len < name.len() && match_len < ONYFS_NAME_MAX {
            if existing_name[match_len] != name[match_len] {
                break;
            }
            match_len += 1;
        }
        if match_len == name.len() && (match_len >= ONYFS_NAME_MAX || existing_name[match_len] == 0) {
            (*pb)[inode_off] = 0;
            (*pb)[inode_off + 1] = 0;
            (*pb)[inode_off + 2] = 0;
            (*pb)[inode_off + 3] = 0;
            journal_log(dir_blk, &*pb)?;
            write_block(dir_blk, &*pb)?;
            return Ok(());
        }
    }
    Err(Errno::NoEnt)
}
