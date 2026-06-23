//! SMP — multi-hart support.
use crate::arch::csr;

static mut G_ONLINE_HARTS: u32 = 1;

extern "Rust" {
    static mut secondary_release: u64;
}

/// Release secondary harts from their spin loop in boot.S.
pub unsafe fn release_secondary_harts() {
    core::ptr::write_volatile(core::ptr::addr_of_mut!(secondary_release), 1);
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

/// Entry point for secondary harts.
#[no_mangle]
pub unsafe extern "Rust" fn secondary_entry() -> ! {
    let hartid = csr::read_mhartid() as usize;
    crate::kinf!(
        "smp",
        "hart %d online",
        onyx_core::fmt::Arg::from(hartid as u32)
    );
    *(&raw mut G_ONLINE_HARTS) += 1;
    loop {
        csr::wfi();
    }
}

pub fn online_harts() -> u32 {
    unsafe { *(&raw const G_ONLINE_HARTS) }
}
