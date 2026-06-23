#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn, non_snake_case, clippy::missing_safety_doc)]

use core::arch::asm;

mod syscalls;
mod auth;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    let ring = syscalls::getring();
    if ring != 1 {
        syscalls::write(1, b"userdel: only root can delete users\n".as_ptr(), 38);
        syscalls::exit(1);
    }

    let mut username = [0u8; 32];
    syscalls::write(1, b"Username: ".as_ptr(), 10);
    let uname = read_line(&mut username);
    if uname.is_empty() {
        syscalls::write(1, b"userdel: no username\n".as_ptr(), 23);
        syscalls::exit(1);
    }

    if uname == b"root" {
        syscalls::write(1, b"userdel: cannot delete root\n".as_ptr(), 30);
        syscalls::exit(1);
    }

    // Check if user exists
    let mut users = [auth::PasswdEntry {
        name: [0; 32],
        uid: 0, gid: 0, home: [0; 64], shell: [0; 32],
    }; auth::MAX_USERS];
    let nusers = auth::read_passwd(&mut users).unwrap_or(0);

    if auth::find_user(&users, nusers, uname).is_none() {
        syscalls::write(1, b"userdel: user not found\n".as_ptr(), 26);
        syscalls::exit(1);
    }

    // Remove from passwd
    if auth::delete_passwd_entry(uname).is_err() {
        syscalls::write(1, b"userdel: failed to update /etc/passwd\n".as_ptr(), 40);
        syscalls::exit(1);
    }

    // Remove from shadow
    if auth::delete_shadow_entry(uname).is_err() {
        syscalls::write(1, b"userdel: failed to update /etc/shadow\n".as_ptr(), 41);
        syscalls::exit(1);
    }

    syscalls::write(1, b"userdel: user deleted\n".as_ptr(), 21);
    syscalls::exit(0);
}

unsafe fn read_line(buf: &mut [u8]) -> &[u8] {
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

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop { unsafe { asm!("wfi"); } }
}
