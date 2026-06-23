#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_builtins)]
#![warn(clippy::all)]
#![allow(
    clippy::module_inception,
    clippy::missing_safety_doc,
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    dead_code,
    non_camel_case_types
)]

extern crate alloc;

pub mod errno;
pub mod fmt;
pub mod formats;
pub mod parser;
pub mod string;
pub mod types;

pub use errno::Errno;
pub use types::*;

pub use core::{cmp, mem, ptr, slice, str};
