//! Make your Rust types readable in GDB and LLDB with one derive.
//!
//! ```
//! use debuggable::Debuggable;
//!
//! #[derive(Debuggable)]
//! #[debuggable(summary = "{len}/{cap}")]
//! pub struct Buffer {
//!     #[debuggable(items, len = "len")]
//!     data: Vec<u32>,
//!     len: usize,
//!     cap: usize,
//! }
//! ```
//!
//! In a debugger, a `Buffer` then reads `3/8 = {len = 3, cap = 8, [0] = 1, [1] = 2, [2] = 3}`
//! instead of its raw representation.
//!
//! To see it, set up your debugger once with
//! [`cargo-debuggable`](https://crates.io/crates/cargo-debuggable):
//! `cargo install cargo-debuggable && cargo debuggable setup`. Nothing runs inside your
//! program: the derive only embeds a description of the type for the debugger to read.
//!
//! # Attributes
//!
//! All attributes are optional. A bare `#[derive(Debuggable)]` hides `PhantomData` fields
//! and, for enums, shows just the variant name when the variant has no fields.
//!
//! ## `summary = "..."`: one-line summary
//!
//! On a struct, or on an enum variant. `{name}` inserts a field, `{0}` a tuple field, and
//! `{{` and `}}` are literal braces. Summaries may use hidden fields. Each inserted field
//! shows the debugger's own one-line rendering of it, so a field that is itself
//! `Debuggable` shows its summary.
//!
//! ```
//! # use debuggable::Debuggable;
//! #[derive(Debuggable)]
//! #[debuggable(summary = "({x}, {y})")]
//! pub struct Point { pub x: i32, pub y: i32 }
//!
//! #[derive(Debuggable)]
//! #[debuggable(summary = "{0} m")]
//! pub struct Meters(pub f64);
//!
//! #[derive(Debuggable)]
//! pub enum Shape {
//!     #[debuggable(summary = "circle r={radius} at {center}")]
//!     Circle { center: Point, radius: Meters },
//!     Empty,
//! }
//! ```
//!
//! A circle then shows as `circle r=2.5 m at (1, 2)`. On an enum, put `summary` on each
//! variant; variants without one show their name.
//!
//! ## `hide`: leave a field out
//!
//! ```
//! # use debuggable::Debuggable;
//! #[derive(Debuggable)]
//! pub struct Cache {
//!     pub hits: u64,
//!     #[debuggable(hide)]
//!     generation: u32,
//!     _marker: std::marker::PhantomData<*const ()>, // hidden automatically
//! }
//! ```
//!
//! ## `rename = "..."`: show a field under another name
//!
//! ```
//! # use debuggable::Debuggable;
//! #[derive(Debuggable)]
//! pub struct Account {
//!     #[debuggable(rename = "balance_pence")]
//!     balance: i64,
//! }
//! ```
//!
//! ## `items` and `len = "field"`: show a collection's elements
//!
//! On one field per struct. The field's elements are shown as `[0]`, `[1]`, ... after the
//! other fields, in place of the field itself.
//!
//! - On a `Vec<T>`: shows its elements. With `len`, at most that many.
//! - On a `*const T`, `*mut T` or `NonNull<T>`: `len` is required and gives the count.
//!
//! ```
//! # use debuggable::Debuggable;
//! # use std::ptr::NonNull;
//! #[derive(Debuggable)]
//! #[debuggable(summary = "{len} items")]
//! pub struct Stack<T> {
//!     #[debuggable(items, len = "len")]
//!     slots: Vec<Option<T>>,   // only the first `len` slots are in use
//!     #[debuggable(hide)]
//!     len: usize,
//! }
//!
//! #[derive(Debuggable)]
//! pub struct RawParts {
//!     #[debuggable(items, len = "len")]
//!     ptr: NonNull<u16>,
//!     len: usize,
//! }
//! ```
//!
//! At most 10,000 elements are shown.
//!
//! # Compile errors
//!
//! Every mistake is a compile error pointing at the attribute, with suggestions for typos:
//!
//! ```compile_fail
//! # use debuggable::Debuggable;
//! #[derive(Debuggable)]
//! #[debuggable(summary = "{lenght} items")] // did you mean `length`?
//! pub struct Stack { length: usize }
//! ```
//!
//! # Generics, enums, and dependencies
//!
//! Generic types work for every instantiation. Enums work with any layout, including
//! niche-optimised ones (for example, an enum whose data variant holds a `String`). Types from
//! dependency crates work in the final binary like your own.
//!
//! # Cost, and opting out
//!
//! About 300 bytes per derived type in the final binary, plus a 4 KB GDB runtime once per
//! Linux binary: data in a section that only debuggers read. On Windows nothing is emitted
//! (not supported yet). Build with `RUSTFLAGS="--cfg debuggable_disable"` to emit nothing
//! anywhere.
//!
//! # Limitations
//!
//! - Types defined inside function bodies are not matched by the debugger.
//! - Summaries refer to fields by name only: no expressions or format specs.
//! - Windows (Natvis) is not supported yet.
//!
//! See the [repository](https://github.com/tommantonclery/debuggable) for the compatibility table and
//! troubleshooting.
#![no_std]

/// Derives debugger visualizers for a struct or an enum. See the [crate docs](crate).
pub use debuggable_derive::Debuggable;

#[doc(hidden)]
pub mod __private;
