// SPDX-License-Identifier: GPL-3.0-or-later
/*
 * SlipperKernel — kernel main entry point (called from boot.S after mret).
 *
 * Receives:
 *   a0 = hartid (always 0 on QEMU virt uniprocessor MVP)
 *   a1 = pointer to FDT in DRAM
 */
#include "types.h"
#include "klog.h"
#include "riscv.h"
#include "fdt.h"
#include "uart.h"
#include "pmm.h"
#include "vmm.h"
#include "heap.h"
#include "trap.h"
#include "timer.h"
#include "virtio.h"
#include "slipperfs.h"
#include "vfs.h"
#include "spx.h"
#include "proc.h"
#include "syscall.h"
#include "plic.h"

extern char __bss_start[];
extern char __bss_end[];
extern char __kernel_end[];
extern char __stack_top[];

/* LBA sector where SlipperFS partition starts on the virtio block device.
 * Must match the disk image layout (see scripts/mkdisk.py). */
#define SLIPPERFS_LBA 10240

static void banner(void)
{
    klog_puts("\n");
    klog_puts("░░░█▀▀░█░░░▀█▀░█▀█░█▀█░█▀▀░█▀▄░█░█░█▀▀░█▀▄░█▀█░█▀▀░█░░\n");
    klog_puts("░░░▀▀█░█░░░░█░░█▀▀░█▀▀░█▀▀░█▀▄░█▀▄░█▀▀░█▀▄░█░█░█▀▀░█░░\n");
    klog_puts("░░░▀▀▀░▀▀▀░▀▀▀░▀░░░▀░░░▀▀▀░▀░▀░▀░▀░▀▀▀░▀░▀░▀░▀░▀▀▀░▀▀▀\n");
    klog_puts("        SlipperKernel v0.1\n\n");
}

static void print_fastfetch(const char *model, u64 mem_size,
                             u64 pages_total, u64 pages_free)
{
    klog_puts("\n");
    klog_puts("         ⠴⠋⠉⠙⠦\n");
    klog_puts("        ⠾     ⠷\n");
    klog_puts("        ⣿⠷⠷⠷⠾⣿\n");
    klog_puts("        ⠙⠦   ⠴⠋\n");
    klog_puts("          ⠴⠷\n");
    klog_puts("         ⠴⠁ ⠳\n");
    klog_puts("        ⠰⠁   ⠁⠦\n");
    klog_puts("        ⠾⠳  ⠻⠟ ⠈⠦\n");
    klog_puts("        ⣿ ⠳      ⠳\n");
    klog_puts("        ⠻        ⣿\n");
    klog_puts("        ⠘⠴       ⣿\n");
    klog_puts("         ⠻       ⣿\n");
    klog_puts("         ⠾       ⣿\n");
    klog_puts("        ⠰⠋       ⠙⠦\n");
    klog_puts("        ⠾          ⠈⠙⠓⠦\n");
    klog_puts("       ⠰⠋              ⠙⠓⠦\n");
    klog_puts("       ⠸                  ⠙⠓⠦\n");
    klog_puts("       ⠸                    ⠙⠦\n");
    klog_puts("        ⠻                    ⠙⠷\n");
    klog_puts("         ⠻                    ⣿⠔⠈⠈⠈\n");
    klog_puts("         ⠌⠻  ⠸     ⠎          ⠟\n");
    klog_puts("    ⠄⠂⠁⠈⠁ ⠘     ⠅⠋⠉⠉⠉⠙⠉⠉⠁  ⠈⠑⠠\n");
    klog_puts("  ⠴⠮--⠄⠠⠄⠈⠁   ⠣    ⠣          ⠈⠠⠄⠁\n");
    klog_puts("               ⠑⠄   ⠜\n");
    klog_puts("                 ⠈⠠⠄⠁\n");
    klog_puts("\n");

    kinf("OS:      SlipperOS 0.1 (%s)", model);
    kinf("Kernel:  SlipperKernel 0.1 (rv64gc, Sv39)");
    kinf("Uptime:  just booted");
    kinf("Memory:  %u MB total, %u pages free",
         (u32)(mem_size / MB(1)), (u32)pages_free);
}

void kmain(u64 hartid, u64 fdt_addr)
{
    /* 1) FDT first — we need UART base/shift from it. */
    fdt_init((const void *)fdt_addr);

    fdt_mmio_t uart_mmio;
    if (fdt_find_uart(&uart_mmio) > 0) {
        uart_init(&g_uart, uart_mmio.base, uart_mmio.reg_shift);
    } else {
        /* Fall back to QEMU defaults. */
        uart_init(&g_uart, 0x10000000, 0);
    }

    banner();
    kinf("kmain: hartid=%lu fdt=0x%lx", hartid, fdt_addr);

    const char *model = fdt_model("unknown");
    kinf("platform: %s", model);

    /* 2) Memory: PMM, VMM, heap. */
    fdt_memory_t mem;
    if (!fdt_memory(&mem)) {
        kpanic("kmain: cannot find /memory in FDT");
    }
    pmm_init(mem.base, mem.size);

    /* Build kernel page tables and enable paging. Until now we were in
     * S-mode with satp=BARE (physical addressing). After this point all
     * addresses go through Sv39 — kernel is identity-mapped, so behaviour
     * is unchanged. */
    vmm_init();

    heap_init();

    /* 3) Traps and syscalls. */
    trap_init();

    /* 4) Timer (CLINT). */
    timer_init();

    /* 4b) PLIC — external interrupt controller. */
    {
        paddr_t plic_base;
        fdt_find_plic(&plic_base);
        plic_init(plic_base);
        plic_set_priority(10, 1);   /* UART0              */
        plic_set_priority(1, 1);    /* VirtIO (first)     */
        plic_enable(10, 0);         /* UART0              */
        plic_enable(1, 0);          /* VirtIO (first)     */
        plic_set_threshold(0);
    }

    /* Enable supervisor external interrupts. */
    csr_set(sie, (1 << 1) | (1 << 9));

    /* 5) Devices: virtio-blk. */
    fdt_mmio_t vdevs[4];
    int nv = fdt_find_virtio(vdevs, ARR_LEN(vdevs));
    kinf("fdt: %d virtio,mmio node(s)", nv);
    kinf("fdt: %d virtio,mmio node(s)", nv);
    int root_dev = -1;
    for (int i = 0; i < nv; ++i) {
        kinf("kmain: probing virtio @0x%lx", vdevs[i].base);
        int idx = virtio_blk_init(vdevs[i].base);
        if (idx >= 0 && root_dev < 0) root_dev = idx;
    }
    if (root_dev < 0) {
        kpanic("kmain: no virtio-blk device found");
    }
    kinf("kmain: root_dev=%d", root_dev);

    /* 6) VFS + SlipperFS. */
    vfs_init();
    kinf("kmain: mounting root...");
    int rc = vfs_mount_root(root_dev, SLIPPERFS_LBA);
    if (rc) {
        kpanic("kmain: cannot mount root: %d", rc);
    }
    kinf("kmain: root mounted");

    /* 7) Load /bin/init. */
    int fd = vfs_open("/bin/init");
    if (fd < 0) {
        kpanic("kmain: /bin/init not found: %d", fd);
    }
    u32 init_size = 0;
    vfs_stat(fd, &init_size);
    kinf("kmain: /bin/init size=%u", init_size);
    if (init_size == 0 || init_size > MB(2)) {
        kpanic("kmain: /bin/init size invalid");
    }
    void *img = kmalloc(init_size);
    if (!img) kpanic("kmain: OOM loading /bin/init");
    int n = vfs_read(fd, img, init_size);
    vfs_close(fd);
    if (n != (int)init_size) {
        kpanic("kmain: short read of /bin/init: %d/%u", n, init_size);
    }

    /* 8) Parse and map as SlipperExec. */
    vaddr_t entry;
    paddr_t user_root;
    vaddr_t ustack;
    rc = spx_load(img, init_size, &entry, &user_root, &ustack);
    if (rc) kpanic("kmain: spx_load failed: %d", rc);

    /* 9) Create processes. */
    proc_init();
    rc = proc_create_user(entry, ustack, user_root, PROC_PID_INIT);
    if (rc) kpanic("kmain: proc_create_user pid 1: %d", rc);

    /* Create a second process with the same binary to test scheduling. */
    {
        vaddr_t entry2;
        paddr_t user_root2;
        vaddr_t ustack2;
        rc = spx_load(img, init_size, &entry2, &user_root2, &ustack2);
        if (rc) kpanic("kmain: spx_load pid 2: %d", rc);
        rc = proc_create_user(entry2, ustack2, user_root2, 2);
        if (rc) kpanic("kmain: proc_create_user pid 2: %d", rc);
    }

    /* Enable S-mode interrupts (for timer + syscalls). */
    csr_set(sstatus, SSTATUS_SIE);

    /* Print fastfetch summary before entering user mode */
    {
        fdt_memory_t mem;
        fdt_memory(&mem);
        print_fastfetch(fdt_model("unknown"), mem.size,
                        pmm_total_pages(), pmm_free_pages());
    }

    proc_enter_user(PROC_PID_INIT);

    /* never reached */
    kpanic("kmain: proc_enter_user returned");
}
