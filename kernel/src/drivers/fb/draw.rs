use crate::font;
use super::put_pixel;
use super::FB_WIDTH;

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
