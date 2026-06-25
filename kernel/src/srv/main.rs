//! kmain — точка входа в S-mode.
use crate::arch::csr;
use crate::arch::regs::*;
use crate::drivers::{fb, pci, plic, sdhci, uart, virtio};
use crate::fs::vfs;
use crate::libfdt::fdt;
use crate::mm::{heap, pmm, vmm};
use crate::proc;
use crate::srv::{timer, trap};
use onyx_core::errno::KResult;
use onyx_core::fmt::Arg;

const BANNER: &str = "\n\x1b[32m░█▀█░█▀█░█░█░█░█\n░█░█░█░█░░█░░▄▀▄\n░▀▀▀░▀░▀░░▀░░▀░▀\x1b[0m\n  OnyxKernel v0.3 (Rust) — RISC-V 64 GC\n\n";

pub unsafe fn kmain(hartid: usize, fdt_addr: usize) -> ! {
    uart::init_default();
    crate::srv::klog::puts(BANNER);
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

    // Probe SDHCI controller (for Milk-V Duo S / QEMU virt SDHCI)
    if let Some(sdhci_info) = fdt::find_sdhci() {
        let sdhci_base = sdhci_info.base as usize;
        let sdhci_irq = sdhci_info.irq;
        if sdhci::probe(sdhci_base) {
            crate::kinf!("sdhci", "found at %p irq=%d", Arg::from(sdhci_base), Arg::from(sdhci_irq));
            if sdhci::init(sdhci_base, sdhci_irq) {
                crate::kinf!("sdhci", "SD card initialized");
            } else {
                crate::kwrn!("sdhci", "SD card init failed");
            }
        } else {
            crate::kinf!("sdhci", "probe failed at %p", Arg::from(sdhci_base));
        }
    }

    // Init framebuffer: try PCI VGA first, fall back to allocated pages
    let fb_pa = pci::find_vga_fb().ok().filter(|&pa| pa != 0).or_else(|| {
        let fb_pages = (fb::FB_SIZE + 4095) / 4096;
        pmm::alloc_n(fb_pages).ok().map(|pa| {
            crate::kinf!("fb", "allocated at %p", Arg::from(pa));
            pa as usize
        })
    });
    if let Some(pa) = fb_pa {
        if fb::init(pa).is_ok() {
            crate::kinf!("fb", "init ok");
        } else {
            crate::kwrn!("fb", "init failed");
        }
    }
    if fb::enabled() {
        fb::clear();
        let banner = "\n░█▀█░█▀█░█░█░█░█\n░█░█░█░█░░█░░▄▀▄\n░▀▀▀░▀░▀░░▀░░▀░▀\n  OnyxKernel v0.3 (Rust) — RISC-V 64 GC\n\n";
        let mut y = 40usize;
        for line in banner.lines() {
            let x = (fb::FB_WIDTH - line.len() * 8) / 2;
            fb::draw_str(x, y, line, 0x00FF00, 0x000000);
            y += 16;
        }
        fb::draw_str(10, y + 8, "Booting...", 0x00AAAA, 0x000000);
    }

    vfs::init();
    if ndevs > 0 {
        match vfs::mount_root(0, ONYXFS_LBA) {
            Ok(()) => crate::kinf!("vfs", "root mounted"),
            Err(e) => crate::kerr!("vfs", "mount failed: %s", Arg::from(e.as_str())),
        }
    }
    vfs::mount_procfs();
    crate::kinf!("vfs", "procfs mounted at /proc");
    vfs::mount_ipcfs();
    crate::kinf!("vfs", "ipcfs mounted at /ipc");

    // Load /font/default.psf
    (|| -> KResult<()> {
        let token = vfs::open(b"/font/default.psf", vfs::PERM_READ)?;
        let mut size = 0u32;
        vfs::stat(token, &mut size).ok();
        if size > 0 {
            let buf = heap::kmalloc(size as usize)?;
            vfs::read(token, buf, size).ok();
            vfs::close(token).ok();
            crate::font::init(core::slice::from_raw_parts(buf, size as usize)).ok();
            heap::kfree(buf);
            crate::kinf!("font", "loaded /font/default.psf");
        } else {
            vfs::close(token).ok();
        }
        Ok(())
    })()
    .unwrap_or_else(|_| crate::kwrn!("font", "no /font/default.psf, using blank font"));

    // Load /bin/init as PID 1 in root space (ring 1).
    let path = b"/bin/init";
    let token = match vfs::open(path, vfs::PERM_READ | vfs::PERM_SEEK) {
        Ok(t) => t,
        Err(e) => {
            crate::kerr!("kmain", "open /bin/init failed: %s", Arg::from(e.as_str()));
            crate::srv::klog::halt();
        }
    };
    let mut size = 0u32;
    vfs::stat(token, &mut size).ok();
    crate::kinf!("kmain", "/bin/init size=%d", Arg::from(size));

    let img = match heap::kmalloc(size as usize) {
        Ok(p) => p,
        Err(e) => {
            crate::kerr!("kmain", "kmalloc failed: %s", Arg::from(e.as_str()));
            crate::srv::klog::halt();
        }
    };
    vfs::read(token, img, size).ok();
    vfs::close(token).ok();

    let r = match crate::proc::onx::load(img, size as usize) {
        Ok(r) => r,
        Err(e) => {
            crate::kerr!("kmain", "onx_load failed: %s", Arg::from(e.as_str()));
            crate::srv::klog::halt();
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
    if let Err(e) = proc::create_user(
        r.entry,
        r.ustack,
        r.root_pa,
        proc::PROC_PID_INIT,
        0,
        r.heap_brk,
        ring,
        0,
        0,
    ) {
        crate::kerr!("kmain", "create_user failed: %s", Arg::from(e.as_str()));
        crate::srv::klog::halt();
    }

    csr::set_sstatus(SSTATUS_SIE);
    crate::kinf!(
        "proc",
        "entering user pid=1 entry=%p ring=%d",
        Arg::from(r.entry),
        Arg::from(ring as u32)
    );
    crate::arch::smp::release_secondary_harts();
    proc::enter_user(1);
}
