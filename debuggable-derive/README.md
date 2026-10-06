# debuggable-derive

The derive macro behind [`debuggable`](https://crates.io/crates/debuggable). Don't depend on this
crate directly: its output uses private items of the matching `debuggable` version, and the two
are released together. Use `debuggable` and `#[derive(debuggable::Debuggable)]`.

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
