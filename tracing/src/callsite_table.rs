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
    extern "C" {
        #[link_name = "__start_tracing_callsite_meta"]
        static START: Entry;
        #[link_name = "__stop_tracing_callsite_meta"]
        static STOP: Entry;
    }
    core::hint::black_box(&SECTION_ANCHOR);
    // SAFETY: the linker joins only fixed-layout Entry statics in this section;
    // each reference points to statically allocated metadata and string data.
    unsafe {
        let start = core::ptr::addr_of!(START);
        let stop = core::ptr::addr_of!(STOP);
        let bytes = (stop as usize) - (start as usize);
        assert_eq!(bytes % core::mem::size_of::<Entry>(), 0);
        core::slice::from_raw_parts(start, bytes / core::mem::size_of::<Entry>())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn entries() -> &'static [Entry] { &[] }

/// All linked records; includes unexecuted sites and excludes the sentinel.
pub fn records() -> impl Iterator<Item = &'static Entry> {
    entries().iter().filter(|entry| entry.metadata().is_some())
}

/// Write exactly one linker record per generated callsite.
#[doc(hidden)]
#[macro_export]
macro_rules! __tracing_record_metadata {
    ($meta:ident) => { $crate::__tracing_record_metadata!($meta; None; None); };
    ($meta:ident; $template:expr) => {
        $crate::__tracing_record_metadata!($meta; $template; Some(__TRACING_SOURCE));
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
        $crate::__tracing_record_metadata!($meta; Some($fmt));
    };
    (@fields $meta:ident; message = format_args ! ($fmt:expr $(, $($args:tt)*)?), $($rest:tt)*) => {
        $crate::__tracing_record_metadata!($meta; Some($fmt));
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
        $crate::__tracing_record_metadata!($meta; Some($fmt));
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
