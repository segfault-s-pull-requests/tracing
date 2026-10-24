//! Link-time callsite metadata and the original positional format strings.
//!
//! Invariants for upstream compatibility:
//! - Metadata, DefaultCallsite, formatting output and subscriber APIs are unchanged.
//! - The table is initialized by linked statics, not registration or allocations.
//! - All new types, implementations and macro helpers remain in this file.
//! - callsite2! passes original field tokens to the local parser.
//! - An explicit message = format_args!(...) is a template; plain message fields are not.
//! - Formatting arguments are never evaluated again to obtain a template.
//! - Future side metadata extends Entry rather than any upstream tracing type.
//! - Source is best-effort: stable macro span information may be incomplete.
//!
//! Section enumeration currently works on Linux/Android ELF. Unretained object
//! files may be removed by linker garbage collection.

use crate::Metadata;

/// Fixed-layout ELF record: metadata, optional format template, optional source.
///
/// The anchor has no metadata and is filtered from the public iterator. All
/// records must have identical alignment and size (40 bytes on 64-bit ELF).
#[doc(hidden)]
#[derive(Debug)]
#[repr(C)]
pub struct Entry {
    metadata: Option<&'static Metadata<'static>>,
    template: Option<&'static str>,
    source: Option<&'static str>,
}

impl Entry {
    #[doc(hidden)]
    pub const fn new(metadata: &'static Metadata<'static>) -> Self {
        Self { metadata: Some(metadata), template: None, source: None }
    }

    #[doc(hidden)]
    pub const fn with_template(metadata: &'static Metadata<'static>, template: Option<&'static str>) -> Self {
        Self { metadata: Some(metadata), template, source: None }
    }

    /// Best-effort original source, or normalized field tokens if unavailable.
    pub const fn source(&self) -> Option<&'static str> {
        self.source
    }

    #[doc(hidden)]
    pub const fn with_source(metadata: &'static Metadata<'static>, template: Option<&'static str>, source: Option<&'static str>) -> Self {
        Self { metadata: Some(metadata), template, source }
    }

    pub const fn metadata(&self) -> Option<&'static Metadata<'static>> {
        self.metadata
    }

    /// Original positional format string; never the value of a message field.
    pub const fn template(&self) -> Option<&'static str> {
        self.template
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[used]
#[link_section = "tracing_callsite_meta"]
static SECTION_ANCHOR: Entry = Entry { metadata: None, template: None, source: None };

#[cfg(any(target_os = "linux", target_os = "android"))]
fn entries() -> &'static [Entry] {
    // The section name is a C identifier so ELF linkers synthesize these
    // boundaries. Keep the name synchronized with the emission macro.
    // Declare addresses as bytes so Rust-only record fields do not cross FFI.
    extern "C" {
        #[link_name = "__start_tracing_callsite_meta"]
        static START: u8;
        #[link_name = "__stop_tracing_callsite_meta"]
        static STOP: u8;
    }
    core::hint::black_box(&SECTION_ANCHOR);
    // SAFETY: the linker joins only fixed-layout Entry statics in this section;
    // each reference points to statically allocated metadata and string data.
    unsafe {
        let start = core::ptr::addr_of!(START);
        let stop = core::ptr::addr_of!(STOP);
        let bytes = (stop as usize) - (start as usize);
        assert_eq!(bytes % core::mem::size_of::<Entry>(), 0);
        core::slice::from_raw_parts(start.cast::<Entry>(), bytes / core::mem::size_of::<Entry>())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn entries() -> &'static [Entry] { &[] }

/// All linked records; includes unexecuted sites and excludes the sentinel.
pub fn records() -> impl Iterator<Item = &'static Entry> {
    entries().iter().filter(|entry| entry.metadata().is_some())
}

/// Prefer instrument's original function text over generated expansion spans.
#[doc(hidden)]
#[macro_export]
macro_rules! __tracing_capture_source {
    ([$source:expr]; $($fields:tt)*) => { $source };
    ([]; $($fields:tt)*) => {
        $crate::__macro_support::__tracing_source_text!($($fields)*)
    };
}

/// Write exactly one linker record per generated callsite.
#[doc(hidden)]
#[macro_export]
macro_rules! __tracing_record_metadata {
    // No format template does not mean no source: both callsite builders
    // capture source independently, including spans and message-free events.
    // Qualify Option variants so expansion also works without the prelude.
    ($meta:ident) => { $crate::__tracing_record_metadata!($meta; $crate::__macro_support::Option::None); };
    ($meta:ident; $template:expr) => {
        $crate::__tracing_record_metadata!($meta; $template; $crate::__macro_support::Option::Some(__TRACING_SOURCE));
    };
    ($meta:ident; $template:expr; $source:expr) => {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        #[used]
        #[link_section = "tracing_callsite_meta"]
        static __TRACING_METADATA_RECORD: $crate::callsite_table::Entry =
            $crate::callsite_table::Entry::with_source(&$meta, $template, $source);
    };
}

/// Why this parser is deliberately separate from tracing's existing field parser:
/// tracing has multiple formatting paths. Some event! arms rewrite positional
/// arguments into a message field, while others leave them among field tokens.
/// fieldset! recognizes the latter, but produces only field names: it cannot
/// pass the original format string to our linker record without changing its
/// output or threading new metadata through several upstream macro expansions.
/// This local parser duplicates part of the field grammar intentionally to keep
/// upstream changes minimal and avoid changing public macro behavior. When
/// upstream field syntax changes, update these patterns alongside fieldset!.
/// Explicit message = format_args!(...) is a template, but plain message is not.
///
/// Capture a positional format template before the formatter macro expands.
/// Some formatting paths contain message = format_args!(...); others retain
/// raw positional formatting tokens after their structured fields. All normal
/// field grammars are consumed before interpreting a remaining expression as
/// a template. A plain message = ... field never qualifies.
#[doc(hidden)]
#[macro_export]
macro_rules! __tracing_find_template {
    ($meta:ident; $($fields:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($fields)* ,);
    };

    (@fields $meta:ident; , $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident;) => { $crate::__tracing_record_metadata!($meta); };

    // Unlike plain message fields, message = format_args!(...) supplies a
    // static format template. Match both tracing's qualified expansion and
    // user-written unqualified forms before the generic named-field arms.
    (@fields $meta:ident; message = $root:tt :: __macro_support :: format_args ! ($fmt:expr $(, $($args:tt)*)?), $($rest:tt)*) => {
        $crate::__tracing_record_metadata!($meta; $crate::__macro_support::Option::Some($fmt));
    };
    (@fields $meta:ident; message = format_args ! ($fmt:expr $(, $($args:tt)*)?), $($rest:tt)*) => {
        $crate::__tracing_record_metadata!($meta; $crate::__macro_support::Option::Some($fmt));
    };

    // Named fields, as accepted by fieldset! (with Debug/Display variants).
    (@fields $meta:ident; $($key:ident).+ = ?$val:expr, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; $($key:ident).+ = %$val:expr, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; $($key:ident).+ = $val:expr, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; ?$($key:ident).+, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; %$($key:ident).+, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    // enabled! accepts trailing-dot field names; consume these as fields
    // rather than attempting to parse them as positional format expressions.
    (@fields $meta:ident; $($key:ident).+ ., $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; $($key:ident).+, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; $key:literal = ?$val:expr, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; $key:literal = %$val:expr, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; $key:literal = $val:expr, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; {$key:expr} = ?$val:expr, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; {$key:expr} = %$val:expr, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };
    (@fields $meta:ident; {$key:expr} = $val:expr, $($rest:tt)*) => {
        $crate::__tracing_find_template!(@fields $meta; $($rest)*);
    };

    // format_args! ultimately requires a literal (including expanded macros
    // such as concat!). Using expr here preserves that upstream validation.
    (@fields $meta:ident; $fmt:expr, $($args:tt)*) => {
        $crate::__tracing_record_metadata!($meta; $crate::__macro_support::Option::Some($fmt));
    };
    (@fields $meta:ident; $($unknown:tt)+) => {
        $crate::__tracing_record_metadata!($meta);
    };
}

#[cfg(all(test, any(target_os = "linux", target_os = "android")))]
mod tests {
    fn template_for(name: &str) -> Option<Option<&'static str>> {
        super::records().find(|r| r.metadata().unwrap().name() == name).map(|r| r.template())
    }

    // Bevy 0.19.1's warn_once! nests a tracing reexport inside once!.
    // Keep that expansion shape; local paths avoid adding Bevy dependencies.
    macro_rules! once {
        ($expression:expr) => {{
            static SHOULD_FIRE: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(true);
            if SHOULD_FIRE.swap(false, core::sync::atomic::Ordering::Relaxed) {
                $expression;
            }
        }};
    }
    macro_rules! warn_once {
        ($($arg:tt)+) => ({
            once!(crate::warn!($($arg)+))
        });
    }

    // Inspect embedded records directly: no scanner or source-file lookup is
    // involved, and the sentinel must never count as a callsite.
    fn record_for(name: &str) -> &'static super::Entry {
        let mut matches = super::records().filter(|r| r.metadata().unwrap().name() == name);
        let record = matches.next().expect("linked callsite record");
        assert!(matches.next().is_none(), "duplicate callsite record");
        record
    }

    #[test]
    fn direct_callsite_source_and_location() {
        let expected_line = line!() + 1;
        crate::event!(name: "direct_source_test", crate::Level::INFO, "direct source {}", 7);
        let record = record_for("direct_source_test");
        assert_eq!(record.metadata().unwrap().file(), Some(file!()));
        assert_eq!(record.metadata().unwrap().line(), Some(expected_line));
        assert_eq!(record.template(), Some("direct source {}"));
        assert_eq!(record.source(), Some(
            r#"crate::event!(name: "direct_source_test", crate::Level::INFO, "direct source {}", 7)"#
        ));
    }

    #[test]
    fn bevy_once_wrapper_source_and_location() {
        let expected_line = line!() + 1;
        warn_once!(name: "bevy_once_source_test", "wrapped source {}", 7);
        let record = record_for("bevy_once_source_test");
        assert_eq!(record.metadata().unwrap().file(), Some(file!()));
        assert_eq!(record.metadata().unwrap().line(), Some(expected_line));
        assert_eq!(record.template(), Some("wrapped source {}"));
        // Require the user's outer invocation, rather than the inner tracing
        // expansion or merely a nonempty fragment from the wrapper definition.
        assert_eq!(record.source(), Some(
            r#"warn_once!(name: "bevy_once_source_test", "wrapped source {}", 7)"#
        ));
    }

    // Unlike warn_once!, Bevy also passes a complete logging expression to
    // once!. Expr forwarding gives the inner tokens different span ancestry.
    // Fails on the current compiler: parent/source/token spans stop at warn!.
    // Keep this runnable with --ignored until Rust exposes the enclosing call.
    #[test]
    #[ignore = "the outer once! invocation is absent from accessible span ancestry"]
    fn explicit_bevy_once_retains_outer_source() {
        let expected_line = line!() + 1;
        once!(crate::warn!(name: "explicit_once_source", "once source"));
        let record = record_for("explicit_once_source");
        assert_eq!(record.metadata().unwrap().file(), Some(file!()));
        assert_eq!(record.metadata().unwrap().line(), Some(expected_line));
        assert_eq!(record.template(), Some("once source"));
        assert_eq!(record.source(), Some(
            r#"once!(crate::warn!(name: "explicit_once_source", "once source"))"#
        ));
    }

    // Preserve layout to match Bevy's multiline once!(warn!(...)) failures.
    #[rustfmt::skip]
    #[test]
    #[ignore = "the outer once! invocation is absent from accessible span ancestry"]
    fn multiline_explicit_bevy_once_retains_outer_source() {
        let expected_line = line!() + 1;
        once!(crate::warn!(
            name: "multiline_explicit_once_source",
            "Should have created the auxiliary depth of field texture by now"
        ));
        let record = record_for("multiline_explicit_once_source");
        assert_eq!(record.metadata().unwrap().line(), Some(expected_line));
        assert_eq!(record.source(), Some(concat!(
            "once!(crate::warn!(\n",
            "            name: \"multiline_explicit_once_source\",\n",
            "            \"Should have created the auxiliary depth of field texture by now\"\n",
            "        ))",
        )));
    }

    // Preserve the invocation's layout: whitespace and newlines are the
    // regression under test, including through Bevy's nested wrapper shape.
    #[rustfmt::skip]
    #[test]
    fn multiline_bevy_once_wrapper_source_and_location() {
        let expected_line = line!() + 1;
        warn_once!(
            name: "multiline_bevy_source_test",
            window = 7,
            "Cannot access window {}",
            7
        );
        let record = record_for("multiline_bevy_source_test");
        assert_eq!(record.metadata().unwrap().file(), Some(file!()));
        assert_eq!(record.metadata().unwrap().line(), Some(expected_line));
        assert_eq!(record.template(), Some("Cannot access window {}"));
        assert_eq!(record.source(), Some(concat!(
            "warn_once!(\n",
            "            name: \"multiline_bevy_source_test\",\n",
            "            window = 7,\n",
            "            \"Cannot access window {}\",\n",
            "            7\n",
            "        )",
        )));
    }

    // Source retention must not depend on whether fields contain a format
    // template. These are the missing-source shapes found in both captures.
    #[test]
    fn events_without_templates_retain_source() {
        crate::event!(name: "source_plain_message", crate::Level::INFO, message = "plain field");
        crate::event!(name: "source_fields_only", crate::Level::INFO, answer = 42);
        for (name, expected) in [
            ("source_plain_message", r#"crate::event!(name: "source_plain_message", crate::Level::INFO, message = "plain field")"#),
            ("source_fields_only", r#"crate::event!(name: "source_fields_only", crate::Level::INFO, answer = 42)"#),
        ] {
            let record = record_for(name);
            assert_eq!(record.template(), None);
            assert_eq!(record.source(), Some(expected), "source for {}", name);
        }
    }

    #[test]
    fn spans_without_templates_retain_source() {
        let _empty = crate::info_span!("source_empty_span");
        let _fields = crate::info_span!("source_fields_span", answer = 42);
        for (name, expected) in [
            ("source_empty_span", r#"crate::info_span!("source_empty_span")"#),
            ("source_fields_span", r#"crate::info_span!("source_fields_span", answer = 42)"#),
        ] {
            let record = record_for(name);
            assert!(record.metadata().unwrap().is_span());
            assert_eq!(record.template(), None);
            assert_eq!(record.source(), Some(expected), "source for {}", name);
        }
    }

    #[test]
    fn legacy_callsite_retains_source() {
        let _callsite = crate::callsite!(name: "source_legacy_callsite", kind: crate::metadata::Kind::EVENT, fields: answer);
        let record = record_for("source_legacy_callsite");
        assert_eq!(record.template(), None);
        assert_eq!(record.source(), Some(
            r#"crate::callsite!(name: "source_legacy_callsite", kind: crate::metadata::Kind::EVENT, fields: answer)"#
        ));
    }

    #[test]
    fn positional_and_manual_format_args_templates() {
        crate::event!(name: "rewritten", target: "table_tests", crate::Level::INFO, {}, "hello {}", 1);
        crate::event!(name: "bare", target: "table_tests", crate::Level::INFO, "bare {}", 2);
        crate::event!(name: "mixed", target: "table_tests", crate::Level::INFO, a = 3, "mixed {}", 3);
        crate::event!(name: "named", target: "table_tests", crate::Level::INFO, message = "just a field");
        crate::event!(name: "manual_fmt", target: "table_tests", crate::Level::INFO, message = format_args!("just {}", 4));

        assert_eq!(template_for("rewritten"), Some(Some("hello {}")));
        assert_eq!(template_for("bare"), Some(Some("bare {}")));
        assert_eq!(template_for("mixed"), Some(Some("mixed {}")));
        assert_eq!(template_for("named"), Some(None));
        assert_eq!(template_for("manual_fmt"), Some(Some("just {}")));
        // Stable spans are best-effort; at minimum the macro must capture a
        // source representation in the single linker record.
        let source = super::records().find(|r| r.metadata().unwrap().name() == "rewritten").unwrap().source();
        assert!(source.is_some_and(|s| !s.is_empty()));
    }
}
