use core::sync::atomic::{AtomicBool, Ordering};
use core::hint::spin_loop;

pub const MAX_HARTS: usize = 8;
pub const SEC_STACK_SIZE: usize = 4096;

#[unsafe(no_mangle)]
pub static mut G_SEC_STACKS: [u8; MAX_HARTS * SEC_STACK_SIZE] = [0; MAX_HARTS * SEC_STACK_SIZE];

static mut G_ONLINE_HARTS: u32 = 1;

#[unsafe(no_mangle)]
pub static mut G_RELEASE: u64 = 0;

#[unsafe(no_mangle)]
pub static mut G_KERNEL_ROOT_PA: u64 = 0;

pub fn current_hart() -> usize {
    let hartid: usize;
    unsafe { core::arch::asm!("mv {}, tp", out(reg) hartid); }
    hartid
}

static mut G_CPU_ONLINE: [bool; MAX_HARTS] = [true, false, false, false, false, false, false, false];

pub fn cpu_online(hart: usize) -> bool {
    unsafe { (*(&raw const G_CPU_ONLINE))[hart] }
}

pub unsafe fn set_cpu_online(hart: usize, v: bool) {
    (*(&raw mut G_CPU_ONLINE))[hart] = v;
}

pub struct SpinLock {
    locked: AtomicBool,
}

impl SpinLock {
    pub const fn new() -> Self {
        SpinLock { locked: AtomicBool::new(false) }
    }
    pub fn lock(&self) {
        while self.locked.swap(true, Ordering::Acquire) {
            while self.locked.load(Ordering::Relaxed) {
                spin_loop();
            }
        }
    }
    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

pub unsafe fn release_secondary_harts() {
    core::ptr::write_volatile(core::ptr::addr_of_mut!(G_RELEASE), 1);
}

#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn secondary_entry() -> ! {
    let hartid: usize;
    core::arch::asm!("mv {0}, tp", out(reg) hartid);
    loop {
        if core::ptr::read_volatile(&raw const G_RELEASE) != 0 { break; }
        core::arch::asm!("wfi");
    }
    let sp = &raw const G_SEC_STACKS as *const u8 as usize + (hartid + 1) * SEC_STACK_SIZE;
    let entry = secondary_kmain as *const () as usize;
    let root_pa = core::ptr::read_volatile(&raw const G_KERNEL_ROOT_PA);
    let satp = if root_pa != 0 { (8u64 << 60) | (root_pa >> 12) } else { 0 };
    core::arch::asm!(
        "mv sp, {0}",
        "csrw mepc, {1}",
        "li t0, 1 << 11",
        "csrs mstatus, t0",
        "li t0, 1 << 12",
        "csrc mstatus, t0",
        "li t0, 1 << 7",
        "csrc mstatus, t0",
        "csrw satp, {2}",
        "sfence.vma zero, zero",
        "mret",
        in(reg) sp,
        in(reg) entry,
        in(reg) satp,
        options(noreturn),
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn secondary_kmain() -> ! {
    let hartid: usize;
    core::arch::asm!("mv {0}, tp", out(reg) hartid);
    crate::proc::process::set_cpu_online(hartid, true);
    *(&raw mut G_ONLINE_HARTS) += 1;
    crate::proc::scheduler::sched_enter_idle()
}

pub fn online_harts() -> u32 {
    unsafe { *(&raw const G_ONLINE_HARTS) }
}
