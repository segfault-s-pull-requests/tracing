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
        formatter.write_str(&self.0.render().map_err(|_| fmt::Error)?)
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
