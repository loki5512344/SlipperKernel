#![no_std]
#![no_main]

mod syscalls;

fn write_dec(v: usize) {
    let mut buf = [0u8; 12];
    let mut p = 11;
    let mut n = v;
    if n == 0 { buf[10] = b'0'; unsafe { syscalls::write(1, buf[10..].as_ptr(), 1); } return; }
    while n > 0 && p > 0 {
        p -= 1;
        buf[p] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    unsafe { syscalls::write(1, buf[p..].as_ptr(), 12 - p); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(a0: usize, a1: usize) -> ! {
    let msg = b"argv_test: a0=";
    unsafe { syscalls::write(1, msg.as_ptr(), msg.len()); }
    write_dec(a0);
    let msg = b" a1=";
    unsafe { syscalls::write(1, msg.as_ptr(), msg.len()); }
    write_dec(a1);
    unsafe { syscalls::write(1, b"\n".as_ptr(), 1); }
    syscalls::exit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop { unsafe { core::arch::asm!("wfi"); } }
}
