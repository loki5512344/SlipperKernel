/* SPDX-License-Identifier: GPL-3.0-or-later */
/*
 * SlipperKernel — SlipperExec (.spx) binary format and loader.
 *
 * Magic: 'SPX1' at offset 0.
 *
 *   struct spx_header {
 *     u32 magic;          // 'SPX1'
 *     u32 version;        // 1
 *     u64 entry;          // virtual entry address
 *     u32 nsegs;
 *     u32 flags;          // bit0: signed (future), bit1: ring1 (root-space)
 *     spx_segment_t segs[8];
 *   };
 *   struct spx_segment {
 *     u64 vaddr;
 *     u64 filesz;
 *     u64 memsz;
 *     u32 offset;         // into file
 *     u32 flags;          // VMM_R/W/X
 *     u32 align;
 *     u32 reserved;
 *   };
 *
 * No relocations, no dynamic linking. Segments are loaded verbatim.
 */
#ifndef SLIPPER_SPX_H
#define SLIPPER_SPX_H

#include "types.h"

#define SPX_MAGIC    0x31585053   /* 'SPX1' little-endian */
#define SPX_VERSION  1
#define SPX_MAX_SEGS 8

typedef struct {
    u64 vaddr;
    u64 filesz;
    u64 memsz;
    u32 offset;
    u32 flags;
    u32 align;
    u32 reserved;
} spx_segment_t;

typedef struct {
    u32 magic;
    u32 version;
    u64 entry;
    u32 nsegs;
    u32 flags;            /* bit1 = ring1 binary (root-space) */
    spx_segment_t segs[SPX_MAX_SEGS];
} spx_header_t;

_Static_assert(sizeof(spx_segment_t) == 40, "spx_segment size");
_Static_assert(sizeof(spx_header_t) == 344, "spx_header size");

/* Loads an .spx image from kernel memory into a fresh user address space.
 * Allocates root page table, maps all segments, allocates user stack + heap.
 * Returns 0 / negative error. On success fills out_entry, out_root_pa,
 * out_ustack, and out_heap_brk. */
int spx_load(const void *image, usize image_size,
             vaddr_t *out_entry, paddr_t *out_root_pa, vaddr_t *out_ustack,
             vaddr_t *out_heap_brk);

#endif
