/* SPDX-License-Identifier: GPL-3.0-or-later */
#ifndef SLIPPER_PLIC_H
#define SLIPPER_PLIC_H
#include "types.h"
void plic_init(uptr base);
void plic_enable(int irq, int hart);
void plic_set_priority(int irq, int prio);
void plic_set_threshold(int threshold);
int plic_claim(void);
void plic_complete(int irq);
#endif
