//! Snapshot syscalls (root-only). These delegate to the OnyxFS snapshot
//! subsystem. The ACL layer in `handler::syscall_allowed` already enforces
//! that only ring ≤ PROC_RING_ROOT may invoke them.
use crate::fs::onyxfs;
use onyx_core::errno::Errno;

use super::handler::user_ptr_ok;

/// SYS_snapshot_create(name): create a filesystem snapshot.
/// `name` is a NUL-terminated user pointer to the snapshot name.
pub(super) unsafe fn sys_snapshot_create(name: u64) -> i64 {
    if !user_ptr_ok(name, 1) {
        return Errno::Inval.as_i64();
    }
    let mut len = 0usize;
    let p = name as *const u8;
    while *p.add(len) != 0 && len < 32 {
        len += 1;
    }
    let name_bytes = core::slice::from_raw_parts(p, len);
    match onyxfs::snapshot_create(name_bytes) {
        Ok(id) => id as i64,
        Err(e) => e.as_i64(),
    }
}

/// SYS_snapshot_rollback(id): restore filesystem state from snapshot `id`.
pub(super) unsafe fn sys_snapshot_rollback(id: u32) -> i64 {
    match onyxfs::snapshot_rollback(id) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

/// SYS_snapshot_list(buf, len): list snapshot names into `buf`.
/// Returns the number of snapshots listed.
pub(super) unsafe fn sys_snapshot_list(buf: u64, len: u64) -> i64 {
    if len == 0 {
        return 0;
    }
    if !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    match onyxfs::snapshot_list(buf as *mut u8, len as usize) {
        Ok(count) => count as i64,
        Err(e) => e.as_i64(),
    }
}
