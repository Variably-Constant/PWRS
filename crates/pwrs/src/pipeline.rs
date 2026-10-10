//! The pipeline-thread token and the stream API it unlocks.

use crate::host::vtable;
use crate::{PsError, PsObject, PsResult};
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};
use pwrs_sys::{PsHandle, PsStr16, PsStreamKind, PS_HOST_COLOR_CURRENT, PS_OK, PS_STREAM_DEBUG, PS_STREAM_INFORMATION, PS_STREAM_VERBOSE, PS_STREAM_WARNING};

pub use crate::worker::{Order, StopSignal};

/// Proof of being on the pipeline thread inside one lifecycle phase.
/// `!Send` and `!Sync` by construction.
pub struct Pipeline<'ps> {
    cmdlet: PsHandle,
    stopping: &'ps AtomicBool,
    /// The instance's UTF-16 buffer, borrowed for the phase.
    scratch: &'ps core::cell::Cell<Vec<u16>>,
    /// The instance's stop waker slot, which [`Pipeline::on_stop`] fills.
    stop_waker: Option<&'ps crate::stop::StopWakerSlot>,
    /// Answers from [`Pipeline::stream_enabled`]: the stream's kind as
    /// a bit in the low half, and whether it has been asked in the
    /// high half.
    streams: core::cell::Cell<u32>,
    /// Set by the trait's default `begin` and `end` bodies, so the
    /// runtime can tell an empty phase from an implemented one.
    default_phase: core::cell::Cell<bool>,
    /// Set when the phase asked the engine to confirm, which is what
    /// [`crate::surface`] reports a `SupportsShouldProcess` cmdlet
    /// against.
    asked: core::cell::Cell<bool>,
    _not_send: PhantomData<*mut ()>,
}

#[inline]
pub(crate) fn utf16(s: &str) -> Vec<u16> {
    crate::text::to_utf16(s)
}

#[inline]
pub(crate) fn str16(v: &[u16]) -> PsStr16 {
    PsStr16 { ptr: v.as_ptr(), len: v.len() }
}

#[inline]
pub(crate) fn check(status: i32, err: PsHandle) -> PsResult<()> {
    if status == PS_OK {
        return Ok(());
    }
    let mut e = PsError::new(crate::ErrorCategory::NotSpecified, "PwrsRuntimeError", describe(err));
    if status == pwrs_sys::PS_ERR_PIPELINE_STOPPED {
        e.category = crate::ErrorCategory::OperationStopped;
        e.terminating = true;
    }
    Err(e)
}

pub(crate) fn describe(err: PsHandle) -> String {
    if err.is_null() {
        return "runtime call failed with no exception".into();
    }
    let mut buf = vec![0u16; 512];
    let mut len = 0usize;
    unsafe { (vtable().exception_describe)(err, buf.as_mut_ptr(), buf.len(), &mut len) };
    if len > buf.len() {
        buf.resize(len, 0);
        unsafe { (vtable().exception_describe)(err, buf.as_mut_ptr(), buf.len(), &mut len) };
    }
    let s = crate::text::from_utf16(&buf[..len.min(buf.len())]);
    unsafe { (vtable().free_handle)(err) };
    s
}

/// Writes to the verbose stream, building the text only when the
/// engine would keep the record.
///
/// ```ignore
/// pwrs::verbose!(ps, "greeting {name}")?;
/// ```
#[macro_export]
macro_rules! verbose {
    ($ps:expr, $($arg:tt)*) => {
        $ps.verbose_if(|| ::std::format!($($arg)*))
    };
}

/// Writes to the debug stream, building the text only when the engine
/// would keep the record. See [`verbose!`].
#[macro_export]
macro_rules! debug {
    ($ps:expr, $($arg:tt)*) => {
        $ps.debug_if(|| ::std::format!($($arg)*))
    };
}

/// Writes to the warning stream, building the text only when the
/// engine would keep the record. See [`verbose!`].
#[macro_export]
macro_rules! warning {
    ($ps:expr, $($arg:tt)*) => {
        $ps.warning_if(|| ::std::format!($($arg)*))
    };
}

/// Writes to the information stream, building the text only when the
/// engine would keep the record. See [`verbose!`].
#[macro_export]
macro_rules! information {
    ($ps:expr, $($arg:tt)*) => {
        $ps.information_if(|| ::std::format!($($arg)*))
    };
}

impl<'ps> Pipeline<'ps> {
    /// # Safety
    /// Only the generated entry point constructs one, on the pipeline
    /// thread, for the duration of one lifecycle phase.
    #[inline]
    pub unsafe fn new(cmdlet: PsHandle, stopping: &'ps AtomicBool, scratch: &'ps core::cell::Cell<Vec<u16>>) -> Self {
        Pipeline {
            cmdlet,
            stopping,
            scratch,
            stop_waker: None,
            streams: core::cell::Cell::new(0),
            default_phase: core::cell::Cell::new(false),
            asked: core::cell::Cell::new(false),
            _not_send: PhantomData,
        }
    }

    /// The token with the instance's stop waker slot, which
    /// [`Pipeline::on_stop`] needs.
    #[inline]
    pub fn with_stop_waker(mut self, slot: &'ps crate::stop::StopWakerSlot) -> Self {
        self.stop_waker = Some(slot);
        self
    }

    /// True once the engine called `StopProcessing`.
    #[inline]
    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::Relaxed)
    }

    /// Registers `waker` to run when the pipeline stops, so a call
    /// blocked on something of its own, such as a channel, a ring or a
    /// parked thread, wakes on Ctrl+C with no poll. The waker runs at
    /// most once: on the thread that stops the pipeline, or here at once
    /// when the stop has already come. It must be short and must touch
    /// no PowerShell object. One waker at a time: a second registration
    /// while the first guard lives is refused with `PwrsStopWakerHeld`.
    ///
    /// ```ignore
    /// let me = std::thread::current();
    /// let waker = ps.on_stop(move || me.unpark())?;
    /// while !ps.stopping() && !done() {
    ///     std::thread::park();
    /// }
    /// drop(waker);
    /// ```
    pub fn on_stop(&self, waker: impl FnOnce() + Send + 'static) -> PsResult<crate::stop::StopWaker<'ps>> {
        let slot = self.stop_waker.ok_or_else(|| {
            PsError::new(crate::ErrorCategory::InvalidOperation, "PwrsStopWakerUnavailable", "this pipeline token was made without its instance's stop waker slot")
        })?;
        crate::stop::on_stop(self.stopping, slot, Box::new(waker))
    }

    /// Called only by the default `Cmdlet::begin` and `Cmdlet::end`
    /// bodies. Not part of the public surface.
    #[doc(hidden)]
    #[inline]
    pub fn __pwrs_default_phase(&self) {
        self.default_phase.set(true);
    }

    /// True when the phase that just ran was the trait's default body.
    #[inline]
    pub(crate) fn took_default_phase(&self) -> bool {
        self.default_phase.get()
    }

    /// True when the phase that just ran asked the engine to confirm.
    #[inline]
    pub(crate) fn asked(&self) -> bool {
        self.asked.get()
    }

    /// The managed cmdlet's handle, for vtable entries that need it.
    pub fn cmdlet_handle(&self) -> PsHandle {
        self.cmdlet
    }

    /// `WriteObject(obj)`: one object, never enumerated.
    pub fn write_object(&self, obj: &PsObject) -> PsResult<()> {
        crate::trace::on_handle_write();
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().write_object)(self.cmdlet, obj.as_raw(), 0, &mut err) };
        check(s, err)
    }

    /// `WriteObject(obj, true)`: enumerates a collection into the pipe.
    pub fn write_enumerated(&self, obj: &PsObject) -> PsResult<()> {
        crate::trace::on_handle_write();
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().write_object)(self.cmdlet, obj.as_raw(), 1, &mut err) };
        check(s, err)
    }

    /// Converts and writes any [`crate::IntoPs`] value. `Vec<T>` is
    /// enumerated; wrap in [`crate::PsArray`] to write it whole.
    ///
    /// Scalars take a direct vtable entry: one crossing and no
    /// `GCHandle`, against three crossings and a handle alloc/free for
    /// the generic object path.
    #[inline]
    pub fn write<T: crate::IntoPs>(&self, value: T) -> PsResult<()> {
        value.write_to(self)
    }

    /// Writes a string with no handle round trip.
    #[inline]
    pub fn write_str(&self, text: &str) -> PsResult<()> {
        crate::trace::on_direct_write();
        let mut err = PsHandle::NULL;
        let s = crate::text::with_utf16_in(self.scratch, text, |u| unsafe { (vtable().write_string)(self.cmdlet, str16(u), &mut err) });
        check(s, err)
    }

    /// Writes an integer with no handle round trip.
    #[inline]
    pub fn write_i64(&self, value: i64) -> PsResult<()> {
        crate::trace::on_direct_write();
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().write_i64)(self.cmdlet, value, &mut err) };
        check(s, err)
    }

    /// Writes a double with no handle round trip.
    #[inline]
    pub fn write_f64(&self, value: f64) -> PsResult<()> {
        crate::trace::on_direct_write();
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().write_f64)(self.cmdlet, value, &mut err) };
        check(s, err)
    }

    /// Writes a boolean with no handle round trip.
    #[inline]
    pub fn write_bool(&self, value: bool) -> PsResult<()> {
        crate::trace::on_direct_write();
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().write_bool)(self.cmdlet, value as u8, &mut err) };
        check(s, err)
    }

    /// `WriteError` or `ThrowTerminatingError` depending on the flag. An
    /// error with details reaches the record as its exception's
    /// InnerException.
    pub fn write_error(&self, e: &PsError) -> PsResult<()> {
        let msg = utf16(&e.message);
        let id = utf16(&e.error_id);
        let target = e.target.as_ref().map(|t| t.as_raw()).unwrap_or(PsHandle::NULL);
        let mut err = PsHandle::NULL;
        let s = match &e.details {
            None => unsafe {
                (vtable().write_error)(self.cmdlet, str16(&msg), str16(&id), e.category as u32, target, e.terminating as u8, &mut err)
            },
            Some(details) => {
                let details = utf16(details);
                unsafe {
                    (vtable().write_error_details)(
                        self.cmdlet,
                        str16(&msg),
                        str16(&id),
                        e.category as u32,
                        target,
                        e.terminating as u8,
                        str16(&details),
                        &mut err,
                    )
                }
            }
        };
        check(s, err)
    }

    /// The verbose, debug, warning and information streams. The text
    /// is built in the instance's buffer.
    pub(crate) fn stream(&self, kind: u32, text: &str) -> PsResult<()> {
        let mut err = PsHandle::NULL;
        let s = crate::text::with_utf16_in(self.scratch, text, |u| unsafe { (vtable().write_stream)(self.cmdlet, kind, str16(u), &mut err) });
        check(s, err)
    }
    /// Whether the engine would keep a record written to `kind`, from
    /// the stream's common parameter where it is bound and from the
    /// session's preference variable otherwise. Asked once per phase
    /// and kept.
    pub fn stream_enabled(&self, kind: PsStreamKind) -> bool {
        let on_bit = 1u32 << kind;
        let asked_bit = on_bit << 16;
        let cached = self.streams.get();
        if cached & asked_bit != 0 {
            return cached & on_bit != 0;
        }
        let mut enabled = 0u8;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().stream_enabled)(self.cmdlet, kind, &mut enabled, &mut err) };
        // A runtime that cannot answer reads as on, so a failure costs
        // a wasted format and never a message the caller believes it
        // wrote.
        let on = if status == PS_OK {
            enabled != 0
        } else {
            if !err.is_null() {
                unsafe { (vtable().free_handle)(err) };
            }
            true
        };
        self.streams.set(cached | asked_bit | if on { on_bit } else { 0 });
        on
    }

    /// Whether `-Verbose` or `$VerbosePreference` would keep the record.
    #[inline]
    pub fn verbose_enabled(&self) -> bool {
        self.stream_enabled(PS_STREAM_VERBOSE)
    }

    /// Whether `-Debug` or `$DebugPreference` would keep the record.
    #[inline]
    pub fn debug_enabled(&self) -> bool {
        self.stream_enabled(PS_STREAM_DEBUG)
    }

    /// Whether the engine would keep a warning: unless `-WarningAction`
    /// or `$WarningPreference` is Ignore with no `-WarningVariable`
    /// bound, since SilentlyContinue still fills `-WarningVariable`.
    #[inline]
    pub fn warning_enabled(&self) -> bool {
        self.stream_enabled(PS_STREAM_WARNING)
    }

    /// Whether the engine would keep an information record: unless
    /// `-InformationAction` or `$InformationPreference` is Ignore with no
    /// `-InformationVariable` bound, since SilentlyContinue, the default,
    /// still reaches `-InformationVariable` and a `6>` redirection.
    #[inline]
    pub fn information_enabled(&self) -> bool {
        self.stream_enabled(PS_STREAM_INFORMATION)
    }

    /// [`Pipeline::verbose`] with `f` called only when the record is
    /// kept. With the stream off it writes nothing and succeeds.
    pub fn verbose_if(&self, f: impl FnOnce() -> String) -> PsResult<()> {
        if !self.verbose_enabled() {
            return Ok(());
        }
        self.verbose(&f())
    }

    /// [`Pipeline::debug`] with the text built only when it is kept.
    pub fn debug_if(&self, f: impl FnOnce() -> String) -> PsResult<()> {
        if !self.debug_enabled() {
            return Ok(());
        }
        self.debug(&f())
    }

    /// [`Pipeline::warning`] with the text built only when it is kept.
    pub fn warning_if(&self, f: impl FnOnce() -> String) -> PsResult<()> {
        if !self.warning_enabled() {
            return Ok(());
        }
        self.warning(&f())
    }

    /// [`Pipeline::information`] with the text built only when kept.
    pub fn information_if(&self, f: impl FnOnce() -> String) -> PsResult<()> {
        if !self.information_enabled() {
            return Ok(());
        }
        self.information(&f())
    }

    pub fn verbose(&self, text: &str) -> PsResult<()> {
        self.stream(PS_STREAM_VERBOSE, text)
    }
    pub fn debug(&self, text: &str) -> PsResult<()> {
        self.stream(PS_STREAM_DEBUG, text)
    }
    pub fn warning(&self, text: &str) -> PsResult<()> {
        self.stream(PS_STREAM_WARNING, text)
    }
    pub fn information(&self, text: &str) -> PsResult<()> {
        self.stream(PS_STREAM_INFORMATION, text)
    }

    /// Writes `text` to the host as `Write-Host` does: an information
    /// record tagged `PSHOST` whose message carries the text, its
    /// colors and whether the line stays open. The engine shows it
    /// whatever `$InformationPreference` says, unless the preference is
    /// Ignore or a `6>` redirection takes the record; `-InformationVariable`
    /// keeps it, and a transcript records it as one line. A color that is
    /// `None` is the host's current one, as `Write-Host` takes it, and is
    /// left unset on a host with no `RawUI`. The text goes as it is, so
    /// any escape codes in it reach the host, which renders or strips
    /// them as `$PSStyle.OutputRendering` says.
    pub fn write_host(&self, text: &str, foreground: Option<crate::ConsoleColor>, background: Option<crate::ConsoleColor>, no_new_line: bool) -> PsResult<()> {
        let t = utf16(text);
        let fg = foreground.map_or(PS_HOST_COLOR_CURRENT, |c| c as i32);
        let bg = background.map_or(PS_HOST_COLOR_CURRENT, |c| c as i32);
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().write_host)(self.cmdlet, str16(&t), fg, bg, no_new_line as u8, &mut err) };
        check(s, err)
    }

    /// `WriteProgress` of a record with an activity, a status and a
    /// percentage; `percent` below zero writes the record that completes
    /// the activity and removes its bar. [`Pipeline::write_progress`]
    /// sets every field.
    pub fn progress(&self, activity_id: i32, activity: &str, status: &str, percent: i32) -> PsResult<()> {
        let a = utf16(activity);
        let st = utf16(status);
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().write_progress)(self.cmdlet, activity_id, str16(&a), str16(&st), percent, &mut err) };
        check(s, err)
    }

    /// `WriteProgress` of `record` with every field it carries: the
    /// parent activity, the current operation, the seconds left, and a
    /// percentage that may be negative on a processing record, which
    /// shows none. The record completes the activity only when
    /// `completed` is set.
    pub fn write_progress(&self, record: &crate::Progress) -> PsResult<()> {
        let activity = utf16(&record.activity);
        let status = utf16(&record.status);
        let operation = utf16(&record.current_operation);
        let mut err = PsHandle::NULL;
        let s = unsafe {
            (vtable().write_progress_record)(
                self.cmdlet,
                record.activity_id,
                record.parent_activity_id,
                str16(&activity),
                str16(&status),
                str16(&operation),
                record.percent_complete,
                record.seconds_remaining,
                record.completed as u8,
                &mut err,
            )
        };
        check(s, err)
    }

    /// `ShouldProcess(target, action)` for `-WhatIf` / `-Confirm`.
    pub fn should_process(&self, target: &str, action: &str) -> PsResult<bool> {
        self.asked.set(true);
        let t = utf16(target);
        let a = utf16(action);
        let mut yes = 0u8;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().should_process)(self.cmdlet, str16(&t), str16(&a), &mut yes, &mut err) };
        check(s, err).map(|_| yes != 0)
    }

    /// `ShouldContinue(query, caption)` for destructive confirmations.
    pub fn should_continue(&self, query: &str, caption: &str) -> PsResult<bool> {
        self.asked.set(true);
        let q = utf16(query);
        let c = utf16(caption);
        let mut yes = 0u8;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().should_continue)(self.cmdlet, str16(&q), str16(&c), &mut yes, &mut err) };
        check(s, err).map(|_| yes != 0)
    }

    /// The running cmdlet's `MyInvocation`: its `InvocationInfo`, read
    /// off the managed cmdlet with one dynamic member access. It
    /// carries where the command stands in its pipeline,
    /// `PipelinePosition` counting from 1 and `PipelineLength`, and
    /// `InvocationName`, `ExpectingInput`, `Line` and the rest. It is
    /// set before `begin`, so any phase can read it.
    pub fn invocation(&self) -> PsResult<PsObject> {
        let cmdlet = unsafe { PsObject::from_raw((vtable().clone_handle)(self.cmdlet)) };
        cmdlet.get("MyInvocation")
    }

    /// The host's user interface, for prompts and lines read from the
    /// person at the console. A host that cannot prompt refuses with
    /// the engine's own error.
    pub fn host_ui(&self) -> PsResult<crate::host_ui::HostUi> {
        let cmdlet = unsafe { PsObject::from_raw((vtable().clone_handle)(self.cmdlet)) };
        crate::host_ui::HostUi::of(&cmdlet)
    }

    /// The engine events of the runspace this cmdlet runs in, which any
    /// thread raises events through, during this call and after it
    /// returns, for as long as the handle lives. Read off the managed
    /// cmdlet's `Events` with one dynamic member access.
    pub fn events(&self) -> PsResult<crate::PsEvents> {
        let cmdlet = unsafe { PsObject::from_raw((vtable().clone_handle)(self.cmdlet)) };
        Ok(crate::PsEvents::over(cmdlet.get("Events")?))
    }

    /// The bound value of a parameter property, or `$null`.
    pub fn parameter(&self, name: &str) -> PsResult<PsObject> {
        let n = utf16(name);
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().get_parameter)(self.cmdlet, str16(&n), &mut out, &mut err) };
        check(s, err)?;
        Ok(unsafe { PsObject::from_raw(out) })
    }

    pub fn parameter_is_bound(&self, name: &str) -> bool {
        let n = utf16(name);
        let mut b = 0u8;
        unsafe { (vtable().parameter_is_bound)(self.cmdlet, str16(&n), &mut b) };
        b != 0
    }

    pub fn variable(&self, name: &str) -> PsResult<PsObject> {
        let n = utf16(name);
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().get_variable)(self.cmdlet, str16(&n), &mut out, &mut err) };
        check(s, err)?;
        Ok(unsafe { PsObject::from_raw(out) })
    }

    pub fn set_variable(&self, name: &str, value: &PsObject) -> PsResult<()> {
        let n = utf16(name);
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().set_variable)(self.cmdlet, str16(&n), value.as_raw(), &mut err) };
        check(s, err)
    }

    /// Resolves a PSPath through the session's providers. `literal`
    /// skips wildcard expansion.
    pub fn resolve_path(&self, path: &str, literal: bool) -> PsResult<Vec<String>> {
        let p = utf16(path);
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().resolve_path)(self.cmdlet, str16(&p), literal as u8, &mut out, &mut err) };
        check(s, err)?;
        let arr = unsafe { PsObject::from_raw(out) };
        <Vec<String> as crate::FromPs>::from_ps(&arr)
    }

    /// Runs the command named `name`, a cmdlet, function or alias the
    /// session can see, with `parameters` bound by name, and returns
    /// every object it wrote. The command is added by name, resolved as
    /// a typed command is, so a module's function runs in its module's
    /// scope, and invoked in the current runspace without a script
    /// block, so nothing is parsed. Its non-terminating errors reach
    /// this cmdlet's error stream, as they would had the user run it; a
    /// terminating one is the `Err`, and so is a name no command answers
    /// to.
    pub fn invoke(&self, name: &str, parameters: &[(&str, PsObject)]) -> PsResult<Vec<PsObject>> {
        self.invoke_with_input(name, parameters, None)
    }

    /// As [`Pipeline::invoke`], with `input` piped to the command: a
    /// collection is unrolled into records, anything else is one.
    pub fn invoke_with_input(&self, name: &str, parameters: &[(&str, PsObject)], input: Option<&PsObject>) -> PsResult<Vec<PsObject>> {
        let table = if parameters.is_empty() {
            PsObject::null()
        } else {
            let t = crate::PsHashtable::new()?;
            for (key, value) in parameters {
                t.set(key, value.clone())?;
            }
            t.0
        };
        let n = utf16(name);
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let s = unsafe {
            (vtable().invoke_command)(
                self.cmdlet,
                str16(&n),
                table.as_raw(),
                input.map_or(PsHandle::NULL, |i| i.as_raw()),
                &mut out,
                &mut err,
            )
        };
        check(s, err)?;
        let results = unsafe { PsObject::from_raw(out) };
        <Vec<PsObject> as crate::FromPs>::from_ps(&results)
    }
}
