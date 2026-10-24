//! Inspect instrument's retained source without a scanner or source files.
#![cfg(all(
    feature = "attributes",
    any(target_os = "linux", target_os = "android")
))]

// Select exactly one record so a separately emitted override cannot hide duplicates.
fn record_for(name: &str) -> &'static tracing::callsite_table::Entry {
    let mut records =
        tracing::callsite_table::records().filter(|r| r.metadata().unwrap().name() == name);
    let record = records.next().expect("instrumented span record");
    assert!(records.next().is_none());
    record
}

#[test]
#[cfg(feature = "attributes")]
fn instrument_captures_attribute_and_full_signature() {
    // Generics, visibility, return type and where clause must survive;
    // body capture applies only to this span, not its nested event.
    #[tracing::instrument(name = "instrument_signature_source", skip_all)]
    fn original<T: core::fmt::Debug>(value: T) -> T
    where
        T: Clone,
    {
        tracing::event!(name: "instrument_nested_event", tracing::Level::INFO, "body marker");
        value.clone()
    }
    assert_eq!(original(7), 7);
    let record = record_for("instrument_signature_source");
    assert_eq!(record.template(), None);
    assert_eq!(record.metadata().unwrap().fields().len(), 0);
    let source = record.source().unwrap();
    let compact: String = source.split_whitespace().collect();
    assert!(
        compact.starts_with(concat!(
            "#[instrument(name=\"instrument_signature_source\",skip_all)]",
            "fnoriginal<T:core::fmt::Debug>(value:T)->TwhereT:Clone",
        )),
        "{}",
        source
    );
    assert_eq!(
        source.contains("body marker"),
        cfg!(feature = "instrument-source-body")
    );
    assert!(record_for("instrument_nested_event")
        .source()
        .unwrap()
        .starts_with("tracing::event!"));
}

#[test]
#[cfg(feature = "attributes")]
fn instrument_captures_async_signature() {
    // Merely constructing the future must retain its static source record.
    #[tracing::instrument(name = "instrument_async_source", skip_all, parent = None)]
    async fn original_async(value: u64) -> u64 {
        value + 1
    }
    drop(original_async(7));
    let source = record_for("instrument_async_source").source().unwrap();
    let compact: String = source.split_whitespace().collect();
    assert!(
        compact.starts_with(concat!(
            "#[instrument(name=\"instrument_async_source\",skip_all,parent=None)]",
            "asyncfnoriginal_async(value:u64)->u64",
        )),
        "{}",
        source
    );
    assert_eq!(
        source.contains("value + 1"),
        cfg!(feature = "instrument-source-body")
    );
}

#[test]
fn instrument_preserves_outer_signature_when_rewriting_boxed_futures() {
    // These are instrument's two async-like rewrites, used by async-trait:
    // a boxed async block and a boxed invocation of a nested async function.
    type Future = core::pin::Pin<Box<dyn core::future::Future<Output = u64>>>;
    #[tracing::instrument(name = "instrument_boxed_block", skip_all)]
    fn boxed_block(value: u64) -> Future {
        Box::pin(async move { value + 2 })
    }
    #[tracing::instrument(name = "instrument_boxed_function", skip_all)]
    fn boxed_function(value: u64) -> Future {
        async fn inner(value: u64) -> u64 {
            value + 3
        }
        Box::pin(inner(value))
    }
    drop(boxed_block(7));
    drop(boxed_function(7));
    for (name, function, body) in [
        (
            "instrument_boxed_block",
            "fnboxed_block(value:u64)->Future",
            "value + 2",
        ),
        (
            "instrument_boxed_function",
            "fnboxed_function(value:u64)->Future",
            "value + 3",
        ),
    ] {
        let source = record_for(name).source().unwrap();
        let compact: String = source.split_whitespace().collect();
        assert!(compact.contains(function), "{}", source);
        assert_eq!(
            source.contains(body),
            cfg!(feature = "instrument-source-body")
        );
    }
}
