#![allow(dead_code)]

#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Errno {
    Ok = 0,
    NoMem = -1,
    Inval = -2,
    NoEnt = -3,
    Io = -4,
    Perm = -5,
    Range = -6,
    NoSys = -7,
    Busy = -8,
    NoSpace = -9,
    NotDir = -10,
    IsDir = -11,
    BadFd = -12,
    Exist = -13,
    Pipe = -14,
    Overflow = -15,
}

impl Errno {
    #[inline]
    pub const fn as_i64(self) -> i64 {
        self as i64
    }
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::NoMem => "ENOMEM",
            Self::Inval => "EINVAL",
            Self::NoEnt => "ENOENT",
            Self::Io => "EIO",
            Self::Perm => "EPERM",
            Self::Range => "ERANGE",
            Self::NoSys => "ENOSYS",
            Self::Busy => "EBUSY",
            Self::NoSpace => "ENOSPC",
            Self::NotDir => "ENOTDIR",
            Self::IsDir => "EISDIR",
            Self::BadFd => "EBADF",
            Self::Exist => "EEXIST",
            Self::Pipe => "EPIPE",
            Self::Overflow => "EOVERFLOW",
        }
    }
}

pub type KResult<T> = core::result::Result<T, Errno>;
