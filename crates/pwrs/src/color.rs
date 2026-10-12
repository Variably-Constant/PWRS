//! `System.ConsoleColor`, the sixteen colors a PowerShell host paints
//! text in, for [`crate::Pipeline::write_host`] and as a parameter type.

use crate::class::PsTyped;
use crate::{ErrorCategory, FromPs, IntoPs, PsError, PsObject, PsResult};

/// One of the sixteen console colors, numbered as `System.ConsoleColor`
/// numbers them. As a parameter type the shell declares it as
/// `System.ConsoleColor`, so the binder converts and completes the
/// names and refuses anything else before the cmdlet runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ConsoleColor {
    Black = 0,
    DarkBlue = 1,
    DarkGreen = 2,
    DarkCyan = 3,
    DarkRed = 4,
    DarkMagenta = 5,
    DarkYellow = 6,
    Gray = 7,
    DarkGray = 8,
    Blue = 9,
    Green = 10,
    Cyan = 11,
    Red = 12,
    Magenta = 13,
    Yellow = 14,
    White = 15,
}

impl ConsoleColor {
    /// Every color, in `System.ConsoleColor` order.
    pub const ALL: [ConsoleColor; 16] = [
        ConsoleColor::Black,
        ConsoleColor::DarkBlue,
        ConsoleColor::DarkGreen,
        ConsoleColor::DarkCyan,
        ConsoleColor::DarkRed,
        ConsoleColor::DarkMagenta,
        ConsoleColor::DarkYellow,
        ConsoleColor::Gray,
        ConsoleColor::DarkGray,
        ConsoleColor::Blue,
        ConsoleColor::Green,
        ConsoleColor::Cyan,
        ConsoleColor::Red,
        ConsoleColor::Magenta,
        ConsoleColor::Yellow,
        ConsoleColor::White,
    ];

    /// The color `System.ConsoleColor` numbers `value`, if any.
    pub fn from_value(value: i64) -> Option<ConsoleColor> {
        usize::try_from(value).ok().and_then(|i| ConsoleColor::ALL.get(i).copied())
    }
}

impl PsTyped for ConsoleColor {
    const CLR_NAME: &'static str = "System.ConsoleColor";
    const VALUE_TYPE: bool = true;
}

impl IntoPs for ConsoleColor {
    fn into_ps(self) -> PsResult<PsObject> {
        crate::values::enum_value("System.ConsoleColor", self as i64)
    }
}

impl FromPs for ConsoleColor {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let value = i64::from_ps(obj)?;
        ConsoleColor::from_value(value).ok_or_else(|| {
            PsError::new(ErrorCategory::InvalidData, "PwrsEnumValue", format!("System.ConsoleColor has no value {value}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ConsoleColor;

    #[test]
    fn numbers_follow_system_console_color() {
        for (i, color) in ConsoleColor::ALL.iter().enumerate() {
            assert_eq!(*color as i64, i as i64);
            assert_eq!(ConsoleColor::from_value(i as i64), Some(*color));
        }
        assert_eq!(ConsoleColor::from_value(16), None);
        assert_eq!(ConsoleColor::from_value(-1), None);
    }
}
