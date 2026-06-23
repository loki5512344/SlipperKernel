//! Snapshot management — create / rollback / list, with RLE-compressed COW
//! storage.
//!
//! Layout in the snapshot area (starting at `super.snapshot_area_start`):
//!   block 0: array of `SnapshotMeta` records (64 bytes each)
//!   block 1 + (id-1)*SNAPSHOT_BLOCKS_EACH .. : per-snapshot data
//!     = inode-table copy + data-bitmap copy
//!
//! These are MVP stubs: they copy the inode table and the first data-bitmap
//! block, write a SnapshotMeta record, and bump `snapshot_count`. Rollback
//! restores those copies in place. Data blocks themselves are NOT copied —
//! a full COW implementation would be needed for production use.
//!
//! Per-snapshot data occupies `SNAPSHOT_BLOCKS_EACH` (64) consecutive blocks
//! in the snapshot area. The first block is a header describing the
//! compressed slots; the remaining 63 blocks hold compressed block data, with
//! each compressed block occupying exactly 2 on-disk blocks (8192 bytes,
//! enough for any 4096-byte input even in the worst-case RLE expansion).
//!
//! Header block layout (4096 bytes):
//!   bytes 0..4       : n_entries (u32) — number of compressed slots that
//!                      follow (<= SNAPSHOT_SLOTS).
//!   bytes 4..        : array of n_entries × (block_num: u32, comp_size: u32)
//!                      pairs. comp_size == ONYFS_BLOCK_SIZE means "stored
//!                      raw" (RLE produced 0 / overflowed); otherwise it is
//!                      the compressed byte count.
//!
//! A snapshot captures: inode-table blocks, the data-bitmap block, and every
//! used data block referenced by a non-zero inode. This is the COW portion —
//! only live blocks are copied.
use super::compress::{rle_compress, rle_decompress};
use super::{
    inode_table_block_count, inodes_per_block, persist_superblock, read_block, write_block, G_BUF,
    G_SB, SNAPSHOT_BLOCKS_EACH,
};
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{OnyfsInode, SnapshotMeta, ONYFS_BLOCK_SIZE, ONYFS_FEAT_SNAPSHOTS};

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
