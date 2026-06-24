#![no_std]
#![no_main]
#![allow(
    unsafe_op_in_unsafe_fn,
    non_snake_case,
    clippy::missing_safety_doc
)]

use core::arch::asm;

mod syscalls;
mod auth;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    syscalls::write(1, b"\nOnyxOS Login\n".as_ptr(), 14);

    let mut users = [auth::PasswdEntry {
        name: [0; 32],
        uid: 0,
        gid: 0,
        home: [0; 64],
        shell: [0; 32],
    }; auth::MAX_USERS];

    // First-boot setup: if no root user in /etc/passwd, run setup wizard.
    let nusers = auth::read_passwd(&mut users).unwrap_or(0);
    if auth::find_user(&users, nusers, b"root").is_none() {
        first_boot_setup();
    }

    loop {
        syscalls::write(1, b"login: ".as_ptr(), 7);
        let mut user_buf = [0u8; 64];
        let n = syscalls::read(0, user_buf.as_mut_ptr(), user_buf.len() as u64);
        if n <= 0 {
            continue;
        }
        let n = n as usize;
        let n = if n > 0 && user_buf[n - 1] == b'\n' { n - 1 } else { n };
        let username = &user_buf[..n];

        syscalls::write(1, b"password: ".as_ptr(), 10);
        let mut pass_buf = [0u8; 64];
        let pn = syscalls::read(0, pass_buf.as_mut_ptr(), pass_buf.len() as u64);
        if pn <= 0 {
            continue;
        }
        let pn = pn as usize;
        let pn = if pn > 0 && pass_buf[pn - 1] == b'\n' { pn - 1 } else { pn };
        let password = &pass_buf[..pn];

        if username.is_empty() || password.is_empty() {
            syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
            continue;
        }

        let nusers = match auth::read_passwd(&mut users) {
            Ok(n) => n,
            Err(_) => {
                syscalls::write(1, b"Login: no passwd file\n".as_ptr(), 23);
                continue;
            }
        };

        if auth::find_user(&users, nusers, username).is_none() {
            syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
            continue;
        }

        if !auth::verify_shadow_password(username, password) {
            syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
            continue;
        }

        syscalls::write(1, b"Login OK\n".as_ptr(), 9);
        syscalls::dropping(2);
        let shell = b"/bin/osh\0";
        syscalls::exec(shell.as_ptr(), core::ptr::null());
        syscalls::write(1, b"login: exec failed\n".as_ptr(), 19);
    }
}

unsafe fn first_boot_setup() {
    syscalls::write(1, b"\n=== First Boot Setup ===\n".as_ptr(), 27);
    syscalls::write(1, b"No root user found. Create root password.\n".as_ptr(), 43);

    loop {
        let mut pass1 = [0u8; 64];
        let mut pass2 = [0u8; 64];

        syscalls::write(1, b"Enter new root password: ".as_ptr(), 26);
        let n1 = read_line(&mut pass1);
        syscalls::write(1, b"\n".as_ptr(), 1);

        syscalls::write(1, b"Retype new root password: ".as_ptr(), 28);
        let n2 = read_line(&mut pass2);
        syscalls::write(1, b"\n".as_ptr(), 1);

        if n1.is_empty() {
            syscalls::write(1, b"Password cannot be empty. Try again.\n".as_ptr(), 39);
            continue;
        }
        if n1.len() != n2.len() {
            syscalls::write(1, b"Passwords do not match. Try again.\n".as_ptr(), 38);
            continue;
        }
        let mut ok = true;
        for i in 0..n1.len() {
            if n1[i] != n2[i] {
                ok = false;
                break;
            }
        }
        if !ok {
            syscalls::write(1, b"Passwords do not match. Try again.\n".as_ptr(), 38);
            continue;
        }

        // Create root user in /etc/passwd
        let home = b"/users/root";
        let shell = b"/bin/osh";
        if auth::update_passwd_entry(b"root", 0, 0, home, shell).is_err() {
            syscalls::write(1, b"Setup: failed to create /etc/passwd\n".as_ptr(), 39);
            syscalls::exit(1);
        }

        // Save password to /etc/shadow
        if auth::update_shadow_password(b"root", n1).is_err() {
            syscalls::write(1, b"Setup: failed to create /etc/shadow\n".as_ptr(), 40);
            syscalls::exit(1);
        }

        // Create /users/root directory
        let mut mkdir_buf = [0u8; 64];
        mkdir_buf[..11].copy_from_slice(b"/users/root");
        let _ = syscalls::mkdir(mkdir_buf.as_ptr());

        syscalls::write(1, b"Root password set. You can now log in.\n\n".as_ptr(), 41);
        return;
    }
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
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
