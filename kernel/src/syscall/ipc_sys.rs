//! IPC syscalls — `sys_chan_create`, `sys_chan_connect`, `sys_chan_send`,
//! `sys_chan_recv`, `sys_chan_close`.
//!
//! All functions here are `pub(super) unsafe fn` so `handler::handle` can
//! dispatch to them. User-pointer validation goes through the shared
//! `super::handler::user_ptr_ok` helper.
use crate::ipc;
use crate::proc;
use onyx_core::errno::Errno;

use super::handler::user_ptr_ok;

/// SYS_chan_create(): create a new IPC channel owned by the caller (root-only
/// via the ACL in `handler::syscall_allowed`). Returns the channel ID.
pub(super) unsafe fn sys_chan_create() -> i64 {
    let pid = proc::current_pid();
    match ipc::create(pid) {
        Ok(id) => id as i64,
        Err(e) => e.as_i64(),
    }
}

/// SYS_chan_connect(chan_id): attach the current process as the client of an
/// existing channel.
pub(super) unsafe fn sys_chan_connect(chan_id: u32) -> i64 {
    let pid = proc::current_pid();
    match ipc::connect(chan_id, pid) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

/// SYS_chan_send(chan_id, buf, len): write `len` bytes from user buffer `buf`
/// to the channel. Returns the number of bytes sent.
pub(super) unsafe fn sys_chan_send(chan_id: u32, buf: u64, len: u64) -> i64 {
    if !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    match ipc::send(chan_id, buf as *const u8, len as u32) {
        Ok(n) => n as i64,
        Err(e) => e.as_i64(),
    }
}

/// SYS_chan_recv(chan_id, buf, len): read up to `len` bytes into user buffer
/// `buf` from the channel. Returns the number of bytes read (0 = empty).
pub(super) unsafe fn sys_chan_recv(chan_id: u32, buf: u64, len: u64) -> i64 {
    if !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    match ipc::recv(chan_id, buf as *mut u8, len as u32) {
        Ok(n) => n as i64,
        Err(e) => e.as_i64(),
    }
}

/// SYS_chan_close(chan_id): close and free a channel.
pub(super) unsafe fn sys_chan_close(chan_id: u32) -> i64 {
    match ipc::close(chan_id) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}
