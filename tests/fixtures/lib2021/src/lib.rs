//! Edition 2021 library with `#![forbid(unsafe_code)]`: entries must still compile.
#![forbid(unsafe_code)]

/// Non-ASCII in a summary literal: the JSON must carry it escaped (schema §3.1).
/// #[debuggable(summary = "{0}°C")]
pub struct Celsius(pub f32);
debuggable::__entry!("Celsius", r#""generic":false,"kind":"struct","summary":[["field","0"],["lit","\u00b0C"]]}"#);

/// Generic type monomorphized only in the downstream binary.
/// #[debuggable(summary = "{a} & {b}")]
pub struct Pair<A, B> { pub a: A, pub b: B }
debuggable::__entry!("Pair", r#""generic":true,"kind":"struct","summary":[["field","a"],["lit"," & "],["field","b"]]}"#);
