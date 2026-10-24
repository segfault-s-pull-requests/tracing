//! Link-time table of metadata for compiled `tracing` event and span callsites.
//!
//! Invariants (these are requirements, not incidental implementation details):
//! - Do not modify `Metadata`, `DefaultCallsite`, or subscriber APIs/layouts.
//! - Record a pointer when a macro expands; do not depend on callsite execution,
//!   registration, subscriber installation, or runtime allocation.
//! - Keep all table-specific types, implementations, and linker mechanics here.
//!   Other tracing source files should contain only the module declaration and
//!   the one-line hooks in `callsite!` and `callsite2!`.
//! - Each linked macro-generated callsite contributes one table entry; neither
//!   using a callsite nor registering it adds further entries.
//! - Do not change how ordinary `tracing` users invoke the existing macros.
//! - Future fields (such as a format template) must extend this side table,
//!   rather than changing existing tracing data layouts.
//!
//! The ELF implementation currently supports Linux/Android. Other targets
//! return an empty iterator. Linker garbage collection may remove callsites
//! that the final program does not otherwise retain.

use crate::Metadata;

/// One section record. None is reserved for the section anchor only.
///
/// A transparent pointer-sized representation is intentional: concatenated
/// section records must form a contiguous array without inter-record padding.
#[doc(hidden)]
#[repr(transparent)]
pub struct Entry(Option<&'static Metadata<'static>>);

impl Entry {
    #[doc(hidden)]
    pub const fn new(metadata: &'static Metadata<'static>) -> Self {
        Self(Some(metadata))
    }

    fn metadata(&self) -> Option<&'static Metadata<'static>> {
        self.0
    }
}

// A guaranteed section member makes __start and __stop available even if the
// program contains no tracing events or spans. The sentinel is never returned.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[used]
#[link_section = "tracing_callsite_meta"]
static SECTION_ANCHOR: Entry = Entry(None);

#[cfg(any(target_os = "linux", target_os = "android"))]
fn entries() -> &'static [Entry] {
    // GNU-compatible ELF linkers synthesize __start_SECTION/__stop_SECTION
    // for section names that are valid C identifiers. Keep this exact section
    // name in sync with the macro below; it must not begin with a dot.
    extern "C" {
        #[link_name = "__start_tracing_callsite_meta"]
        static START: Entry;
        #[link_name = "__stop_tracing_callsite_meta"]
        static STOP: Entry;
    }

    // Preserve an explicit reference to the anchor so it is linked into the
    // executable, including when none of the application callsites are kept.
    core::hint::black_box(&SECTION_ANCHOR);

    // SAFETY: The linker boundaries bracket contiguous immutable Entry records
    // emitted with exactly the same section and layout. The start is aligned;
    // the byte length is a multiple of the Entry size. All stored references
    // originate from static META values and therefore outlive this slice.
    // This is ELF/linker-specific and must not be copied to arbitrary targets.
    unsafe {
        let start = core::ptr::addr_of!(START);
        let stop = core::ptr::addr_of!(STOP);
        let bytes = (stop as usize) - (start as usize);
        assert_eq!(bytes % core::mem::size_of::<Entry>(), 0);
        core::slice::from_raw_parts(start, bytes / core::mem::size_of::<Entry>())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn entries() -> &'static [Entry] {
    &[]
}

/// All linked macro-generated callsite metadata, including unexecuted sites.
///
/// No dispatcher/subscriber or runtime registration is required. Custom
/// implementations of the `Callsite` trait are not represented unless they
/// also use the macro hook. Ordering is determined by the linker, not source
/// order. On unsupported targets the iterator is empty.
pub fn all() -> impl Iterator<Item = &'static Metadata<'static>> {
    entries().iter().filter_map(Entry::metadata)
}

/// Internal insertion hook for both tracing callsite construction macros.
///
/// Invariant: emit a static *reference to* META, not a copy of Metadata, so
/// table identity matches the metadata used by the existing callsite.
/// This hook must stay orthogonal to the registration and enabled paths.
#[doc(hidden)]
#[macro_export]
macro_rules! __tracing_record_metadata {
    ($meta:ident) => {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        #[used]
        #[link_section = "tracing_callsite_meta"]
        static __TRACING_METADATA_RECORD: $crate::callsite_table::Entry =
            $crate::callsite_table::Entry::new(&$meta);
    };
}
