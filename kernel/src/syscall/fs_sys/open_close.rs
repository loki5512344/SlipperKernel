use crate::fs::vfs;
use crate::proc;
use onyx_core::errno::Errno;

use super::super::handler::user_ptr_ok;

pub(in super::super) unsafe fn sys_open(path: u64, _flags: u64, _mode: u64) -> i64 {
    if !user_ptr_ok(path, 1) {
        return Errno::Inval.as_i64();
    }
    let mut len = 0usize;
    let p = path as *const u8;
    while *p.add(len) != 0 && len < 256 {
        len += 1;
    }
    let path_bytes = core::slice::from_raw_parts(p, len);

    let ring = proc::current_ring();
    if ring == proc::PROC_RING_USER {
        if path_bytes.starts_with(b"/service/") {
            return Errno::Perm.as_i64();
        }
    }

    match vfs::open(path_bytes, vfs::PERM_READ | vfs::PERM_SEEK) {
        Ok(token) => token as i64,
        Err(e) => e.as_i64(),
    }
}

pub(in super::super) unsafe fn sys_close(token: u64) -> i64 {
    match vfs::close(token) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub(in super::super) unsafe fn sys_lseek(token: u64, off: i64, whence: u32) -> i64 {
    match vfs::lseek(token, off, whence) {
        Ok(pos) => pos as i64,
        Err(e) => e.as_i64(),
    }
}

pub(in super::super) unsafe fn sys_stat(path: u64, _st: u64) -> i64 {
    if !user_ptr_ok(path, 1) {
        return Errno::Inval.as_i64();
    }
    let mut len = 0usize;
    let p = path as *const u8;
    while *p.add(len) != 0 && len < 256 {
        len += 1;
    }
    let path_bytes = core::slice::from_raw_parts(p, len);
    let token = match vfs::open(path_bytes, vfs::PERM_READ) {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    let mut size = 0u32;
    let res = vfs::stat(token, &mut size);
    let _ = vfs::close(token);
    match res {
        Ok(()) => size as i64,
        Err(e) => e.as_i64(),
    }
}
