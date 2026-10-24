//! Capture normalized original input before instrument rewrites its body.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::LitStr;

/// Keep source generation separate from instrumentation and its async rewrites.
/// The attribute's own path is not supplied by Rust, so use canonical instrument.
pub(crate) fn capture(
    args: TokenStream,
    item: TokenStream,
    include_body: bool,
) -> syn::Result<LitStr> {
    let attribute = if args.is_empty() {
        "#[instrument]".to_owned()
    } else {
        format!("#[instrument({})]", args)
    };
    let function = if include_body {
        item.to_string()
    } else {
        // Parse only the signature; the speculative instrumentation path must
        // still support bodies containing syntax syn cannot fully parse.
        let input = syn::parse2::<crate::MaybeItemFn>(item)?;
        let vis = input.vis;
        let sig = input.sig;
        quote!(#vis #sig).to_string()
    };
    Ok(LitStr::new(
        &format!("{}\n{}", attribute, function),
        Span::call_site(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_and_body_modes_preserve_original_tokens() {
        // Methods exercise receiver, lifetime, generics, return and where clauses.
        let args = quote!(skip_all);
        let item = quote! {
            pub async unsafe fn example<'a, T>(&'a self, value: T) -> Option<T>
            where T: Clone {
                let original_body = value.clone();
                Some(original_body)
            }
        };
        let signature = capture(args.clone(), item.clone(), false).unwrap().value();
        let full = capture(args, item, true).unwrap().value();
        let expected = quote!(pub async unsafe fn example<'a, T>(&'a self, value: T) -> Option<T> where T: Clone);
        assert_eq!(signature, format!("#[instrument(skip_all)]\n{}", expected));
        assert!(full.starts_with(&signature));
        assert!(full.contains("original_body"));
        assert!(!signature.contains("original_body"));
        assert!(!full.contains("__tracing_attr"));
    }

    #[test]
    fn empty_attribute_is_captured_without_parentheses() {
        let source = capture(
            quote!(),
            quote!(
                fn example() {}
            ),
            false,
        )
        .unwrap()
        .value();
        assert_eq!(source, format!("#[instrument]\n{}", quote!(fn example())));
    }

    #[test]
    fn signature_capture_does_not_parse_the_body() {
        // Keep capture compatible with speculative instrumentation when syn
        // does not understand body syntax accepted by a newer Rust compiler.
        let item = quote!(fn example(value: u64) { arbitrary incomplete + });
        let source = capture(quote!(), item, false).unwrap().value();
        assert_eq!(
            source,
            format!("#[instrument]\n{}", quote!(fn example(value: u64)))
        );
    }
}
