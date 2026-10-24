//! Best-effort stable source text for a tracing callsite.
//!
//! Span::source_text is stable, but compiler-generated expansion spans can
//! lack source text or refer to an inner macro rather than its outer wrapper.
//! We fall back to normalized input tokens, never guessing an outer invocation.

use proc_macro::{Span, TokenStream};

#[proc_macro]
pub fn source_text(input: TokenStream) -> TokenStream {
    let text = Span::call_site()
        .source_text()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| input.to_string());
    format!("{:?}", text).parse().expect("escaped string literal")
}
