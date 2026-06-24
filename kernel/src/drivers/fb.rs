//! Framebuffer — linear 32bpp pixel buffer with PSF font text output.
//!
//! Manages a physical framebuffer (allocated from PMM, identity-mapped).
//! Provides pixel-level drawing (`put_pixel`, `draw_char`, `draw_str`) and
//! screen management (`clear`, `scroll`). Terminal emulation lives in
//! `fb_term.rs` — see `writer()`, `write_str()`, `write_char()`.
use crate::font;
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

pub fn draw_char(x: usize, y: usize, c: u8, fg: u32, bg: u32) {
    let glyph = font::glyph_bitmap(c);
    for row in 0..font::FONT_H {
        let bits = glyph[row];
        for col in 0..font::FONT_W {
            let on = (bits >> (7 - col)) & 1;
            put_pixel(x + col, y + row, if on != 0 { fg } else { bg });
        }
    }
}

pub fn draw_str(mut x: usize, y: usize, s: &str, fg: u32, bg: u32) {
    for &b in s.as_bytes() {
        match b {
            b'\n' => return,
            b'\r' => x = 0,
            b'\t' => x = (x / (4 * font::FONT_W) + 1) * (4 * font::FONT_W),
            _ => {
                if x + font::FONT_W > FB_WIDTH {
                    return;
                }
                draw_char(x, y, b, fg, bg);
                x += font::FONT_W;
            }
        }
    }
}

pub fn scroll() {
    unsafe {
        if !G_FB.enabled {
            return;
        }
        let base = G_FB.base;
        let row_bytes = font::FONT_H * G_FB.pitch;
        let total = G_FB.height * G_FB.pitch;
        for i in 0..(total - row_bytes) {
            *base.add(i) = *base.add(i + row_bytes);
        }
        for i in (total - row_bytes)..total {
            *base.add(i) = 0;
        }
    }
}

pub fn flush() {
    // No-op for memory framebuffer.
    // Real display devices (PCI VGA, etc.) will override this.
}
