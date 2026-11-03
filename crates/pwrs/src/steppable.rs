//! Another command a cmdlet steps through its own begin, process and
//! end, as a C# cmdlet wrapping a command does with a
//! `SteppablePipeline`.

use crate::host::vtable;
use crate::pipeline::{check, str16, utf16};
use crate::{PsObject, PsResult};
use pwrs_sys::PsHandle;

/// A command begun by [`Pipeline::steppable`](crate::Pipeline::steppable),
/// stored in a cmdlet's field from `begin` to `end`. What the command
/// writes goes to the cmdlet's own pipeline and streams, so a record the
/// command outputs is the cmdlet's output. [`process`](Self::process)
/// passes it a record, and [`end`](Self::end) runs its end blocks and
/// disposes it. Dropping one that has not ended disposes it, running the
/// clean blocks of its commands in PowerShell 7.3 and later, as the engine
/// does for a stopped pipeline.
///
/// A stop or a terminating error coming back through any of its calls
/// ends the command as it ends a compiled cmdlet making the same call:
/// the call returns it as the `Err`, every later write and step of the
/// phase returns it again, and the command ends with the engine's own
/// exception once the phase returns. An inner `Select-Object -First 2`
/// stopping the records the cmdlet passes it therefore ends the cmdlet
/// quietly after two, as it ends a C# wrapper.
pub struct PsSteppable {
    /// A handle of its own to the managed cmdlet, so a later phase's call
    /// needs no [`Pipeline`](crate::Pipeline).
    cmdlet: PsObject,
    pipeline: PsObject,
    command: String,
    disposed: bool,
}

impl PsSteppable {
    /// Begins `command` for the managed cmdlet `cmdlet`, with `parameters`
    /// bound by name, and returns whether the command takes `-WhatIf`.
    pub(crate) fn begin(cmdlet: PsHandle, command: &str, parameters: &[(&str, PsObject)]) -> PsResult<(PsSteppable, bool)> {
        let table = if parameters.is_empty() {
            PsObject::null()
        } else {
            let t = crate::PsHashtable::new()?;
            for (key, value) in parameters {
                t.set(key, value.clone())?;
            }
            t.0
        };
        let n = utf16(command);
        let mut out = PsHandle::NULL;
        let mut delegates = 0u8;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().steppable_begin)(cmdlet, str16(&n), table.as_raw(), &mut out, &mut delegates, &mut err) };
        check(status, err)?;
        let pipeline = unsafe { PsObject::from_raw(out) };
        // The pipeline's handle to the cmdlet is borrowed for the phase;
        // the steppable stores a clone of its own.
        let borrowed = core::mem::ManuallyDrop::new(unsafe { PsObject::from_raw(cmdlet) });
        let step = PsSteppable { cmdlet: (*borrowed).clone(), pipeline, command: command.to_string(), disposed: false };
        Ok((step, delegates != 0))
    }

    /// Passes `input` to the command as one record, as `Process($input)`
    /// does. A collection is one record, not one per element.
    pub fn process(&self, input: &PsObject) -> PsResult<()> {
        self.step(input.as_raw(), 1)
    }

    /// Runs the command's process blocks without a record, as `Process()`
    /// does, for a cmdlet with no pipeline input of its own to pass on.
    pub fn process_empty(&self) -> PsResult<()> {
        self.step(PsHandle::NULL, 0)
    }

    /// Runs the command's end blocks, then disposes it. Its error comes
    /// first; the disposal's is returned when the end succeeded.
    pub fn end(mut self) -> PsResult<()> {
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().steppable_end)(self.cmdlet.as_raw(), self.pipeline.as_raw(), &mut err) };
        let ended = check(status, err);
        let disposed = self.dispose();
        ended.and(disposed)
    }

    fn step(&self, input: PsHandle, has_input: u8) -> PsResult<()> {
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().steppable_process)(self.cmdlet.as_raw(), self.pipeline.as_raw(), input, has_input, &mut err) };
        check(status, err)
    }

    fn dispose(&mut self) -> PsResult<()> {
        self.disposed = true;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().steppable_dispose)(self.cmdlet.as_raw(), self.pipeline.as_raw(), &mut err) };
        check(status, err)
    }
}

impl Drop for PsSteppable {
    fn drop(&mut self) {
        if self.disposed {
            return;
        }
        if let Err(e) = self.dispose() {
            eprintln!("pwrs: disposing the steppable pipeline of {} failed: {e}", self.command);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cmdlet::{Cmdlet, CmdletBind, CmdletMeta, Instance};
    use crate::{IntoPs, Pipeline, PsResult, PsSteppable};
    use core::ffi::c_void;
    use core::sync::atomic::AtomicU32;
    use pwrs_sys::{PsHandle, PS_PHASE_BEGIN, PS_PHASE_END, PS_PHASE_PROCESS};

    /// Wraps `Select-Object`, storing the steppable in a field from begin
    /// to end, and ends it there unless `abandon` is set.
    #[derive(Default)]
    struct Wraps {
        abandon: bool,
        step: Option<PsSteppable>,
    }

    impl CmdletBind for Wraps {
        unsafe fn bind(&mut self, _params: *const c_void) -> PsResult<()> {
            Ok(())
        }
    }

    impl CmdletMeta for Wraps {
        const NAME: &'static str = "Test-Wraps";
        fn descriptor() -> String {
            String::from("{}")
        }
        fn phase_mask() -> &'static AtomicU32 {
            static MASK: AtomicU32 = AtomicU32::new(pwrs_sys::PS_PHASE_MASK_ALL);
            &MASK
        }
    }

    impl Cmdlet for Wraps {
        fn begin(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
            self.step = Some(ps.steppable("Select-Object", &[])?);
            Ok(())
        }
        fn process(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> {
            let Some(step) = &self.step else {
                return Err(crate::PsError::new(crate::ErrorCategory::InvalidOperation, "TestNotBegun", "process ran before begin"));
            };
            step.process(&7i64.into_ps()?)?;
            step.process_empty()
        }
        fn end(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> {
            match self.step.take() {
                Some(step) if !self.abandon => step.end(),
                Some(step) => {
                    self.step = Some(step);
                    Ok(())
                }
                None => Ok(()),
            }
        }
    }

    fn run_phases(abandon: bool) -> Vec<String> {
        let _host = crate::testing::install();
        let hdr = Instance::<Wraps>::leak_new();
        unsafe { (*(hdr as *mut Instance<Wraps>)).cmdlet.abandon = abandon };
        for phase in [PS_PHASE_BEGIN, PS_PHASE_PROCESS, PS_PHASE_END] {
            unsafe { ((*hdr).run)(hdr, phase, PsHandle::NULL, core::ptr::null()) }.expect("phase");
        }
        unsafe { ((*hdr).drop)(hdr) };
        crate::testing::steppable_calls()
    }

    #[test]
    fn a_steppable_kept_across_phases_is_begun_stepped_ended_and_disposed_once() {
        assert_eq!(run_phases(false), ["begin Select-Object", "process Int(7)", "process", "end", "dispose"]);
    }

    #[test]
    fn a_steppable_dropped_without_ending_is_disposed_without_an_end() {
        assert_eq!(run_phases(true), ["begin Select-Object", "process Int(7)", "process", "dispose"]);
    }
}
