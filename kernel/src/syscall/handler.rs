//! Syscall handler with ACL (ring-aware dispatch).
use crate::arch::trap_frame::TrapFrame;
use crate::drivers::uart;
use crate::fs::vfs;
use crate::proc::proc;
use crate::syscall::abi::*;
use onyx_core::errno::Errno;

const USER_BASE: u64 = 0x10000;
const USER_TOP: u64 = 0x4000_0000;

fn user_ptr_ok(p: u64, len: u64) -> bool {
    p >= USER_BASE && p.checked_add(len).is_some_and(|end| end <= USER_TOP)
}

/// ACL: check if current process ring can call this syscall.
fn syscall_allowed(nr: u64, ring: u8) -> bool {
    match nr {
        // Available to all (ring 2 = user):
        SYS_write | SYS_read | SYS_exit | SYS_yield | SYS_getpid | SYS_sbrk | SYS_open
        | SYS_close | SYS_lseek | SYS_stat | SYS_exec | SYS_readdir | SYS_getring
        | SYS_dropring | SYS_sigmask | SYS_write_fd => true,
        // Root-only (ring 0 or 1):
        SYS_spawn
        | SYS_wait
        | SYS_snapshot_create
        | SYS_snapshot_rollback
        | SYS_snapshot_list
        | SYS_kill
        | SYS_create
        | SYS_mkdir => ring <= proc::PROC_RING_ROOT,
        // Stubbed:
        SYS_brk | SYS_mmap => false,
        _ => false,
    }
}

pub unsafe fn handle(tf: &mut TrapFrame) -> i64 {
    let nr = tf.a7;
    let a0 = tf.a0;
    let a1 = tf.a1;
    let a2 = tf.a2;

    let cur_ring = proc::current_ring();

    // ACL check.
    if !syscall_allowed(nr, cur_ring) {
        return Errno::Perm.as_i64();
    }

    match nr {
        SYS_write => sys_write(tf, a0, a1, a2),
        SYS_read => sys_read(tf, a0, a1, a2),
        SYS_exit => sys_exit(a0),
        SYS_yield => sys_yield(),
        SYS_getpid => sys_getpid(),
        SYS_open => sys_open(a0, a1, a2),
        SYS_close => sys_close(a0),
        SYS_lseek => sys_lseek(a0, a1 as i64, a2 as u32),
        SYS_stat => sys_stat(a0, a1),
        SYS_exec => sys_exec(tf, a0),
        SYS_sbrk => sys_sbrk(a0 as i64),
        SYS_spawn => sys_spawn(a0, a1 as u8),
        SYS_wait => sys_wait(tf, a0),
        SYS_readdir => sys_readdir(a0, a1, a2),
        SYS_getring => sys_getring(),
        SYS_dropring => sys_dropring(a0 as u8),
        SYS_snapshot_create => sys_snapshot_create(a0),
        SYS_snapshot_rollback => sys_snapshot_rollback(a0 as u32),
        SYS_snapshot_list => sys_snapshot_list(a0, a1),
        SYS_kill => sys_kill(a0 as u32, a1 as u32),
        SYS_sigmask => sys_sigmask(a0 as u32, a1 as u32),
        SYS_write_fd => sys_write_fd(a0, a1, a2),
        SYS_create => sys_create(a0, a1, a2),
        SYS_mkdir => sys_mkdir(a0),
        SYS_brk | SYS_mmap => Errno::NoSys.as_i64(),
        _ => Errno::NoSys.as_i64(),
    }
}

unsafe fn sys_write(tf: &mut TrapFrame, _fd: u64, buf: u64, len: u64) -> i64 {
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

unsafe fn sys_read(tf: &mut TrapFrame, _fd: u64, buf: u64, len: u64) -> i64 {
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

unsafe fn sys_exit(code: u64) -> i64 {
    let pid = proc::current_pid();
    proc::exit(pid, code as i32);
    0
}

unsafe fn sys_yield() -> i64 {
    proc::set_need_resched(true);
    0
}
unsafe fn sys_getpid() -> i64 {
    proc::current_pid() as i64
}

unsafe fn sys_open(path: u64, _flags: u64, _mode: u64) -> i64 {
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

unsafe fn sys_close(token: u64) -> i64 {
    match vfs::close(token) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

unsafe fn sys_lseek(token: u64, off: i64, whence: u32) -> i64 {
    match vfs::lseek(token, off, whence) {
        Ok(pos) => pos as i64,
        Err(e) => e.as_i64(),
    }
}

unsafe fn sys_stat(path: u64, _st: u64) -> i64 {
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

unsafe fn sys_exec(tf: &mut TrapFrame, path: u64) -> i64 {
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
    use crate::fs::vfs;
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

unsafe fn sys_sbrk(incr: i64) -> i64 {
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

/// SYS_spawn: create new process from .onx file.
unsafe fn sys_spawn(path: u64, ring_hint: u8) -> i64 {
    if !user_ptr_ok(path, 1) {
        return Errno::Inval.as_i64();
    }
    let mut len = 0usize;
    let p = path as *const u8;
    while *p.add(len) != 0 && len < 256 {
        len += 1;
    }
    let path_bytes = core::slice::from_raw_parts(p, len);
    let parent_pid = proc::current_pid();
    match proc::spawn(path_bytes, ring_hint, parent_pid) {
        Ok(pid) => pid as i64,
        Err(e) => e.as_i64(),
    }
}

/// SYS_wait: wait for child exit. Blocks (yields) until a child exits.
unsafe fn sys_wait(tf: &mut TrapFrame, status_out: u64) -> i64 {
    let status_ptr = if status_out != 0 && user_ptr_ok(status_out, 4) {
        status_out as *mut i32
    } else {
        core::ptr::null_mut()
    };
    match proc::wait(tf, status_ptr) {
        Ok(pid) => pid as i64,
        Err(e) => e.as_i64(),
    }
}

/// SYS_readdir: list directory entries (stateful per process).
/// Returns 1 = entry read, 0 = EOF, negative = error.
unsafe fn sys_readdir(dir: u64, name_out: u64, len: u64) -> i64 {
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

/// SYS_getring: return current process ring (0/1/2).
unsafe fn sys_getring() -> i64 {
    proc::current_ring() as i64
}

/// SYS_dropping: drop to less privileged ring (one-way, never raises).
unsafe fn sys_dropring(target: u8) -> i64 {
    let p = proc::current();
    if target < p.ring {
        return Errno::Perm.as_i64();
    } // cannot raise
    if target == p.ring {
        return 0;
    }
    p.ring = target;
    0
}

// ── Snapshot syscalls (root-only) ──────────────────────────────────────────
// These delegate to the OnyxFS snapshot stubs. The ACL layer already enforces
// that only ring ≤ PROC_RING_ROOT may invoke them.

/// SYS_snapshot_create(name): create a filesystem snapshot.
/// `name` is a NUL-terminated user pointer to the snapshot name.
unsafe fn sys_snapshot_create(name: u64) -> i64 {
    if !user_ptr_ok(name, 1) {
        return Errno::Inval.as_i64();
    }
    let mut len = 0usize;
    let p = name as *const u8;
    while *p.add(len) != 0 && len < 32 {
        len += 1;
    }
    let name_bytes = core::slice::from_raw_parts(p, len);
    match crate::fs::onyxfs::snapshot_create(name_bytes) {
        Ok(id) => id as i64,
        Err(e) => e.as_i64(),
    }
}

/// SYS_snapshot_rollback(id): restore filesystem state from snapshot `id`.
unsafe fn sys_snapshot_rollback(id: u32) -> i64 {
    match crate::fs::onyxfs::snapshot_rollback(id) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

/// SYS_snapshot_list(buf, len): list snapshot names into `buf`.
/// Returns the number of snapshots listed.
unsafe fn sys_snapshot_list(buf: u64, len: u64) -> i64 {
    if len == 0 {
        return 0;
    }
    if !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    match crate::fs::onyxfs::snapshot_list(buf as *mut u8, len as usize) {
        Ok(count) => count as i64,
        Err(e) => e.as_i64(),
    }
}

// ── Signal syscalls ───────────────────────────────────────────────────────

/// SYS_kill(pid, signal): deliver `signal` to process `pid`.
/// Root-only (ACL enforced in `syscall_allowed`).
unsafe fn sys_kill(pid: u32, signal: u32) -> i64 {
    match proc::signal_send(pid, signal) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

/// SYS_sigmask(how, sig): block / unblock / set the signal mask for one
/// signal. `how`: 0 = block, 1 = unblock, 2 = set mask to just `sig`.
/// Signal 9 (KILL) cannot be blocked — `how == 0` on signal 9 is a no-op.
unsafe fn sys_sigmask(how: u32, sig: u32) -> i64 {
    if sig >= 32 {
        return Errno::Inval.as_i64();
    }
    let p = proc::current();
    match how {
        0 => {
            // Block — but KILL cannot be blocked.
            if sig != proc::SIG_KILL {
                p.signal_mask |= 1u32 << sig;
            }
        }
        1 => {
            p.signal_mask &= !(1u32 << sig);
        }
        2 => {
            // Set mask to exactly {sig} (plus KILL-ignoring: KILL still
            // cannot be blocked, so don't add it).
            let mut m = 0u32;
            if sig != proc::SIG_KILL {
                m = 1u32 << sig;
            }
            p.signal_mask = m;
        }
        _ => return Errno::Inval.as_i64(),
    }
    0
}

// ── File write / create / mkdir syscalls ──────────────────────────────────

/// SYS_write_fd(fd, buf, len): write `len` bytes from user buffer `buf` to
/// the open file referred to by the capability fd `fd`. Returns the number
/// of bytes written (or a negative errno).
unsafe fn sys_write_fd(token: u64, buf: u64, len: u64) -> i64 {
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
unsafe fn sys_create(path: u64, mode: u64, _reserved: u64) -> i64 {
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
unsafe fn sys_mkdir(path: u64) -> i64 {
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
