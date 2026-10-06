//! Derive macro for `debuggable`. Use the `debuggable` crate instead.

use proc_macro::TokenStream;

/// Placeholder: expands to nothing for now.
#[proc_macro_derive(Debuggable, attributes(debuggable))]
pub fn derive_debuggable(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}