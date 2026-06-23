//! Path resolution, dirent parsing, and readdir support.
//!
//! `lookup_in` resolves a single name within a directory; `lookup` walks a
//! slash-separated path starting from the root inode. `parse_dirent` handles
//! both v1 (36-byte) and v2 (40-byte) dirent layouts. `readdir_entry`
//! returns one directory entry per call (stateful iteration handled by the
//! VFS layer).
use super::inode::{read_inode, stat};
use super::{
    dirents_per_block, read_block, OnyfsStat, G_BUF, G_VERSION, ONYFS_V1, ONYFS_V1_DIRENT_SIZE,
};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    OnyfsDirent, OnyfsInode, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_DT_DIR, ONYFS_NAME_MAX,
    ONYFS_ROOT_INO,
};

/// Parse a dirent from the current `G_BUF` contents at the given slot index.
/// Handles both v1 (36-byte) and v2 (40-byte) layouts, returning the v2
/// `OnyfsDirent` struct in both cases.
unsafe fn parse_dirent(slot: usize) -> KResult<OnyfsDirent> {
    let buf_view: &[u8] = &(*(&raw const G_BUF));
    match *(&raw const G_VERSION) {
        ONYFS_V1 => {
            let off = slot * ONYFS_V1_DIRENT_SIZE;
            if off + ONYFS_V1_DIRENT_SIZE > ONYFS_BLOCK_SIZE {
                return Err(Errno::Inval);
            }
            let s = &buf_view[off..off + ONYFS_V1_DIRENT_SIZE];
            let mut name = [0u8; ONYFS_NAME_MAX];
            name.copy_from_slice(&s[0..ONYFS_NAME_MAX]);
            let inode = u32::from_le_bytes([s[32], s[33], s[34], s[35]]);
            // v1 has no name_len field; derive from NUL-termination.
            let name_len = name.iter().position(|&b| b == 0).unwrap_or(ONYFS_NAME_MAX) as u8;
            Ok(OnyfsDirent {
                name,
                inode,
                dtype: 0,
                name_len,
                reserved: [0, 0],
            })
        }
        _ => {
            let off = slot * OnyfsDirent::SIZE;
            if off + OnyfsDirent::SIZE > ONYFS_BLOCK_SIZE {
                return Err(Errno::Inval);
            }
            OnyfsDirent::from_bytes(&buf_view[off..off + OnyfsDirent::SIZE]).ok_or(Errno::Io)
        }
    }
}

/// Lookup name in a directory inode. Returns inode number and fills `out` stat.
/// Supports subdirectories: if name contains '/', splits and walks.
pub unsafe fn lookup_in(dir_ino: u32, name: &[u8], out: &mut OnyfsStat) -> KResult<u32> {
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
    read_inode(dir_ino, &mut inode)?;
    let dir_blk = inode.blocks[0];
    if dir_blk == 0 {
        return Err(Errno::NoEnt);
    }
    {
        let pb = &raw mut G_BUF;
        read_block(dir_blk, &mut *pb)
    }?;
    let dpb = dirents_per_block();
    for i in 0..dpb {
        let d = parse_dirent(i)?;
        if d.inode == 0 {
            continue;
        }
        // Resolve actual name length: prefer name_len field (v2), fall back to
        // NUL-termination scan (v1 / malformed v2).
        let nl = if d.name_len > 0 && (d.name_len as usize) <= ONYFS_NAME_MAX {
            d.name_len as usize
        } else {
            d.name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(ONYFS_NAME_MAX)
        };
        if nl == name.len() && d.name[..nl] == *name {
            // Capture inode number BEFORE calling stat — stat() overwrites G_BUF.
            let found_ino = d.inode;
            stat(found_ino, out)?;
            return Ok(found_ino);
        }
    }
    Err(Errno::NoEnt)
}

/// Lookup full path (supports subdirectories like "service/fs.bin").
pub unsafe fn lookup(path: &[u8], out: &mut OnyfsStat) -> KResult<u32> {
    let mut cur_ino = ONYFS_ROOT_INO;
    let mut remaining = path;
    loop {
        // Skip leading '/'.
        while !remaining.is_empty() && remaining[0] == b'/' {
            remaining = &remaining[1..];
        }
        if remaining.is_empty() {
            break;
        }
        // Find next '/'.
        let component = match remaining.iter().position(|&b| b == b'/') {
            Some(idx) => &remaining[..idx],
            None => remaining,
        };
        if component.is_empty() {
            break;
        }
        cur_ino = lookup_in(cur_ino, component, out)?;
        match remaining.iter().position(|&b| b == b'/') {
            Some(idx) => remaining = &remaining[idx + 1..],
            None => break,
        }
    }
    stat(cur_ino, out)?;
    Ok(cur_ino)
}

/// Read a directory entry by index. Returns (inode, name_len, is_dir).
/// Used by SYS_readdir.
pub unsafe fn readdir_entry(
    dir_ino: u32,
    entry_idx: u32,
    name_out: *mut u8,
    name_len: usize,
) -> KResult<Option<u32>> {
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
    read_inode(dir_ino, &mut inode)?;
    // Check it's a directory.
    if inode.mode & 0o170000 != ONYFS_DT_DIR & 0o170000 {
        return Err(Errno::NotDir);
    }
    let dir_blk = inode.blocks[0];
    if dir_blk == 0 {
        return Ok(None);
    }
    {
        let pb = &raw mut G_BUF;
        read_block(dir_blk, &mut *pb)
    }?;
    let dpb = dirents_per_block();
    if (entry_idx as usize) >= dpb {
        return Ok(None);
    }
    let d = parse_dirent(entry_idx as usize)?;
    if d.inode == 0 {
        return Ok(None);
    }
    // Copy name (NUL-terminated) to caller's buffer.
    let nl = if d.name_len > 0 && (d.name_len as usize) <= ONYFS_NAME_MAX {
        d.name_len as usize
    } else {
        d.name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(ONYFS_NAME_MAX)
    };
    let copy_n = nl.min(name_len.saturating_sub(1));
    for i in 0..copy_n {
        *name_out.add(i) = d.name[i];
    }
    if copy_n < name_len {
        *name_out.add(copy_n) = 0;
    }
    Ok(Some(d.inode))
}

/// Resolve a directory path to inode number.
pub unsafe fn resolve_dir(path: &[u8]) -> KResult<u32> {
    let mut st = OnyfsStat::default();
    let ino = lookup(path, &mut st)?;
    if st.mode & 0o170000 != ONYFS_DT_DIR & 0o170000 {
        return Err(Errno::NotDir);
    }
    Ok(ino)
}
