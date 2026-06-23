//! OnyxFS — kernel-side driver supporting both v1 (ONY1) and v2 (ONY2) formats.
//!
//! v1 (legacy): 64-byte super, 64-byte inode (u32 size), 36-byte dirent.
//! v2 (current): 128-byte super, 128-byte inode (u64 size + timestamps),
//!               40-byte dirent (with dtype/name_len), snapshot area.
//!
//! All on-disk structures come from `onyx_core::formats`; this module only
//! owns the kernel-side I/O glue, the VFS-facing `OnyfsStat`, and snapshot
//! management stubs. Backward compatibility with v1 images produced by the
//! legacy `mkimage` tool is preserved: the detected version is stored in
//! `G_VERSION` and the per-version inode/dirent sizes are used throughout.
use crate::drivers::virtio_req;
use crate::kernel::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    OnyfsDirent, OnyfsInode, OnyfsSuper, SnapshotMeta, ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS,
    ONYFS_DT_DIR, ONYFS_FEAT_SNAPSHOTS, ONYFS_MAGIC, ONYFS_MAGIC_V1, ONYFS_NAME_MAX,
    ONYFS_ROOT_INO,
};

/// VFS-facing stat structure. Kept local (kernel-internal) because the VFS
/// layer expects a fixed ABI independent of the on-disk inode format.
/// Carries the new v2 timestamp fields (`mtime`, `atime`, `ctime`) so future
/// write support can surface them to user space.
#[derive(Debug, Clone, Copy, Default)]
pub struct OnyfsStat {
    pub ino: u32,
    pub size: u64,
    pub mode: u32,
    pub mtime: u64,
    pub atime: u64,
    pub ctime: u64,
}

/// Filesystem format version detected at mount time.
const ONYFS_V1: u32 = 1;
const ONYFS_V2: u32 = 2;

/// v1 on-disk inode size (legacy 64-byte format).
const ONYFS_V1_INODE_SIZE: usize = 64;
/// v1 on-disk dirent size (legacy 36-byte format).
const ONYFS_V1_DIRENT_SIZE: usize = 36;

/// Number of 4096-byte blocks reserved per snapshot in the snapshot area.
/// Must be large enough to hold the inode table copy + data bitmap copy.
const SNAPSHOT_BLOCKS_EACH: u32 = 64;

static mut G_DEV: usize = 0;
static mut G_LBA_BASE: u32 = 0;
/// Detected filesystem version (0 = unmounted, 1 = v1, 2 = v2).
static mut G_VERSION: u32 = 0;
static mut G_SB: OnyfsSuper = OnyfsSuper {
    magic: 0,
    version: 0,
    block_size: 0,
    total_blocks: 0,
    inode_count: 0,
    inode_table_start: 0,
    data_bitmap_start: 0,
    data_blocks_start: 0,
    root_inode: 0,
    snapshot_area_start: 0,
    snapshot_count: 0,
    journal_start: 0,
    journal_size: 0,
    feature_flags: 0,
    creation_time: 0,
    last_mount_time: 0,
    reserved: [0; 10],
};
static mut G_BUF: [u8; ONYFS_BLOCK_SIZE] = [0; ONYFS_BLOCK_SIZE];

unsafe fn read_block(blk: u32, buf: &mut [u8; ONYFS_BLOCK_SIZE]) -> KResult<()> {
    let lba = *(&raw const G_LBA_BASE) + blk * 8;
    // ── I/O batching opportunity ───────────────────────────────────────────
    // A single OnyxFS block is 4096 bytes = 8 × 512-byte sectors. Ideally we
    // would issue ONE virtio-blk request covering all 8 sectors in a single
    // descriptor chain (scatter-gather). The current virtio-blk driver only
    // supports single-sector reads (`virtio_req::read` issues one 512-byte
    // IN op per call), so we fall back to 8 sequential requests here. Once
    // the driver gains multi-sector / scatter-gather support, replace this
    // loop with a single batched `virtio_req::read_multi(dev, lba, 8, buf)`.
    // ──────────────────────────────────────────────────────────────────────
    for i in 0u32..8 {
        virtio_req::read(
            *(&raw const G_DEV),
            (lba + i) as u64,
            buf.as_mut_ptr().add((i * 512) as usize),
        )?;
    }
    Ok(())
}

/// Write a 4096-byte block back to disk. Used by `update_mtime`,
/// `write_inode`, and the snapshot management stubs.
unsafe fn write_block(blk: u32, buf: &[u8; ONYFS_BLOCK_SIZE]) -> KResult<()> {
    let lba = *(&raw const G_LBA_BASE) + blk * 8;
    // Same batching caveat as `read_block` — 8 sequential single-sector
    // writes until the virtio-blk driver supports multi-sector ops.
    for i in 0u32..8 {
        virtio_req::write(
            *(&raw const G_DEV),
            (lba + i) as u64,
            buf.as_ptr().add((i * 512) as usize),
        )?;
    }
    Ok(())
}

#[inline]
unsafe fn inodes_per_block() -> usize {
    match *(&raw const G_VERSION) {
        ONYFS_V1 => ONYFS_BLOCK_SIZE / ONYFS_V1_INODE_SIZE, // 64
        _ => ONYFS_BLOCK_SIZE / OnyfsInode::SIZE,           // 32 (v2)
    }
}

#[inline]
unsafe fn dirents_per_block() -> usize {
    match *(&raw const G_VERSION) {
        ONYFS_V1 => ONYFS_BLOCK_SIZE / ONYFS_V1_DIRENT_SIZE, // 113
        _ => ONYFS_BLOCK_SIZE / OnyfsDirent::SIZE,           // 102 (v2)
    }
}

pub unsafe fn mount(dev: usize, lba_offset: u32) -> KResult<()> {
    *(&raw mut G_DEV) = dev;
    *(&raw mut G_LBA_BASE) = lba_offset;
    {
        let pb = &raw mut G_BUF;
        read_block(0, &mut *pb)
    }?;
    let buf_view: &[u8] = &(*(&raw const G_BUF));
    let sb_val = OnyfsSuper::from_bytes(buf_view).ok_or(Errno::Inval)?;
    if sb_val.block_size != ONYFS_BLOCK_SIZE as u32 {
        return Err(Errno::Inval);
    }
    // Detect version from magic. v2 = ONY2, v1 = ONY1 (legacy).
    let ver = if sb_val.magic == ONYFS_MAGIC {
        ONYFS_V2
    } else if sb_val.magic == ONYFS_MAGIC_V1 {
        ONYFS_V1
    } else {
        return Err(Errno::Inval);
    };
    *(&raw mut G_VERSION) = ver;
    *(&raw mut G_SB) = sb_val;
    Ok(())
}

/// Read an inode by number into the v2 `OnyfsInode` struct.
/// Works for both v1 (64-byte) and v2 (128-byte) on-disk layouts;
/// v1 fields are upcast (size u32 → u64, timestamps zeroed).
unsafe fn read_inode(ino: u32, out: &mut OnyfsInode) -> KResult<()> {
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
unsafe fn write_inode(ino: u32, inode: &OnyfsInode) -> KResult<()> {
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

// ════════════════════════════════════════════════════════════════════════════
// Snapshot management (stubs)
// ════════════════════════════════════════════════════════════════════════════
//
// Layout in the snapshot area (starting at `super.snapshot_area_start`):
//   block 0: array of `SnapshotMeta` records (64 bytes each)
//   block 1 + (id-1)*SNAPSHOT_BLOCKS_EACH .. : per-snapshot data
//     = inode-table copy + data-bitmap copy
//
// These are MVP stubs: they copy the inode table and the first data-bitmap
// block, write a SnapshotMeta record, and bump `snapshot_count`. Rollback
// restores those copies in place. Data blocks themselves are NOT copied —
// a full COW implementation would be needed for production use.

/// Persist the in-memory superblock back to disk block 0.
unsafe fn persist_superblock() -> KResult<()> {
    let bytes = (*(&raw const G_SB)).to_bytes();
    let pb = &raw mut G_BUF;
    // Zero the block so stale data beyond the superblock doesn't leak.
    for b in (*pb).iter_mut() {
        *b = 0;
    }
    for i in 0..bytes.len() {
        (*pb)[i] = bytes[i];
    }
    write_block(0, &*pb)
}

/// Number of inode-table blocks occupied by the current filesystem.
#[inline]
unsafe fn inode_table_block_count() -> u32 {
    let ipb = inodes_per_block() as u32;
    let cnt = (*(&raw const G_SB)).inode_count;
    if cnt == 0 {
        1
    } else {
        (cnt + ipb - 1) / ipb
    }
}

/// Create a snapshot: copy the inode table and data bitmap into the snapshot
/// area, write a `SnapshotMeta` record, and bump `snapshot_count`.
/// Returns the new snapshot ID.
pub unsafe fn snapshot_create(name: &[u8]) -> KResult<u32> {
    let sb_ptr = &raw const G_SB;
    if (*sb_ptr).snapshot_area_start == 0 {
        return Err(Errno::NoSys);
    }
    if (*sb_ptr).feature_flags & ONYFS_FEAT_SNAPSHOTS == 0 {
        return Err(Errno::NoSys);
    }
    let new_id = (*sb_ptr).snapshot_count + 1;

    let inode_tbl_blocks = inode_table_block_count();
    let bitmap_blocks: u32 = 1; // simplified: copy just the first bitmap block
    let total_copy = inode_tbl_blocks + bitmap_blocks;
    if total_copy > SNAPSHOT_BLOCKS_EACH {
        return Err(Errno::NoMem);
    }

    let snap_data_start = (*sb_ptr).snapshot_area_start + 1 + (new_id - 1) * SNAPSHOT_BLOCKS_EACH;

    let pb = &raw mut G_BUF;
    // Copy inode-table blocks into the snapshot area.
    for i in 0..inode_tbl_blocks {
        read_block((*sb_ptr).inode_table_start + i, &mut *pb)?;
        write_block(snap_data_start + i, &*pb)?;
    }
    // Copy data-bitmap block(s).
    for i in 0..bitmap_blocks {
        read_block((*sb_ptr).data_bitmap_start + i, &mut *pb)?;
        write_block(snap_data_start + inode_tbl_blocks + i, &*pb)?;
    }

    // Build SnapshotMeta and write it into the snapshot-area header block.
    let mut name_buf = [0u8; 32];
    let n = name.len().min(32);
    for i in 0..n {
        name_buf[i] = name[i];
    }
    let meta = SnapshotMeta {
        id: new_id,
        timestamp: *(&raw const timer::G_JIFFIES),
        root_inode_snapshot: (*sb_ptr).root_inode,
        block_count: total_copy,
        name: name_buf,
        parent_id: 0,
        flags: 0,
        reserved: [0; 4],
    };
    read_block((*sb_ptr).snapshot_area_start, &mut *pb)?;
    let meta_off = ((new_id - 1) as usize) * SnapshotMeta::SIZE;
    if meta_off + SnapshotMeta::SIZE > ONYFS_BLOCK_SIZE {
        return Err(Errno::NoMem);
    }
    let meta_bytes = meta.to_bytes();
    for i in 0..SnapshotMeta::SIZE {
        (*pb)[meta_off + i] = meta_bytes[i];
    }
    write_block((*sb_ptr).snapshot_area_start, &*pb)?;

    // Bump snapshot_count in the in-memory superblock and persist it.
    {
        let sb_mut = &raw mut G_SB;
        (*sb_mut).snapshot_count = new_id;
    }
    persist_superblock()?;
    Ok(new_id)
}

/// Roll back filesystem state from a snapshot.
/// Restores the inode table and data bitmap from the snapshot area.
/// Data blocks are NOT restored in this stub implementation.
pub unsafe fn snapshot_rollback(snapshot_id: u32) -> KResult<()> {
    let sb_ptr = &raw const G_SB;
    if (*sb_ptr).snapshot_area_start == 0 {
        return Err(Errno::NoSys);
    }
    if snapshot_id == 0 || snapshot_id > (*sb_ptr).snapshot_count {
        return Err(Errno::NoEnt);
    }
    let inode_tbl_blocks = inode_table_block_count();
    let bitmap_blocks: u32 = 1;
    let snap_data_start =
        (*sb_ptr).snapshot_area_start + 1 + (snapshot_id - 1) * SNAPSHOT_BLOCKS_EACH;

    let pb = &raw mut G_BUF;
    // Restore inode table.
    for i in 0..inode_tbl_blocks {
        read_block(snap_data_start + i, &mut *pb)?;
        write_block((*sb_ptr).inode_table_start + i, &*pb)?;
    }
    // Restore data bitmap.
    for i in 0..bitmap_blocks {
        read_block(snap_data_start + inode_tbl_blocks + i, &mut *pb)?;
        write_block((*sb_ptr).data_bitmap_start + i, &*pb)?;
    }
    Ok(())
}

/// List all snapshots: write each snapshot name (NUL-terminated, newline-
/// separated) into `names_out`. Returns the number of snapshots listed.
pub unsafe fn snapshot_list(names_out: *mut u8, max_len: usize) -> KResult<u32> {
    let sb_ptr = &raw const G_SB;
    if (*sb_ptr).snapshot_area_start == 0 {
        return Ok(0);
    }
    let count = (*sb_ptr).snapshot_count;
    if count == 0 || max_len == 0 {
        return Ok(0);
    }
    let pb = &raw mut G_BUF;
    read_block((*sb_ptr).snapshot_area_start, &mut *pb)?;
    let mut written: usize = 0;
    let mut listed: u32 = 0;
    for i in 0..count {
        let off = (i as usize) * SnapshotMeta::SIZE;
        if off + SnapshotMeta::SIZE > ONYFS_BLOCK_SIZE {
            break;
        }
        let buf_view: &[u8] = &*pb;
        let slice = &buf_view[off..off + SnapshotMeta::SIZE];
        let meta = match SnapshotMeta::from_bytes(slice) {
            Some(m) => m,
            None => continue,
        };
        // Copy name (up to 32 bytes, stopping at NUL) + trailing newline.
        let mut name_len = 0;
        for j in 0..32 {
            if meta.name[j] == 0 {
                break;
            }
            name_len += 1;
        }
        for j in 0..name_len {
            if written + 1 >= max_len {
                return Ok(listed); // out of space
            }
            *names_out.add(written) = meta.name[j];
            written += 1;
        }
        if written + 1 < max_len {
            *names_out.add(written) = b'\n';
            written += 1;
        }
        listed += 1;
    }
    if written < max_len {
        *names_out.add(written) = 0;
    }
    Ok(listed)
}
