//! Compile-fail tests: what a user sees for invalid `#[debuggable(...)]` attributes.
//! Messages are checked in debuggable-derive's unit tests; these check the spans.
//! Run these on the toolchain pinned in `tests/fixtures/rust-toolchain.toml`, as CI does:
//! rustc's diagnostic layout is part of the expected output. Regenerate it with
//! `TRYBUILD=overwrite`, then review every `.stderr` diff.

#[test]
fn ui() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
