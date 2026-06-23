//! Inter-process communication — channel-based message passing.
//!
//! `channel` owns the ring-buffer implementation; this module just re-exports
//! its public surface so callers can use `crate::ipc::create` etc.
pub mod channel;
pub use channel::*;
