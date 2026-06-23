#![no_std]
#![no_main]
#![allow(dead_code, unsafe_op_in_unsafe_fn, non_snake_case, clippy::missing_safety_doc)]

use core::arch::asm;

mod syscalls;
mod auth;

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    let ring = syscalls::getring();

    if ring == 2 {
        do_user_passwd();
    } else {
        do_root_passwd();
    }

    syscalls::exit(0);
}

unsafe fn do_user_passwd() {
    syscalls::write(1, b"Changing password.\n".as_ptr(), 19);

    let mut old_pass = [0u8; 64];
    syscalls::write(1, b"Current password: ".as_ptr(), 18);
    let old_n = read_line(&mut old_pass);

    let stored = match auth::read_shadow_password(b"root") {
        Ok(p) => p,
        Err(_) => {
            syscalls::write(1, b"passwd: Authentication failure\n".as_ptr(), 33);
            syscalls::exit(1);
        }
    };

    let stored_len = stored.iter().position(|&b| b == 0).unwrap_or(stored.len());
    if old_n.len() != stored_len || !const_time_eq(old_n, &stored[..stored_len]) {
        syscalls::write(1, b"passwd: Authentication failure\n".as_ptr(), 33);
        syscalls::exit(1);
    }

    let mut new_pass = [0u8; 64];
    let mut confirm = [0u8; 64];
    syscalls::write(1, b"New password: ".as_ptr(), 14);
    let n1 = read_line(&mut new_pass);
    syscalls::write(1, b"\n".as_ptr(), 1);
    syscalls::write(1, b"Retype new password: ".as_ptr(), 22);
    let n2 = read_line(&mut confirm);
    syscalls::write(1, b"\n".as_ptr(), 1);

    if n1.is_empty() || n1.len() != n2.len() || !const_time_eq(n1, n2) {
        syscalls::write(1, b"passwd: Passwords do not match\n".as_ptr(), 34);
        syscalls::exit(1);
    }

    match auth::update_shadow_password(b"root", n1) {
        Ok(()) => { syscalls::write(1, b"passwd: password updated\n".as_ptr(), 25); }
        Err(_) => { syscalls::write(1, b"passwd: Failed to update password\n".as_ptr(), 34); }
    }
}

unsafe fn do_root_passwd() {
    let mut username = [0u8; 32];
    syscalls::write(1, b"Username: ".as_ptr(), 10);
    let uname = read_line(&mut username);
    if uname.is_empty() {
        syscalls::write(1, b"passwd: no username\n".as_ptr(), 21);
        syscalls::exit(1);
    }

    let mut new_pass = [0u8; 64];
    let mut confirm = [0u8; 64];
    syscalls::write(1, b"New password: ".as_ptr(), 14);
    let n1 = read_line(&mut new_pass);
    syscalls::write(1, b"\n".as_ptr(), 1);
    syscalls::write(1, b"Retype new password: ".as_ptr(), 22);
    let n2 = read_line(&mut confirm);
    syscalls::write(1, b"\n".as_ptr(), 1);

    if n1.is_empty() || n1.len() != n2.len() || !const_time_eq(n1, n2) {
        syscalls::write(1, b"passwd: Passwords do not match\n".as_ptr(), 34);
        syscalls::exit(1);
    }

    match auth::update_shadow_password(uname, n1) {
        Ok(()) => { syscalls::write(1, b"passwd: password updated\n".as_ptr(), 25); }
        Err(_) => { syscalls::write(1, b"passwd: Failed to update password\n".as_ptr(), 34); }
    }
}

unsafe fn read_line<'a>(buf: &'a mut [u8]) -> &'a [u8] {
    let n = syscalls::read(0, buf.as_mut_ptr(), (buf.len() - 1) as u64);
    if n <= 0 {
        return &[];
    }
    let mut n = n as usize;
    while n > 0 && (buf[n - 1] == b'\n' || buf[n - 1] == b'\r' || buf[n - 1] == 0) {
        n -= 1;
    }
    &buf[..n]
}

fn const_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for i in 0..a.len() {
        result |= a[i] ^ b[i];
    }
    result == 0
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop { unsafe { asm!("wfi"); } }
}
