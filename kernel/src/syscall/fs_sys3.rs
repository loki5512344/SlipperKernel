use crate::fs::onyxfs;
use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use onyx_core::errno::Errno;

use super::handler::{parse_user_path, user_ptr_ok};

pub(super) unsafe fn sys_brk(addr: u64) -> i64 {
    let p = proc::current();
    let cur = p.heap_brk;
    let heap_base = crate::arch::regs::USER_HEAP_BASE;
    let heap_end = heap_base + crate::arch::regs::USER_HEAP_SIZE;
    if addr == 0 {
        return cur as i64;
    }
    if addr < heap_base || addr > heap_end {
        return Errno::NoMem.as_i64();
    }
    p.heap_brk = addr;
    addr as i64
}

pub(super) unsafe fn sys_mmap(addr: u64, length: u64, prot: u64, _flags: u64, _fd: u64, _offset: u64) -> i64 {
    let prot_r = prot & 1;
    let prot_w = (prot >> 1) & 1;
    let prot_x = (prot >> 2) & 1;
    let mut flags = crate::arch::regs::PTE_U | crate::arch::regs::PTE_A | crate::arch::regs::PTE_D;
    if prot_r != 0 { flags |= crate::arch::regs::PTE_R; }
    if prot_w != 0 { flags |= crate::arch::regs::PTE_W; }
    if prot_x != 0 { flags |= crate::arch::regs::PTE_X; }
    if flags & crate::arch::regs::PTE_R == 0 && flags & crate::arch::regs::PTE_X == 0 {
        flags |= crate::arch::regs::PTE_R;
    }
    let size = length.max(4096);
    let p = proc::current();
    let vaddr = if addr == 0 {
        let va = p.mmap_brk;
        p.mmap_brk = va.wrapping_add(size);
        va
    } else {
        if addr & 0xFFF != 0 {
            return Errno::Inval.as_i64();
        }
        addr
    };
    if vaddr < crate::arch::regs::USER_BASE || vaddr.wrapping_add(size) > crate::arch::regs::USER_HEAP_BASE {
        return Errno::NoMem.as_i64();
    }
    match vmm::map_anon(p.root_pa, vaddr, size as usize, flags) {
        Ok(()) => vaddr as i64,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_munmap(addr: u64, length: u64) -> i64 {
    if addr & 0xFFF != 0 {
        return Errno::Inval.as_i64();
    }
    let size = length.max(4096) as usize;
    let p = proc::current();
    match vmm::unmap(p.root_pa, addr, size) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_dup(old_token: u64) -> i64 {
    match vfs::dup(old_token) {
        Ok(new_token) => new_token as i64,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_pipe(pipefd: u64) -> i64 {
    if !user_ptr_ok(pipefd, 8) {
        return Errno::Inval.as_i64();
    }
    let (r_token, w_token) = match vfs::create_pipe() {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    let out = pipefd as *mut u64;
    *out = r_token;
    *out.add(1) = w_token;
    0
}

pub(super) unsafe fn sys_unlink(path: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    match vfs::unlink(path_bytes) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_rename(old_path: u64, new_path: u64) -> i64 {
    let old = match parse_user_path(old_path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    let new = match parse_user_path(new_path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    match vfs::rename(old, new) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_chdir(path: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    match onyxfs::resolve_dir(path_bytes) {
        Ok(_ino) => {
            proc::set_cwd(path_bytes);
            0
        }
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_getcwd(buf: u64, len: u64) -> i64 {
    if !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    let cwd = proc::cwd();
    let n = cwd.len().min(len as usize - 1);
    core::ptr::copy_nonoverlapping(cwd.as_ptr(), buf as *mut u8, n);
    *(buf as *mut u8).add(n) = 0;
    n as i64
}

pub(super) unsafe fn sys_truncate(path: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    let token = match vfs::open(path_bytes, vfs::PERM_WRITE) {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    let r = match vfs::truncate(token) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    };
    vfs::close(token).ok();
    r
}

pub(super) unsafe fn sys_access(path: u64, _mode: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    let token = match vfs::open(path_bytes, vfs::PERM_READ) {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    vfs::close(token).ok();
    0
}

pub(super) unsafe fn sys_gettimeofday(tv: u64) -> i64 {
    if !user_ptr_ok(tv, 16) {
        return Errno::Inval.as_i64();
    }
    let us = crate::srv::timer::uptime_us();
    let secs = us / 1_000_000;
    let usecs = us % 1_000_000;
    let out = tv as *mut u64;
    *out = secs;
    *out.add(1) = usecs;
    0
}

pub(super) unsafe fn sys_fcntl(fd: u64, cmd: u64, arg: u64) -> i64 {
    match cmd {
        0 => {
            match vfs::dup2(fd, arg) {
                Ok(new_fd) => new_fd as i64,
                Err(e) => e.as_i64(),
            }
        }
        _ => Errno::NoSys.as_i64(),
    }
}

pub(super) unsafe fn sys_getuid() -> i64 {
    let p = proc::current();
    p.uid as i64
}

pub(super) unsafe fn sys_getgid() -> i64 {
    let p = proc::current();
    p.gid as i64
}

pub(super) unsafe fn sys_utimens(path: u64, times: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    if times == 0 {
        let now = *(&raw const crate::srv::timer::G_JIFFIES);
        return match vfs::utimens(path_bytes, now, now) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        };
    }
    if !user_ptr_ok(times, 16) {
        return Errno::Inval.as_i64();
    }
    let t = times as *const u64;
    let atime = *t;
    let mtime = *t.add(1);
    match vfs::utimens(path_bytes, mtime, atime) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_uname(buf: u64) -> i64 {
    if !user_ptr_ok(buf, 390) {
        return Errno::Inval.as_i64();
    }
    let out = buf as *mut u8;
    let sysname = b"Onyx\0";
    let nodename = b"onyx\0";
    let release = b"0.3.0\0";
    let version = b"#1 Onyx Kernel 0.3.0\0";
    let machine = b"riscv64\0";
    let mut off = 0;
    for &b in sysname { *out.add(off) = b; off += 1; } let sz = 65usize;
    off = sz;
    for &b in nodename { *out.add(off) = b; off += 1; } off = sz * 2;
    for &b in release { *out.add(off) = b; off += 1; } off = sz * 3;
    for &b in version { *out.add(off) = b; off += 1; } off = sz * 4;
    for &b in machine { *out.add(off) = b; off += 1; }
    0
}

pub(super) unsafe fn sys_nanosleep(req: u64, _rem: u64) -> i64 {
    if !user_ptr_ok(req, 8) {
        return Errno::Inval.as_i64();
    }
    let ns = *(req as *const u64);
    let ticks = ns / 10_000_000;
    let target = (*(&raw const crate::srv::timer::G_JIFFIES)).wrapping_add(ticks);
    loop {
        let now = *(&raw const crate::srv::timer::G_JIFFIES);
        if now >= target {
            break;
        }
        crate::proc::set_need_resched(true);
    }
    0
}
