//! A cmdlet that wraps another command, as a proxy command does:
//! `#[cmdlet(wraps = "Verb-Noun")]` declares the command's parameters on
//! the cmdlet, from the command's own declarations in each PowerShell the
//! module loads in, and runs the command as a steppable pipeline through
//! the cmdlet's begin, process and end. [`Wrapper`] is where the cmdlet
//! changes what goes in and what comes out.

use core::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::host::vtable;
use crate::pipeline::check;
use crate::runtime::{error_handle, panic_message, string_handle};
use crate::{ErrorCategory, Pipeline, PsError, PsHashtable, PsObject, PsResult};
use pwrs_sys::{PsHandle, PsStatus, PS_ERR_MANAGED_EXCEPTION, PS_ERR_NATIVE_PANIC, PS_ERR_PIPELINE_STOPPED, PS_OK};

/// The steps a cmdlet declared with `#[cmdlet(wraps = "Verb-Noun")]`
/// takes beside the command it wraps. Each has a default that changes
/// nothing, so `impl Wrapper for MyCmdlet {}` forwards the caller's
/// arguments, records and output as they are.
///
/// The command's output passes through [`output`](Self::output) as it is
/// written, during the cmdlet's own begin, process and end, so a record
/// reaches the caller in the order the command wrote it. Its errors,
/// warnings and other streams go to the caller as they would from the
/// command run directly.
pub trait Wrapper {
    /// The arguments the command begins with: those the caller bound to
    /// the parameters the cmdlet copies from it, by parameter name. A
    /// parameter of the cmdlet's own is not among them. Changing the table
    /// changes what the command is given.
    fn arguments(&mut self, arguments: &mut PsHashtable, ps: &Pipeline<'_>) -> PsResult<()> {
        let _unchanged = (arguments, ps);
        Ok(())
    }

    /// A record piped to the cmdlet, as it was piped, before the command
    /// binds it: the record to pass to the command, or `None` to drop it.
    fn input(&mut self, record: PsObject, ps: &Pipeline<'_>) -> PsResult<Option<PsObject>> {
        let _unused = ps;
        Ok(Some(record))
    }

    /// A record the command wrote: write what the caller gets, any number
    /// of records. A non-terminating error is written and the command goes
    /// on; a terminating one ends the command and the cmdlet with it.
    fn output(&mut self, record: PsObject, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write_object(&record)
    }
}

/// What the wrapped command's output goes through for one call: the
/// cmdlet's [`Wrapper::output`] over the phase's [`Pipeline`].
type Hook<'a> = &'a mut dyn FnMut(PsObject) -> PsResult<()>;

/// Whether `e` is the stop a write or a step reports for a stopped
/// pipeline, as [`check`] makes it.
fn is_stop(e: &PsError) -> bool {
    e.terminating && e.category == ErrorCategory::OperationStopped
}

/// Calls the hook `context` points at with one record the wrapped
/// command wrote. A stop goes back as one, so the managed side stops the
/// command as the engine does; any other error aborts the command, which
/// the wrapping call then reports.
unsafe extern "C" fn deliver(context: *mut c_void, record: PsHandle, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let hook = unsafe { &mut *(context as *mut Hook<'_>) };
        let given = if record.is_null() { PsObject::null() } else { unsafe { PsObject::from_raw((vtable().clone_handle)(record)) } };
        hook(given)
    }));
    match result {
        Ok(Ok(())) => PS_OK,
        Ok(Err(e)) if is_stop(&e) => PS_ERR_PIPELINE_STOPPED,
        Ok(Err(e)) => {
            if !err.is_null() {
                unsafe { *err = error_handle(&e) };
            }
            PS_ERR_MANAGED_EXCEPTION
        }
        Err(payload) => {
            if !err.is_null() {
                unsafe { *err = string_handle(&panic_message(payload)) };
            }
            PS_ERR_NATIVE_PANIC
        }
    }
}

/// Runs `call` with a hook over `w`'s [`Wrapper::output`]. A non-terminating
/// error the hook returns is written and the command goes on; a
/// terminating one is stored, aborts the command, and is what this returns,
/// as it would end a cmdlet whose own phase returned it.
fn with_hook<W: Wrapper>(w: &mut W, ps: &Pipeline<'_>, call: impl FnOnce(*mut c_void) -> PsResult<()>) -> PsResult<()> {
    let mut failed: Option<PsError> = None;
    let called = {
        let mut each = |record: PsObject| -> PsResult<()> {
            match w.output(record, ps) {
                Ok(()) => Ok(()),
                Err(e) if is_stop(&e) => Err(e),
                Err(e) if !e.terminating => ps.write_error(&e),
                Err(e) => {
                    let abort = PsError::new(e.category, e.error_id.clone(), e.message.clone()).terminating();
                    failed = Some(e);
                    Err(abort)
                }
            }
        };
        let mut hook: Hook<'_> = &mut each;
        call(&mut hook as *mut Hook<'_> as *mut c_void)
    };
    match failed {
        Some(e) => Err(e),
        None => called,
    }
}

/// The cmdlet's `begin`: the arguments, [`Wrapper::arguments`], then the
/// command begun with them.
#[doc(hidden)]
pub fn begin<W: Wrapper>(w: &mut W, ps: &Pipeline<'_>) -> PsResult<()> {
    let mut out = PsHandle::NULL;
    let mut err = PsHandle::NULL;
    let status = unsafe { (vtable().wrapped_arguments)(ps.cmdlet_handle(), &mut out, &mut err) };
    check(status, err)?;
    let mut arguments = PsHashtable(unsafe { PsObject::from_raw(out) });
    w.arguments(&mut arguments, ps)?;
    let mut delegates = 0u8;
    with_hook(w, ps, |context| {
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().wrapped_begin)(ps.cmdlet_handle(), arguments.0.as_raw(), deliver, context, &mut delegates, &mut err) };
        check(status, err)
    })?;
    if delegates != 0 {
        ps.mark_asked();
    }
    Ok(())
}

/// The cmdlet's `process`: the record piped to it through
/// [`Wrapper::input`] to the command, or the command's process blocks
/// without one when nothing is piped.
#[doc(hidden)]
pub fn process<W: Wrapper>(w: &mut W, ps: &Pipeline<'_>) -> PsResult<()> {
    let mut out = PsHandle::NULL;
    let mut has = 0u8;
    let mut err = PsHandle::NULL;
    let status = unsafe { (vtable().wrapped_input)(ps.cmdlet_handle(), &mut out, &mut has, &mut err) };
    check(status, err)?;
    let (record, has_input) = if has == 0 {
        (PsObject::null(), 0u8)
    } else {
        match w.input(unsafe { PsObject::from_raw(out) }, ps)? {
            Some(record) => (record, 1u8),
            None => return Ok(()),
        }
    };
    with_hook(w, ps, |context| {
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().wrapped_process)(ps.cmdlet_handle(), record.as_raw(), has_input, deliver, context, &mut err) };
        check(status, err)
    })
}

/// The cmdlet's `end`: the command's end blocks, then its disposal.
#[doc(hidden)]
pub fn end<W: Wrapper>(w: &mut W, ps: &Pipeline<'_>) -> PsResult<()> {
    with_hook(w, ps, |context| {
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().wrapped_end)(ps.cmdlet_handle(), deliver, context, &mut err) };
        check(status, err)
    })
}

#[cfg(test)]
mod tests {
    use super::Wrapper;
    use crate::cmdlet::{Cmdlet, CmdletBind, CmdletMeta, Instance};
    use crate::testing::{self, Value};
    use crate::{ErrorCategory, FromPs, IntoPs, Pipeline, PsError, PsObject, PsResult};
    use core::ffi::c_void;
    use core::sync::atomic::AtomicU32;
    use pwrs_sys::{PsHandle, PS_PHASE_BEGIN, PS_PHASE_END, PS_PHASE_PROCESS};

    /// What the test's wrapper does beside the command it wraps.
    #[derive(Default, Clone, Copy, PartialEq)]
    enum Steps {
        /// Every default.
        #[default]
        Forward,
        /// Writes each record the command writes in upper case.
        Upper,
        /// Passes the command every piped record but `skipped`.
        DropSkipped,
        /// Fails on `begun` with a non-terminating error.
        FailSoftly,
        /// Fails on `one` with a terminating error.
        FailHard,
    }

    #[derive(Default)]
    struct Wraps {
        steps: Steps,
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
            super::begin(self, ps)
        }
        fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
            super::process(self, ps)
        }
        fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
            super::end(self, ps)
        }
    }

    impl Wrapper for Wraps {
        fn input(&mut self, record: PsObject, _ps: &Pipeline<'_>) -> PsResult<Option<PsObject>> {
            if self.steps == Steps::DropSkipped && String::from_ps(&record)? == "skipped" {
                return Ok(None);
            }
            Ok(Some(record))
        }

        fn output(&mut self, record: PsObject, ps: &Pipeline<'_>) -> PsResult<()> {
            let text = String::from_ps(&record)?;
            match self.steps {
                Steps::Upper => ps.write_object(&text.to_uppercase().into_ps()?),
                Steps::FailSoftly if text == "begun" => Err(PsError::new(ErrorCategory::InvalidData, "TestSoft", "soft")),
                Steps::FailHard if text == "one" => Err(PsError::new(ErrorCategory::InvalidData, "TestHard", "hard").terminating()),
                _other => ps.write_object(&record),
            }
        }
    }

    /// What one run wrote, in order, and the error ids written.
    struct Ran {
        written: Vec<String>,
        errors: Vec<String>,
        calls: Vec<String>,
        /// The error id each failed phase returned, by phase.
        failed: Vec<String>,
    }

    /// Runs begin, a process for each record piped or one when none is,
    /// and end, as the runtime runs a cmdlet's phases.
    fn run(steps: Steps, piped: &[&str]) -> Ran {
        let _host = testing::install();
        testing::pipe_to_wrapped(piped.iter().map(|r| Value::Str((*r).to_string())).collect());
        let hdr = Instance::<Wraps>::leak_new();
        unsafe { (*(hdr as *mut Instance<Wraps>)).cmdlet.steps = steps };
        let mut phases = vec![PS_PHASE_BEGIN];
        phases.extend(core::iter::repeat_n(PS_PHASE_PROCESS, piped.len().max(1)));
        phases.push(PS_PHASE_END);
        let mut failed = Vec::new();
        for phase in phases {
            if let Err(e) = unsafe { ((*hdr).run)(hdr, phase, PsHandle::NULL, core::ptr::null()) } {
                failed.push(format!("{} terminating={}", e.error_id, e.terminating));
            }
        }
        unsafe { ((*hdr).drop)(hdr) };
        let written = testing::take_output()
            .into_iter()
            .map(|v| match v {
                Value::Str(s) => s,
                other => format!("{other:?}"),
            })
            .collect();
        let errors = testing::take_errors().into_iter().map(|(_, id, _, _)| id).collect();
        Ran { written, errors, calls: testing::wrapped_calls(), failed }
    }

    #[test]
    fn a_wrapper_with_every_default_passes_on_each_record_and_writes_all_the_command_writes() {
        let ran = run(Steps::Forward, &["one", "two"]);
        assert_eq!(ran.calls, ["arguments", "begin Str(\"arguments\")", "process Str(\"one\")", "process Str(\"two\")", "end"]);
        assert_eq!(ran.written, ["begun", "one", "two", "ended"]);
        assert!(ran.errors.is_empty() && ran.failed.is_empty(), "{:?} {:?}", ran.errors, ran.failed);
    }

    #[test]
    fn with_nothing_piped_the_command_steps_once_without_a_record() {
        let ran = run(Steps::Forward, &[]);
        assert_eq!(ran.calls, ["arguments", "begin Str(\"arguments\")", "process", "end"]);
        assert_eq!(ran.written, ["begun", "stepped", "ended"]);
    }

    #[test]
    fn the_output_hook_sees_what_the_command_writes_in_every_phase() {
        assert_eq!(run(Steps::Upper, &["one"]).written, ["BEGUN", "ONE", "ENDED"]);
    }

    #[test]
    fn a_record_the_input_hook_drops_never_reaches_the_command() {
        let ran = run(Steps::DropSkipped, &["one", "skipped", "two"]);
        assert_eq!(ran.calls, ["arguments", "begin Str(\"arguments\")", "process Str(\"one\")", "process Str(\"two\")", "end"]);
        assert_eq!(ran.written, ["begun", "one", "two", "ended"]);
    }

    #[test]
    fn a_non_terminating_error_from_the_output_hook_is_written_and_the_command_goes_on() {
        let ran = run(Steps::FailSoftly, &["one"]);
        assert_eq!(ran.errors, ["TestSoft"]);
        assert_eq!(ran.written, ["one", "ended"]);
        assert!(ran.failed.is_empty(), "{:?}", ran.failed);
    }

    #[test]
    fn a_terminating_error_from_the_output_hook_is_what_the_phase_returns() {
        let ran = run(Steps::FailHard, &["one", "two"]);
        assert_eq!(ran.written, ["begun", "two", "ended"]);
        assert_eq!(ran.failed, ["TestHard terminating=true"]);
    }
}
