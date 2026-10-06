//! Compile-fail tests: what a user sees for invalid `#[debuggable(...)]` attributes.
//! Messages are checked in debuggable-derive's unit tests; these check the spans.
//! Regenerate the expected output with `TRYBUILD=overwrite cargo test -p debuggable --test ui`,
//! then review the `.stderr` diffs.

#[test]
fn ui() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
