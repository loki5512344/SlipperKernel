use onyx_core::errno::{Errno, KResult};

pub const FONT_W: usize = 8;
pub const FONT_H: usize = 16;
pub const FONT_NUM_GLYPHS: usize = 256;
pub const FONT_GLYPH_BYTES: usize = FONT_H;

static mut G_FONT: Option<PcfFont> = None;

#[derive(Clone, Copy)]
pub struct PcfFont {
    pub width: u32,
    pub height: u32,
    pub charsize: u32,
    pub num_glyphs: u32,
    pub glyphs: *const u8,
}

pub fn font() -> Option<PcfFont> {
    unsafe { G_FONT }
}

pub unsafe fn init(data: &[u8]) -> KResult<()> {
    if data.len() < 4 {
        return Err(Errno::Io);
    }
    let magic = u32::from_le_bytes(data[..4].try_into().unwrap());
    if magic == 0x0436 || (magic & 0xFFFF) == 0x0436 {
        init_psf1(data)
    } else if magic == 0x864ab572 {
        init_psf2(data)
    } else {
        Err(Errno::NoEnt)
    }
}

unsafe fn init_psf1(data: &[u8]) -> KResult<()> {
    if data.len() < 4 {
        return Err(Errno::Io);
    }
    let magic = u16::from_le_bytes(data[..2].try_into().unwrap());
    if magic != 0x0436 {
        return Err(Errno::NoEnt);
    }
    let mode = data[2];
    let charsize = data[3] as u32;
    let num_glyphs: u32 = if mode & 0x01 != 0 { 512 } else { 256 };
    let glyph_bytes = (num_glyphs as usize) * (charsize as usize);
    if data.len() < 4 + glyph_bytes {
        return Err(Errno::Io);
    }
    G_FONT = Some(PcfFont {
        width: 8,
        height: charsize,
        charsize,
        num_glyphs,
        glyphs: data.as_ptr().add(4),
    });
    Ok(())
}

unsafe fn init_psf2(data: &[u8]) -> KResult<()> {
    if data.len() < 32 {
        return Err(Errno::Io);
    }
    let hdr_size = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let num_glyphs = u32::from_le_bytes(data[16..20].try_into().unwrap());
    let charsize = u32::from_le_bytes(data[20..24].try_into().unwrap());
    let height = u32::from_le_bytes(data[24..28].try_into().unwrap());
    let width = u32::from_le_bytes(data[28..32].try_into().unwrap());
    let glyph_bytes = (num_glyphs as usize) * (charsize as usize);
    if data.len() < hdr_size + glyph_bytes {
        return Err(Errno::Io);
    }
    G_FONT = Some(PcfFont {
        width,
        height,
        charsize,
        num_glyphs,
        glyphs: data.as_ptr().add(hdr_size),
    });
    Ok(())
}

pub fn glyph_bitmap(c: u8) -> &'static [u8; FONT_GLYPH_BYTES] {
    unsafe {
        if let Some(f) = G_FONT {
            let idx = (c as u32).min(f.num_glyphs - 1) as usize;
            let off = idx * f.charsize as usize;
            let ptr = f.glyphs.add(off) as *const [u8; FONT_GLYPH_BYTES];
            &*ptr
        } else {
            &BLANK_GLYPH
        }
    }
}

static BLANK_GLYPH: [u8; FONT_GLYPH_BYTES] = [0u8; FONT_GLYPH_BYTES];
