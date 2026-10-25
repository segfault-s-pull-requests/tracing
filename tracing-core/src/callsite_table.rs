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




/// Why a call went through the log backend instead of native tracing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LogFallbackReason {
    /// An explicit \`logger:\` argument selects a particular log::Log instance.
    ExplicitLogger = 1,
    /// A runtime expression was used as a structured key name.
    DynamicKey = 2,
    /// Reserved for future formats not expressible as native tracing events.
    UnsupportedFormat = 3,
}

/// One original log invocation which uses the log backend rather than tracing.
///
/// Unlike an event Entry, this is not a tracing callsite and does not own a
/// tracing Metadata or DefaultCallsite. Its level/target may be runtime
/// expressions, so their source tokens are retained rather than guessed.
#[derive(Debug)]
#[repr(C)]
pub struct LogFallbackEntry {
    file: Option<&'static str>,
    line: u32,
    module_path: &'static str,
    level_expression: &'static str,
    target_expression: &'static str,
    template: Option<&'static str>,
    source: Option<&'static str>,
    reason: LogFallbackReason,
}

impl LogFallbackEntry {
    /// Constructs a linker record. Intended for macro-generated statics.
    #[doc(hidden)]
    pub const fn new(
        file: &'static str,
        line: u32,
        module_path: &'static str,
        level_expression: &'static str,
        target_expression: &'static str,
        template: Option<&'static str>,
        source: Option<&'static str>,
        reason: LogFallbackReason,
    ) -> Self {
        Self {
            file: Some(file), line, module_path, level_expression,
            target_expression, template, source, reason,
        }
    }

    /// Source file containing the original log callsite.
    pub const fn file(&self) -> Option<&'static str> { self.file }
    /// Original source line.
    pub const fn line(&self) -> u32 { self.line }
    /// Module containing the original callsite.
    pub const fn module_path(&self) -> &'static str { self.module_path }
    /// Source tokens for the log level, which may be dynamic.
    pub const fn level_expression(&self) -> &'static str { self.level_expression }
    /// Source tokens for the target, which may be dynamic.
    pub const fn target_expression(&self) -> &'static str { self.target_expression }
    /// Original positional format string, if available.
    pub const fn template(&self) -> Option<&'static str> { self.template }
    /// Best-effort original invocation source.
    pub const fn source(&self) -> Option<&'static str> { self.source }
    /// Explanation for selecting log rather than tracing.
    pub const fn reason(&self) -> LogFallbackReason { self.reason }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[used]
#[link_section = "tracing_log_fallback_meta"]
static LOG_FALLBACK_SECTION_ANCHOR: LogFallbackEntry = LogFallbackEntry {
    file: None, line: 0, module_path: "", level_expression: "",
    target_expression: "", template: None, source: None,
    reason: LogFallbackReason::UnsupportedFormat,
};

/// Enumerates log-backend callsites embedded in this executable.
/// A callsite need not have executed to appear in the table.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn log_fallback_records() -> impl Iterator<Item = &'static LogFallbackEntry> {
    extern "C" {
        #[link_name = "__start_tracing_log_fallback_meta"]
        static START: u8;
        #[link_name = "__stop_tracing_log_fallback_meta"]
        static STOP: u8;
    }
    core::hint::black_box(&LOG_FALLBACK_SECTION_ANCHOR);
    // SAFETY: all statics in the section are LogFallbackEntry, repr(C),
    // and have the same alignment and size.
    let records: &'static [LogFallbackEntry] = unsafe {
        let start = core::ptr::addr_of!(START);
        let stop = core::ptr::addr_of!(STOP);
        let bytes = (stop as usize) - (start as usize);
        assert_eq!(bytes % core::mem::size_of::<LogFallbackEntry>(), 0);
        core::slice::from_raw_parts(
            start.cast::<LogFallbackEntry>(),
            bytes / core::mem::size_of::<LogFallbackEntry>(),
        )
    };
    records.iter().filter(|entry| entry.file.is_some())
}

/// Unsupported platforms currently return no linker records.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn log_fallback_records() -> impl Iterator<Item = &'static LogFallbackEntry> {
    core::iter::empty()
}

/// Insert one diagnostic linker record for a log-backend callsite.
///
/// This macro does not evaluate any arguments or invoke any log backend.
/// A string-literal format template is checked by log's usual format_args!
#[doc(hidden)]
#[macro_export]
macro_rules! __tracing_record_log_fallback {
    (
        level: $level:expr, target: $target:expr, reason: $reason:expr,
        template: $template:expr, source: $source:expr
    ) => {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        #[used]
        #[link_section = "tracing_log_fallback_meta"]
        static __LOG_FALLBACK_METADATA_RECORD: $crate::callsite_table::LogFallbackEntry =
            $crate::callsite_table::LogFallbackEntry::new(
                file!(), line!(), module_path!(), stringify!($level),
                stringify!($target), $template, $source, $reason
            );
    };
}

#[cfg(all(test, any(target_os = "linux", target_os = "android")))]
mod fallback_tests {
    use super::{log_fallback_records, LogFallbackReason};

    #[test]
    fn fallback_diagnostic_records_are_enumerable_without_executing_callsites() {
        let line = line!() + 1;
        crate::__tracing_record_log_fallback!(
            level: some_runtime_level, target: some_runtime_target,
            reason: LogFallbackReason::DynamicKey,
            template: Some("text {}"), source: Some("log!(dynamic key)")
        );
        let mut found = log_fallback_records().filter(
            |r| r.file() == Some(file!()) && r.line() == line
        );
        let record = found.next().expect("linked fallback record");
        assert!(found.next().is_none(), "exactly one record per macro use");
        assert_eq!(record.reason(), LogFallbackReason::DynamicKey);
        assert_eq!(record.module_path(), module_path!());
        assert_eq!(record.level_expression(), "some_runtime_level");
        assert_eq!(record.target_expression(), "some_runtime_target");
        assert_eq!(record.template(), Some("text {}"));
        assert_eq!(record.source(), Some("log!(dynamic key)"));
    }

    #[test]
    fn normal_event_records_remain_separate() {
        let line = line!() + 1;
        crate::__tracing_record_log_fallback!(
            level: level, target: target,
            reason: LogFallbackReason::ExplicitLogger,
            template: Some("fallback"), source: Some("source")
        );
        assert_eq!(
            log_fallback_records()
                .filter(|r| r.file() == Some(file!()) && r.line() == line)
                .count(),
            1
        );
        assert_eq!(
            super::records()
                .filter(|r| r.metadata().unwrap().file() == Some(file!())
                    && r.metadata().unwrap().line() == Some(line))
                .count(),
            0
        );
    }
}
