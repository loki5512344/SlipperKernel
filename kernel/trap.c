// SPDX-License-Identifier: GPL-3.0-or-later
/*
 * SlipperKernel — trap dispatch.
 *
 * scause (top bit = interrupt):
 *   0x8000000000000005  S-mode timer interrupt  -> timer_handle()
 *   0x8000000000000009  S-mode external         -> plic_handle() (TODO)
 *   8                   ecall from U-mode       -> syscall_handler()
 *   2,3,5,7,12,13,15    fault                   -> vmm fault / panic
 */
#include "types.h"
#include "trap.h"
#include "klog.h"
#include "riscv.h"
#include "syscall.h"
#include "timer.h"
#include "vmm.h"
#include "proc.h"
#include "plic.h"

void trap_init(void)
{
    /* stvec = direct mode: | mode(2) | base(>>2) | */
    usize vec = (usize)&trap_entry;
    csr_write(stvec, vec);
    kinf("trap: stvec=0x%lx", vec);
}

void trap_handler(trap_frame_t *f)
{
    u64 cause = csr_read(scause);
    u64 is_int = cause & SCAUSE_INT;
    u64 code   = cause & SCAUSE_CODE_MASK;

    if (is_int) {
        switch (code) {
        case 5: /* S-timer */
            timer_handle();
            break;
        case 9: /* S-external (PLIC) */
            {
                int irq = plic_claim();
                if (irq) {
                    kinf("trap: PLIC IRQ %d", irq);
                    plic_complete(irq);
                }
            }
            break;
        case 1: /* S-soft */
            kwrn("trap: soft IRQ (unhandled)");
            break;
        default:
            kwrn("trap: unknown IRQ scause=0x%lx", cause);
            break;
        }
    } else {
        switch (code) {
        case CAUSE_U_ECALL:
            syscall_handler(f);
            f->sepc += 4;
            break;

        case CAUSE_INST_PF:
        case CAUSE_LD_PF:
        case CAUSE_ST_PF:
        case CAUSE_IAMISS:
        case CAUSE_LDAMISS:
        case CAUSE_STAMISS:
            kerr("trap: page fault sepc=0x%lx stval=0x%lx code=%lu",
                 f->sepc, csr_read(stval), code);
            proc_exit(proc_current()->pid, (int)(code + 100));
            break;

        case CAUSE_ILL:
            kerr("trap: illegal instruction sepc=0x%lx stval=0x%lx",
                 f->sepc, csr_read(stval));
            proc_exit(proc_current()->pid, 132);
            break;

        case CAUSE_BRK:
            kerr("trap: breakpoint sepc=0x%lx", f->sepc);
            proc_exit(proc_current()->pid, 133);
            break;

        default:
            kpanic("trap: unhandled scause=0x%lx sepc=0x%lx stval=0x%lx",
                   cause, f->sepc, csr_read(stval));
        }
    }

    /* Post-dispatch: reschedule if needed. */
    if (proc_current()) {
        if (unlikely(proc_current()->state == PROC_STATE_EXITED)) {
            sched_yield(f);
            khalt();
        }
        if (need_resched) {
            need_resched = 0;
            sched_yield(f);
        }
    }
}
