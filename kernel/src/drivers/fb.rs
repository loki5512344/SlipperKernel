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

/// Draw a Unicode character at `(x, y)` using the font's actual dimensions.
///
/// Supports PSF2 fonts with variable charsize. Uses the Unicode table to
/// look up the glyph index for the given codepoint; falls back to '?' if
/// the codepoint is not mapped.
pub fn draw_unicode_char(x: usize, y: usize, cp: u32, fg: u32, bg: u32) {
    let gd = font::glyph_bitmap_unicode(cp);
    let fh = gd.height as usize;
    let fw = gd.width as usize;
    let bytes_per_row = (fw + 7) / 8;
    for row in 0..fh {
        let row_off = row * bytes_per_row;
        for col in 0..fw {
            let byte_idx = col / 8;
            let bit_idx = 7 - (col % 8);
            let bits = unsafe {
                let off = row_off + byte_idx;
                if off < gd.charsize as usize {
                    *gd.data.add(off)
                } else {
                    0
                }
            };
            let on = (bits >> bit_idx) & 1;
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

/// Draw a UTF-8 string at `(x, y)` using Unicode-aware glyph lookup.
///
/// Unlike `draw_str`, this function decodes UTF-8 sequences and uses the
/// font's Unicode table to render non-ASCII characters (e.g. Cyrillic).
pub fn draw_unicode_str(mut x: usize, y: usize, s: &str, fg: u32, bg: u32) {
    let fw = font::font_width();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\n' => return,
            b'\r' => { x = 0; i += 1; continue; }
            b'\t' => { x = (x / (4 * fw) + 1) * (4 * fw); i += 1; continue; }
            _ => {}
        }
        // Decode UTF-8 codepoint
        let cp;
        if b < 0x80 {
            cp = b as u32;
            i += 1;
        } else if b < 0xE0 {
            if i + 1 >= bytes.len() { break; }
            cp = ((b & 0x1F) as u32) << 6 | ((bytes[i + 1] & 0x3F) as u32);
            i += 2;
        } else if b < 0xF0 {
            if i + 2 >= bytes.len() { break; }
            cp = ((b & 0x0F) as u32) << 12
                | ((bytes[i + 1] & 0x3F) as u32) << 6
                | ((bytes[i + 2] & 0x3F) as u32);
            i += 3;
        } else {
            if i + 3 >= bytes.len() { break; }
            cp = ((b & 0x07) as u32) << 18
                | ((bytes[i + 1] & 0x3F) as u32) << 12
                | ((bytes[i + 2] & 0x3F) as u32) << 6
                | ((bytes[i + 3] & 0x3F) as u32);
            i += 4;
        }
        if x + fw > FB_WIDTH {
            return;
        }
        draw_unicode_char(x, y, cp, fg, bg);
        x += fw;
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
