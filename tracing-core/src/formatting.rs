//! Text adapter for visitors that do not override Visit::visit_format.

use core::fmt;
use introspect_fmt::Format;

// Keep the borrowed Format intact until the subscriber chooses to format it.
// In particular, do not expose Format's structural Debug representation as a
// message and do not use its retained std::fmt::Arguments to render messages.
/// Message adapter used only by the hidden macro expansion support module.
pub struct FormatValue<'a>(pub Format<'a>);

impl fmt::Debug for FormatValue<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.render_to(formatter).map_err(|_| fmt::Error)
    }
}

impl fmt::Display for FormatValue<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{cell::Cell, fmt::Write};

    #[test]
    fn failed_output_stops_before_formatting_later_arguments() {
        // A subscriber may stop accepting output. Streaming must propagate
        // that failure before invoking a later user formatter.
        struct Reject;
        impl fmt::Write for Reject {
            fn write_str(&mut self, _: &str) -> fmt::Result {
                Err(fmt::Error)
            }
        }
        struct Counted<'a>(&'a Cell<usize>);
        impl fmt::Display for Counted<'_> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.set(self.0.get() + 1);
                formatter.write_str("custom")
            }
        }
        let calls = Cell::new(0);
        introspect_fmt::inspect_format!(
            |view| {
                let value = FormatValue(Format::new(view.template(), view.args(), None));
                assert!(write!(Reject, "{:?}", value).is_err());
                assert_eq!(calls.get(), 0);
            },
            "prefix {}",
            Counted(&calls)
        );
    }

    #[test]
    fn user_formatter_errors_propagate_without_retrying() {
        struct Fails<'a>(&'a Cell<usize>);
        impl fmt::Display for Fails<'_> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.set(self.0.get() + 1);
                formatter.write_str("partial")?;
                Err(fmt::Error)
            }
        }
        let calls = Cell::new(0);
        introspect_fmt::inspect_format!(
            |view| {
                let mut output = std::string::String::new();
                assert!(write!(output, "{}", FormatValue(view)).is_err());
                assert_eq!(calls.get(), 1);
                assert_eq!(output, "prefix partial");
            },
            "prefix {} suffix",
            Fails(&calls)
        );
    }

    #[test]
    fn rendering_uses_inspectable_arguments_without_a_retained_formatter() {
        introspect_fmt::inspect_format!(
            |view| {
                assert!(matches!(
                    view.args()[0].value(),
                    introspect_fmt::Value::U32(&31)
                ));
                // Rebuild the view without fmt::Arguments to prevent the adapter
                // from accidentally using the prototype's optional formatter.
                let value = FormatValue(Format::new(view.template(), view.args(), None));
                assert_eq!(std::format!("{:?}", value), "value=0x1f");
            },
            "value={:#x}",
            31u32
        );
    }
}
