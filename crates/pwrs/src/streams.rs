//! The output and message streams a cmdlet's work writes to, which the
//! pipeline and a job both have, so work written once against
//! [`Streams`] runs inline or in a job.

use crate::{IntoPs, Pipeline, Progress, PsError, PsObject, PsResult};

/// What a cmdlet's work writes and reads wherever it runs: on the pipeline
/// thread through [`Pipeline`], or in a job through [`crate::JobContext`].
/// Calls only the pipeline thread can make, such as `should_process`,
/// session state, `invoke` and host prompts, are not here, so code taking
/// `&impl Streams` can run in a job.
///
/// ```ignore
/// fn copy(&self, out: &impl Streams) -> PsResult<()> {
///     out.write(self.name.clone())?;
///     pwrs::verbose!(out, "copied {}", self.name)
/// }
/// ```
pub trait Streams {
    /// Writes any convertible value: a `Vec<T>` enumerates, a `PsArray<T>`
    /// is one object.
    fn write<T: IntoPs>(&self, value: T) -> PsResult<()>;
    /// Writes one object, never enumerated.
    fn write_object(&self, obj: &PsObject) -> PsResult<()>;
    /// Writes a collection's elements, each its own object.
    fn write_enumerated(&self, obj: &PsObject) -> PsResult<()>;
    /// Writes an error record and goes on.
    fn write_error(&self, e: &PsError) -> PsResult<()>;
    fn verbose(&self, text: &str) -> PsResult<()>;
    fn debug(&self, text: &str) -> PsResult<()>;
    fn warning(&self, text: &str) -> PsResult<()>;
    fn information(&self, text: &str) -> PsResult<()>;
    /// Writes the text `text` returns to the verbose stream, built only
    /// when the record would go somewhere; `verbose!` calls it.
    fn verbose_if(&self, text: impl FnOnce() -> String) -> PsResult<()>;
    /// As `verbose_if`, for the debug stream; `debug!` calls it.
    fn debug_if(&self, text: impl FnOnce() -> String) -> PsResult<()>;
    /// As `verbose_if`, for the warning stream; `warning!` calls it.
    fn warning_if(&self, text: impl FnOnce() -> String) -> PsResult<()>;
    /// As `verbose_if`, for the information stream; `information!` calls
    /// it.
    fn information_if(&self, text: impl FnOnce() -> String) -> PsResult<()>;
    /// Writes a progress record with every field it has.
    fn write_progress(&self, record: &Progress) -> PsResult<()>;
    /// True once a stop has been requested: Ctrl+C or a stopped pipeline
    /// for the pipeline, Stop-Job for a job.
    fn stopping(&self) -> bool;
}

impl Streams for Pipeline<'_> {
    fn write<T: IntoPs>(&self, value: T) -> PsResult<()> {
        Pipeline::write(self, value)
    }
    fn write_object(&self, obj: &PsObject) -> PsResult<()> {
        Pipeline::write_object(self, obj)
    }
    fn write_enumerated(&self, obj: &PsObject) -> PsResult<()> {
        Pipeline::write_enumerated(self, obj)
    }
    fn write_error(&self, e: &PsError) -> PsResult<()> {
        Pipeline::write_error(self, e)
    }
    fn verbose(&self, text: &str) -> PsResult<()> {
        Pipeline::verbose(self, text)
    }
    fn debug(&self, text: &str) -> PsResult<()> {
        Pipeline::debug(self, text)
    }
    fn warning(&self, text: &str) -> PsResult<()> {
        Pipeline::warning(self, text)
    }
    fn information(&self, text: &str) -> PsResult<()> {
        Pipeline::information(self, text)
    }
    fn verbose_if(&self, text: impl FnOnce() -> String) -> PsResult<()> {
        Pipeline::verbose_if(self, text)
    }
    fn debug_if(&self, text: impl FnOnce() -> String) -> PsResult<()> {
        Pipeline::debug_if(self, text)
    }
    fn warning_if(&self, text: impl FnOnce() -> String) -> PsResult<()> {
        Pipeline::warning_if(self, text)
    }
    fn information_if(&self, text: impl FnOnce() -> String) -> PsResult<()> {
        Pipeline::information_if(self, text)
    }
    fn write_progress(&self, record: &Progress) -> PsResult<()> {
        Pipeline::write_progress(self, record)
    }
    fn stopping(&self) -> bool {
        Pipeline::stopping(self)
    }
}
