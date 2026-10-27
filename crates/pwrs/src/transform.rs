//! `#[transform]`: a Rust function the binder runs over an argument
//! before it is assigned to a parameter.
//!
//! `ArgumentTransformationAttribute` is the engine's own hook for
//! this, and it runs where nothing else can: before the parameter is
//! converted to its declared type, before validation, and before the
//! cmdlet instance exists. That is what a transform offers that
//! converting in the body does not. A `-Size` parameter declared `i64` can
//! accept `1GB`, and a bad value is refused with the parameter named,
//! by the binder, in the same shape as every other binding failure.
//!
//! The value arrives as whatever the caller wrote, so a transform
//! reads it defensively: a string, a number, or an object it does not
//! recognize, which it returns untouched for the binder to convert
//! or reject. Returning the value unchanged is always correct.
//!
//! One transform per parameter. It runs on the pipeline thread during
//! binding, and no instance exists yet, so it takes no `Pipeline` and
//! cannot write to a stream.

use crate::{PsObject, PsResult};

/// One `#[transform]` function. The macro implements this on a unit
/// struct named after the function.
pub trait TransformFn {
    /// `Verb-Noun/Parameter`, the parameter whose argument this
    /// transforms.
    const TARGET: &'static str;
    fn transform(value: &PsObject) -> PsResult<PsObject>;
}

/// Registry entry; the index in the module's table is the transform
/// id written into the generated attribute.
pub struct TransformEntry {
    pub target: &'static str,
    pub run: fn(&PsObject) -> PsResult<PsObject>,
}
