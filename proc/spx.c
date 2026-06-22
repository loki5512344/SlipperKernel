#include "types.h"
#include "spx.h"
#include "vmm.h"
#include "pmm.h"
#include "heap.h"
#include "klog.h"
#include "riscv.h"

#define USER_STACK_PAGES  16
#define USER_HEAP_PAGES   16     /* 64 KB heap */
#define USER_HEAP_BASE    0x3FF00000UL

int spx_load(const void *image, usize image_size,
             vaddr_t *out_entry, paddr_t *out_root_pa, vaddr_t *out_ustack,
             vaddr_t *out_heap_brk)
{
    if (image_size < sizeof(spx_header_t)) return SL_ERR_INVAL;
    const spx_header_t *h = (const spx_header_t *)image;
    if (h->magic != SPX_MAGIC) {
        kerr("spx: bad magic 0x%x", h->magic);
        return SL_ERR_INVAL;
    }
    if (h->version != SPX_VERSION) {
        kerr("spx: unsupported version %u", h->version);
        return SL_ERR_INVAL;
    }
    if (h->nsegs == 0 || h->nsegs > SPX_MAX_SEGS) {
        kerr("spx: bad nsegs %u", h->nsegs);
        return SL_ERR_INVAL;
    }

    paddr_t root = vmm_new_root();
    if (!root) return SL_ERR_NOMEM;

    u32 rwx = VMM_R | VMM_W | VMM_X;
    u64 pte_flags = PTE_V | rwx | PTE_A | PTE_D;
    u64 *urt = (u64 *)root;
    urt[0] = ((0UL >> 12) << PTE_PPN_SHIFT) | pte_flags;
    urt[1] = ((0x40000000UL >> 12) << PTE_PPN_SHIFT) | pte_flags;
    urt[2] = ((0x80000000UL >> 12) << PTE_PPN_SHIFT) | pte_flags;

    for (u32 i = 0; i < h->nsegs; ++i) {
        const spx_segment_t *s = &h->segs[i];
        if (s->vaddr < USER_BASE || s->vaddr >= USER_TOP) {
            kerr("spx: seg %u vaddr 0x%lx out of user region", i, s->vaddr);
            vmm_destroy_root(root);
            return SL_ERR_RANGE;
        }
        usize filesz = (usize)s->filesz;
        usize memsz  = (usize)s->memsz;
        if (filesz > memsz) { vmm_destroy_root(root); return SL_ERR_INVAL; }
        if (s->offset + filesz > image_size) {
            kerr("spx: seg %u extends past image end", i);
            vmm_destroy_root(root);
            return SL_ERR_INVAL;
        }
        usize map_sz = PAGE_ALIGN_UP(memsz);
        vaddr_t vstart = PAGE_ALIGN_DOWN(s->vaddr);
        usize npages = map_sz / PAGE_SIZE;
        for (usize p = 0; p < npages; ++p) {
            paddr_t pa = pmm_alloc_zero();
            if (!pa) { vmm_destroy_root(root); return SL_ERR_NOMEM; }
            int rc = vmm_map((u64 *)root, vstart + p * PAGE_SIZE, pa,
                             PAGE_SIZE, s->flags | VMM_U);
            if (rc) { vmm_destroy_root(root); return rc; }
            u64 seg_off = s->vaddr + (u64)p * PAGE_SIZE;
            u64 file_off = s->offset + (seg_off - s->vaddr);
            if (seg_off < s->vaddr + filesz) {
                usize chunk = MIN(PAGE_SIZE, (usize)(s->vaddr + filesz - seg_off));
                const u8 *src = (const u8 *)image + file_off;
                u8 *dst = (u8 *)pa;
                for (usize k = 0; k < chunk; ++k) dst[k] = src[k];
            }
        }
        kinf("spx: seg %u va=0x%lx fsz=%lu msz=%lu fl=0x%x",
             i, s->vaddr, filesz, memsz, s->flags);
    }

    /* User stack at top of user space, grows down. */
    vaddr_t ustack_top = USER_TOP - PAGE_SIZE;
    for (usize i = 0; i < USER_STACK_PAGES; ++i) {
        paddr_t pa = pmm_alloc_zero();
        if (!pa) { vmm_destroy_root(root); return SL_ERR_NOMEM; }
        int rc = vmm_map((u64 *)root, ustack_top - i * PAGE_SIZE, pa,
                         PAGE_SIZE, VMM_R | VMM_W | VMM_U);
        if (rc) { vmm_destroy_root(root); return rc; }
    }
    vaddr_t ustack = ustack_top - 16;

    /* Pre-allocated 64KB heap at USER_HEAP_BASE, grows up. */
    for (usize i = 0; i < USER_HEAP_PAGES; ++i) {
        paddr_t pa = pmm_alloc_zero();
        if (!pa) { vmm_destroy_root(root); return SL_ERR_NOMEM; }
        int rc = vmm_map((u64 *)root, USER_HEAP_BASE + i * PAGE_SIZE, pa,
                         PAGE_SIZE, VMM_R | VMM_W | VMM_U);
        if (rc) { vmm_destroy_root(root); return rc; }
    }

    *out_entry    = (vaddr_t)h->entry;
    *out_root_pa  = root;
    *out_ustack   = ustack;
    *out_heap_brk = USER_HEAP_BASE;
    kinf("spx: entry=0x%lx root=0x%lx ustack=0x%lx heap=0x%lx",
         *out_entry, root, ustack, USER_HEAP_BASE);
    return 0;
}
