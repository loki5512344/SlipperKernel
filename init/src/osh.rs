#![no_std]
#![no_main]
#![allow(
    unsafe_op_in_unsafe_fn,
    non_snake_case,
    clippy::missing_safety_doc
)]

use core::arch::asm;

mod syscalls;

// Shell prompt
const PROMPT: &str = "osh$ ";

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    syscalls::write(1, b"OnyxShell v0.1 (user space)\n".as_ptr(), 30);

    let mut buf = [0u8; 256];
    loop {
        syscalls::write(1, PROMPT.as_ptr(), PROMPT.len());
        let n = syscalls::read(0, buf.as_mut_ptr(), buf.len() as u64);
        if n <= 0 {
            continue;
        }
        let n = n as usize;
        // Strip trailing newline
        let n = if n > 0 && buf[n - 1] == b'\n' {
            n - 1
        } else {
            n
        };
        let line = &buf[..n];
        if line.is_empty() {
            continue;
        }

        let (cmd, rest) = split_first_word(line);
        match cmd {
            b"help" => do_help(),
            b"echo" => {
                syscalls::write(1, rest.as_ptr(), rest.len());
                syscalls::write(1, b"\n".as_ptr(), 1);
            }
            b"cat" => do_cat(rest),
            b"ls" => do_ls(rest),
            b"exec" => do_exec(rest),
            b"clear" => {
                syscalls::write(1, b"\x1b[2J\x1b[H".as_ptr(), 7);
            }
            b"exit" => syscalls::exit(0),
            b"whoami" => do_whoami(),
            b"pwd" => {
                syscalls::write(1, b"/\n".as_ptr(), 2);
            }
            _ => {
                syscalls::write(1, b"? ".as_ptr(), 2);
                syscalls::write(1, line.as_ptr(), line.len());
                syscalls::write(1, b"\n".as_ptr(), 1);
            }
        }
    }
}

fn split_first_word(line: &[u8]) -> (&[u8], &[u8]) {
    let mut i = 0;
    while i < line.len() && line[i] != b' ' {
        i += 1;
    }
    let rest = if i < line.len() {
        let mut j = i;
        while j < line.len() && line[j] == b' ' {
            j += 1;
        }
        &line[j..]
    } else {
        &[]
    };
    (&line[..i], rest)
}

unsafe fn do_help() {
    static HELP: &str = "Commands:\n  help       this help\n  echo <txt> print text\n  cat <path> print file\n  ls <path>  list directory\n  exec <path> run binary\n  clear      clear screen\n  exit       quit\n  whoami     show ring\n  pwd        print cwd\n";
    syscalls::write(1, HELP.as_ptr(), HELP.len());
}

unsafe fn do_cat(path: &[u8]) {
    // NUL-terminate path
    let mut buf = [0u8; 64];
    if path.len() >= buf.len() {
        return;
    }
    for (i, &b) in path.iter().enumerate() {
        buf[i] = b;
    }
    let fd = syscalls::open(buf.as_ptr(), 0, 0);
    if fd < 0 {
        let m = b"cat: open failed\n";
        syscalls::write(1, m.as_ptr(), m.len());
        return;
    }
    let mut rbuf = [0u8; 512];
    loop {
        let n = syscalls::read(fd as u64, rbuf.as_mut_ptr(), rbuf.len() as u64);
        if n <= 0 {
            break;
        }
        syscalls::write(1, rbuf.as_ptr(), n as usize);
    }
    syscalls::close(fd as u64);
}

unsafe fn do_ls(path: &[u8]) {
    let mut dir = [0u8; 64];
    if path.is_empty() {
        dir[..1].copy_from_slice(b"/");
    } else {
        let n = path.len().min(63);
        dir[..n].copy_from_slice(&path[..n]);
    }
    let mut name = [0u8; 64];
    loop {
        let ret = syscalls::readdir(dir.as_ptr(), name.as_mut_ptr(), name.len() as u64);
        if ret <= 0 {
            break;
        }
        let mut nlen = 0;
        while nlen < name.len() && name[nlen] != 0 {
            nlen += 1;
        }
        syscalls::write(1, name.as_ptr(), nlen);
        syscalls::write(1, b"\n".as_ptr(), 1);
    }
}

unsafe fn do_exec(path: &[u8]) {
    let mut buf = [0u8; 64];
    if path.len() >= buf.len() {
        return;
    }
    for (i, &b) in path.iter().enumerate() {
        buf[i] = b;
    }
    syscalls::exec(buf.as_ptr());
    let m = b"exec: failed\n";
    syscalls::write(1, m.as_ptr(), m.len());
}

unsafe fn do_whoami() {
    let ring = syscalls::getring();
    let msg: &[u8] = match ring {
        0 => b"kernel\n",
        1 => b"root\n",
        2 => b"user\n",
        _ => b"unknown\n",
    };
    syscalls::write(1, msg.as_ptr(), msg.len());
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
