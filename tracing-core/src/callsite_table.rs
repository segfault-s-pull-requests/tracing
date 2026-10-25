//! Link-time callsite metadata and the original positional format strings.
//!
//! Invariants for upstream compatibility:
//! - Metadata, DefaultCallsite, formatting output and subscriber APIs are unchanged.
//! - The table is initialized by linked statics, not registration or allocations.
//! - Reused by tracing macros and log shims through tracing-core.
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
        $crate::callsite_source_text!($($fields)*)
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


