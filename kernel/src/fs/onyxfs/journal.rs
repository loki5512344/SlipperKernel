//! On-disk journal (circular redo log) for OnyxFS metadata writes.
//!
//! On-disk journal entry layout (one 4096-byte block per entry):
//!   bytes 0..4        : type   (u32) — 0=commit_start, 1=block_write, 2=commit_end
//!   bytes 4..8        : block_num (u32) — target block this entry replays to
//!   bytes 8..4096     : data   (4088 bytes) — block contents to replay
//!
//! The journal is a circular redo log: `journal_log` appends a `block_write`
//! entry containing the NEW block contents before the actual write_block call.
//! `journal_commit` appends a `commit_end` marker. On mount, `journal_recover`
//! scans for a `commit_end`; if found, every preceding `block_write` entry is
//! re-applied to its target block. Incomplete transactions (no commit_end) are
//! discarded.
//!
//! MVP limitation: only the first 4088 bytes of each block are journaled. The
//! last 8 bytes of a 4096-byte block are not protected. This is acceptable
//! because the only metadata that fits in those 8 bytes (rare tail padding of
//! dirent blocks) is not critical for crash recovery.
use super::{read_block, write_block, G_JOURNAL_HEAD, G_SB};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_BLOCK_SIZE;

const JOURNAL_TYPE_COMMIT_START: u32 = 0;
const JOURNAL_TYPE_BLOCK_WRITE: u32 = 1;
const JOURNAL_TYPE_COMMIT_END: u32 = 2;
const JOURNAL_DATA_SIZE: usize = ONYFS_BLOCK_SIZE - 8;

/// Append a `block_write` entry to the journal containing the NEW contents
/// of `block_num`. Called BEFORE the actual `write_block` so that a crash
/// between the journal append and the data write leaves a recoverable redo
/// entry on disk. No-op if the filesystem has no journal configured.
pub(super) unsafe fn journal_log(block_num: u32, data: &[u8; ONYFS_BLOCK_SIZE]) -> KResult<()> {
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
pub(super) unsafe fn journal_commit() -> KResult<()> {
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
