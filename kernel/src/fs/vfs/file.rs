//! File operations — open, close, read, write, stat, lseek.
use super::{
    alloc_fd, fd_check, fd_check_perm, fd_clear, fd_get, fd_set, fd_token, fd_update_pos, FdToken,
    Fs, G_ROOT_FS, PERM_READ, PERM_SEEK, PERM_WRITE,
};
use crate::fs::{fat32, onyxfs, procfs};
use onyx_core::errno::{Errno, KResult};

pub unsafe fn open(path: &[u8], perms: u32) -> KResult<FdToken> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &path[1..];
    let idx = alloc_fd(perms)?;

    // Check mount table first.
    let (fs, subpath) = super::resolve_mount(name);
    let (ino, size) = match fs {
        Fs::Proc => {
            let ino = procfs::lookup(subpath)?;
            let st = procfs::stat(ino)?;
            (ino, st.size)
        }
        _ => {
            let mut st = onyxfs::OnyfsStat::default();
            match *(&raw const G_ROOT_FS) {
                Fs::Onyx => {
                    onyxfs::lookup(name, &mut st)?;
                    (st.ino, st.size.min(u32::MAX as u64) as u32)
                }
                Fs::Fat32 => {
                    let mut cluster = 0u32;
                    let mut sz = 0u32;
                    fat32::lookup(name, &mut cluster, &mut sz)?;
                    (cluster, sz)
                }
                _ => return Err(Errno::Inval),
            }
        }
    };

    fd_set(idx, ino, size, fs, 0);
    let fd = fd_get(idx);
    Ok(fd_token(idx, fd.epoch))
}

pub unsafe fn close(token: FdToken) -> KResult<()> {
    let idx = fd_check(token)?;
    fd_clear(idx);
    Ok(())
}

pub unsafe fn read(token: FdToken, buf: *mut u8, len: u32) -> KResult<u32> {
    let idx = fd_check_perm(token, PERM_READ)?;
    let fd = fd_get(idx);
    let avail = fd.size.saturating_sub(fd.pos);
    let to_read = len.min(avail);
    if to_read == 0 {
        return Ok(0);
    }
    let read_n = match fd.fs {
        Fs::Onyx => onyxfs::read(fd.ino, buf, fd.pos, to_read)?,
        Fs::Fat32 => fat32::read(fd.ino, buf, fd.pos, to_read)?,
        Fs::Proc => procfs::read(fd.ino, buf, fd.pos, to_read)?,
        Fs::None => return Err(Errno::Inval),
    };
    fd_update_pos(idx, fd.pos + read_n);
    Ok(read_n)
}

pub unsafe fn write(token: FdToken, buf: *const u8, len: u32) -> KResult<u32> {
    let idx = fd_check_perm(token, PERM_WRITE)?;
    let fd = fd_get(idx);
    let written = match fd.fs {
        Fs::Onyx => onyxfs::write(fd.ino, buf, fd.pos, len)?,
        Fs::Proc => return Err(Errno::Perm),
        _ => return Err(Errno::NoSys),
    };
    let new_pos = fd.pos + written;
    fd_update_pos(idx, new_pos);
    if new_pos > fd.size {
        if super::is_kernel_boot() {
            let p = &raw mut super::G_KERNEL_FDS;
            (*p)[idx].size = new_pos;
        } else {
            let p = crate::proc::current();
            p.fds[idx].size = new_pos;
        }
    }
    Ok(written)
}

pub unsafe fn stat(token: FdToken, size_out: &mut u32) -> KResult<()> {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    *size_out = fd.size;
    Ok(())
}

pub unsafe fn lseek(token: FdToken, off: i64, whence: u32) -> KResult<u32> {
    let idx = fd_check_perm(token, PERM_SEEK)?;
    let fd = fd_get(idx);
    let new_pos: i64 = match whence {
        0 => off,
        1 => fd.pos as i64 + off,
        2 => fd.size as i64 + off,
        _ => return Err(Errno::Inval),
    };
    if new_pos < 0 || new_pos > fd.size as i64 {
        return Err(Errno::Range);
    }
    fd_update_pos(idx, new_pos as u32);
    Ok(new_pos as u32)
}
