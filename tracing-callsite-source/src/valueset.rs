//! Private field parser for callback-scoped formatting. Keep its grammar in
//! step with tracing's `valueset_all!`; field names still come from `fieldset!`.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::{
    braced,
    ext::IdentExt,
    parenthesized,
    parse::{Parse, ParseStream},
    parse_macro_input, Expr, Ident, Lit, LitStr, Result, Token,
};

struct Invocation {
    runtime: Tokens,
    fields: Expr,
    callback: Expr,
    values: Vec<Field>,
    original: Tokens,
}

struct Field {
    value: Expr,
    style: Style,
}

#[derive(Clone, Copy)]
enum Style {
    Value,
    Debug,
    Display,
}

impl Style {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        Ok(if input.peek(Token![?]) {
            input.parse::<Token![?]>()?;
            Self::Debug
        } else if input.peek(Token![%]) {
            input.parse::<Token![%]>()?;
            Self::Display
        } else {
            Self::Value
        })
    }
}

// Parse only the key grammar; a failed probe means the remaining tokens are
// positional formatting inputs, including macro-generated templates.
fn key(input: ParseStream<'_>) -> Result<Tokens> {
    if input.peek(syn::token::Brace) {
        let inner;
        braced!(inner in input);
        let expression: Expr = inner.parse()?;
        Ok(quote!({ #expression }))
    } else if input.peek(Lit) {
        let literal: Lit = input.parse()?;
        Ok(quote!(#literal))
    } else {
        let first = input.call(Ident::parse_any)?;
        let mut path = quote!(#first);
        while input.peek(Token![.]) {
            input.parse::<Token![.]>()?;
            let next = input.call(Ident::parse_any)?;
            path.extend(quote!(.#next));
        }
        Ok(path)
    }
}

impl Parse for Invocation {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let runtime;
        parenthesized!(runtime in input);
        let runtime = runtime.parse()?;
        let fields;
        parenthesized!(fields in input);
        let fields = fields.parse()?;
        let callback;
        parenthesized!(callback in input);
        let callback = callback.parse()?;
        let body;
        braced!(body in input);
        let original = body.fork().parse()?;
        let mut values = Vec::new();
        while !body.is_empty() {
            let probe = body.fork();
            let _ = Style::parse(&probe)?;
            // macro_rules! ident fragments accept keywords, including true.
            let shorthand = probe.peek(Ident::peek_any);
            let is_field = key(&probe).is_ok()
                && (probe.peek(Token![=])
                    || shorthand && (probe.is_empty() || probe.peek(Token![,])));
            if !is_field {
                // Upstream places a trailing positional message first in both
                // its field array and evaluation order.
                let rest: Tokens = body.parse()?;
                values.insert(
                    0,
                    Field {
                        value: syn::parse2(quote!(::core::format_args!(#rest)))?,
                        style: Style::Value,
                    },
                );
                break;
            }
            let mut style = Style::parse(&body)?;
            let name = key(&body)?;
            let value = if body.peek(Token![=]) {
                body.parse::<Token![=]>()?;
                style = Style::parse(&body)?;
                body.parse()?
            } else {
                syn::parse2(name)?
            };
            values.push(Field { value, style });
            if !body.is_empty() {
                body.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            runtime,
            fields,
            callback,
            values,
            original,
        })
    }
}

struct FormatInput {
    template: Expr,
    args: Vec<(Option<Ident>, Expr)>,
}

impl Parse for FormatInput {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let template = input.parse()?;
        let mut args = Vec::new();
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let name = if input.peek(Ident::peek_any)
                && input.peek2(Token![=])
                && !input.peek2(Token![==])
                && !input.peek2(Token![=>])
            {
                let name = input.call(Ident::parse_any)?;
                input.parse::<Token![=]>()?;
                Some(name)
            } else {
                None
            };
            args.push((name, input.parse()?));
        }
        Ok(Self { template, args })
    }
}

fn format_input(expression: &Expr) -> Result<Option<FormatInput>> {
    match expression {
        // macro_rules! expr forwarding inserts invisible groups. Preserve
        // explicit parenthesized format_args! fields as well.
        Expr::Group(group) => format_input(&group.expr),
        Expr::Paren(paren) => format_input(&paren.expr),
        Expr::Macro(expression)
            if expression.mac.path.segments.last().unwrap().ident == "format_args"
                && expression
                    .mac
                    .path
                    .segments
                    .iter()
                    .rev()
                    .nth(1)
                    .is_some_and(|segment| {
                        segment.ident == "std"
                            || segment.ident == "core"
                            || segment.ident == "__macro_support"
                    }) =>
        {
            syn::parse2(expression.mac.tokens.clone()).map(Some)
        }
        // An unqualified macro may be user-defined. Keep it, aliases, and
        // caller-supplied fmt::Arguments on their existing Value path.
        _ => Ok(None),
    }
}

pub(super) fn expand(input: TokenStream) -> TokenStream {
    let invocation = parse_macro_input!(input as Invocation);
    match generate(invocation) {
        Ok(output) => output.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn generate(invocation: Invocation) -> Result<Tokens> {
    let Invocation {
        runtime,
        fields,
        callback,
        values,
        original,
    } = invocation;
    // Most callsites only have structured fields. Keep their existing expansion
    // and avoid introducing extra borrows or scopes when no formatting is needed.
    if !values
        .iter()
        .map(|field| format_input(&field.value))
        .collect::<Result<Vec<_>>>()?
        .iter()
        .any(Option::is_some)
    {
        return Ok(quote!((#callback)(#runtime::valueset_all!(#fields, #original))));
    }
    let mut operands = Vec::new();
    let mut bindings = Vec::new();
    let mut recorded = Vec::new();
    let mut formats = Vec::new();
    for field in values {
        let format = format_input(&field.value)?;
        let value = if let Some(format) = format {
            let view = format_ident!(
                "__tracing_format_{}",
                formats.len(),
                span = Span::mixed_site()
            );
            let mut args = Vec::new();
            let mut expressions = Vec::new();
            for (name, expression) in format.args {
                expressions.push(LitStr::new(
                    &quote!(#expression).to_string(),
                    Span::call_site(),
                ));
                let binding = bind(expression, &mut operands, &mut bindings);
                // Dereference our shared binding so trait dispatch and type
                // inference still see the original argument, including &mut T.
                args.push(match name {
                    Some(name) => quote!(#name = *#binding),
                    None => quote!(*#binding),
                });
            }
            formats.push((view.clone(), format.template, args, expressions));
            quote!(#runtime::__macro_support::FormatValue(#view))
        } else {
            let binding = bind(field.value, &mut operands, &mut bindings);
            quote!(*#binding)
        };
        let wrapped = match field.style {
            // FormatValue implements Value directly so ordinary messages reach
            // visit_format; explicit ?/% fields still request text formatting.
            Style::Value => value,
            Style::Debug => quote!(#runtime::field::debug(&#value)),
            Style::Display => quote!(#runtime::field::display(&#value)),
        };
        recorded.push(quote!(#runtime::__macro_support::Option::Some(&#wrapped as &dyn #runtime::field::Value)));
    }
    let mut dispatch = quote!((#callback)((#fields).value_set_all(&[#(#recorded),*])));
    for (view, template, args, expressions) in formats.into_iter().rev() {
        dispatch = quote!(#runtime::__macro_support::inspect_format!(
            @__expressions [#(#expressions),*] |#view| { #dispatch }, #template #(, #args)*
        ));
    }
    // All user expressions stay outside the callbacks: return/?/await still
    // target the caller, evaluations occur once in upstream order, and owners
    // of borrowed temporaries live through every subscriber/log invocation.
    Ok(quote!({
        #[allow(unused_imports)]
        use #runtime::field::{debug, display, Value};
        match (#(#operands,)*) {
            (#(#bindings,)*) => { #dispatch }
        }
    }))
}

fn bind(expression: Expr, operands: &mut Vec<Tokens>, bindings: &mut Vec<Ident>) -> Ident {
    let binding = format_ident!(
        "__tracing_operand_{}",
        bindings.len(),
        span = Span::mixed_site()
    );
    operands.push(quote!(&(#expression)));
    bindings.push(binding.clone());
    binding
}
