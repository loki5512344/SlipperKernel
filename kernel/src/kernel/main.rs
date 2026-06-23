//! kmain — точка входа в S-mode.
use crate::arch::csr;
use crate::arch::regs::*;
use crate::drivers::{plic, uart, virtio};
use crate::fs::vfs;
use crate::kernel::{timer, trap};
use crate::libfdt::fdt;
use crate::mm::{heap, pmm, vmm};
use crate::proc::proc;
use onyx_core::fmt::Arg;

const BANNER: &str = "\n  ___ _ _                  _\n / __(_) |_ _____ __ _____| |__\n \\__ \\ | \\ V / -_) V /___| / /_\n |___/_|_|\\_/\\___|\\_/    |_\\__/\n  OnyxKernel v0.3 (Rust) — RISC-V 64 GC\n\n";

pub unsafe fn kmain(hartid: usize, fdt_addr: usize) -> ! {
    uart::init_default();
    crate::kernel::klog::puts(BANNER);
    crate::kinf!(
        "kmain",
        "hartid=%d fdt=%p",
        Arg::from(hartid),
        Arg::from(fdt_addr)
    );

    if fdt::init(fdt_addr) {
        crate::kinf!("fdt", "parsed successfully");
    } else {
        crate::kwrn!("fdt", "parse failed, using defaults");
    }

    let mem = fdt::memory().unwrap_or(fdt::FdtMemory {
        base: 0x8000_0000,
        size: 0x1000_0000,
    });
    pmm::init(mem.base, mem.size);

    let _ = vmm::init();
    crate::kinf!(
        "vmm",
        "Sv39 on, kernel root @%p",
        Arg::from(vmm::kernel_root())
    );

    heap::init();
    crate::kinf!("heap", "ready");

    trap::init();
    timer::init();

    if let Some(plic_base) = fdt::find_plic() {
        plic::init(plic_base);
        plic::set_priority(PLIC_PRIO_UART, 7);
        plic::set_priority(PLIC_PRIO_VIRTIO, 5);
        plic::enable(PLIC_PRIO_UART, 0);
        plic::set_threshold(0);
        csr::set_sie((1 << 1) | (1 << 9));
        crate::kinf!("plic", "base=%p", Arg::from(plic_base));
    }

    let mut ndevs = 0;
    let bases = [0x1000_1000usize, 0x1000_8000, 0x1000_3000, 0x1000_4000];
    for &b in &bases {
        if virtio::probe(b) && virtio::init(b).is_ok() {
            ndevs += 1;
        }
    }
    crate::kinf!("virtio-blk", "%d device(s)", Arg::from(ndevs));

    vfs::init();
    if ndevs > 0 {
        match vfs::mount_root(0, ONYXFS_LBA) {
            Ok(()) => crate::kinf!("vfs", "root mounted"),
            Err(e) => crate::kerr!("vfs", "mount failed: %s", Arg::from(e.as_str())),
        }
    }

    // Load /bin/init as PID 1 in root space (ring 1).
    let path = b"/bin/init";
    let token = match vfs::open(path, vfs::PERM_READ | vfs::PERM_SEEK) {
        Ok(t) => t,
        Err(e) => {
            crate::kerr!("kmain", "open /bin/init failed: %s", Arg::from(e.as_str()));
            crate::kernel::klog::halt();
        }
    };
    let mut size = 0u32;
    vfs::stat(token, &mut size).ok();
    crate::kinf!("kmain", "/bin/init size=%d", Arg::from(size));

    let img = match heap::kmalloc(size as usize) {
        Ok(p) => p,
        Err(e) => {
            crate::kerr!("kmain", "kmalloc failed: %s", Arg::from(e.as_str()));
            crate::kernel::klog::halt();
        }
    };
    vfs::read(token, img, size).ok();
    vfs::close(token).ok();

    let r = match crate::proc::onx::load(img, size as usize) {
        Ok(r) => r,
        Err(e) => {
            crate::kerr!("kmain", "onx_load failed: %s", Arg::from(e.as_str()));
            crate::kernel::klog::halt();
        }
    };
    heap::kfree(img);

    crate::kinf!(
        "onx",
        "entry=%p root=%p ustack=%p ring=%d",
        Arg::from(r.entry),
        Arg::from(r.root_pa),
        Arg::from(r.ustack),
        Arg::from(r.ring as u32)
    );

    proc::init();
    // PID 1 is init — launched in ring 1 (root space) if binary has RING1 flag, else ring 2.
    let ring = if r.ring == 1 {
        proc::PROC_RING_ROOT
    } else {
        proc::PROC_RING_USER
    };
    proc::create_user(
        r.entry,
        r.ustack,
        r.root_pa,
        proc::PROC_PID_INIT,
        0,
        r.heap_brk,
        ring,
    )
    .ok();

    csr::set_sstatus(SSTATUS_SIE);
    crate::kinf!(
        "proc",
        "entering user pid=1 entry=%p ring=%d",
        Arg::from(r.entry),
        Arg::from(ring as u32)
    );
    proc::enter_user(1);
}
