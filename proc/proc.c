// SPDX-License-Identifier: GPL-3.0-or-later
/*
 * SlipperKernel — process / task management and round-robin scheduler.
 */
#include "types.h"
#include "proc.h"
#include "trap.h"
#include "vmm.h"
#include "klog.h"
#include "riscv.h"
#include "vfs.h"

static proc_t g_procs[4];
static proc_t *g_current = NULL;
static u32 g_next_pid = 1;

volatile int need_resched = 0;

static void sched_switch(usize new_sp) __attribute__((noreturn));

int proc_init(void)
{
    for (int i = 0; i < (int)ARR_LEN(g_procs); ++i) {
        g_procs[i].state = PROC_STATE_FREE;
        g_procs[i].pid = 0;
    }
    g_next_pid = PROC_PID_INIT;
    need_resched = 0;
    return 0;
}

proc_t *proc_current(void) { return g_current; }

proc_t *proc_by_pid(u32 pid)
{
    for (int i = 0; i < (int)ARR_LEN(g_procs); ++i)
        if (g_procs[i].pid == pid) return &g_procs[i];
    return NULL;
}

int proc_create_user(vaddr_t entry, vaddr_t ustack, paddr_t root_pa, u32 pid)
{
    proc_t *p = NULL;
    for (int i = 0; i < (int)ARR_LEN(g_procs); ++i) {
        if (g_procs[i].state == PROC_STATE_FREE) { p = &g_procs[i]; break; }
    }
    if (!p) return SL_ERR_NOMEM;

    u8 *tfb = (u8 *)&p->tf;
    for (usize i = 0; i < sizeof(trap_frame_t); ++i) tfb[i] = 0;

    p->pid     = pid;
    p->ring    = PROC_RING_USER;
    p->state   = PROC_STATE_READY;
    p->root_pa = root_pa;
    p->entry   = entry;
    p->ustack  = ustack;
    p->tf.sepc = entry;
    p->tf.sp   = ustack;
    p->tf.a0   = 0;
    p->tf.a1   = ustack - 256;
    p->tf.sstatus = SSTATUS_SPIE;
    p->tf.satp    = SATP_MODE_SV39 | ((u64)root_pa >> PAGE_SHIFT);

    return 0;
}

__attribute__((noreturn))
void proc_enter_user(u32 pid)
{
    proc_t *p = proc_by_pid(pid);
    if (!p) kpanic("proc_enter_user: no such pid %u", pid);
    g_current = p;

    kinf("proc: entering user pid=%u entry=0x%lx", p->pid, p->entry);

    vaddr_t entry   = p->entry;
    vaddr_t ustack  = p->ustack;
    paddr_t root_pa = p->root_pa;
    usize kstack_top = (usize)&p->kstack + sizeof(p->kstack);
    kstack_top &= ~15UL;

    asm volatile("mv sp, %0" : : "r"(kstack_top) : "memory");

    drop_to_user(entry, ustack, root_pa);
    kpanic("proc_enter_user: drop_to_user returned");
}

void proc_exit(u32 pid, int code)
{
    proc_t *p = proc_by_pid(pid);
    if (p) {
        kinf("proc: pid %u exited with code %d", pid, code);
        if (p->root_pa) {
            vmm_destroy_root(p->root_pa);
            p->root_pa = 0;
        }
        p->state = PROC_STATE_EXITED;
    } else {
        kerr("proc: exit unknown pid %u", pid);
    }
}

void sched_tick(void)
{
    if (g_current)
        need_resched = 1;
}

void sched_yield(trap_frame_t *f)
{
    if (!g_current)
        return;

    g_current->tf = *f;

    if (g_current->state == PROC_STATE_RUNNING)
        g_current->state = PROC_STATE_READY;

    proc_t *next = NULL;
    int start = (int)(g_current - g_procs + 1) % (int)ARR_LEN(g_procs);
    for (int i = 0; i < (int)ARR_LEN(g_procs); ++i) {
        int idx = (start + i) % (int)ARR_LEN(g_procs);
        if (g_procs[idx].state == PROC_STATE_READY) {
            next = &g_procs[idx];
            break;
        }
    }

    if (!next) {
        if (g_current->state == PROC_STATE_EXITED) {
            kinf("sched: no more processes, halting");
            khalt();
        }
        g_current->state = PROC_STATE_RUNNING;
        return;
    }

    next->state = PROC_STATE_RUNNING;
    g_current = next;

    usize kstack_top = (usize)&next->kstack + sizeof(next->kstack);
    kstack_top &= ~15UL;
    usize new_sp = kstack_top - sizeof(trap_frame_t);

    *(trap_frame_t *)new_sp = next->tf;

    sched_switch(new_sp);
}

static void sched_switch(usize new_sp)
{
    asm volatile("mv sp, %0" : : "r"(new_sp) : "memory");
    extern void trap_return(void);
    asm volatile("j trap_return");
    __builtin_unreachable();
}
