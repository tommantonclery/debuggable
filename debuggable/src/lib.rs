//! Make your Rust types readable in debuggers. Work in progress.
#![no_std]

pub use debuggable_derive::Debuggable;

#[doc(hidden)]
pub mod __private;