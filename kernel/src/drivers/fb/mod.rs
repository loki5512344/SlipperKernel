use onyx_core::errno::KResult;

pub const FB_WIDTH: usize = 1280;
pub const FB_HEIGHT: usize = 720;
pub const FB_BPP: usize = 32;
pub const FB_PITCH: usize = FB_WIDTH * (FB_BPP / 8);
pub const FB_SIZE: usize = FB_HEIGHT * FB_PITCH;
pub(crate) const COL_BLACK: u32 = 0x000000;
pub(crate) const COL_GREEN: u32 = 0x00FF00;

static mut G_FB: Fb = Fb {
    base: core::ptr::null_mut(),
    width: FB_WIDTH,
    height: FB_HEIGHT,
    pitch: FB_PITCH,
    bpp: FB_BPP,
    enabled: false,
};

#[derive(Clone, Copy)]
pub struct Fb {
    base: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    bpp: usize,
    enabled: bool,
}

pub fn enabled() -> bool {
    unsafe { G_FB.enabled }
}

pub unsafe fn init(paddr: usize) -> KResult<()> {
    G_FB = Fb {
        base: paddr as *mut u8,
        width: FB_WIDTH,
        height: FB_HEIGHT,
        pitch: FB_PITCH,
        bpp: FB_BPP,
        enabled: true,
    };
    clear();
    Ok(())
}

pub fn clear() {
    unsafe {
        if !G_FB.enabled {
            return;
        }
        let base = G_FB.base;
        let size = G_FB.pitch * G_FB.height;
        for i in 0..size {
            *base.add(i) = 0;
        }
    }
}

fn put_pixel(x: usize, y: usize, color: u32) {
    unsafe {
        if !G_FB.enabled || x >= G_FB.width || y >= G_FB.height {
            return;
        }
        let off = y * G_FB.pitch + x * (G_FB.bpp / 8);
        let base = G_FB.base;
        *base.add(off) = (color & 0xFF) as u8;
        *base.add(off + 1) = ((color >> 8) & 0xFF) as u8;
        *base.add(off + 2) = ((color >> 16) & 0xFF) as u8;
    }
}

pub mod draw;
pub mod scroll;
pub use draw::*;
pub use scroll::*;
