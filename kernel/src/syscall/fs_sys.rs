//! Filesystem syscalls — `sys_write`, `sys_read`, `sys_open`, `sys_close`,
//! `sys_lseek`, `sys_stat`, `sys_exec`, `sys_sbrk`, `sys_write_fd`,
//! `sys_create`, `sys_mkdir`, and the related `sys_readdir`.
//!
//! All functions here are `pub(super) unsafe fn` so `handler::handle` can
//! dispatch to them. User-pointer validation goes through the shared
//! `super::handler::user_ptr_ok` helper.
use crate::arch::trap_frame::TrapFrame;
use crate::drivers::uart;
use crate::fs::vfs;
use crate::proc;
use onyx_core::errno::Errno;

use super::handler::user_ptr_ok;

pub(super) unsafe fn sys_write(tf: &mut TrapFrame, _fd: u64, buf: u64, len: u64) -> i64 {
    if !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    if _fd != 1 && _fd != 2 {
        return Errno::BadFd.as_i64();
    }
    let src = buf as *const u8;
    let mut written: i64 = 0;
    let mut i: u64 = 0;
    while i < len {
        let b = *src.add(i as usize);
        if b == b'\n' {
            uart::putc(b'\r');
        }
        uart::putc(b);
        written += 1;
        i += 1;
    }
    let _ = tf;
    written
}

pub(super) unsafe fn sys_read(tf: &mut TrapFrame, _fd: u64, buf: u64, len: u64) -> i64 {
    if !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    if _fd != 0 {
        return Errno::BadFd.as_i64();
    }
    if len == 0 {
        return 0;
    }
    let dst = buf as *mut u8;
    let mut n: usize = 0;
    let max = (len - 1) as usize;
    while n < max {
        match uart::getc() {
            None => {
                proc::sched_yield(tf);
                continue;
            }
            Some(b) => {
                if b == b'\r' || b == b'\n' {
                    *dst.add(n) = b'\n';
                    uart::putc(b'\r');
                    uart::putc(b'\n');
                    n += 1;
                    break;
                } else if b == 0x7F || b == 0x08 {
                    if n > 0 {
                        n -= 1;
                        uart::putc(0x08);
                        uart::putc(b' ');
                        uart::putc(0x08);
                    }
                } else {
                    *dst.add(n) = b;
                    uart::putc(b);
                    n += 1;
                }
            }
        }
    }
    *dst.add(n) = 0;
    n as i64
}

pub(super) unsafe fn sys_open(path: u64, _flags: u64, _mode: u64) -> i64 {
    if !user_ptr_ok(path, 1) {
        return Errno::Inval.as_i64();
    }
    let mut len = 0usize;
    let p = path as *const u8;
    while *p.add(len) != 0 && len < 256 {
        len += 1;
    }
    let path_bytes = core::slice::from_raw_parts(p, len);

    // Ring-aware path policy.
    let ring = proc::current_ring();
    if ring == proc::PROC_RING_USER {
        // User processes cannot open /service/* or /dev/uart*
        if path_bytes.starts_with(b"/service/") {
            return Errno::Perm.as_i64();
        }
    }

    match vfs::open(path_bytes, vfs::PERM_READ | vfs::PERM_SEEK) {
        Ok(token) => token as i64,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_close(token: u64) -> i64 {
    match vfs::close(token) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_lseek(token: u64, off: i64, whence: u32) -> i64 {
    match vfs::lseek(token, off, whence) {
        Ok(pos) => pos as i64,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_stat(path: u64, _st: u64) -> i64 {
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

pub(super) unsafe fn sys_exec(tf: &mut TrapFrame, path: u64) -> i64 {
    if !user_ptr_ok(path, 1) {
        return Errno::Inval.as_i64();
    }
    let mut len = 0usize;
    let p = path as *const u8;
    while *p.add(len) != 0 && len < 256 {
        len += 1;
    }
    let path_bytes = core::slice::from_raw_parts(p, len);

    // Security: user process cannot exec a root binary (SPX_FLAGS_RING1).
    // The onx::load will parse the ring. If current ring is USER and binary
    // has RING1 flag, deny.
    let cur_ring = proc::current_ring();
    let token = match vfs::open(path_bytes, vfs::PERM_READ | vfs::PERM_SEEK) {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    let mut size = 0u32;
    vfs::stat(token, &mut size).ok();
    let img = match crate::mm::heap::kmalloc(size as usize) {
        Ok(p) => p,
        Err(e) => return e.as_i64(),
    };
    vfs::read(token, img, size).ok();
    vfs::close(token).ok();
    let r = match crate::proc::onx::load(img, size as usize) {
        Ok(r) => r,
        Err(e) => {
            crate::mm::heap::kfree(img);
            return e.as_i64();
        }
    };
    crate::mm::heap::kfree(img);
    if cur_ring == proc::PROC_RING_USER && r.ring == 1 {
        return Errno::Perm.as_i64(); // privilege escalation attempt
    }
    let p = proc::current();
    if p.root_pa != 0 {
        crate::mm::vmm::destroy_root(p.root_pa);
    }
    p.root_pa = r.root_pa;
    p.entry = r.entry;
    p.ustack = r.ustack;
    p.heap_brk = r.heap_brk;
    p.ring = if r.ring == 1 {
        proc::PROC_RING_ROOT
    } else {
        proc::PROC_RING_USER
    };
    p.tf = TrapFrame::zero();
    p.tf.sepc = r.entry;
    p.tf.sp = r.ustack;
    p.tf.sstatus = crate::arch::regs::SSTATUS_SPIE;
    p.tf.satp = crate::arch::regs::SATP_MODE_SV39 | (r.root_pa >> 12);
    *tf = p.tf;
    0
}

pub(super) unsafe fn sys_sbrk(incr: i64) -> i64 {
    let pid = proc::current_pid();
    let p = proc::by_pid(pid).unwrap();
    let cur = p.heap_brk;
    let heap_end = crate::arch::regs::USER_HEAP_BASE + crate::arch::regs::USER_HEAP_SIZE;
    let new_brk = (cur as i64 + incr) as u64;
    if new_brk < crate::arch::regs::USER_HEAP_BASE || new_brk > heap_end {
        return Errno::NoMem.as_i64();
    }
    p.heap_brk = new_brk;
    cur as i64
}

/// SYS_readdir: list directory entries (stateful per process).
/// Returns 1 = entry read, 0 = EOF, negative = error.
pub(super) unsafe fn sys_readdir(dir: u64, name_out: u64, len: u64) -> i64 {
    if !user_ptr_ok(dir, 1) {
        return Errno::Inval.as_i64();
    }
    if !user_ptr_ok(name_out, len) {
        return Errno::Inval.as_i64();
    }
    let mut dlen = 0usize;
    let dp = dir as *const u8;
    while *dp.add(dlen) != 0 && dlen < 256 {
        dlen += 1;
    }
    let dir_path = core::slice::from_raw_parts(dp, dlen);
    let name_buf = name_out as *mut u8;
    let name_len = len as usize;
    match vfs::readdir(dir_path, name_buf, name_len) {
        Ok(has_entry) => {
            if has_entry {
                1
            } else {
                0
            }
        }
        Err(e) => e.as_i64(),
    }
}

// ── File write / create / mkdir syscalls ──────────────────────────────────

/// SYS_write_fd(fd, buf, len): write `len` bytes from user buffer `buf` to
/// the open file referred to by the capability fd `fd`. Returns the number
/// of bytes written (or a negative errno).
pub(super) unsafe fn sys_write_fd(token: u64, buf: u64, len: u64) -> i64 {
    if !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    match vfs::write(token, buf as *const u8, len as u32) {
        Ok(n) => n as i64,
        Err(e) => e.as_i64(),
    }
}

/// SYS_create(path, mode, _reserved): create a new regular file at `path`
/// with the given OnyxFS mode bits and return a writable fd token.
pub(super) unsafe fn sys_create(path: u64, mode: u64, _reserved: u64) -> i64 {
    if !user_ptr_ok(path, 1) {
        return Errno::Inval.as_i64();
    }
    let mut len = 0usize;
    let p = path as *const u8;
    while *p.add(len) != 0 && len < 256 {
        len += 1;
    }
    let path_bytes = core::slice::from_raw_parts(p, len);
    let mode_u32 = if mode == 0 {
        onyx_core::formats::ONYFS_DT_REG
    } else {
        mode as u32
    };
    match vfs::create(path_bytes, mode_u32) {
        Ok(token) => token as i64,
        Err(e) => e.as_i64(),
    }
}

/// SYS_mkdir(path): create a new directory at `path`.
pub(super) unsafe fn sys_mkdir(path: u64) -> i64 {
    if !user_ptr_ok(path, 1) {
        return Errno::Inval.as_i64();
    }
    let mut len = 0usize;
    let p = path as *const u8;
    while *p.add(len) != 0 && len < 256 {
        len += 1;
    }
    let path_bytes = core::slice::from_raw_parts(p, len);
    match vfs::mkdir(path_bytes) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}
