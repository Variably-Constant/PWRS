//! One of the two modules `tools/coload_gate.ps1` imports into one
//! session.
//!
//! The gate builds this source as `Alpha`, and again with every
//! `Alpha` replaced by `Beta` as `Beta`. The two modules then declare
//! the same classes in the same order, so a class id names the same
//! shape in both, and every value carries the name of the module that
//! made it.

use pwrs::prelude::*;

/// The module every value here names as its origin.
const ORIGIN: &str = "Alpha";

/// A record copied into a CLR object on output.
#[psclass(name = "Alpha.Record")]
#[derive(Default, Clone)]
pub struct Record {
    /// The module that made the record.
    pub origin: String,
    /// The name it was given.
    pub name: String,
    /// The number it was given.
    pub value: i64,
}

/// A counter whose value lives in Rust until the object is disposed.
#[psclass(name = "Alpha.Counter", mode = proxy)]
#[derive(Default, Clone)]
pub struct Counter {
    /// The module that made the counter.
    pub origin: String,
    /// The current value.
    pub value: i64,
}

/// Methods a script calls on an `Alpha.Counter`.
#[psmethods]
impl Counter {
    /// Adds `by` and returns the new value.
    pub fn add(&mut self, by: i64) -> PsResult<i64> {
        self.value = match self.value.checked_add(by) {
            Some(v) => v,
            None => return Err(PsError::new(ErrorCategory::InvalidOperation, "CounterOverflow", "the counter would overflow")),
        };
        Ok(self.value)
    }

    /// Moves half of the value into a new counter, which is returned
    /// as its own proxy object.
    pub fn split(&mut self) -> PsResult<Counter> {
        let half = self.value / 2;
        self.value -= half;
        Ok(Counter { origin: ORIGIN.to_string(), value: half })
    }

    /// The current value as a copied record named `snapshot`.
    pub fn snapshot(&self) -> PsResult<Record> {
        Ok(Record { origin: ORIGIN.to_string(), name: "snapshot".to_string(), value: self.value })
    }
}

/// A color, declared to PowerShell as the enum `Alpha.Color`.
#[psenum(name = "Alpha.Color")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Color {
    /// The first color.
    #[default]
    Red,
    /// The second color.
    Green,
    /// The third color.
    Blue,
}

/// Writes an `Alpha.Record`.
#[cmdlet(verb = "Get", noun = "AlphaRecord", output = ["Alpha.Record"])]
#[derive(Default)]
pub struct GetAlphaRecord {
    /// The record's name.
    #[param(mandatory, position = 0)]
    pub name: String,
    /// The record's number.
    #[param(mandatory, position = 1)]
    pub value: i64,
}

impl Cmdlet for GetAlphaRecord {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(Record { origin: ORIGIN.to_string(), name: self.name.clone(), value: self.value })
    }
}

/// Reads an `Alpha.Record` back into Rust and writes
/// `origin:name:value`.
#[cmdlet(verb = "Test", noun = "AlphaRecord", output = ["System.String"])]
#[derive(Default)]
pub struct TestAlphaRecord {
    /// The record to read.
    #[param(mandatory, position = 0)]
    pub record: Record,
}

impl Cmdlet for TestAlphaRecord {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(format!("{}:{}:{}", self.record.origin, self.record.name, self.record.value))
    }
}

/// Writes an `Alpha.Counter` holding `Value`.
#[cmdlet(verb = "New", noun = "AlphaCounter", output = ["Alpha.Counter"])]
#[derive(Default)]
pub struct NewAlphaCounter {
    /// The starting value.
    #[param(mandatory, position = 0)]
    pub value: i64,
}

impl Cmdlet for NewAlphaCounter {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(Counter { origin: ORIGIN.to_string(), value: self.value })
    }
}

/// Reads an `Alpha.Counter` back into Rust and writes `origin=value`.
#[cmdlet(verb = "Test", noun = "AlphaCounter", output = ["System.String"])]
#[derive(Default)]
pub struct TestAlphaCounter {
    /// The counter to read.
    #[param(mandatory, position = 0)]
    pub counter: Counter,
}

impl Cmdlet for TestAlphaCounter {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(format!("{}={}", self.counter.origin, self.counter.value))
    }
}

/// Writes the `Alpha.Color` whose underlying value is given.
#[cmdlet(verb = "Get", noun = "AlphaColor", output = ["Alpha.Color"])]
#[derive(Default)]
pub struct GetAlphaColor {
    /// The underlying value: 0, 1 or 2.
    #[param(mandatory, position = 0)]
    pub value: i64,
}

impl Cmdlet for GetAlphaColor {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let color = match self.value {
            0 => Color::Red,
            1 => Color::Green,
            2 => Color::Blue,
            other => return Err(PsError::new(ErrorCategory::InvalidArgument, "NoSuchColor", format!("{other} is not a color value"))),
        };
        ps.write(color)
    }
}

/// Reads an `Alpha.Color` back into Rust and writes `origin:name`.
#[cmdlet(verb = "Test", noun = "AlphaColor", output = ["System.String"])]
#[derive(Default)]
pub struct TestAlphaColor {
    /// The color to read.
    #[param(mandatory, position = 0)]
    pub color: Color,
}

impl Cmdlet for TestAlphaColor {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(format!("{}:{:?}", ORIGIN, self.color))
    }
}

pwrs::export_module! {
    name: "Alpha",
    cmdlets: [GetAlphaRecord, TestAlphaRecord, NewAlphaCounter, TestAlphaCounter, GetAlphaColor, TestAlphaColor],
    classes: [Record, Counter],
    enums: [Color],
}
