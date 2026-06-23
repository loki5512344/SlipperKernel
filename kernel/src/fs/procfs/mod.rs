//! procfs — virtual filesystem exposing kernel state.
//!
//! Mounted at `/proc` by the VFS mount table. All files are generated
//! on-the-fly from kernel global variables.
//!
//! Inode allocation:
//!   1 → /proc            (directory)
//!   2 → /proc/version
//!   3 → /proc/cpuinfo
//!   4 → /proc/meminfo
//!   5 → /proc/uptime
//!   6 → /proc/load
//!   7 → /proc/stat
//!
//! Each inode > 1 has a fixed maximum size (PROCFS_MAX_SIZE) so the FD
//! table can store `size`. The actual content is generated on first read
//! and cached in a static buffer per-file (single-user assumption).

use crate::mm::{heap, pmm};
use crate::proc;
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

pub const PROCFS_ROOT_INO: u32 = ONYFS_ROOT_INO;
pub const PROCFS_VERSION_INO: u32 = 2;
pub const PROCFS_CPUINFO_INO: u32 = 3;
pub const PROCFS_MEMINFO_INO: u32 = 4;
pub const PROCFS_UPTIME_INO: u32 = 5;
pub const PROCFS_LOAD_INO: u32 = 6;
pub const PROCFS_STAT_INO: u32 = 7;
const PROCFS_MAX_INO: u32 = 7;

pub const PROCFS_MAX_SIZE: u32 = 512;
const DIRENT_SIZE: usize = 40;

const VERSION_STR: &str = "OnyxKernel v0.3 (Rust) — RISC-V 64 GC\n";

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProcfsStat {
    pub ino: u32,
    pub size: u32,
    pub mode: u32,
}

pub unsafe fn stat(ino: u32) -> KResult<ProcfsStat> {
    match ino {
        PROCFS_ROOT_INO => Ok(ProcfsStat {
            ino: PROCFS_ROOT_INO,
            size: 0,
            mode: 0o040755,
        }),
        PROCFS_VERSION_INO => Ok(ProcfsStat {
            ino: PROCFS_VERSION_INO,
            size: VERSION_STR.len() as u32,
            mode: 0o100444,
        }),
        PROCFS_CPUINFO_INO => Ok(ProcfsStat {
            ino: PROCFS_CPUINFO_INO,
            size: PROCFS_MAX_SIZE,
            mode: 0o100444,
        }),
        PROCFS_MEMINFO_INO => Ok(ProcfsStat {
            ino: PROCFS_MEMINFO_INO,
            size: PROCFS_MAX_SIZE,
            mode: 0o100444,
        }),
        PROCFS_UPTIME_INO => Ok(ProcfsStat {
            ino: PROCFS_UPTIME_INO,
            size: 32,
            mode: 0o100444,
        }),
        PROCFS_LOAD_INO => Ok(ProcfsStat {
            ino: PROCFS_LOAD_INO,
            size: PROCFS_MAX_SIZE,
            mode: 0o100444,
        }),
        PROCFS_STAT_INO => Ok(ProcfsStat {
            ino: PROCFS_STAT_INO,
            size: PROCFS_MAX_SIZE,
            mode: 0o100444,
        }),
        _ => Err(Errno::NoEnt),
    }
}

pub unsafe fn read(ino: u32, buf: *mut u8, offset: u32, len: u32) -> KResult<u32> {
    let content = generate_content(ino)?;
    let avail = (content.len() as u32).saturating_sub(offset);
    let to_copy = len.min(avail) as usize;
    if to_copy > 0 {
        core::ptr::copy_nonoverlapping(content.as_ptr().add(offset as usize), buf, to_copy);
    }
    Ok(to_copy as u32)
}

unsafe fn generate_content(ino: u32) -> KResult<&'static [u8]> {
    // Use the FS global buffer (G_BUF) sparingly — generate into a local then
    // copy. Actually onyxfs owns G_BUF; we use a separate static buffer.
    static mut G_PROCBUF: [u8; PROCFS_MAX_SIZE as usize] = [0; PROCFS_MAX_SIZE as usize];
    let pb = &raw mut G_PROCBUF;

    let s = match ino {
        PROCFS_VERSION_INO => VERSION_STR,
        PROCFS_CPUINFO_INO => {
            let harts = crate::arch::smp::online_harts() as u64;
            let buf = &mut *pb;
            let mut pos = 0;
            pos += format_line(b"harts\t\t: ", harts, b"\n", buf, pos);
            for h in 0..harts {
                pos += format_line(b"processor\t: ", h as u64, b"\n", buf, pos);
                if pos >= buf.len() { break; }
            }
            pos += format_line_raw(b"model name\t: rv64gc\n", buf, pos);
            pos += format_line_raw(b"model\t\t: rv64gc\n", buf, pos);
            pos += format_line_raw(b"frequency\t: ~10 MHz (QEMU)\n", buf, pos);
            pos += format_line_raw(b"isa\t\t: rv64imafdc\n", buf, pos);
            core::str::from_utf8_unchecked(&buf[..pos.min(buf.len())])
        }
        PROCFS_MEMINFO_INO => {
            let buf = &mut *pb;
            let mut pos = 0;
            let total_pages = pmm::total_pages();
            let free_pages = pmm::free_pages();
            let used_pages = total_pages - free_pages;
            let total_kb = (total_pages * 4) as u64;
            let free_kb = (free_pages * 4) as u64;
            let used_kb = (used_pages * 4) as u64;
            let heap_used = heap::used();
            pos += format_line(b"MemTotal\t: ", total_kb, b" kB\n", buf, pos);
            pos += format_line(b"MemFree\t\t: ", free_kb, b" kB\n", buf, pos);
            pos += format_line(b"MemUsed\t\t: ", used_kb, b" kB\n", buf, pos);
            pos += format_line(b"HeapTotal\t: 4096 kB\n", 0, b"", buf, pos);
            pos += format_line(b"HeapUsed\t: ", (heap_used / 1024) as u64, b" kB\n", buf, pos);
            pos += format_line(b"HeapFree\t: ", ((4096 - heap_used / 1024).max(0)) as u64, b" kB\n", buf, pos);
            core::str::from_utf8_unchecked(&buf[..pos.min(buf.len())])
        }
        PROCFS_UPTIME_INO => {
            let buf = &mut *pb;
            let mut pos = 0;
            let us = timer::uptime_us();
            let secs = us / 1_000_000;
            let frac = (us % 1_000_000) / 10_000; // hundredths
            pos += format_dec(secs, buf, pos);
            pos += b".".len();
            if pos + 2 <= buf.len() {
                buf[pos] = b'0' + (frac / 10) as u8;
                buf[pos + 1] = b'0' + (frac % 10) as u8;
                pos += 2;
            }
            pos += format_line_raw(b"\n", buf, pos);
            core::str::from_utf8_unchecked(&buf[..pos.min(buf.len())])
        }
        PROCFS_LOAD_INO => {
            let buf = &mut *pb;
            let mut pos = 0;
            let procs = proc::count();
            pos += format_line(b"processes\t: ", procs as u64, b"\n", buf, pos);
            let running = 1u64;
            pos += format_line(b"procs_running\t: ", running, b"\n", buf, pos);
            pos += format_line(b"procs_blocked\t: 0\n", 0, b"", buf, pos);
            core::str::from_utf8_unchecked(&buf[..pos.min(buf.len())])
        }
        PROCFS_STAT_INO => {
            let buf = &mut *pb;
            let mut pos = 0;
            let us = timer::uptime_us();
            let secs = us / 1_000_000;
            let _harts = crate::arch::smp::online_harts();
            let procs = proc::count();
            pos += format_line(b"btime ", secs, b"\n", buf, pos);
            pos += format_line(b"cpu 0 0 0 0 0 0 0 0 0 0\n", 0, b"", buf, pos);
            pos += format_line(b"processes ", procs as u64, b"\n", buf, pos);
            pos += format_line(b"procs_running 1\n", 0, b"", buf, pos);
            pos += format_line(b"procs_blocked 0\n", 0, b"", buf, pos);
            core::str::from_utf8_unchecked(&buf[..pos.min(buf.len())])
        }
        _ => return Err(Errno::NoEnt),
    };
    Ok(s.as_bytes())
}

/// Generate a line like `prefix + dec_number + suffix` into buf at pos.
/// Returns the number of bytes written.
unsafe fn format_line(prefix: &[u8], num: u64, suffix: &[u8], buf: &mut [u8], pos: usize) -> usize {
    let mut written = 0;
    // prefix
    if pos + written + prefix.len() <= buf.len() {
        buf[pos + written..pos + written + prefix.len()].copy_from_slice(prefix);
        written += prefix.len();
    }
    // number
    written += format_dec(num, buf, pos + written);
    // suffix
    if pos + written + suffix.len() <= buf.len() {
        buf[pos + written..pos + written + suffix.len()].copy_from_slice(suffix);
        written += suffix.len();
    }
    written
}

unsafe fn format_line_raw(line: &[u8], buf: &mut [u8], pos: usize) -> usize {
    if pos + line.len() <= buf.len() {
        buf[pos..pos + line.len()].copy_from_slice(line);
        line.len()
    } else {
        0
    }
}

/// Format u64 as decimal into buf at pos. Returns bytes written.
unsafe fn format_dec(mut n: u64, buf: &mut [u8], pos: usize) -> usize {
    if n == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
            return 1;
        }
        return 0;
    }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    while n > 0 && i > 0 {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let digits = &tmp[i..];
    let n = digits.len().min(buf.len().saturating_sub(pos));
    if n > 0 {
        buf[pos..pos + n].copy_from_slice(&digits[..n]);
    }
    n
}

/// Lookup a single path component within procfs root.
pub unsafe fn lookup(name: &[u8]) -> KResult<u32> {
    match name {
        b"" | b"/" | b"." => Ok(PROCFS_ROOT_INO),
        b"version" => Ok(PROCFS_VERSION_INO),
        b"cpuinfo" => Ok(PROCFS_CPUINFO_INO),
        b"meminfo" => Ok(PROCFS_MEMINFO_INO),
        b"uptime" => Ok(PROCFS_UPTIME_INO),
        b"load" => Ok(PROCFS_LOAD_INO),
        b"stat" => Ok(PROCFS_STAT_INO),
        _ => Err(Errno::NoEnt),
    }
}

/// Read a directory entry by index. Returns the inode of the entry, or None
/// if there are no more entries.
pub unsafe fn readdir_entry(idx: u32, name_out: *mut u8, name_len: usize) -> Option<u32> {
    let (name, ino): (&[u8], u32) = match idx {
        0 => (b"." as &[u8], PROCFS_ROOT_INO),
        1 => (b".." as &[u8], PROCFS_ROOT_INO),
        2 => (b"version" as &[u8], PROCFS_VERSION_INO),
        3 => (b"cpuinfo" as &[u8], PROCFS_CPUINFO_INO),
        4 => (b"meminfo" as &[u8], PROCFS_MEMINFO_INO),
        5 => (b"uptime" as &[u8], PROCFS_UPTIME_INO),
        6 => (b"load" as &[u8], PROCFS_LOAD_INO),
        7 => (b"stat" as &[u8], PROCFS_STAT_INO),
        _ => return None,
    };
    let n = name.len().min(name_len.saturating_sub(1));
    unsafe {
        core::ptr::copy_nonoverlapping(name.as_ptr(), name_out, n);
        *name_out.add(n) = 0;
    }
    Some(ino)
}
