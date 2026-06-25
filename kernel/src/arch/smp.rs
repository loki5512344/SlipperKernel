//! SMP — secondary hart init, per-CPU stacks, release.
//!
//! Hart ID is passed from M-mode via the `tp` register (set at `_start`).
//! Secondary harts wait in M-mode for `G_RELEASE`, transition to S-mode with
//! the kernel page table (Sv39), and enter the scheduler.

pub const MAX_HARTS: usize = 8;
pub const SEC_STACK_SIZE: usize = 4096;

#[unsafe(no_mangle)]
pub static mut G_SEC_STACKS: [u8; MAX_HARTS * SEC_STACK_SIZE] = [0; MAX_HARTS * SEC_STACK_SIZE];

static mut G_ONLINE_HARTS: u32 = 1;

#[unsafe(no_mangle)]
pub static mut G_RELEASE: u64 = 0;

#[unsafe(no_mangle)]
pub static mut G_KERNEL_ROOT_PA: u64 = 0;

pub unsafe fn release_secondary_harts() {
    core::ptr::write_volatile(core::ptr::addr_of_mut!(G_RELEASE), 1);
}

/// Called from boot.S `park` loop — runs in M-mode.
/// Waits for `G_RELEASE` then transitions to S-mode with Sv39.
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

/// Runs in S-mode with Sv39 paging active.
///
/// Initializes trap handling, per-hart timer, and enters the scheduler
/// idle loop so this hart can pick up Ready processes.
#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn secondary_kmain() -> ! {
    let hartid: usize;
    core::arch::asm!("mv {0}, tp", out(reg) hartid);
    *(&raw mut G_ONLINE_HARTS) += 1;

    // Enter the scheduler — sets up stvec, sscratch, timer, and
    // parks in a wfi loop. Timer interrupts will trigger sched_yield()
    // which may assign a Ready process to this hart.
    crate::proc::scheduler::sched_enter_idle()
}

pub fn online_harts() -> u32 {
    unsafe { *(&raw const G_ONLINE_HARTS) }
}
