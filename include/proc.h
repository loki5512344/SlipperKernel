/* SPDX-License-Identifier: GPL-3.0-or-later */
/*
 * SlipperKernel — process / task descriptor and scheduler.
 */
#ifndef SLIPPER_PROC_H
#define SLIPPER_PROC_H

#include "types.h"
#include "trap.h"

#define PROC_RING_KERNEL 0
#define PROC_RING_ROOT   1
#define PROC_RING_USER   2

#define PROC_PID_INIT    1

#define PROC_STATE_FREE    0
#define PROC_STATE_READY   1
#define PROC_STATE_RUNNING 2
#define PROC_STATE_EXITED  3

typedef struct {
    u32           pid;
    u8            ring;
    u8            state;
    u8            pad[2];
    paddr_t       root_pa;      /* root of user page table (satp value) */
    vaddr_t       entry;
    vaddr_t       ustack;
    trap_frame_t  tf;           /* saved across traps */
    u8            kstack[16 * 1024] __attribute__((aligned(16)));
} proc_t;

extern volatile int  need_resched;

int  proc_init(void);
int  proc_create_user(vaddr_t entry, vaddr_t ustack, paddr_t root_pa, u32 pid);
proc_t *proc_current(void);
proc_t *proc_by_pid(u32 pid);

__attribute__((noreturn))
void proc_enter_user(u32 pid);

void proc_exit(u32 pid, int code);

void sched_tick(void);
void sched_yield(trap_frame_t *f);

#endif
