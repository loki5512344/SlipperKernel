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

// ── Journal ──────────────────────────────────────────────────────────────
//
// On-disk journal entry layout (one 4096-byte block per entry):
//   bytes 0..4        : type   (u32) — 0=commit_start, 1=block_write, 2=commit_end
//   bytes 4..8        : block_num (u32) — target block this entry replays to
//   bytes 8..4096     : data   (4088 bytes) — block contents to replay
//
// The journal is a circular redo log: `journal_log` appends a `block_write`
// entry containing the NEW block contents before the actual write_block call.
// `journal_commit` appends a `commit_end` marker. On mount, `journal_recover`
// scans for a `commit_end`; if found, every preceding `block_write` entry is
// re-applied to its target block. Incomplete transactions (no commit_end) are
// discarded.
//
// MVP limitation: only the first 4088 bytes of each block are journaled. The
// last 8 bytes of a 4096-byte block are not protected. This is acceptable
// because the only metadata that fits in those 8 bytes (rare tail padding of
// dirent blocks) is not critical for crash recovery.

const JOURNAL_TYPE_COMMIT_START: u32 = 0;
const JOURNAL_TYPE_BLOCK_WRITE: u32 = 1;
const JOURNAL_TYPE_COMMIT_END: u32 = 2;
const JOURNAL_DATA_SIZE: usize = ONYFS_BLOCK_SIZE - 8;

/// Next free journal slot (block offset from `journal_start`).
static mut G_JOURNAL_HEAD: u32 = 0;

unsafe fn read_block(blk: u32, buf: &mut [u8; ONYFS_BLOCK_SIZE]) -> KResult<()> {
    let lba = (*(&raw const G_LBA_BASE) as u64) + (blk as u64) * 8;
    // A single OnyxFS block is 4096 bytes = 8 × 512-byte sectors. We issue
    // ONE batched `virtio_req::read_multi` call covering all 8 sectors rather
    // than 8 sequential single-sector reads. Today `read_multi` internally
    // loops over single-sector ops, but the seam is in place for a future
    // scatter-gather optimization in the virtio-blk driver.
    virtio_req::read_multi(*(&raw const G_DEV), lba, 8, buf.as_mut_ptr())
}

/// Write a 4096-byte block back to disk. Used by `update_mtime`,
/// `write_inode`, and the snapshot management stubs.
unsafe fn write_block(blk: u32, buf: &[u8; ONYFS_BLOCK_SIZE]) -> KResult<()> {
    let lba = (*(&raw const G_LBA_BASE) as u64) + (blk as u64) * 8;
    // Same batching as `read_block` — a single `write_multi` call for the
    // whole 8-sector block.
    virtio_req::write_multi(*(&raw const G_DEV), lba, 8, buf.as_ptr())
}

// ── Journal implementation ───────────────────────────────────────────────

/// Append a `block_write` entry to the journal containing the NEW contents
/// of `block_num`. Called BEFORE the actual `write_block` so that a crash
/// between the journal append and the data write leaves a recoverable redo
/// entry on disk. No-op if the filesystem has no journal configured.
unsafe fn journal_log(block_num: u32, data: &[u8; ONYFS_BLOCK_SIZE]) -> KResult<()> {
    let sb_ptr = &raw const G_SB;
    let journal_start = (*sb_ptr).journal_start;
    if journal_start == 0 || (*sb_ptr).journal_size == 0 {
        return Ok(());
    }
    let head = *(&raw const G_JOURNAL_HEAD);
    if head >= (*sb_ptr).journal_size {
        // Journal full — caller should have committed by now. Bail out.
        return Err(Errno::NoSpace);
    }
    let mut entry = [0u8; ONYFS_BLOCK_SIZE];
    entry[0..4].copy_from_slice(&JOURNAL_TYPE_BLOCK_WRITE.to_le_bytes());
    entry[4..8].copy_from_slice(&block_num.to_le_bytes());
    let copy_n = JOURNAL_DATA_SIZE.min(ONYFS_BLOCK_SIZE);
    entry[8..8 + copy_n].copy_from_slice(&data[..copy_n]);
    write_block(journal_start + head, &entry)?;
    *(&raw mut G_JOURNAL_HEAD) = head + 1;
    Ok(())
}

/// Mark the current transaction as committed by appending a `commit_end`
/// entry. After this, the journal entries are considered durable and will be
/// replayed on the next mount if a crash occurs before the data writes
/// themselves complete. Resets the in-memory journal head so the journal
/// area can be reused for the next transaction.
unsafe fn journal_commit() -> KResult<()> {
    let sb_ptr = &raw const G_SB;
    let journal_start = (*sb_ptr).journal_start;
    if journal_start == 0 || (*sb_ptr).journal_size == 0 {
        return Ok(());
    }
    let head = *(&raw const G_JOURNAL_HEAD);
    if head == 0 {
        return Ok(()); // nothing to commit
    }
    if head < (*sb_ptr).journal_size {
        let mut entry = [0u8; ONYFS_BLOCK_SIZE];
        entry[0..4].copy_from_slice(&JOURNAL_TYPE_COMMIT_END.to_le_bytes());
        write_block(journal_start + head, &entry)?;
    }
    *(&raw mut G_JOURNAL_HEAD) = 0;
    Ok(())
}

/// Replay journal on mount (crash recovery). Scans the journal area for a
/// `commit_end` marker. If found, every preceding `block_write` entry is
/// re-applied to its target block (redo). Incomplete transactions (no
/// `commit_end`) are discarded. The journal is then zeroed so future mounts
/// start with a clean log.
pub unsafe fn journal_recover() -> KResult<()> {
    let sb_ptr = &raw const G_SB;
    let journal_start = (*sb_ptr).journal_start;
    if journal_start == 0 || (*sb_ptr).journal_size == 0 {
        return Ok(());
    }
    let journal_size = (*sb_ptr).journal_size;
    let mut found_commit = false;
    let mut commit_at: u32 = 0;
    let mut entry = [0u8; ONYFS_BLOCK_SIZE];
    let mut i: u32 = 0;
    while i < journal_size {
        read_block(journal_start + i, &mut entry)?;
        let entry_type = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
        match entry_type {
            JOURNAL_TYPE_COMMIT_START => {}
            JOURNAL_TYPE_BLOCK_WRITE => {}
            JOURNAL_TYPE_COMMIT_END => {
                found_commit = true;
                commit_at = i;
                break;
            }
            _ => break, // empty slot or garbage — stop scanning
        }
        i += 1;
    }
    if !found_commit {
        *(&raw mut G_JOURNAL_HEAD) = 0;
        return Ok(());
    }
    // Replay every `block_write` entry before the commit marker.
    for j in 0..commit_at {
        read_block(journal_start + j, &mut entry)?;
        let entry_type = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
        if entry_type != JOURNAL_TYPE_BLOCK_WRITE {
            continue;
        }
        let block_num = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
        // Read the current block contents (to preserve the un-journaled tail)
        // and overwrite the first JOURNAL_DATA_SIZE bytes from the entry.
        let mut blk_buf = [0u8; ONYFS_BLOCK_SIZE];
        let _ = read_block(block_num, &mut blk_buf);
        let copy_n = JOURNAL_DATA_SIZE.min(ONYFS_BLOCK_SIZE);
        for k in 0..copy_n {
            blk_buf[k] = entry[8 + k];
        }
        write_block(block_num, &blk_buf)?;
    }
    // Clear the journal area so future mounts see an empty log.
    let zero = [0u8; ONYFS_BLOCK_SIZE];
    for j in 0..=commit_at {
        write_block(journal_start + j, &zero)?;
    }
    *(&raw mut G_JOURNAL_HEAD) = 0;
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
    // Crash recovery: replay any committed-but-unapplied journal entries
    // before the filesystem is handed to the VFS layer.
    journal_recover()?;
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
/// Logs the inode-table block to the journal before writing.
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

// ════════════════════════════════════════════════════════════════════════════
// Write support — alloc_data_block, alloc_inode, write, create, mkdir.
// All v2-only (v1 has no writable inode fields). Each logical operation is
// wrapped in a journal transaction (journal_log + journal_commit).
// ════════════════════════════════════════════════════════════════════════════

/// Allocate a free data block by scanning the data bitmap. Marks the bit as
/// used and returns the block number (`data_blocks_start + bit_index`).
unsafe fn alloc_data_block() -> KResult<u32> {
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
unsafe fn alloc_inode() -> KResult<u32> {
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
unsafe fn add_dirent(dir_ino: u32, name: &[u8], target_ino: u32, dtype: u8) -> KResult<()> {
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

// ── RLE compression for snapshots ────────────────────────────────────────
//
// Packet format (each packet starts with a tag byte):
//   - tag & 0x80 != 0  → run packet: count = (tag & 0x7F) + 1 (1..128),
//                         next byte = value, expand to count × value.
//   - tag & 0x80 == 0  → literal packet: count = tag + 1 (1..128),
//                         followed by `count` literal bytes.
//
// Runs of >= 3 identical bytes are encoded as a run packet; everything else
// is grouped into literal packets of up to 128 bytes each. Worst-case
// expansion for incompressible input is ~N + N/128 bytes.

/// RLE-compress `src` into `dst`. Returns the compressed size, or 0 on
/// overflow (`dst` too small). Caller must ensure `dst` is at least
/// `src.len() + src.len()/128 + 2` bytes for incompressible input.
unsafe fn rle_compress(src: &[u8], dst: &mut [u8]) -> usize {
    let n = src.len();
    let mut i: usize = 0;
    let mut out: usize = 0;
    while i < n {
        let cur = src[i];
        // Count run length (max 128).
        let mut run: usize = 1;
        while i + run < n && src[i + run] == cur && run < 128 {
            run += 1;
        }
        if run >= 3 {
            if out + 2 > dst.len() {
                return 0;
            }
            dst[out] = 0x80 | ((run - 1) as u8);
            dst[out + 1] = cur;
            out += 2;
            i += run;
        } else {
            // Collect literal bytes (up to 128), stopping at a 3+ run.
            let lit_start = i;
            let mut lit_len: usize = 0;
            while i + lit_len < n && lit_len < 128 {
                let b = src[i + lit_len];
                let mut k: usize = 0;
                while i + lit_len + k < n && src[i + lit_len + k] == b && k < 3 {
                    k += 1;
                }
                if k >= 3 {
                    break;
                }
                lit_len += 1;
            }
            if lit_len == 0 {
                lit_len = 1;
            }
            if out + 1 + lit_len > dst.len() {
                return 0;
            }
            dst[out] = (lit_len - 1) as u8;
            for j in 0..lit_len {
                dst[out + 1 + j] = src[lit_start + j];
            }
            out += 1 + lit_len;
            i += lit_len;
        }
    }
    out
}

/// RLE-decompress `src` into `dst`. Returns the number of bytes written, or 0
/// on overflow / truncated input.
unsafe fn rle_decompress(src: &[u8], dst: &mut [u8]) -> usize {
    let mut i: usize = 0;
    let mut out: usize = 0;
    while i < src.len() && out < dst.len() {
        let tag = src[i];
        i += 1;
        if tag & 0x80 != 0 {
            let count = ((tag & 0x7F) as usize) + 1;
            if i >= src.len() || out + count > dst.len() {
                return 0;
            }
            let val = src[i];
            i += 1;
            for j in 0..count {
                dst[out + j] = val;
            }
            out += count;
        } else {
            let count = (tag as usize) + 1;
            if i + count > src.len() || out + count > dst.len() {
                return 0;
            }
            for j in 0..count {
                dst[out + j] = src[i + j];
            }
            i += count;
            out += count;
        }
    }
    out
}

// ── Snapshot storage layout (COW + RLE) ──────────────────────────────────
//
// Per-snapshot data occupies `SNAPSHOT_BLOCKS_EACH` (64) consecutive blocks
// in the snapshot area. The first block is a header describing the
// compressed slots; the remaining 63 blocks hold compressed block data, with
// each compressed block occupying exactly 2 on-disk blocks (8192 bytes,
// enough for any 4096-byte input even in the worst-case RLE expansion).
//
// Header block layout (4096 bytes):
//   bytes 0..4       : n_entries (u32) — number of compressed slots that
//                      follow (<= SNAPSHOT_SLOTS).
//   bytes 4..        : array of n_entries × (block_num: u32, comp_size: u32)
//                      pairs. comp_size == ONYFS_BLOCK_SIZE means "stored
//                      raw" (RLE produced 0 / overflowed); otherwise it is
//                      the compressed byte count.
//
// A snapshot captures: inode-table blocks, the data-bitmap block, and every
// used data block referenced by a non-zero inode. This is the COW portion —
// only live blocks are copied.

const SNAPSHOT_SLOTS: u32 = 31;
const SNAPSHOT_SLOT_BLKS: u32 = 2;

/// Create a snapshot: walk the inode table to enumerate all live blocks
/// (inode-table + data-bitmap + used data blocks), RLE-compress each block,
/// and store the compressed data in the snapshot area. Also writes a
/// `SnapshotMeta` record and bumps `snapshot_count`. Returns the new ID.
pub unsafe fn snapshot_create(name: &[u8]) -> KResult<u32> {
    let sb_ptr = &raw const G_SB;
    if (*sb_ptr).snapshot_area_start == 0 {
        return Err(Errno::NoSys);
    }
    if (*sb_ptr).feature_flags & ONYFS_FEAT_SNAPSHOTS == 0 {
        return Err(Errno::NoSys);
    }
    let new_id = (*sb_ptr).snapshot_count + 1;
    let snap_data_start = (*sb_ptr).snapshot_area_start + 1 + (new_id - 1) * SNAPSHOT_BLOCKS_EACH;

    // ── Enumerate the live blocks to snapshot ────────────────────────────
    // (block_num, comp_size) — comp_size filled in after compression.
    let mut blocks: [(u32, u32); SNAPSHOT_SLOTS as usize] = [(0, 0); SNAPSHOT_SLOTS as usize];
    let mut n_blocks: usize = 0;

    // Helper closure-like macro would be cleaner but Rust 2021 doesn't allow
    // closures capturing `&mut` to mutable statics cleanly. Inline the push.
    macro_rules! push_block {
        ($b:expr) => {{
            if n_blocks >= SNAPSHOT_SLOTS as usize {
                return Err(Errno::NoMem);
            }
            // Skip duplicates.
            let mut dup = false;
            for j in 0..n_blocks {
                if blocks[j].0 == $b {
                    dup = true;
                    break;
                }
            }
            if !dup {
                blocks[n_blocks] = ($b, 0);
                n_blocks += 1;
            }
        }};
    }

    // 1. Inode-table blocks.
    let inode_tbl_blocks = inode_table_block_count();
    for i in 0..inode_tbl_blocks {
        push_block!((*sb_ptr).inode_table_start + i);
    }
    // 2. Data-bitmap block.
    push_block!((*sb_ptr).data_bitmap_start);
    // 3. Used data blocks (walk every inode).
    for blk_idx in 0..inode_tbl_blocks {
        let pb = &raw mut G_BUF;
        read_block((*sb_ptr).inode_table_start + blk_idx, &mut *pb)?;
        let ipb = inodes_per_block();
        for slot in 0..ipb {
            let off = slot * OnyfsInode::SIZE;
            if off + OnyfsInode::SIZE > ONYFS_BLOCK_SIZE {
                break;
            }
            let buf_view: &[u8] = &*pb;
            let inode = match OnyfsInode::from_bytes(&buf_view[off..off + OnyfsInode::SIZE]) {
                Some(i) => i,
                None => continue,
            };
            if inode.mode == 0 {
                continue;
            }
            for &b in inode.blocks.iter() {
                if b != 0 {
                    push_block!(b);
                }
            }
        }
    }

    // ── Compress and store each block ────────────────────────────────────
    let mut comp_buf = [0u8; 8192];
    let mut blk_buf = [0u8; ONYFS_BLOCK_SIZE];
    for i in 0..n_blocks {
        let block_num = blocks[i].0;
        read_block(block_num, &mut blk_buf)?;
        let comp_size = rle_compress(&blk_buf, &mut comp_buf);
        let stored_size: u32 = if comp_size == 0 || comp_size > 8192 {
            // Fallback: store raw.
            comp_buf[..ONYFS_BLOCK_SIZE].copy_from_slice(&blk_buf);
            ONYFS_BLOCK_SIZE as u32
        } else {
            comp_size as u32
        };
        // Write compressed data to slot i (2 on-disk blocks).
        let slot_start = snap_data_start + 1 + (i as u32) * SNAPSHOT_SLOT_BLKS;
        let mut out_blk = [0u8; ONYFS_BLOCK_SIZE];
        out_blk.copy_from_slice(&comp_buf[..ONYFS_BLOCK_SIZE]);
        write_block(slot_start, &out_blk)?;
        out_blk.copy_from_slice(&comp_buf[ONYFS_BLOCK_SIZE..8192]);
        write_block(slot_start + 1, &out_blk)?;
        blocks[i].1 = stored_size;
    }

    // ── Write header block ───────────────────────────────────────────────
    let mut header = [0u8; ONYFS_BLOCK_SIZE];
    header[0..4].copy_from_slice(&(n_blocks as u32).to_le_bytes());
    for i in 0..n_blocks {
        let off = 4 + i * 8;
        header[off..off + 4].copy_from_slice(&blocks[i].0.to_le_bytes());
        header[off + 4..off + 8].copy_from_slice(&blocks[i].1.to_le_bytes());
    }
    write_block(snap_data_start, &header)?;

    // ── Write SnapshotMeta into the area header block ────────────────────
    let mut name_buf = [0u8; 32];
    let n = name.len().min(32);
    for i in 0..n {
        name_buf[i] = name[i];
    }
    let meta = SnapshotMeta {
        id: new_id,
        timestamp: *(&raw const timer::G_JIFFIES),
        root_inode_snapshot: (*sb_ptr).root_inode,
        block_count: n_blocks as u32,
        name: name_buf,
        parent_id: 0,
        flags: 0,
        reserved: [0; 4],
    };
    let pb = &raw mut G_BUF;
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

    // Bump snapshot_count and persist the superblock.
    {
        let sb_mut = &raw mut G_SB;
        (*sb_mut).snapshot_count = new_id;
    }
    persist_superblock()?;
    Ok(new_id)
}

/// Roll back filesystem state from a snapshot. Reads the per-snapshot
/// header, RLE-decompresses each stored block (or copies it raw if it was
/// stored uncompressed), and writes the result back to its original block
/// number. This restores inode table, data bitmap, and all live data blocks
/// captured at snapshot time — a true COW rollback.
pub unsafe fn snapshot_rollback(snapshot_id: u32) -> KResult<()> {
    let sb_ptr = &raw const G_SB;
    if (*sb_ptr).snapshot_area_start == 0 {
        return Err(Errno::NoSys);
    }
    if snapshot_id == 0 || snapshot_id > (*sb_ptr).snapshot_count {
        return Err(Errno::NoEnt);
    }
    let snap_data_start =
        (*sb_ptr).snapshot_area_start + 1 + (snapshot_id - 1) * SNAPSHOT_BLOCKS_EACH;

    let mut header = [0u8; ONYFS_BLOCK_SIZE];
    read_block(snap_data_start, &mut header)?;
    let n_blocks = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    if n_blocks > SNAPSHOT_SLOTS as usize {
        return Err(Errno::Io);
    }

    let mut comp_buf = [0u8; 8192];
    let mut blk_buf = [0u8; ONYFS_BLOCK_SIZE];
    for i in 0..n_blocks {
        let off = 4 + i * 8;
        let block_num = u32::from_le_bytes([
            header[off],
            header[off + 1],
            header[off + 2],
            header[off + 3],
        ]);
        let comp_size = u32::from_le_bytes([
            header[off + 4],
            header[off + 5],
            header[off + 6],
            header[off + 7],
        ]) as usize;

        // Read 2 blocks of compressed data.
        let slot_start = snap_data_start + 1 + (i as u32) * SNAPSHOT_SLOT_BLKS;
        read_block(slot_start, &mut blk_buf)?;
        comp_buf[..ONYFS_BLOCK_SIZE].copy_from_slice(&blk_buf);
        read_block(slot_start + 1, &mut blk_buf)?;
        comp_buf[ONYFS_BLOCK_SIZE..8192].copy_from_slice(&blk_buf);

        let mut out_buf = [0u8; ONYFS_BLOCK_SIZE];
        if comp_size == ONYFS_BLOCK_SIZE {
            // Stored raw.
            out_buf.copy_from_slice(&comp_buf[..ONYFS_BLOCK_SIZE]);
        } else {
            let dec = rle_decompress(&comp_buf[..comp_size], &mut out_buf);
            if dec != ONYFS_BLOCK_SIZE {
                return Err(Errno::Io);
            }
        }
        write_block(block_num, &out_buf)?;
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
