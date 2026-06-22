/* SPDX-License-Identifier: GPL-3.0-or-later */
/*
 * SlipperKernel — syscall numbers (NOT POSIX, our own ABI).
 *
 * Calling convention:
 *   a7 = syscall number
 *   a0..a5 = arguments
 *   a0 = return value (negative = -SL_ERR_*)
 *
 * Returns to caller via sret. Does NOT follow Linux errno convention.
 */
#ifndef SLIPPER_SYSCALL_H
#define SLIPPER_SYSCALL_H

#include "types.h"
#include "trap.h"

#define SYS_write    1
#define SYS_read     2
#define SYS_exit     3
#define SYS_yield    4
#define SYS_getpid   5
#define SYS_brk      6
#define SYS_mmap     7
#define SYS_open     8
#define SYS_close    9
#define SYS_lseek    10
#define SYS_stat     11
#define SYS_exec     12
#define SYS_sbrk     13
#define SYS_nosys    0xFFFFFFFFUL

void syscall_handler(trap_frame_t *f);

#endif
