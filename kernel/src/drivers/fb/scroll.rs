use crate::font;
use super::G_FB;

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
}
