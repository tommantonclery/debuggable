//! Edition 2024 library with `#![forbid(unsafe_code)]` and a pure-data type (no functions):
//! its entry must still reach the final binary.
#![forbid(unsafe_code)]

/// #[debuggable(summary = "on={on}")]
pub struct Flag { pub on: bool }
debuggable::__entry!("Flag", r#""generic":false,"kind":"struct","summary":[["lit","on="],["field","on"]]}"#);
