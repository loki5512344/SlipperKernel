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
