#![no_std]
#![no_main]
#![allow(dead_code, unsafe_op_in_unsafe_fn, non_snake_case, clippy::missing_safety_doc)]

use core::arch::asm;

mod syscalls;
mod auth;

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    let ring = syscalls::getring();
    if ring != 1 {
        syscalls::write(1, b"useradd: only root can add users\n".as_ptr(), 35);
        syscalls::exit(1);
    }

    let mut username = [0u8; 32];
    syscalls::write(1, b"Username: ".as_ptr(), 10);
    let uname = read_line(&mut username);
    if uname.is_empty() {
        syscalls::write(1, b"useradd: no username\n".as_ptr(), 23);
        syscalls::exit(1);
    }

    let mut uid_str = [0u8; 12];
    syscalls::write(1, b"UID: ".as_ptr(), 5);
    let uid_s = read_line(&mut uid_str);
    let uid = parse_dec(uid_s);

    let mut password = [0u8; 64];
    syscalls::write(1, b"Password: ".as_ptr(), 10);
    let pass = read_line(&mut password);
    if pass.is_empty() {
        syscalls::write(1, b"useradd: no password\n".as_ptr(), 23);
        syscalls::exit(1);
    }

    // Check if user already exists
    let mut users = [auth::PasswdEntry {
        name: [0; 32],
        uid: 0, gid: 0, home: [0; 64], shell: [0; 32],
    }; auth::MAX_USERS];
    let nusers = auth::read_passwd(&mut users).unwrap_or(0);

    if auth::find_user(&users, nusers, uname).is_some() {
        syscalls::write(1, b"useradd: user already exists\n".as_ptr(), 30);
        syscalls::exit(1);
    }

    // Build home path
    let mut home = [0u8; 64];
    home[..7].copy_from_slice(b"/users/");
    for i in 0..uname.len().min(56) {
        home[7 + i] = uname[i];
    }

    let shell = b"/bin/osh";

    // Add entry
    let home_len = 7 + uname.len().min(56);
    if let Err(_e) = auth::update_passwd_entry(uname, uid, uid, &home[..home_len], shell) {
        syscalls::write(1, b"useradd: failed to update /etc/passwd\n".as_ptr(), 40);
        syscalls::exit(1);
    }

    // Add shadow entry
    if let Err(_e) = auth::update_shadow_password(uname, pass) {
        syscalls::write(1, b"useradd: failed to update /etc/shadow\n".as_ptr(), 41);
        syscalls::exit(1);
    }

    // Create home directory
    let mut mkdir_path = [0u8; 64];
    let np = home_len.min(63);
    mkdir_path[..np].copy_from_slice(&home[..np]);
    let ret = syscalls::mkdir(mkdir_path.as_ptr());
    if ret < 0 && ret != -13 {
        syscalls::write(1, b"useradd: warning: could not create home\n".as_ptr(), 42);
    }

    syscalls::write(1, b"useradd: user added\n".as_ptr(), 20);
    syscalls::exit(0);
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

fn parse_dec(s: &[u8]) -> u32 {
    let mut val: u32 = 0;
    for &b in s.iter() {
        if b >= b'0' && b <= b'9' {
            val = val.wrapping_mul(10).wrapping_add(u32::from(b - b'0'));
        } else {
            break;
        }
    }
    val
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop { unsafe { asm!("wfi"); } }
}
