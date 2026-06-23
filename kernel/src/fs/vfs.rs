//! VFS — Virtual File System with Capability FDs + opendir/readdir.
use crate::fs::{fat32, onyxfs};
use onyx_core::errno::{Errno, KResult};

pub const VFS_MAX_FDS: usize = 16;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fs {
    None = 0,
    Onyx = 1,
    Fat32 = 2,
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

static mut G_ROOT_FS: Fs = Fs::None;
static mut G_FDS: [VfsFd; VFS_MAX_FDS] = [VfsFd {
    ino: 0,
    size: 0,
    pos: 0,
    fs: Fs::None,
    used: false,
    perms: 0,
    epoch: 0,
}; VFS_MAX_FDS];

pub unsafe fn init() {
    let pf = &raw mut G_FDS;
    for fd in (*pf).iter_mut() {
        *fd = VfsFd::default();
    }
}

pub unsafe fn mount_root(dev: usize, onyxfs_lba: u32) -> KResult<()> {
    if onyxfs::mount(dev, onyxfs_lba).is_ok() {
        (*&raw mut G_ROOT_FS) = Fs::Onyx;
        return Ok(());
    }
    if fat32::mount(dev).is_ok() {
        (*&raw mut G_ROOT_FS) = Fs::Fat32;
        return Ok(());
    }
    Err(Errno::Io)
}

unsafe fn alloc_fd(perms: u32) -> KResult<usize> {
    let pf = &raw mut G_FDS;
    for i in 0..VFS_MAX_FDS {
        if !(*pf)[i].used {
            (*pf)[i].used = true;
            (*pf)[i].perms = perms;
            (*pf)[i].epoch = (*pf)[i].epoch.wrapping_add(1);
            if (*pf)[i].epoch == 0 {
                (*pf)[i].epoch = 1;
            }
            return Ok(i);
        }
    }
    Err(Errno::NoMem)
}

pub unsafe fn open(path: &[u8], perms: u32) -> KResult<FdToken> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &path[1..];
    let idx = alloc_fd(perms)?;
    let pf = &raw mut G_FDS;
    let fd = &mut (*pf)[idx];
    let mut st = onyxfs::OnyfsStat::default();
    match (*&raw const G_ROOT_FS) {
        Fs::Onyx => {
            onyxfs::lookup(name, &mut st)?;
            fd.ino = st.ino;
            // OnyfsStat.size is u64 (v2); VfsFd.size is u32 — truncate.
            // The kernel does not yet support >4 GiB files via VFS.
            fd.size = st.size.min(u32::MAX as u64) as u32;
            fd.fs = Fs::Onyx;
            fd.pos = 0;
        }
        Fs::Fat32 => {
            let mut cluster = 0u32;
            let mut size = 0u32;
            fat32::lookup(name, &mut cluster, &mut size)?;
            fd.ino = cluster;
            fd.size = size;
            fd.fs = Fs::Fat32;
            fd.pos = 0;
        }
        Fs::None => return Err(Errno::Inval),
    }
    Ok(fd_token(idx, fd.epoch))
}

unsafe fn fd_check(token: FdToken) -> KResult<&'static mut VfsFd> {
    let idx = fd_token_idx(token);
    if idx >= VFS_MAX_FDS {
        return Err(Errno::BadFd);
    }
    let pf = &raw mut G_FDS;
    let fd = &mut (*pf)[idx];
    if !fd.used || fd.epoch != fd_token_epoch(token) {
        return Err(Errno::BadFd);
    }
    Ok(fd)
}

unsafe fn fd_check_perm(token: FdToken, perm: u32) -> KResult<&'static mut VfsFd> {
    let fd = fd_check(token)?;
    if fd.perms & perm == 0 {
        return Err(Errno::Perm);
    }
    Ok(fd)
}

pub unsafe fn close(token: FdToken) -> KResult<()> {
    let fd = fd_check(token)?;
    fd.used = false;
    Ok(())
}
pub unsafe fn read(token: FdToken, buf: *mut u8, len: u32) -> KResult<u32> {
    let fd = fd_check_perm(token, PERM_READ)?;
    let avail = fd.size.saturating_sub(fd.pos);
    let to_read = len.min(avail);
    if to_read == 0 {
        return Ok(0);
    }
    let read_n = match fd.fs {
        Fs::Onyx => onyxfs::read(fd.ino, buf, fd.pos, to_read)?,
        Fs::Fat32 => fat32::read(fd.ino, buf, fd.pos, to_read)?,
        Fs::None => return Err(Errno::Inval),
    };
    fd.pos += read_n;
    Ok(read_n)
}

/// Write `len` bytes from `buf` to an open file at its current position.
/// Grows the file as needed. The fd must have been opened with PERM_WRITE.
/// Only OnyxFS is supported (FAT32 is read-only in this kernel).
pub unsafe fn write(token: FdToken, buf: *const u8, len: u32) -> KResult<u32> {
    let fd = fd_check_perm(token, PERM_WRITE)?;
    let written = match fd.fs {
        Fs::Onyx => onyxfs::write(fd.ino, buf, fd.pos, len)?,
        _ => return Err(Errno::NoSys),
    };
    fd.pos += written;
    if fd.pos > fd.size {
        fd.size = fd.pos;
    }
    Ok(written)
}

/// Split a NUL-free path like "/foo/bar/baz" into ("foo/bar", "baz").
/// The leading '/' is stripped. If the path has no '/', returns ("", "foo").
/// Used by `create` and `mkdir` to find the parent directory.
unsafe fn split_parent(path: &[u8]) -> (&[u8], &[u8]) {
    // Strip leading '/'.
    let p = if !path.is_empty() && path[0] == b'/' {
        &path[1..]
    } else {
        path
    };
    match p.iter().rposition(|&b| b == b'/') {
        Some(idx) => (&p[..idx], &p[idx + 1..]),
        None => (&[], p),
    }
}

/// Create a new regular file at `path` and open it with read+write+seek
/// permissions. Returns the new fd token. `mode` is the OnyxFS mode bits
/// (e.g. `ONYFS_DT_REG`).
pub unsafe fn create(path: &[u8], mode: u32) -> KResult<FdToken> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let (parent_path, filename) = split_parent(path);
    if filename.is_empty() {
        return Err(Errno::Inval);
    }
    let mut st = onyxfs::OnyfsStat::default();
    let parent_ino = if parent_path.is_empty() {
        onyx_core::formats::ONYFS_ROOT_INO
    } else {
        onyxfs::lookup(parent_path, &mut st)?
    };
    let new_ino = onyxfs::create(parent_ino, filename, mode)?;
    // Open the new file with read+write+seek perms.
    let idx = alloc_fd(PERM_READ | PERM_WRITE | PERM_SEEK)?;
    let pf = &raw mut G_FDS;
    let fd = &mut (*pf)[idx];
    fd.ino = new_ino;
    fd.size = 0;
    fd.fs = Fs::Onyx;
    fd.pos = 0;
    Ok(fd_token(idx, fd.epoch))
}

/// Create a new directory at `path`. Returns Ok(()) on success.
pub unsafe fn mkdir(path: &[u8]) -> KResult<()> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let (parent_path, dirname) = split_parent(path);
    if dirname.is_empty() {
        return Err(Errno::Inval);
    }
    let mut st = onyxfs::OnyfsStat::default();
    let parent_ino = if parent_path.is_empty() {
        onyx_core::formats::ONYFS_ROOT_INO
    } else {
        onyxfs::lookup(parent_path, &mut st)?
    };
    onyxfs::mkdir(parent_ino, dirname)?;
    Ok(())
}

pub unsafe fn stat(token: FdToken, size_out: &mut u32) -> KResult<()> {
    let fd = fd_check(token)?;
    *size_out = fd.size;
    Ok(())
}
pub unsafe fn lseek(token: FdToken, off: i64, whence: u32) -> KResult<u32> {
    let fd = fd_check_perm(token, PERM_SEEK)?;
    let new_pos: i64 = match whence {
        0 => off,
        1 => fd.pos as i64 + off,
        2 => fd.size as i64 + off,
        _ => return Err(Errno::Inval),
    };
    if new_pos < 0 || new_pos > fd.size as i64 {
        return Err(Errno::Range);
    }
    fd.pos = new_pos as u32;
    Ok(fd.pos)
}

/// readdir: stateful per-process directory listing.
/// Uses a static cursor (MVP: single active readdir at a time).
static mut G_DIR_CURSOR_INO: u32 = 0;
static mut G_DIR_CURSOR_IDX: u32 = 0;
static mut G_DIR_ACTIVE: bool = false;

pub unsafe fn readdir(dir_path: &[u8], name_out: *mut u8, name_len: usize) -> KResult<bool> {
    // Check if same directory as last call.
    let ino = onyxfs::resolve_dir(dir_path)?;
    if !G_DIR_ACTIVE || G_DIR_CURSOR_INO != ino {
        G_DIR_CURSOR_INO = ino;
        G_DIR_CURSOR_IDX = 0;
        G_DIR_ACTIVE = true;
    }
    // Read next entry.
    match onyxfs::readdir_entry(G_DIR_CURSOR_INO, G_DIR_CURSOR_IDX, name_out, name_len)? {
        Some(_ino) => {
            G_DIR_CURSOR_IDX += 1;
            Ok(true)
        }
        None => {
            G_DIR_ACTIVE = false;
            Ok(false)
        }
    }
}

pub fn root_fs() -> Fs {
    unsafe { *(&raw const G_ROOT_FS) }
}
