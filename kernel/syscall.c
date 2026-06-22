// SPDX-License-Identifier: GPL-3.0-or-later
/*
 * SlipperKernel — syscall dispatcher.
 *
 * Convention:
 *   a7 = syscall number
 *   a0..a5 = args
 *   a0 = return value (negative = -SL_ERR_*)
 */
#include "types.h"
#include "syscall.h"
#include "trap.h"
#include "proc.h"
#include "vfs.h"
#include "uart.h"
#include "klog.h"
#include "riscv.h"
#include "vmm.h"
#include "pmm.h"

/* Validate that a pointer from user-space points into the user region.
 * We don't yet implement proper user-pinned buffers; we just sanity-check
 * the address range. Real check: PTE has U bit. */
static bool user_ptr_ok(const void *p, usize len)
{
    vaddr_t va = (vaddr_t)p;
    if (va < USER_BASE) return false;
    if (va + len > USER_TOP) return false;
    if (va + len < va) return false;        /* overflow */
    return true;
}

static long sys_write(long fd, const void *buf, usize len)
{
    if (fd != 1 && fd != 2) return SL_ERR_INVAL;
    if (!user_ptr_ok(buf, len)) return SL_ERR_PERM;
    /* For now, dump to UART directly. Later: route through VFS fd 1. */
    const u8 *p = (const u8 *)buf;
    for (usize i = 0; i < len; ++i) {
        if (p[i] == '\n') uart_putc(&g_uart, '\r');
        uart_putc(&g_uart, p[i]);
    }
    return (long)len;
}

static long sys_read(long fd, void *buf, usize len)
{
    if (fd != 0) return SL_ERR_INVAL;
    if (!user_ptr_ok(buf, len)) return SL_ERR_PERM;
    /* Line-discipline read from UART: echo, backspace, enter. */
    u8 *p = (u8 *)buf;
    usize got = 0;
    for (;;) {
        int c = uart_getc(&g_uart);
        if (c < 0) {
            if (got == 0) continue;
            break;
        }
        if (c == '\r' || c == '\n') {
            uart_putc(&g_uart, '\r');
            uart_putc(&g_uart, '\n');
            p[got] = '\n';
            got++;
            break;
        }
        if (c == 127 || c == '\b') {
            if (got > 0) {
                got--;
                uart_putc(&g_uart, '\b');
                uart_putc(&g_uart, ' ');
                uart_putc(&g_uart, '\b');
            }
            continue;
        }
        if (c < 32) continue;
        if (got >= len - 1) continue;
        uart_putc(&g_uart, (char)c);
        p[got] = (u8)c;
        got++;
    }
    return (long)got;
}

static long sys_exit(long code)
{
    proc_exit(proc_current()->pid, (int)code);
}

static long sys_yield(void)
{
    need_resched = 1;
    return 0;
}

static long sys_getpid(void)
{
    return (long)proc_current()->pid;
}

static long sys_open(const char *path, long flags)
{
    if (!user_ptr_ok(path, 1)) return SL_ERR_PERM;
    /* path is NUL-terminated user string in user region. */
    (void)flags;
    return (long)vfs_open(path);
}

static long sys_close(long fd)
{
    return (long)vfs_close((int)fd);
}

static long sys_lseek(long fd, long off, long whence)
{
    vfs_fd_t *f = vfs_get_fd((int)fd);
    if (!f) return SL_ERR_INVAL;
    if (whence == 0)      f->pos = (u32)off;
    else if (whence == 1) f->pos = (u32)((long)f->pos + off);
    else if (whence == 2) f->pos = f->size + (u32)off;
    else return SL_ERR_INVAL;
    return (long)f->pos;
}

static long sys_stat(const char *path, void *st)
{
    (void)st;
    if (!user_ptr_ok(path, 1)) return SL_ERR_PERM;
    int fd = vfs_open(path);
    if (fd < 0) return fd;
    vfs_fd_t *f = vfs_get_fd(fd);
    long sz = (long)f->size;
    vfs_close(fd);
    return sz;
}

void syscall_handler(trap_frame_t *f)
{
    long nr = (long)f->a7;
    long a0 = (long)f->a0;
    long a1 = (long)f->a1;
    long a2 = (long)f->a2;
    long a3 = (long)f->a3;

    long ret;
    switch (nr) {
    case SYS_write:    ret = sys_write(a0, (const void *)a1, (usize)a2); break;
    case SYS_read:     ret = sys_read(a0, (void *)a1, (usize)a2); break;
    case SYS_exit:     ret = sys_exit(a0); break;
    case SYS_yield:    ret = sys_yield(); break;
    case SYS_getpid:   ret = sys_getpid(); break;
    case SYS_open:     ret = sys_open((const char *)a0, a1); break;
    case SYS_close:    ret = sys_close(a0); break;
    case SYS_lseek:    ret = sys_lseek(a0, a1, a2); break;
    case SYS_stat:     ret = sys_stat((const char *)a0, (void *)a1); break;
    case SYS_brk:
    case SYS_mmap:
        /* Not implemented in MVP. */
        ret = SL_ERR_NOSYS;
        break;
    default:
        kwrn("syscall: unknown nr=%ld", nr);
        ret = SL_ERR_NOSYS;
        break;
    }

    f->a0 = (u64)ret;
}
