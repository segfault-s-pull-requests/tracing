//! Best-effort source text for a tracing callsite.
//!
//! Stable Rust can inspect source_text, but has no expansion ancestry API.
//! When a compile probe confirms proc_macro_span support, walk expansion
//! parents to try to recover the user's outermost logging wrapper. A span need not cover the full invocation;
//! retain the stable result unless an ancestor contains a full macro call.
//! Expression forwarding can omit the enclosing macro from all accessible
//! spans (e.g. once!(warn!(...))); ancestor selection cannot recover that text.

#![cfg_attr(tracing_proc_macro_span_unstable, feature(proc_macro_span))]

use proc_macro::{Span, TokenStream};

#[cfg(tracing_proc_macro_span)]
fn outer_invocation(mut span: Span) -> Option<String> {
    let mut found = None;
    // Guard against unexpected expansion cycles or extraordinarily deep macros.
    for _ in 0..64 {
        if let Some(text) = span.source_text() {
            let s = text.trim();
            // Ancestor spans may be only an identifier or argument. Only
            // replace the result when the span resembles a complete macro call.
            if s.contains('!') && (s.contains('(') || s.contains('{') || s.contains('[')) {
                found = Some(text);
            }
        }
        match span.parent() {
            Some(parent) => span = parent,
            None => break,
        }
    }
    found
}

#[proc_macro]
pub fn source_text(input: TokenStream) -> TokenStream {
    let callsite = Span::call_site();
    let stable = || callsite.source_text()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| input.to_string());

    #[cfg(tracing_proc_macro_span)]
    let text = outer_invocation(callsite).unwrap_or_else(stable);
    #[cfg(not(tracing_proc_macro_span))]
    let text = stable();

    // A single static string literal is embedded in the caller's linker record.
    format!("{:?}", text).parse().expect("escaped string literal")
}
