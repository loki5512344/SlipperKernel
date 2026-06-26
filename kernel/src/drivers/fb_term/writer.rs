use crate::drivers::fb;
use crate::font;

pub struct FbWriter {
    pub col: usize,
    pub row: usize,
    pub fg: u32,
    pub bg: u32,
}

impl FbWriter {
    pub const fn new() -> Self {
        Self {
            col: 0,
            row: 0,
            fg: fb::COL_GREEN,
            bg: fb::COL_BLACK,
        }
    }

    pub fn putc(&mut self, c: u8) {
        match c {
            b'\n' => {
                self.col = 0;
                self.row += 1;
                if self.row >= fb::FB_HEIGHT / font::FONT_H {
                    fb::scroll();
                    self.row = fb::FB_HEIGHT / font::FONT_H - 1;
                }
            }
            b'\r' => self.col = 0,
            _ => {
                let max_col = fb::FB_WIDTH / font::FONT_W;
                fb::draw_char(
                    self.col * font::FONT_W,
                    self.row * font::FONT_H,
                    c,
                    self.fg,
                    self.bg,
                );
                self.col += 1;
                if self.col >= max_col {
                    self.col = 0;
                    self.row += 1;
                    if self.row >= fb::FB_HEIGHT / font::FONT_H {
                        fb::scroll();
                        self.row = fb::FB_HEIGHT / font::FONT_H - 1;
                    }
                }
            }
        }
    }

    pub fn puts(&mut self, s: &str) {
        for &b in s.as_bytes() {
            self.putc(b);
        }
    }

    pub fn put_unicode(&mut self, cp: u32) {
        let fw = font::font_width();
        let fh = font::font_height();
        let max_col = fb::FB_WIDTH / fw;
        let max_row = fb::FB_HEIGHT / fh;
        if cp < 0x20 {
            self.putc(cp as u8);
            return;
        }
        fb::draw_unicode_char(
            self.col * fw,
            self.row * fh,
            cp,
            self.fg,
            self.bg,
        );
        self.col += 1;
        if self.col >= max_col {
            self.col = 0;
            self.row += 1;
            if self.row >= max_row {
                fb::scroll();
                self.row = max_row - 1;
            }
        }
    }

    pub fn puts_unicode(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b < 0x80 {
                self.putc(b);
                i += 1;
                continue;
            }
            let cp;
            if b < 0xE0 {
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
            self.put_unicode(cp);
        }
    }
}
