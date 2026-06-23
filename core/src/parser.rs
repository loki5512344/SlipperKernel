#![allow(dead_code)]

#[inline]
pub fn be32(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}
#[inline]
pub fn be64(p: &[u8]) -> u64 {
    u64::from_be_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]])
}
#[inline]
pub fn le32(p: &[u8]) -> u32 {
    u32::from_le_bytes([p[0], p[1], p[2], p[3]])
}
#[inline]
pub fn le64(p: &[u8]) -> u64 {
    u64::from_le_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]])
}

pub struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}
impl<'a> Cursor<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }
    pub fn eof(&self) -> bool {
        self.pos >= self.buf.len()
    }
    pub fn read_u8(&mut self) -> Option<u8> {
        let v = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }
    pub fn read_le32(&mut self) -> Option<u32> {
        if self.pos + 4 > self.buf.len() {
            return None;
        }
        let v = le32(&self.buf[self.pos..]);
        self.pos += 4;
        Some(v)
    }
    pub fn read_le64(&mut self) -> Option<u64> {
        if self.pos + 8 > self.buf.len() {
            return None;
        }
        let v = le64(&self.buf[self.pos..]);
        self.pos += 8;
        Some(v)
    }
    pub fn read_be32(&mut self) -> Option<u32> {
        if self.pos + 4 > self.buf.len() {
            return None;
        }
        let v = be32(&self.buf[self.pos..]);
        self.pos += 4;
        Some(v)
    }
    pub fn read_be64(&mut self) -> Option<u64> {
        if self.pos + 8 > self.buf.len() {
            return None;
        }
        let v = be64(&self.buf[self.pos..]);
        self.pos += 8;
        Some(v)
    }
    pub fn read_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.pos + n > self.buf.len() {
            return None;
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }
    pub fn skip(&mut self, n: usize) -> Option<()> {
        if self.pos + n > self.buf.len() {
            return None;
        }
        self.pos += n;
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_be() {
        assert_eq!(be32(&[0x12, 0x34, 0x56, 0x78]), 0x12345678);
    }
    #[test]
    fn test_le() {
        assert_eq!(le32(&[0x78, 0x56, 0x34, 0x12]), 0x12345678);
    }
    #[test]
    fn test_cursor() {
        let buf = [0u8, 1, 2, 3, 4, 5, 6, 7];
        let mut c = Cursor::new(&buf);
        assert_eq!(c.read_u8(), Some(0));
        assert_eq!(c.read_le32(), Some(0x04030201));
        assert_eq!(c.read_le32(), None);
    }
}
