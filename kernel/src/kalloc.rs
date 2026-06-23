//! Global allocator for kernel — bridges alloc crate to heap::kmalloc/kfree.

use crate::mm::heap;
use core::alloc::{GlobalAlloc, Layout};

struct KernelAlloc;

unsafe impl GlobalAlloc for KernelAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        if align <= 16 {
            match heap::kmalloc(size) {
                Ok(p) => p,
                Err(_) => core::ptr::null_mut(),
            }
        } else {
            // For larger alignments, over-allocate and adjust.
            let total = size + align;
            match heap::kmalloc(total) {
                Ok(p) => {
                    let addr = p as usize;
                    let aligned = (addr + align - 1) & !(align - 1);
                    aligned as *mut u8
                }
                Err(_) => core::ptr::null_mut(),
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        heap::kfree(ptr);
    }
}

#[global_allocator]
static ALLOCATOR: KernelAlloc = KernelAlloc;
