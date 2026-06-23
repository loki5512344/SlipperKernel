//! Process management module.
//!
//! Re-exports the public surface from `process`, `scheduler`, and `signals`
//! submodules so callers can use `crate::proc::current_pid()` etc. without
//! picking a submodule.
pub mod onx;
pub mod process;
pub mod scheduler;
pub mod signals;

pub use process::*;
pub use scheduler::*;
pub use signals::*;
