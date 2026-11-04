//! The pipeline-thread token and the stream API it unlocks.

use crate::host::vtable;
use crate::{PsError, PsObject, PsResult, PsTransaction};
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
    /// The results of [`Pipeline::stream_enabled`], one bit per stream
    /// kind: the low half says whether the stream is on, and the high
    /// half whether it has been read yet.
    streams: core::cell::Cell<u32>,
    /// Set by the trait's default `begin` and `end` bodies, so the
    /// runtime can tell an empty phase from an implemented one.
    default_phase: core::cell::Cell<bool>,
    /// Set when the phase called `should_process` or `should_continue`;
    /// [`crate::surface`] counts these calls for each cmdlet.
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
/// record would go somewhere: be shown, saved in a variable or
/// redirected, as one written by PowerShell's own `Write-Verbose` would.
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

/// Writes to the debug stream, building the text only when the record
/// would go somewhere. See [`verbose!`].
#[macro_export]
macro_rules! debug {
    ($ps:expr, $($arg:tt)*) => {
        $ps.debug_if(|| ::std::format!($($arg)*))
    };
}

/// Writes to the warning stream, building the text only when the
/// record would go somewhere. See [`verbose!`].
#[macro_export]
macro_rules! warning {
    ($ps:expr, $($arg:tt)*) => {
        $ps.warning_if(|| ::std::format!($($arg)*))
    };
}

/// Writes to the information stream, building the text only when the
/// record would go somewhere. See [`verbose!`].
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

    /// True when the phase that just ran called `should_process` or
    /// `should_continue`.
    #[inline]
    pub(crate) fn asked(&self) -> bool {
        self.asked.get()
    }

    /// Counts the phase as having called `should_process`, for a command
    /// it begins that takes `-WhatIf` and so honors the cmdlet's
    /// ShouldProcess itself.
    #[inline]
    pub(crate) fn mark_asked(&self) {
        self.asked.set(true);
    }

    /// The managed cmdlet's handle, for vtable entries that need it.
    pub fn cmdlet_handle(&self) -> PsHandle {
        self.cmdlet
    }

    /// The `InstanceId` of the runspace running the command, the value
    /// `[runspace]::DefaultRunspace.InstanceId` gives a script there. An `on_remove`
    /// taking a [`crate::Removal`] is given it for a removal of that
    /// runspace's import, so a command records it and the hook compares.
    pub fn runspace_id(&self) -> PsResult<crate::PsGuid> {
        let runspace = crate::PsType::from_name("System.Management.Automation.Runspaces.Runspace").call_static("get_DefaultRunspace", &[])?;
        crate::FromPs::from_ps(&runspace.get("InstanceId")?)
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
    /// Scalars take a direct vtable entry: one call into the runtime and
    /// no `GCHandle`, against three calls and a handle alloc/free for the
    /// generic object path.
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
    /// InnerException. An error that names its exception type has that
    /// exception as the record's, so a script's catch of the type catches
    /// it, and the record keeps its id, category and target.
    pub fn write_error(&self, e: &PsError) -> PsResult<()> {
        let msg = utf16(&e.message);
        let id = utf16(&e.error_id);
        let target = e.target.as_ref().map(|t| t.as_raw()).unwrap_or(PsHandle::NULL);
        let mut err = PsHandle::NULL;
        if let Some(type_name) = &e.exception {
            let exception = unsafe { PsObject::from_raw(crate::runtime::named_exception(type_name, &msg, e)) };
            let s = unsafe { (vtable().write_error_exception)(self.cmdlet, exception.as_raw(), str16(&id), e.category as u32, target, e.terminating as u8, &mut err) };
            return check(s, err);
        }
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

    /// The verbose, debug, warning and information streams. Text of more
    /// than 512 UTF-8 bytes is built in the instance's buffer, shorter
    /// text on the stack.
    pub(crate) fn stream(&self, kind: u32, text: &str) -> PsResult<()> {
        let mut err = PsHandle::NULL;
        let s = crate::text::with_utf16_in(self.scratch, text, |u| unsafe { (vtable().write_stream)(self.cmdlet, kind, str16(u), &mut err) });
        check(s, err)
    }
    /// Whether a record written to `kind` would go somewhere, by the
    /// preference the engine gives a compiled cmdlet: the stream's own
    /// common parameter where it is bound, then `-Debug`, under which
    /// PowerShell prompts the user after each verbose record and warning,
    /// and `-Verbose`, which shows warnings, then the session's preference
    /// variable. Read from the host once per cmdlet instance: the managed
    /// cmdlet works each stream out once for its instance, so the
    /// instance's later phases and records use the stored result.
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
        // A failed read counts as on, so a failure costs at most a
        // message formatted for nothing, and never a message silently
        // left unwritten.
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

    /// Starts the phase knowing what earlier phases of the instance read
    /// of [`Pipeline::stream_enabled`], from [`Pipeline::streams`].
    #[inline]
    pub(crate) fn with_streams(self, known: u32) -> Self {
        self.streams.set(known);
        self
    }

    /// What [`Pipeline::stream_enabled`] has read so far, for the next
    /// phase of the instance to start from.
    #[inline]
    pub(crate) fn streams(&self) -> u32 {
        self.streams.get()
    }

    /// Whether `-Verbose`, `-Debug` or `$VerbosePreference` would show the record.
    #[inline]
    pub fn verbose_enabled(&self) -> bool {
        self.stream_enabled(PS_STREAM_VERBOSE)
    }

    /// Whether `-Debug` or `$DebugPreference` would show the record.
    #[inline]
    pub fn debug_enabled(&self) -> bool {
        self.stream_enabled(PS_STREAM_DEBUG)
    }

    /// Whether a warning would go somewhere: `-WarningAction` where it
    /// is bound, then `-Debug` and `-Verbose`, which show it, then
    /// `$WarningPreference`; only Ignore with no `-WarningVariable`
    /// bound sends it nowhere, since SilentlyContinue still fills
    /// `-WarningVariable`.
    #[inline]
    pub fn warning_enabled(&self) -> bool {
        self.stream_enabled(PS_STREAM_WARNING)
    }

    /// Whether an information record would go somewhere: unless
    /// `-InformationAction` or `$InformationPreference` is Ignore with no
    /// `-InformationVariable` bound, since SilentlyContinue, the default,
    /// still reaches `-InformationVariable` and a `6>` redirection.
    #[inline]
    pub fn information_enabled(&self) -> bool {
        self.stream_enabled(PS_STREAM_INFORMATION)
    }

    /// [`Pipeline::verbose`] with `f` called only when the record would
    /// go somewhere. With the stream off it writes nothing and succeeds.
    pub fn verbose_if(&self, f: impl FnOnce() -> String) -> PsResult<()> {
        if !self.verbose_enabled() {
            return Ok(());
        }
        self.verbose(&f())
    }

    /// [`Pipeline::debug`] with the text built only when it would go somewhere.
    pub fn debug_if(&self, f: impl FnOnce() -> String) -> PsResult<()> {
        if !self.debug_enabled() {
            return Ok(());
        }
        self.debug(&f())
    }

    /// [`Pipeline::warning`] with the text built only when it would go somewhere.
    pub fn warning_if(&self, f: impl FnOnce() -> String) -> PsResult<()> {
        if !self.warning_enabled() {
            return Ok(());
        }
        self.warning(&f())
    }

    /// [`Pipeline::information`] with the text built only when it would go somewhere.
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
    /// record tagged `PSHOST` whose message has the text, its colors
    /// and whether the line stays open. The engine shows it
    /// whatever `$InformationPreference` says, unless the preference is
    /// Ignore or a `6>` redirection takes the record; `-InformationVariable`
    /// saves it, and a transcript records it as one line. A color that is
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

    /// `WriteProgress` of `record` with every field it has: the
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

    /// A ShouldProcess form other than `(target, action)`, with its
    /// result and whether that result came from `-WhatIf`.
    fn should_process_form(&self, form: u8, a: &str, b: &str, caption: &str) -> PsResult<(bool, ShouldProcessReason)> {
        self.asked.set(true);
        let (a, b, caption) = (utf16(a), utf16(b), utf16(caption));
        let (mut yes, mut reason) = (0u8, 0u8);
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().should_process_form)(self.cmdlet, form, str16(&a), str16(&b), str16(&caption), &mut yes, &mut reason, &mut err) };
        check(s, err)?;
        let reason = if reason == 1 { ShouldProcessReason::WhatIf } else { ShouldProcessReason::None };
        Ok((yes != 0, reason))
    }

    /// `ShouldProcess(target)`: the engine names the command as the
    /// action.
    pub fn should_process_target(&self, target: &str) -> PsResult<bool> {
        self.should_process_form(0, target, "", "").map(|(yes, _)| yes)
    }

    /// `ShouldProcess(verboseDescription, verboseWarning, caption)`:
    /// `-WhatIf` and `-Verbose` show the description, and `-Confirm`
    /// prompts with the warning under the caption.
    pub fn should_process_described(&self, verbose_description: &str, verbose_warning: &str, caption: &str) -> PsResult<bool> {
        self.should_process_form(1, verbose_description, verbose_warning, caption).map(|(yes, _)| yes)
    }

    /// `ShouldProcess` as [`should_process_described`](Self::should_process_described),
    /// with whether the result came from `-WhatIf`.
    pub fn should_process_reason(&self, verbose_description: &str, verbose_warning: &str, caption: &str) -> PsResult<(bool, ShouldProcessReason)> {
        self.should_process_form(2, verbose_description, verbose_warning, caption)
    }

    /// `ShouldContinue(query, caption, ref yesToAll, ref noToAll)`: the
    /// prompt offers Yes to All and No to All, and the answer to all applies
    /// to this invocation's later records, for which the user is then not
    /// prompted.
    pub fn should_continue_all(&self, query: &str, caption: &str) -> PsResult<bool> {
        self.should_continue_all_form(query, caption, 0)
    }

    /// [`should_continue_all`](Self::should_continue_all) with
    /// `hasSecurityImpact`, which makes No the prompt's default.
    pub fn should_continue_all_with_impact(&self, query: &str, caption: &str, has_security_impact: bool) -> PsResult<bool> {
        self.should_continue_all_form(query, caption, if has_security_impact { 2 } else { 1 })
    }

    fn should_continue_all_form(&self, query: &str, caption: &str, impact: u8) -> PsResult<bool> {
        self.asked.set(true);
        let (q, c) = (utf16(query), utf16(caption));
        let mut yes = 0u8;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().should_continue_all)(self.cmdlet, str16(&q), str16(&c), impact, &mut yes, &mut err) };
        check(s, err).map(|_| yes != 0)
    }

    /// The values the caller gave a cmdlet declaring `supports_paging`:
    /// `-First`, `-Skip` and `-IncludeTotalCount`, read from the engine's
    /// `PagingParameters`. The cmdlet applies them itself, so it can skip
    /// at the source rather than make records nobody reads. A cmdlet
    /// without `supports_paging` gets an error naming it.
    pub fn paging(&self) -> PsResult<PsPaging> {
        let mut first = u64::MAX;
        let mut skip = 0u64;
        let mut include_total_count = 0u8;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().paging)(self.cmdlet, &mut first, &mut skip, &mut include_total_count, &mut err) };
        check(s, err)?;
        Ok(PsPaging { first, skip, include_total_count: include_total_count != 0 })
    }

    /// Writes the total count the caller requests with
    /// `-IncludeTotalCount`, as the engine's
    /// `PagingParameters.NewTotalCount` makes it: the total, whose text
    /// says whether it is exact, estimated or not known, with an
    /// `Accuracy` property. An `accuracy` of 1.0 is an exact count, 0.0
    /// one not known, and a value between them an estimate.
    pub fn write_total_count(&self, total: u64, accuracy: f64) -> PsResult<()> {
        crate::trace::on_handle_write();
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().write_total_count)(self.cmdlet, total, accuracy, &mut err) };
        check(s, err)
    }

    /// The PowerShell transaction a cmdlet declaring
    /// `supports_transactions` runs in: `Some` when it was given
    /// `-UseTransaction` inside `Start-Transaction`, so its work can be
    /// enlisted with [`PsTransaction::enlist`]. Only Windows PowerShell 5.1
    /// gives a cmdlet `-UseTransaction`, so in PowerShell 7 this is always
    /// `None`. A cmdlet without `supports_transactions` gets an error
    /// naming it.
    pub fn transaction(&self) -> PsResult<Option<PsTransaction>> {
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().transaction_current)(self.cmdlet, &mut out, &mut err) };
        check(s, err)?;
        Ok(if out.is_null() { None } else { Some(PsTransaction::from_object(unsafe { PsObject::from_raw(out) })) })
    }

    /// Starts a job that runs `work` on a thread of its own, and returns
    /// the job for the cmdlet to write. PowerShell's job cmdlets see it
    /// as a `RustJob`: Get-Job lists it, Receive-Job reads what `work`
    /// writes through its [`crate::JobContext`], Wait-Job waits for it,
    /// Stop-Job requests a stop and Remove-Job removes it. `name` is its
    /// name; with `None` the engine names it `Job<n>`.
    ///
    /// ```ignore
    /// let job = ps.start_job(Some("Count"), move |job| {
    ///     for n in 1..=count {
    ///         if job.stopping() {
    ///             break;
    ///         }
    ///         job.write(n)?;
    ///     }
    ///     Ok(())
    /// })?;
    /// ps.write_object(&job)?;
    /// ```
    pub fn start_job(&self, name: Option<&str>, work: impl FnOnce(&crate::JobContext) -> PsResult<()> + Send + 'static) -> PsResult<PsObject> {
        crate::job::start(self.cmdlet, name, work)
    }

    /// [`start_job`](Self::start_job) with the work as a value of a type
    /// implementing [`crate::JobBody`].
    pub fn start_job_body(&self, name: Option<&str>, body: impl crate::JobBody) -> PsResult<PsObject> {
        crate::job::start(self.cmdlet, name, body)
    }

    /// Whether the caller gave `-AsJob`, the switch `#[cmdlet(as_job)]`
    /// declares; false for a cmdlet without it.
    pub fn as_job(&self) -> PsResult<bool> {
        let given = self.parameter("AsJob")?;
        if given.is_null() {
            return Ok(false);
        }
        <bool as crate::FromPs>::from_ps(&given.get("IsPresent")?)
    }

    /// The running cmdlet's `MyInvocation`: its `InvocationInfo`, read
    /// off the managed cmdlet with one dynamic member access. It has
    /// the command's place in its pipeline, `PipelinePosition` counting
    /// from 1 and `PipelineLength`, as well as `InvocationName`,
    /// `ExpectingInput`, `Line` and the rest. It is set before `begin`,
    /// so any phase can read it.
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
        Ok(crate::PsEvents::of_cmdlet(cmdlet.get("Events")?, cmdlet))
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
    /// terminating one is the `Err`, and so is a name that matches no
    /// command.
    pub fn invoke(&self, name: &str, parameters: &[(&str, PsObject)]) -> PsResult<Vec<PsObject>> {
        self.invoke_with_input(name, parameters, None)
    }

    /// As [`Pipeline::invoke`], with `input` piped to the command: a
    /// collection is unrolled into records, anything else is one.
    pub fn invoke_with_input(&self, name: &str, parameters: &[(&str, PsObject)], input: Option<&PsObject>) -> PsResult<Vec<PsObject>> {
        invoke_on(self.cmdlet, name, parameters, input)
    }

    /// Begins the command named `command`, resolved as a typed command
    /// is, as a [`PsSteppable`](crate::PsSteppable) whose output and
    /// streams are this cmdlet's, with `parameters` bound by name: how a
    /// cmdlet wraps another, passing it records from its own `process` and
    /// ending it from its own `end`. Each of `-WhatIf`, `-Confirm`,
    /// `-Verbose`, `-Debug`, `-ErrorAction`, `-WarningAction` and
    /// `-InformationAction` the caller gave this cmdlet goes to the
    /// command too, unless `parameters` names it or the command does not
    /// take it, so `-WhatIf` on the wrapper reaches what it wraps. For
    /// `cargo pwrs test`'s check that a `supports_should_process` cmdlet
    /// calls `should_process`, a command taking `-WhatIf` counts as this
    /// cmdlet calling it. A
    /// name that matches no command is PowerShell's own
    /// `CommandNotFoundException`, ending the command as a compiled
    /// cmdlet's would.
    pub fn steppable(&self, command: &str, parameters: &[(&str, PsObject)]) -> PsResult<crate::PsSteppable> {
        let (step, delegates) = crate::PsSteppable::begin(self.cmdlet, command, parameters)?;
        if delegates {
            self.asked.set(true);
        }
        Ok(step)
    }
}

/// [`Pipeline::invoke_with_input`] through the managed cmdlet `cmdlet`,
/// on its pipeline thread; on any other thread the entry refuses with
/// status 3.
pub(crate) fn invoke_on(cmdlet: PsHandle, name: &str, parameters: &[(&str, PsObject)], input: Option<&PsObject>) -> PsResult<Vec<PsObject>> {
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
    let s = unsafe { (vtable().invoke_command)(cmdlet, str16(&n), table.as_raw(), input.map_or(PsHandle::NULL, |i| i.as_raw()), &mut out, &mut err) };
    check(s, err)?;
    let results = unsafe { PsObject::from_raw(out) };
    <Vec<PsObject> as crate::FromPs>::from_ps(&results)
}

/// Why `ShouldProcess` returned what it did: PowerShell's
/// `ShouldProcessReason`, from [`Pipeline::should_process_reason`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShouldProcessReason {
    /// The result came from the preferences or from the user's answer.
    None,
    /// `-WhatIf` made the result false, after showing what would happen.
    WhatIf,
}

/// The values `-First`, `-Skip` and `-IncludeTotalCount` gave a cmdlet
/// declaring `supports_paging`, from [`Pipeline::paging`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PsPaging {
    /// `-First`: how many records to write after the skipped ones;
    /// `u64::MAX` when the caller did not give it.
    pub first: u64,
    /// `-Skip`: how many records to leave out before the first one
    /// written; 0 when the caller did not give it.
    pub skip: u64,
    /// `-IncludeTotalCount`: whether to write the count of every record
    /// with [`Pipeline::write_total_count`].
    pub include_total_count: bool,
}

impl PsPaging {
    /// The positions to write out of `len` records: from `skip`, at
    /// most `first` of them, and none past the end.
    pub fn window(&self, len: u64) -> core::ops::Range<u64> {
        let start = self.skip.min(len);
        start..start + self.first.min(len - start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{self, Value};

    #[test]
    fn paging_reads_what_the_caller_gave_and_the_count_carries_its_accuracy() {
        let _host = testing::install();
        let stopping = AtomicBool::new(false);
        let scratch = core::cell::Cell::new(Vec::new());
        let ps = unsafe { Pipeline::new(PsHandle::NULL, &stopping, &scratch) };
        // The fake host returns what a caller who gave -First 2 -Skip 1
        // -IncludeTotalCount would bind.
        assert_eq!(ps.paging().expect("paging"), PsPaging { first: 2, skip: 1, include_total_count: true });
        testing::take_output();
        ps.write_total_count(7, 0.5).expect("count");
        match &testing::take_output()[..] {
            [Value::PsObject(name, notes)] => {
                assert_eq!(name, "TotalCount");
                assert!(matches!(notes[0].1, Value::Int(7)), "{notes:?}");
                assert!(matches!(notes[1].1, Value::Float(a) if a.to_bits() == 0.5f64.to_bits()), "{notes:?}");
            }
            other => panic!("one count object, got {other:?}"),
        }
    }

    #[test]
    fn transaction_is_none_until_one_is_open_and_then_that_one() {
        let _host = testing::install();
        let stopping = AtomicBool::new(false);
        let scratch = core::cell::Cell::new(Vec::new());
        let ps = unsafe { Pipeline::new(PsHandle::NULL, &stopping, &scratch) };
        assert!(ps.transaction().expect("the transaction read").is_none());
        let open = testing::start_transaction();
        let seen = ps.transaction().expect("the transaction read").expect("the open transaction");
        assert_eq!(seen.id().expect("an id"), open.id().expect("an id"));
        testing::undo_transaction();
        assert!(ps.transaction().expect("the transaction read").is_none());
    }

    #[test]
    fn a_window_starts_at_skip_and_stops_at_first_or_the_end() {
        let paging = |first, skip| PsPaging { first, skip, include_total_count: false };
        assert_eq!(paging(u64::MAX, 0).window(5), 0..5);
        assert_eq!(paging(2, 1).window(5), 1..3);
        assert_eq!(paging(10, 3).window(5), 3..5);
        assert_eq!(paging(2, 9).window(5), 5..5);
        assert_eq!(paging(0, 0).window(5), 0..0);
        assert_eq!(paging(u64::MAX, u64::MAX).window(u64::MAX), u64::MAX..u64::MAX);
    }
}
