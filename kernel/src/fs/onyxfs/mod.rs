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
pub mod alloc;
pub mod compress;
pub mod inode;
pub mod journal;
pub mod lookup;
pub mod mkdir;
pub mod mount;
pub mod read;
pub mod readdir;
pub mod rename;
pub mod snapshot;
pub mod snapshot_io;
pub mod unlink;
pub mod write;

pub use compress::*;
pub use inode::*;
pub use journal::*;
pub use lookup::*;
pub use mkdir::*;
pub use mount::*;
pub use read::*;
pub use readdir::*;
pub use rename::*;
pub use snapshot::*;
pub use snapshot_io::*;
pub use unlink::*;
pub use write::*;

use crate::drivers::virtio_req;
use onyx_core::errno::KResult;
use onyx_core::formats::{OnyfsSuper, ONYFS_BLOCK_SIZE};

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
pub(super) const ONYFS_V1: u32 = 1;
pub(super) const ONYFS_V2: u32 = 2;

/// v1 on-disk inode size (legacy 64-byte format).
pub(super) const ONYFS_V1_INODE_SIZE: usize = 64;
/// v1 on-disk dirent size (legacy 36-byte format).
pub(super) const ONYFS_V1_DIRENT_SIZE: usize = 36;

/// Number of 4096-byte blocks reserved per snapshot in the snapshot area.
/// Must be large enough to hold the inode table copy + data bitmap copy.
pub(super) const SNAPSHOT_BLOCKS_EACH: u32 = 64;

pub(super) static mut G_DEV: usize = 0;
pub(super) static mut G_LBA_BASE: u32 = 0;
/// Detected filesystem version (0 = unmounted, 1 = v1, 2 = v2).
pub(super) static mut G_VERSION: u32 = 0;
pub(super) static mut G_SB: OnyfsSuper = OnyfsSuper {
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
pub(super) static mut G_BUF: [u8; ONYFS_BLOCK_SIZE] = [0; ONYFS_BLOCK_SIZE];

/// Next free journal slot (block offset from `journal_start`).
pub(super) static mut G_JOURNAL_HEAD: u32 = 0;

pub(super) unsafe fn read_block(blk: u32, buf: &mut [u8; ONYFS_BLOCK_SIZE]) -> KResult<()> {
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
pub(super) unsafe fn write_block(blk: u32, buf: &[u8; ONYFS_BLOCK_SIZE]) -> KResult<()> {
    let lba = (*(&raw const G_LBA_BASE) as u64) + (blk as u64) * 8;
    // Same batching as `read_block` — a single `write_multi` call for the
    // whole 8-sector block.
    virtio_req::write_multi(*(&raw const G_DEV), lba, 8, buf.as_ptr())
}

#[inline]
pub(super) unsafe fn inodes_per_block() -> usize {
    match *(&raw const G_VERSION) {
        ONYFS_V1 => ONYFS_BLOCK_SIZE / ONYFS_V1_INODE_SIZE, // 64
        _ => ONYFS_BLOCK_SIZE / onyx_core::formats::OnyfsInode::SIZE, // 32 (v2)
    }
}

#[inline]
pub(super) unsafe fn dirents_per_block() -> usize {
    match *(&raw const G_VERSION) {
        ONYFS_V1 => ONYFS_BLOCK_SIZE / ONYFS_V1_DIRENT_SIZE, // 113
        _ => ONYFS_BLOCK_SIZE / onyx_core::formats::OnyfsDirent::SIZE, // 102 (v2)
    }
}
