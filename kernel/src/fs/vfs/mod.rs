//! VFS — Virtual File System with Capability FDs + opendir/readdir.
//!
//! This is the directory module root. It owns the global FD table and the
//! `Fs` enum, plus the constants and the `mount_root`/`init` entry points.
//! File operations (open/close/read/write/stat/lseek/create/mkdir) live in
//! `file.rs`; `readdir` lives in `dir.rs`.
use crate::fs::{fat32, ipcfs, onyxfs, procfs};
use onyx_core::errno::{Errno, KResult};

pub const VFS_MAX_FDS: usize = 16;
pub const MAX_MOUNTS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fs {
    None = 0,
    Onyx = 1,
    Fat32 = 2,
    Proc = 3,
    Ipc = 4,
}

#[derive(Clone, Copy)]
pub struct MountEntry {
    pub path: &'static [u8],
    pub fs: Fs,
}

/// Global mount table. procfs is mounted at "proc" during init.
pub(super) static mut G_MOUNTS: [MountEntry; MAX_MOUNTS] = [
    MountEntry { path: b"", fs: Fs::None },
    MountEntry { path: b"", fs: Fs::None },
    MountEntry { path: b"", fs: Fs::None },
    MountEntry { path: b"", fs: Fs::None },
    MountEntry { path: b"", fs: Fs::None },
];

pub unsafe fn mount_procfs() {
    G_MOUNTS[0] = MountEntry {
        path: b"proc",
        fs: Fs::Proc,
    };
}

pub unsafe fn mount_ipcfs() {
    G_MOUNTS[1] = MountEntry {
        path: b"ipc",
        fs: Fs::Ipc,
    };
}

/// Resolve a path to the target filesystem and sub-path.
/// The input `path` has no leading '/'.
pub(super) unsafe fn resolve_mount(path: &[u8]) -> (Fs, &[u8]) {
    for m in G_MOUNTS.iter() {
        if m.fs == Fs::None {
            continue;
        }
        if path == m.path {
            return (m.fs, b"");
        }
        if path.starts_with(m.path) && path.len() > m.path.len() && path[m.path.len()] == b'/' {
            let sub = &path[m.path.len() + 1..];
            return (m.fs, sub);
        }
    }
    (root_fs(), path)
}
pub const PERM_READ: u32 = 1;
pub const PERM_WRITE: u32 = 2;
pub const PERM_SEEK: u32 = 4;
pub const PERM_EXEC: u32 = 8;
pub const PERM_ALL: u32 = PERM_READ | PERM_WRITE | PERM_SEEK | PERM_EXEC;

#[derive(Clone, Copy)]
pub struct VfsFd {
    pub ino: u32,
    pub size: u32,
    pub pos: u32,
    pub fs: Fs,
    pub used: bool,
    pub perms: u32,
    pub epoch: u32,
}
impl Default for VfsFd {
    fn default() -> Self {
        Self {
            ino: 0,
            size: 0,
            pos: 0,
            fs: Fs::None,
            used: false,
            perms: 0,
            epoch: 0,
        }
    }
}

pub type FdToken = u64;
pub const FD_TOKEN_NONE: FdToken = 0xFFFF_FFFF_FFFF_FFFF;
#[inline]
pub const fn fd_token(idx: usize, epoch: u32) -> FdToken {
    ((idx as u64) << 32) | (epoch as u64)
}
#[inline]
pub const fn fd_token_idx(token: FdToken) -> usize {
    (token >> 32) as usize
}
#[inline]
pub const fn fd_token_epoch(token: FdToken) -> u32 {
    token as u32
}

pub(super) static mut G_ROOT_FS: Fs = Fs::None;

/// Kernel-global FD table for early boot (before any process exists).
/// Used when `proc::current()` returns null (kmain loading /bin/init).
static mut G_KERNEL_FDS: [VfsFd; VFS_MAX_FDS] = [VfsFd {
    ino: 0,
    size: 0,
    pos: 0,
    fs: Fs::None,
    used: false,
    perms: 0,
    epoch: 0,
}; VFS_MAX_FDS];

/// VFS init — no global FD table anymore (per-process tables live in `Proc`).
/// Kept as a no-op so `kmain` can still call it without a code change.
pub unsafe fn init() {}

/// Returns true if we're in early boot (no current process).
unsafe fn is_kernel_boot() -> bool {
    crate::proc::current_pid() == 0
}

pub unsafe fn mount_root(dev: usize, onyxfs_lba: u32) -> KResult<()> {
    if onyxfs::mount(dev, onyxfs_lba).is_ok() {
        *(&raw mut G_ROOT_FS) = Fs::Onyx;
        return Ok(());
    }
    if fat32::mount(dev).is_ok() {
        *(&raw mut G_ROOT_FS) = Fs::Fat32;
        return Ok(());
    }
    Err(Errno::Io)
}

pub fn root_fs() -> Fs {
    unsafe { *(&raw const G_ROOT_FS) }
}

/// Allocate a free FD slot in the *current process's* FD table.
/// During early boot (no process), uses the kernel-global FD table.
pub(super) unsafe fn alloc_fd(perms: u32) -> KResult<usize> {
    if is_kernel_boot() {
        let p = &raw mut G_KERNEL_FDS;
        for i in 0..VFS_MAX_FDS {
            if !(*p)[i].used {
                (*p)[i].used = true;
                (*p)[i].perms = perms;
                (*p)[i].epoch = (*p)[i].epoch.wrapping_add(1);
                if (*p)[i].epoch == 0 {
                    (*p)[i].epoch = 1;
                }
                return Ok(i);
            }
        }
        return Err(Errno::NoMem);
    }
    let p = crate::proc::current();
    for i in 0..VFS_MAX_FDS {
        if !p.fds[i].used {
            p.fds[i].used = true;
            p.fds[i].perms = perms;
            p.fds[i].epoch = p.fds[i].epoch.wrapping_add(1);
            if p.fds[i].epoch == 0 {
                p.fds[i].epoch = 1;
            }
            return Ok(i);
        }
    }
    Err(Errno::NoMem)
}

/// Validate a capability FD token. Returns the slot index on success.
pub(super) unsafe fn fd_check(token: FdToken) -> KResult<usize> {
    let idx = fd_token_idx(token);
    if idx >= VFS_MAX_FDS {
        return Err(Errno::BadFd);
    }
    let fd = fd_get(idx);
    if !fd.used || fd.epoch != fd_token_epoch(token) {
        return Err(Errno::BadFd);
    }
    Ok(idx)
}

/// Like `fd_check`, but also requires the given permission bits.
pub(super) unsafe fn fd_check_perm(token: FdToken, perm: u32) -> KResult<usize> {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    if fd.perms & perm == 0 {
        return Err(Errno::Perm);
    }
    Ok(idx)
}

/// Get a copy of an FD by index (read-only).
pub(super) unsafe fn fd_get(idx: usize) -> VfsFd {
    if is_kernel_boot() {
        let p = &raw const G_KERNEL_FDS;
        (*p)[idx]
    } else {
        let p = crate::proc::current();
        p.fds[idx]
    }
}

/// Set FD fields by index.
pub(super) unsafe fn fd_set(idx: usize, ino: u32, size: u32, fs: Fs, pos: u32) {
    if is_kernel_boot() {
        let p = &raw mut G_KERNEL_FDS;
        (*p)[idx].ino = ino;
        (*p)[idx].size = size;
        (*p)[idx].fs = fs;
        (*p)[idx].pos = pos;
    } else {
        let p = crate::proc::current();
        p.fds[idx].ino = ino;
        p.fds[idx].size = size;
        p.fds[idx].fs = fs;
        p.fds[idx].pos = pos;
    }
}

/// Update FD position by index.
pub(super) unsafe fn fd_update_pos(idx: usize, pos: u32) {
    if is_kernel_boot() {
        let p = &raw mut G_KERNEL_FDS;
        (*p)[idx].pos = pos;
    } else {
        let p = crate::proc::current();
        p.fds[idx].pos = pos;
    }
}

/// Mark FD as unused.
pub(super) unsafe fn fd_clear(idx: usize) {
    if is_kernel_boot() {
        let p = &raw mut G_KERNEL_FDS;
        (*p)[idx].used = false;
    } else {
        let p = crate::proc::current();
        p.fds[idx].used = false;
    }
}

pub unsafe fn rename(old_path: &[u8], new_path: &[u8]) -> KResult<()> {
    crate::fs::onyxfs::rename(old_path, new_path)
}

pub mod create;
pub mod dir;
pub mod file;
pub mod truncate;
pub mod unlink;
pub mod utimens;

pub use create::*;
pub use dir::*;
pub use file::*;
pub use truncate::*;
pub use unlink::*;
pub use utimens::*;
