//! Regression coverage for the private introspect-fmt dispatch bridge.
#![cfg(feature = "std")]

use std::{cell::RefCell, fmt};
use tracing::{field::debug, subscriber::with_default, Level};
use tracing_mock::{expect, subscriber};

struct Custom;

impl fmt::Display for Custom {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.pad("custom")
    }
}

#[test]
fn captures_counts_custom_traits_and_template_macros_keep_their_output() {
    // Compare against the builtin formatter, including custom Formatter flags.
    let name = "world";
    let width = 9;
    let custom = Custom;
    let pointer = &custom;
    let expected = format!(
        "{name} {0:*>width$} {1:p} {2:#06x} {2:?}",
        custom, pointer, 31
    );
    let (subscriber, handle) = subscriber::mock()
        .event(expect::event().with_fields(expect::msg(format_args!("{}", expected))))
        .event(expect::event().with_fields(expect::msg(format_args!("custom*** macro"))))
        .only()
        .run_with_handle();
    with_default(subscriber, || {
        tracing::info!(
            "{name} {0:*>width$} {1:p} {2:#06x} {2:?}",
            custom,
            pointer,
            31
        );
        tracing::info!(concat!("{:*<9}", " macro"), custom);
    });
    handle.assert_finished();
}

#[test]
fn explicit_format_fields_are_scoped_for_events_and_spans() {
    let (subscriber, handle) = subscriber::mock()
        .event(
            expect::event().with_fields(
                expect::field("before")
                    .with_value(&1)
                    .and(expect::msg(format_args!("temporary 2")))
                    .and(expect::field("after").with_value(&3))
                    .only(),
            ),
        )
        .new_span(
            expect::span().named("formatted").with_fields(
                expect::field("detail")
                    .with_value(&debug(format_args!("borrowed 4")))
                    .only(),
            ),
        )
        .drop_span("formatted")
        .only()
        .run_with_handle();
    with_default(subscriber, || {
        tracing::info!(
            before = 1,
            message = std::format_args!("{} {}", String::from("temporary"), 2),
            after = 3
        );
        // Non-message fields and qualified/parenthesized format_args! follow
        // the same bridge without changing the visitor API.
        let _span = tracing::info_span!(
            "formatted",
            detail = (core::format_args!("{} {}", String::from("borrowed"), 4))
        );
    });
    handle.assert_finished();
}

#[test]
fn user_expressions_run_once_in_upstream_order() {
    let order = RefCell::new(Vec::new());
    let mark = |label, value| {
        order.borrow_mut().push(label);
        value
    };
    let (subscriber, handle) = subscriber::mock()
        .event(expect::event())
        .event(expect::event())
        .only()
        .run_with_handle();
    with_default(subscriber, || {
        // Positional messages precede their structured fields, as upstream's
        // valueset_all! does; explicit message fields follow source order.
        tracing::info!(field = mark("field", 1), "{}", mark("message", 2));
        tracing::info!(
            before = mark("before", 1),
            message = std::format_args!("{}", mark("explicit", 2)),
            after = mark("after", 3)
        );
    });
    assert_eq!(
        *order.borrow(),
        ["message", "field", "before", "explicit", "after"]
    );
    handle.assert_finished();
}

#[test]
fn question_mark_and_return_still_apply_to_the_caller() {
    // Expressions after a formatted field must not migrate into its callback.
    fn emit(value: Result<u32, ()>) -> Result<(), ()> {
        tracing::info!(message = std::format_args!("{}", 1), after = value?);
        tracing::info!(
            message = std::format_args!("{}", 2),
            after = {
                if value == Ok(3) {
                    return Ok(());
                }
                4u32
            }
        );
        Err(())
    }
    let (subscriber, handle) = subscriber::mock()
        .event(expect::event())
        .only()
        .run_with_handle();
    with_default(subscriber, || {
        assert_eq!(emit(Err(())), Err(()));
        assert_eq!(emit(Ok(3)), Ok(()));
    });
    handle.assert_finished();
}

#[test]
fn disabled_events_do_not_evaluate_formatting_inputs() {
    // A disabled subscriber must not evaluate either message or field inputs.
    let (subscriber, handle) = subscriber::mock()
        .with_filter(|_| false)
        .only()
        .run_with_handle();
    with_default(subscriber, || {
        tracing::event!(Level::INFO, "{}", should_not_run());
    });
    handle.assert_finished();
}

fn should_not_run() -> u32 {
    panic!("disabled message evaluated")
}

#[test]
fn caller_supplied_arguments_and_shadowed_macros_keep_working() {
    // Public Value support for fmt::Arguments must remain available, and a
    // user's macro with the same spelling must retain ordinary name resolution.
    let (subscriber, handle) = subscriber::mock()
        .event(expect::event().with_fields(expect::msg(format_args!("count 7"))))
        .event(expect::event().with_fields(expect::field("answer").with_value(&42u64)))
        .only()
        .run_with_handle();
    with_default(subscriber, || {
        let count = 7;
        let arguments = format_args!("count {count}");
        tracing::info!(message = arguments);
        macro_rules! format_args {
            ($ignored:literal) => {
                42u64
            };
        }
        tracing::info!(answer = format_args!("user-defined"));
    });
    handle.assert_finished();
}

#[test]
fn large_counts_preserve_default_subscriber_output() {
    // Exercise the largest count accepted by the pinned standard formatter,
    // including dynamic precision and explicit format fields.
    let width = 65_535usize;
    let precision = 65_535usize;
    let width_message = format!("prefix {} [{:width$}] suffix", 7u32, "x");
    let precision_message = format!("prefix {} [{:.precision$}] suffix", 7u32, "xyz");
    let (subscriber, handle) = subscriber::mock()
        .event(expect::event().with_fields(expect::msg(format_args!("{}", width_message))))
        .event(expect::event().with_fields(expect::msg(format_args!("{}", precision_message))))
        .event(expect::event().with_fields(expect::msg(format_args!("{}", width_message))))
        .only()
        .run_with_handle();
    with_default(subscriber, || {
        tracing::info!("prefix {} [{:width$}] suffix", 7u32, "x");
        tracing::info!("prefix {} [{:.precision$}] suffix", 7u32, "xyz");
        tracing::info!(message = std::format_args!("prefix {} [{:width$}] suffix", 7u32, "x"));
    });
    handle.assert_finished();
}

#[test]
fn events_spans_and_recorded_formats_reach_the_typed_visitor() {
    use std::sync::{Arc, Mutex};
    use tracing::{
        field::{Field, Format, Visit},
        span, Event, Metadata, Subscriber,
    };

    // A subscriber may now inspect formats without constructing message text.
    #[derive(Clone)]
    struct Inspect(Arc<Mutex<Vec<(&'static str, u32)>>>);
    impl Visit for Inspect {
        fn visit_format(&mut self, _: &Field, value: &Format<'_>) {
            let introspect_fmt::Value::U32(number) = value.args()[0].value() else {
                panic!("the original primitive type must reach the visitor")
            };
            self.0.lock().unwrap().push((value.template(), **number));
        }
        fn record_debug(&mut self, _: &Field, _: &dyn fmt::Debug) {
            panic!("ordinary formatted fields must use visit_format")
        }
    }
    impl Subscriber for Inspect {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, attributes: &span::Attributes<'_>) -> span::Id {
            attributes.record(&mut self.clone());
            span::Id::from_u64(1)
        }
        fn event(&self, event: &Event<'_>) {
            event.record(&mut self.clone());
        }
        fn record(&self, _: &span::Id, record: &span::Record<'_>) {
            record.record(&mut self.clone());
        }
        fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
        fn enter(&self, _: &span::Id) {}
        fn exit(&self, _: &span::Id) {}
    }
    let subscriber = Inspect(Arc::default());
    with_default(subscriber.clone(), || {
        tracing::info!("event {}", 7u32);
        let span = tracing::info_span!("formatted", detail = std::format_args!("span {}", 31u32));
        introspect_fmt::inspect_format!(
            |view| {
                span.record("detail", &view);
            },
            "update {}",
            42u32
        );
    });
    assert_eq!(
        *subscriber.0.lock().unwrap(),
        [("event {}", 7), ("span {}", 31), ("update {}", 42)]
    );
}
