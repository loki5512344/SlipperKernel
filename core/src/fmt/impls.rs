//! Arg enum + From impls.


#[derive(Clone, Copy)]
pub enum Arg<'a> {
    Str(&'a str),
    CStr(*const u8),
    Char(u8),
    I64(i64),
    U64(u64),
    ISize(isize),
    USize(usize),
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(s: &'a str) -> Self {
        Self::Str(s)
    }
}
impl<'a> From<&'a [u8]> for Arg<'a> {
    fn from(s: &'a [u8]) -> Self {
        Self::Str(core::str::from_utf8(s).unwrap_or("?"))
    }
}
impl From<i32> for Arg<'_> {
    fn from(v: i32) -> Self {
        Self::I64(i64::from(v))
    }
}
impl From<i64> for Arg<'_> {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}
impl From<u32> for Arg<'_> {
    fn from(v: u32) -> Self {
        Self::U64(u64::from(v))
    }
}
impl From<u64> for Arg<'_> {
    fn from(v: u64) -> Self {
        Self::U64(v)
    }
}
impl From<usize> for Arg<'_> {
    fn from(v: usize) -> Self {
        Self::USize(v)
    }
}
impl From<u8> for Arg<'_> {
    fn from(v: u8) -> Self {
        Self::Char(v)
    }
}

#[cfg(test)]
mod tests {
    use super::super::writer::{vformat, Write};
    use super::*;

    struct Buf {
        s: alloc::string::String,
    }
    impl Buf {
        fn new() -> Self {
            Self {
                s: alloc::string::String::new(),
            }
        }
    }
    impl Write for Buf {
        fn write_str(&mut self, s: &str) {
            self.s.push_str(s);
        }
    }

    #[test]
    fn test_simple() {
        let mut b = Buf::new();
        vformat(&mut b, "hello %s!", &[Arg::from("world")]);
        assert_eq!(b.s, "hello world!");
    }
    #[test]
    fn test_dec_pad() {
        let mut b = Buf::new();
        vformat(&mut b, "n=%05d", &[Arg::from(42i32)]);
        assert_eq!(b.s, "n=00042");
    }
    #[test]
    fn test_hex() {
        let mut b = Buf::new();
        vformat(&mut b, "0x%08x", &[Arg::from(0xDEADBEEFu32)]);
        assert_eq!(b.s, "0xdeadbeef");
    }
    #[test]
    fn test_neg() {
        let mut b = Buf::new();
        vformat(&mut b, "%d", &[Arg::from(-123i64)]);
        assert_eq!(b.s, "-123");
    }
    #[test]
    fn test_int64_min() {
        let mut b = Buf::new();
        vformat(&mut b, "%d", &[Arg::from(i64::MIN)]);
        assert_eq!(b.s, "-9223372036854775808");
    }
}
