//! Syscall wrappers for /bin/init.
#![allow(dead_code)]
use core::arch::asm;

pub const SYS_WRITE: u64 = 1;
pub const SYS_READ: u64 = 2;
pub const SYS_EXIT: u64 = 3;
pub const SYS_YIELD: u64 = 4;
pub const SYS_GETPID: u64 = 5;
pub const SYS_OPEN: u64 = 8;
pub const SYS_CLOSE: u64 = 9;
pub const SYS_LSEEK: u64 = 10;
pub const SYS_STAT: u64 = 11;
pub const SYS_EXEC: u64 = 12;
pub const SYS_SBRK: u64 = 13;
pub const SYS_SPAWN: u64 = 14;
pub const SYS_WAIT: u64 = 15;
pub const SYS_READDIR: u64 = 16;
pub const SYS_GETRING: u64 = 17;
pub const SYS_DROPRING: u64 = 18;

#[inline]
pub unsafe fn write(fd: u64, buf: *const u8, len: usize) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_WRITE, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn read(fd: u64, buf: *mut u8, len: u64) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_READ, in("a0") fd, in("a1") buf as usize, in("a2") len as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn exit(code: u64) -> ! {
    asm!("ecall", in("a7") SYS_EXIT, in("a0") code);
    loop {
        asm!("wfi");
    }
}

#[inline]
pub fn yield_cpu() {
    unsafe {
        let _ret: i64;
        asm!("ecall", in("a7") SYS_YIELD, lateout("a0") _ret);
    }
}

#[inline]
pub unsafe fn getpid() -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_GETPID, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn spawn(path: *const u8, argv: *const u64, ring_hint: u8) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_SPAWN, in("a0") path as usize, in("a1") argv as usize, in("a2") ring_hint as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn wait(status_out: *mut i32) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_WAIT, in("a0") status_out as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn readdir(dir: *const u8, name_out: *mut u8, len: u64) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_READDIR, in("a0") dir as usize, in("a1") name_out as usize, in("a2") len as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn getring() -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_GETRING, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn dropping(target: u8) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_DROPRING, in("a0") target as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn open(path: *const u8, flags: u64, mode: u64) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_OPEN, in("a0") path as usize, in("a1") flags, in("a2") mode, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn close(fd: u64) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_CLOSE, in("a0") fd, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn exec(path: *const u8, argv: *const u64) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_EXEC, in("a0") path as usize, in("a1") argv as usize, lateout("a0") ret);
    ret
}

pub const SYS_CHAN_CREATE: u64 = 27;
pub const SYS_CHAN_CONNECT: u64 = 28;
pub const SYS_CHAN_SEND: u64 = 29;
pub const SYS_CHAN_RECV: u64 = 30;
pub const SYS_CHAN_CLOSE: u64 = 31;
pub const SYS_CHAN_CREATE_NAMED: u64 = 32;
pub const SYS_CHAN_OPEN: u64 = 33;

pub const SYS_WRITE_FD: u64 = 24;
pub const SYS_CREATE: u64 = 25;
pub const SYS_MKDIR: u64 = 26;

#[inline]
pub unsafe fn create(path: *const u8, mode: u64, reserved: u64) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_CREATE, in("a0") path as usize, in("a1") mode, in("a2") reserved, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn write_fd(fd: u64, buf: *const u8, len: usize) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_WRITE_FD, in("a0") fd, in("a1") buf as usize, in("a2") len, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn mkdir(path: *const u8) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_MKDIR, in("a0") path as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn chan_create() -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_CHAN_CREATE, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn chan_connect(chan_id: u32) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_CHAN_CONNECT, in("a0") chan_id as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn chan_send(chan_id: u32, buf: *const u8, len: u32) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_CHAN_SEND, in("a0") chan_id as usize, in("a1") buf as usize, in("a2") len as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn chan_recv(chan_id: u32, buf: *mut u8, len: u32) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_CHAN_RECV, in("a0") chan_id as usize, in("a1") buf as usize, in("a2") len as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn chan_close(chan_id: u32) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_CHAN_CLOSE, in("a0") chan_id as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn chan_create_named(name: *const u8) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_CHAN_CREATE_NAMED, in("a0") name as usize, lateout("a0") ret);
    ret
}

#[inline]
pub unsafe fn chan_open(name: *const u8) -> i64 {
    let ret: i64;
    asm!("ecall", in("a7") SYS_CHAN_OPEN, in("a0") name as usize, lateout("a0") ret);
    ret
}
