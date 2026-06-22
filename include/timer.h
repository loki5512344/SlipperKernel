/* SPDX-License-Identifier: GPL-3.0-or-later */
/*
 * SlipperKernel — timer (CLINT-driven, 100 Hz tick).
 */
#ifndef SLIPPER_TIMER_H
#define SLIPPER_TIMER_H

#include "types.h"

void timer_init(void);
void timer_handle(void);        /* called from trap_handler on S-timer interrupt */
u64  timer_uptime_us(void);
u64  timer_now(void);           /* raw mtime */

extern volatile u64 g_jiffies;

#endif
