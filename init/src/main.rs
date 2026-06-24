//! OnyxOS PID 1 init — root space service manager.
//!
//! This is the new init that uses SYS_spawn, SYS_wait, SYS_readdir.
//! Runs in ring 1 (root space). Scans /service/ for *.bin files,
//! spawns each as a root service, then launches /bin/login.

#![no_std]
#![no_main]
#![warn(clippy::all)]
#![allow(
    clippy::missing_safety_doc,
    unsafe_op_in_unsafe_fn,
    non_snake_case
)]

use core::arch::asm;

mod syscalls;

const BANNER: &str = "[init] OnyxOS init v0.3 (root space)\n";

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    syscalls::write(1, BANNER.as_ptr(), BANNER.len());

    // Scan /service/ for *.bin and *.osh files.
    let mut name_buf = [0u8; 64];
    let mut service_pids = [0u32; 16];
    let mut num_services = 0usize;
    let dir = b"/service\0";

    loop {
        let ret = syscalls::readdir(dir.as_ptr(), name_buf.as_mut_ptr(), name_buf.len() as u64);
        if ret <= 0 {
            break;
        }

        // Check if name ends with .bin or .osh
        let name_len = {
            let mut n = 0;
            while n < name_buf.len() && name_buf[n] != 0 {
                n += 1;
            }
            n
        };
        if name_len < 4 {
            continue;
        }
        let ext_start = name_len - 4;
        let is_bin = &name_buf[ext_start..name_len] == b".bin";
        let is_osh = name_len >= 4 && &name_buf[ext_start..name_len] == b".osh";
        if !is_bin && !is_osh {
            continue;
        }

        // Build full path: /service/<name>
        let mut path = [0u8; 96];
        path[..8].copy_from_slice(b"/service/");
        path[9..9 + name_len].copy_from_slice(&name_buf[..name_len]);

        // Spawn the service in root space (ring 1).
        let pid = syscalls::spawn(path.as_ptr(), core::ptr::null(), 1);
        if pid > 0 {
            syscalls::write(1, b"[init] + service ".as_ptr(), 17);
            syscalls::write(1, name_buf.as_ptr(), name_len);
            syscalls::write(1, b" pid=".as_ptr(), 5);
            // Print pid as decimal (simplified).
            let pid_str = format_dec(pid);
            syscalls::write(1, pid_str.as_ptr(), pid_str.len());
            syscalls::write(1, b"\n".as_ptr(), 1);
            if num_services < service_pids.len() {
                service_pids[num_services] = pid as u32;
                num_services += 1;
            }
        } else {
            syscalls::write(1, b"[init:ERR] spawn failed\n".as_ptr(), 24);
        }
    }

    // Report service count.
    let count_msg = b"[init] services started\n";
    syscalls::write(1, count_msg.as_ptr(), count_msg.len());

    // Launch /bin/login.
    let login = b"/bin/login\0";
    syscalls::write(1, b"[init] launching /bin/login\n".as_ptr(), 28);
    let _login_pid = syscalls::spawn(login.as_ptr(), core::ptr::null(), 1);

    // Reaper loop: wait for children.
    let mut status: i32 = 0;
    loop {
        let ret = syscalls::wait(&mut status as *mut i32);
        if ret > 0 {
            syscalls::write(1, b"[init] child exited\n".as_ptr(), 20);
        } else {
            // No children or error — yield.
            syscalls::yield_cpu();
        }
    }
}

fn format_dec(n: i64) -> [u8; 12] {
    let mut buf = [0u8; 12];
    let mut pos = 11;
    if n == 0 {
        buf[10] = b'0';
        return buf;
    }
    let mut val = n as u64;
    while val > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    buf
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
