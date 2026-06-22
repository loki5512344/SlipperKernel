// SPDX-License-Identifier: GPL-3.0-or-later
/*
 * SlipperKernel — PLIC (Platform-Level Interrupt Controller) driver.
 *
 * Register layout (RISC-V PLIC spec v1.0.0):
 *   Priority:    base + 4*irq            (per source)
 *   Pending:     base + 0x1000           (read-only, bit per irq)
 *   Enable:      base + 0x2000 + ctx*0x80  (bit per irq, per context)
 *   Threshold:   base + 0x200000 + ctx*0x1000
 *   Claim/Comp:  base + 0x200004 + ctx*0x1000
 *
 * Context mapping (single-hart MVP): context = hart.
 */
#include "types.h"
#include "plic.h"
#include "klog.h"

static uptr g_base = 0;

void plic_init(uptr base)
{
    g_base = base;
    kinf("plic: init @0x%lx", base);
}

void plic_set_priority(int irq, int prio)
{
    if (irq < 1 || irq > 1023) return;
    volatile u32 *r = (volatile u32 *)(g_base + (u32)irq * 4);
    *r = (u32)(prio & 7);
}

void plic_enable(int irq, int hart)
{
    if (irq < 1 || irq > 1023) return;
    u32 word_off = ((u32)irq / 32) * 4;
    u32 bit      = (u32)irq % 32;
    u32 ctx      = (u32)hart;
    volatile u32 *r = (volatile u32 *)(g_base + 0x2000 + ctx * 0x80 + word_off);
    *r |= (1u << bit);
}

void plic_set_threshold(int threshold)
{
    volatile u32 *r = (volatile u32 *)(g_base + 0x200000);
    *r = (u32)(threshold & 7);
}

int plic_claim(void)
{
    return (int)(*(volatile u32 *)(g_base + 0x200004));
}

void plic_complete(int irq)
{
    *(volatile u32 *)(g_base + 0x200004) = (u32)irq;
}
