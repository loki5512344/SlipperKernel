//! FbWriter — terminal emulator over the framebuffer.
//!
//! Manages a text cursor (col/row), handles newline/scroll, and prints with
//! configurable foreground/background colors. Accessed via `fb::writer()`.
use super::fb;
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

    /// Write a Unicode codepoint to the terminal.
    ///
    /// Uses `fb::draw_unicode_char` which consults the font's Unicode
    /// table to find the correct glyph. Falls back to '?' if the
    /// codepoint is not mapped.
    pub fn put_unicode(&mut self, cp: u32) {
        let fw = font::font_width();
        let fh = font::font_height();
        let max_col = fb::FB_WIDTH / fw;
        let max_row = fb::FB_HEIGHT / fh;
        // For ASCII control characters, delegate to putc
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

    /// Write a UTF-8 string using Unicode-aware glyph lookup.
    ///
    /// Decodes UTF-8 sequences and renders each codepoint via
    /// `put_unicode`, supporting non-ASCII characters like Cyrillic.
    pub fn puts_unicode(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            // ASCII fast-path
            if b < 0x80 {
                self.putc(b);
                i += 1;
                continue;
            }
            // Decode UTF-8
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

static mut G_WRITER: FbWriter = FbWriter::new();

pub fn writer() -> Option<&'static mut FbWriter> {
    unsafe {
        if fb::enabled() {
            Some(&mut G_WRITER)
        } else {
            None
        }
    }
}

pub fn write_str(s: &str) {
    if let Some(w) = writer() {
        w.puts(s);
    }
}

pub fn write_char(c: u8) {
    if let Some(w) = writer() {
        w.putc(c);
    }
}

/// Write a Unicode codepoint to the framebuffer terminal.
pub fn write_unicode(cp: u32) {
    if let Some(w) = writer() {
        w.put_unicode(cp);
    }
}

/// Write a UTF-8 string to the framebuffer terminal using Unicode-aware rendering.
pub fn write_unicode_str(s: &str) {
    if let Some(w) = writer() {
        w.puts_unicode(s);
    }
}
