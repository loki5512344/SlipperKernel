#![no_std]
#![no_main]
#![allow(
    dead_code,
    unsafe_op_in_unsafe_fn,
    non_snake_case,
    clippy::missing_safety_doc
)]

use core::arch::asm;

mod syscalls;

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    syscalls::write(1, b"\nOnyxOS Login\n".as_ptr(), 14);

    loop {
        // Read username
        syscalls::write(1, b"login: ".as_ptr(), 7);
        let mut user = [0u8; 64];
        let n = syscalls::read(0, user.as_mut_ptr(), user.len() as u64);
        if n <= 0 {
            continue;
        }
        let n = n as usize;
        let n = if n > 0 && user[n - 1] == b'\n' {
            n - 1
        } else {
            n
        };
        let username = &user[..n];

        // Read password
        syscalls::write(1, b"password: ".as_ptr(), 10);
        let mut pass = [0u8; 64];
        let pn = syscalls::read(0, pass.as_mut_ptr(), pass.len() as u64);
        if pn <= 0 {
            continue;
        }
        let pn = pn as usize;
        let pn = if pn > 0 && pass[pn - 1] == b'\n' {
            pn - 1
        } else {
            pn
        };
        let password = &pass[..pn];

        // MVP: accept any non-empty username/password
        if !username.is_empty() && !password.is_empty() {
            syscalls::write(1, b"Login OK\n".as_ptr(), 9);
            // Drop to user ring
            syscalls::dropping(2);
            // Exec user shell
            let shell = b"/bin/osh\0";
            syscalls::exec(shell.as_ptr());
            syscalls::write(1, b"login: exec failed\n".as_ptr(), 19);
        } else {
            syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
