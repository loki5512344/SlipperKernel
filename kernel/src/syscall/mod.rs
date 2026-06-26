//! Syscall subsystem.
//!
//! `abi`     — syscall numbers and ABI constants.
//! `handler` — the central `handle()` entry point, ACL, and `user_ptr_ok`.
//! `fs_sys`  — filesystem syscalls (`sys_write`, `sys_open`, …).
//! `fs_sys2` — filesystem syscalls part 2 (`sys_exec`, `sys_sbrk`, …).
//! `ipc_sys` — IPC channel syscalls (`sys_chan_create`, `sys_chan_send`, …).
//! `proc_sys`— process syscalls (`sys_exit`, `sys_spawn`, `sys_wait`, …).
//! `snap_sys`— snapshot syscalls (root-only).
//! `ring_sys`— ring-transition syscalls (`sys_getring`, `sys_dropring`).
pub mod abi;
pub mod fs_sys;
pub mod fs_sys2;
pub mod fs_sys3;
pub mod handler;
pub mod ipc_sys;
pub mod proc_sys;
pub mod ring_sys;
pub mod snap_sys;

pub use abi::*;
