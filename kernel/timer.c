// SPDX-License-Identifier: GPL-3.0-or-later
/*
 * SlipperKernel — S-mode timer driven by the CLINT.
 *
 * We program mtimecmp directly via MMIO (no SBI calls). The CLINT lives
 * at 0x02000000 on QEMU virt.
 *
 * Tick rate: 100 Hz.
 */
#include "types.h"
#include "timer.h"
#include "klog.h"
#include "riscv.h"
#include "fdt.h"
#include "proc.h"

#define CLINT_TIMER_HZ   100

static volatile u64 *g_mtime    = NULL;
static volatile u64 *g_mtimecmp = NULL;
static u64  g_tick_interval     = 0;
static u64  g_uptime_ticks      = 0;
volatile u64 g_jiffies          = 0;
static paddr_t g_clint_base     = 0;

static inline u64 read_mtime(void)
{
    /* Guard against non-atomic 64-bit MMIO read by retrying on low/high
     * mismatch. On QEMU this is a single 64-bit load and always returns
     * cleanly, but real boards (SG2000) may return torn reads. */
    u32 hi, lo, hi2;
    do {
        hi  = (u32)(*g_mtime >> 32);
        lo  = (u32)(*g_mtime & 0xFFFFFFFF);
        hi2 = (u32)(*g_mtime >> 32);
    } while (hi != hi2);
    return ((u64)hi << 32) | lo;
}

static inline void write_mtimecmp(u64 v)
{
    /* Per spec: write high word to 0xFFFFFFFF, then low word, then high
     * word, to avoid spurious interrupts from a transient low value. */
    volatile u32 *p = (volatile u32 *)g_mtimecmp;
    p[1] = 0xFFFFFFFF;
    p[0] = (u32)v;
    p[1] = (u32)(v >> 32);
}

void timer_init(void)
{
    fdt_find_clint(&g_clint_base);
    g_mtime    = (volatile u64 *)(g_clint_base + 0xBFF8);
    g_mtimecmp = (volatile u64 *)(g_clint_base + 0x4000);   /* hart 0 */

    /* QEMU virt CLINT runs at 10 MHz. Compute interval for CLINT_TIMER_HZ. */
    u64 freq = 10 * 1000 * 1000;
    g_tick_interval = freq / CLINT_TIMER_HZ;

    /* First deadline. */
    write_mtimecmp(read_mtime() + g_tick_interval);

    /* Enable S-mode timer interrupt:
     *   sie.STIE  (S-mode interrupt-enable for S-timer)
     *   sip.STIP  is read-only set by CLINT.
     * We are in S-mode, so we use sie (not mie which is M-mode only).
     * Globally enabling sstatus.SIE is done in kmain before proc_enter_user. */
    csr_set(sie, 1U << 5);

    kinf("timer: CLINT @0x%lx, tick=%lu ns",
         g_clint_base, g_tick_interval * 1000ULL / (freq / 1000000));
}

void timer_handle(void)
{
    g_uptime_ticks++;
    g_jiffies++;
    /* Re-arm. */
    write_mtimecmp(read_mtime() + g_tick_interval);
    sched_tick();
}

u64 timer_now(void)
{
    return read_mtime();
}

u64 timer_uptime_us(void)
{
    /* CLINT @10MHz → 1 tick = 100 ns = 0.1 µs */
    return g_uptime_ticks * (1000 * 1000 / CLINT_TIMER_HZ);
}
