//! Edition 2024 library with `#![forbid(unsafe_code)]` and a pure-data type (no functions):
//! its entry must still reach the final binary.
#![forbid(unsafe_code)]
use debuggable::Debuggable;

#[derive(Debuggable)]
#[debuggable(summary = "on={on}")]
pub struct Flag { pub on: bool }
