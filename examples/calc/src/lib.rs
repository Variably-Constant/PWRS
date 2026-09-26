//! A second module, so two pwrs modules can be imported into one
//! session and shown not to clobber each other (per-module ALC).

use pwrs::prelude::*;

/// Adds two numbers.
#[cmdlet(verb = "Add", noun = "CalcNumber", output = ["System.Int64"])]
#[derive(Default)]
pub struct AddCalcNumber {
    #[param(mandatory, position = 0)]
    pub x: i64,
    #[param(mandatory, position = 1)]
    pub y: i64,
}

impl Cmdlet for AddCalcNumber {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(self.x + self.y)
    }
}

pwrs::export_module! {
    name: "Calc",
    cmdlets: [AddCalcNumber],
}
