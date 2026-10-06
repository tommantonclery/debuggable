//! Edition 2021 library with `#![forbid(unsafe_code)]`: the derive must still compile.
#![forbid(unsafe_code)]
use debuggable::Debuggable;

/// Non-ASCII in a summary literal: the derive escapes it in the JSON (schema-v1 §3.1).
#[derive(Debuggable)]
#[debuggable(summary = "{0}°C")]
pub struct Celsius(pub f32);

/// Generic type monomorphized only in the downstream binary.
#[derive(Debuggable)]
#[debuggable(summary = "{a} & {b}")]
pub struct Pair<A, B> { pub a: A, pub b: B }
