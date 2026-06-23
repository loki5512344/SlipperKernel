#![allow(dead_code)]

pub unsafe fn strlen(s: *const u8) -> usize {
    let mut n = 0;
    while *s.add(n) != 0 {
        n += 1;
    }
    n
}
pub unsafe fn strcmp(a: *const u8, b: *const u8) -> i32 {
    let mut i = 0;
    loop {
        let ca = *a.add(i);
        let cb = *b.add(i);
        if ca != cb {
            return i32::from(ca) - i32::from(cb);
        }
        if ca == 0 {
            return 0;
        }
        i += 1;
    }
}
pub unsafe fn strncmp(mut a: *const u8, mut b: *const u8, mut n: usize) -> i32 {
    while n > 0 {
        let ca = *a;
        let cb = *b;
        if ca != cb {
            return i32::from(ca) - i32::from(cb);
        }
        if ca == 0 {
            return 0;
        }
        a = a.add(1);
        b = b.add(1);
        n -= 1;
    }
    0
}
pub unsafe fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *dst.add(i) = *src.add(i);
        i += 1;
    }
    dst
}
pub unsafe fn memset(s: *mut u8, c: u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *s.add(i) = c;
        i += 1;
    }
    s
}
pub unsafe fn memmove(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if dst < src as *mut u8 {
        memcpy(dst, src, n)
    } else if dst > src as *mut u8 {
        let mut i = n;
        while i > 0 {
            i -= 1;
            *dst.add(i) = *src.add(i);
        }
        dst
    } else {
        dst
    }
}

#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn rust_memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    memcpy(d, s, n)
}
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn rust_memset(s: *mut u8, c: u8, n: usize) -> *mut u8 {
    memset(s, c, n)
}
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn rust_memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    memmove(d, s, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_strlen() {
        let s = b"hello\0";
        unsafe {
            assert_eq!(strlen(s.as_ptr()), 5);
        }
    }
    #[test]
    fn test_strcmp() {
        unsafe {
            assert_eq!(strcmp(b"abc\0".as_ptr(), b"abc\0".as_ptr()), 0);
            assert!(strcmp(b"abc\0".as_ptr(), b"abd\0".as_ptr()) < 0);
        }
    }
    #[test]
    fn test_strncmp() {
        unsafe {
            assert_eq!(strncmp(b"abcdef\0".as_ptr(), b"abcxyz\0".as_ptr(), 3), 0);
            let r = strncmp(b"abcdef\0".as_ptr(), b"abcxyz\0".as_ptr(), 4);
            assert!(r < 0);
        }
    }
    #[test]
    fn test_memcpy() {
        let src = b"hello world";
        let mut dst = [0u8; 11];
        unsafe {
            memcpy(dst.as_mut_ptr(), src.as_ptr(), 11);
        }
        assert_eq!(&dst, src);
    }
    #[test]
    fn test_memset() {
        let mut buf = [0u8; 8];
        unsafe {
            memset(buf.as_mut_ptr(), 0xAA, 8);
        }
        assert_eq!(buf, [0xAA; 8]);
    }
    #[test]
    fn test_memmove() {
        let mut buf = *b"abcdefg";
        unsafe {
            memmove(buf.as_mut_ptr().add(2), buf.as_ptr(), 5);
        }
        assert_eq!(&buf, b"ababcde");
    }
}
