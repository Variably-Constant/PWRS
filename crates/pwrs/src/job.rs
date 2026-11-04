//! Background jobs: Rust work on a thread of its own that PowerShell's
//! job cmdlets see as a `RustJob`. [`Pipeline::start_job`] and
//! [`Pipeline::start_job_body`] start one; a cmdlet declaring
//! `#[cmdlet(as_job = "runtime")]` has the runtime run its
//! [`CmdletJob`] phases in one when `-AsJob` is given.

use crate::host::vtable;
use crate::pipeline::{check, describe, str16, utf16};
use crate::stop::{StopWaker, StopWakerSlot};
use crate::{Cmdlet, ErrorCategory, IntoPs, Pipeline, Progress, PsError, PsObject, PsResult, Streams};
use core::ffi::c_void;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};
use pwrs_sys::{PsHandle, PsStreamKind, PS_OK, PS_STREAM_DEBUG, PS_STREAM_INFORMATION, PS_STREAM_VERBOSE, PS_STREAM_WARNING};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

/// What a job's thread and its stop request share.
struct Shared {
    stopping: AtomicBool,
    waker: StopWakerSlot,
}

/// The text the job entries take for an error's details: the details
/// themselves, or the empty text they read as having none.
fn details_text(e: &PsError) -> &str {
    match &e.details {
        Some(details) => details,
        None => "",
    }
}

/// What a job's work writes to and reads: the job's output, error,
/// verbose, debug, warning, information and progress streams, which
/// Receive-Job reads, and whether Stop-Job has requested a stop. It lives on
/// the job's thread for the work's run, and is `!Send`, as the pipeline
/// token is. Every record is stored, whatever the preferences:
/// Receive-Job applies the receiving session's.
pub struct JobContext {
    job: PsObject,
    shared: Arc<Shared>,
    _not_send: PhantomData<*mut ()>,
}

impl JobContext {
    /// The job, as Get-Job gives it.
    pub fn job(&self) -> &PsObject {
        &self.job
    }

    /// Adds any convertible value to the job's output: a `Vec<T>`
    /// enumerates, a `PsArray<T>` is one object.
    pub fn write<T: IntoPs>(&self, value: T) -> PsResult<()> {
        let obj = value.into_ps()?;
        self.output(&obj, T::ENUMERATE)
    }

    /// Adds one object to the job's output, never enumerated.
    pub fn write_object(&self, obj: &PsObject) -> PsResult<()> {
        self.output(obj, false)
    }

    /// Adds each element of a collection to the job's output.
    pub fn write_enumerated(&self, obj: &PsObject) -> PsResult<()> {
        self.output(obj, true)
    }

    fn output(&self, obj: &PsObject, enumerate: bool) -> PsResult<()> {
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().job_write)(self.job.as_raw(), obj.as_raw(), enumerate as u8, &mut err) };
        check(s, err)
    }

    /// Adds an error record to the job's error stream, its details as
    /// the exception's inner one, and the exception the error names as
    /// the record's when it names one. The work goes on; only an `Err` the
    /// work returns fails the job.
    pub fn write_error(&self, e: &PsError) -> PsResult<()> {
        let (message, id, details) = (utf16(&e.message), utf16(&e.error_id), utf16(details_text(e)));
        let target = e.target.as_ref().map_or(PsHandle::NULL, |t| t.as_raw());
        let mut err = PsHandle::NULL;
        if let Some(type_name) = &e.exception {
            let exception = unsafe { PsObject::from_raw(crate::runtime::named_exception(type_name, &message, e)) };
            let s = unsafe { (vtable().job_write_error_exception)(self.job.as_raw(), exception.as_raw(), str16(&id), e.category as u32, target, &mut err) };
            return check(s, err);
        }
        let s = unsafe { (vtable().job_write_error)(self.job.as_raw(), str16(&message), str16(&id), e.category as u32, target, str16(&details), &mut err) };
        check(s, err)
    }

    fn stream(&self, kind: PsStreamKind, text: &str) -> PsResult<()> {
        let t = utf16(text);
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().job_write_stream)(self.job.as_raw(), kind, str16(&t), &mut err) };
        check(s, err)
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

    /// Adds a progress record with every field it has to the job's
    /// progress stream.
    pub fn write_progress(&self, record: &Progress) -> PsResult<()> {
        let (activity, status, operation) = (utf16(&record.activity), utf16(&record.status), utf16(&record.current_operation));
        let mut err = PsHandle::NULL;
        let s = unsafe {
            (vtable().job_write_progress)(
                self.job.as_raw(),
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

    /// True once Stop-Job, or Remove-Job -Force while the job ran, has
    /// requested a stop. The job is Stopped whatever the work returns.
    pub fn stopping(&self) -> bool {
        self.shared.stopping.load(Ordering::SeqCst)
    }

    /// Registers `waker` for the stop request to run, on the thread that
    /// requests the stop, or at once when it already came, so work blocked on
    /// something of its own wakes with no poll. As
    /// [`Pipeline::on_stop`]: at most once, one at a time.
    pub fn on_stop(&self, waker: impl FnOnce() + Send + 'static) -> PsResult<StopWaker<'_>> {
        crate::stop::on_stop(&self.shared.stopping, &self.shared.waker, Box::new(waker))
    }
}

impl Streams for JobContext {
    fn write<T: IntoPs>(&self, value: T) -> PsResult<()> {
        JobContext::write(self, value)
    }
    fn write_object(&self, obj: &PsObject) -> PsResult<()> {
        JobContext::write_object(self, obj)
    }
    fn write_enumerated(&self, obj: &PsObject) -> PsResult<()> {
        JobContext::write_enumerated(self, obj)
    }
    fn write_error(&self, e: &PsError) -> PsResult<()> {
        JobContext::write_error(self, e)
    }
    fn verbose(&self, text: &str) -> PsResult<()> {
        JobContext::verbose(self, text)
    }
    fn debug(&self, text: &str) -> PsResult<()> {
        JobContext::debug(self, text)
    }
    fn warning(&self, text: &str) -> PsResult<()> {
        JobContext::warning(self, text)
    }
    fn information(&self, text: &str) -> PsResult<()> {
        JobContext::information(self, text)
    }
    fn verbose_if(&self, text: impl FnOnce() -> String) -> PsResult<()> {
        self.verbose(&text())
    }
    fn debug_if(&self, text: impl FnOnce() -> String) -> PsResult<()> {
        self.debug(&text())
    }
    fn warning_if(&self, text: impl FnOnce() -> String) -> PsResult<()> {
        self.warning(&text())
    }
    fn information_if(&self, text: impl FnOnce() -> String) -> PsResult<()> {
        self.information(&text())
    }
    fn write_progress(&self, record: &Progress) -> PsResult<()> {
        JobContext::write_progress(self, record)
    }
    fn stopping(&self) -> bool {
        JobContext::stopping(self)
    }
}

/// The work of a job, run once on the job's own thread: a type that
/// implements this, for [`Pipeline::start_job_body`], or any closure
/// `FnOnce(&JobContext) -> PsResult<()>`, for [`Pipeline::start_job`].
/// `Ok` ends the job Completed and `Err` Failed, with its message as the
/// reason Receive-Job reports, or the exception it names when it names
/// one ([`PsError::with_exception`]); once a stop has been requested it ends
/// Stopped whatever the work returns. A panic fails the job with its message.
pub trait JobBody: Send + 'static {
    fn run(self, job: &JobContext) -> PsResult<()>;
}

impl<F> JobBody for F
where
    F: FnOnce(&JobContext) -> PsResult<()> + Send + 'static,
{
    fn run(self, job: &JobContext) -> PsResult<()> {
        self(job)
    }
}

/// The phases a cmdlet declaring `#[cmdlet(as_job = "runtime")]` runs in
/// a job. Given `-AsJob`, one job runs the whole invocation on its own
/// thread: `begin_job` once, `process_job` for each record, with that
/// record's parameters moved into the one instance, so its state stays
/// from one record to the next, and `end_job` once input ends. The job is
/// written as the cmdlet's output when it starts. Without `-AsJob`,
/// [`Cmdlet`]'s phases run as usual. A phase's `Err` is an error record
/// in the job, as it is inline, and a terminating one ends the job
/// Failed. Each record crosses to the job's thread as a clone.
pub trait CmdletJob: Cmdlet + Clone {
    fn begin_job(&mut self, _job: &JobContext) -> PsResult<()> {
        Ok(())
    }

    fn process_job(&mut self, job: &JobContext) -> PsResult<()>;

    fn end_job(&mut self, _job: &JobContext) -> PsResult<()> {
        Ok(())
    }
}

/// A started invocation's job: the sender its records go through, and
/// the job.
#[doc(hidden)]
pub type Started<T> = (Sender<T>, PsObject);

/// How the runtime starts and feeds the job of a cmdlet declaring
/// `as_job = "runtime"`. Made by `#[cmdlet]`.
#[doc(hidden)]
pub struct JobRunner<T> {
    /// Starts the invocation's job with a copy of the bound cmdlet.
    pub start: fn(&T, &Pipeline<'_>) -> PsResult<Started<T>>,
    /// Sends a copy of the cmdlet with this record's parameters; false
    /// when the job ended early and takes no more.
    pub record: fn(&T, &Sender<T>) -> bool,
}

#[doc(hidden)]
pub fn job_runner<T: CmdletJob>() -> JobRunner<T> {
    JobRunner { start: start_invocation::<T>, record: send_record::<T> }
}

fn start_invocation<T: CmdletJob>(cmdlet: &T, ps: &Pipeline<'_>) -> PsResult<Started<T>> {
    let (records, received) = std::sync::mpsc::channel::<T>();
    let job = ps.start_job_body(None, Invocation { cmdlet: cmdlet.clone(), records: received })?;
    Ok((records, job))
}

fn send_record<T: CmdletJob>(cmdlet: &T, records: &Sender<T>) -> bool {
    records.send(cmdlet.clone()).is_ok()
}

/// One invocation's job: the instance its phases run on, and the
/// records still to come.
struct Invocation<T> {
    cmdlet: T,
    records: Receiver<T>,
}

impl<T: CmdletJob> JobBody for Invocation<T> {
    fn run(self, job: &JobContext) -> PsResult<()> {
        let Invocation { mut cmdlet, records } = self;
        phase(job, cmdlet.begin_job(job))?;
        loop {
            if job.stopping() {
                return Ok(());
            }
            match records.recv_timeout(Duration::from_millis(50)) {
                Ok(record) => {
                    cmdlet.take_params(record);
                    phase(job, cmdlet.process_job(job))?;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        if job.stopping() {
            return Ok(());
        }
        phase(job, cmdlet.end_job(job))
    }
}

/// A phase's result in a job: an error is a record in the job's error
/// stream, as it is inline, and a terminating one ends the job.
fn phase(job: &JobContext, result: PsResult<()>) -> PsResult<()> {
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.terminating => Err(e),
        Err(e) => job.write_error(&e),
    }
}

/// Makes the job for `cmdlet` and starts `body` on a thread of its own.
/// A job given no name goes to `job_new` as the empty name, which the
/// engine replaces with its own `Job<n>`.
pub(crate) fn start<B: JobBody>(cmdlet: PsHandle, name: Option<&str>, body: B) -> PsResult<PsObject> {
    let shared = Arc::new(Shared { stopping: AtomicBool::new(false), waker: StopWakerSlot::new() });
    let context = Arc::into_raw(Arc::clone(&shared)) as *mut c_void;
    let n = match name {
        Some(name) => utf16(name),
        None => Vec::new(),
    };
    let mut out = PsHandle::NULL;
    let mut err = PsHandle::NULL;
    let s = unsafe { (vtable().job_new)(cmdlet, str16(&n), stop_job, context, release_job, &mut out, &mut err) };
    if s != PS_OK {
        // SAFETY: a refused job never took the context, so this is the
        // reference `Arc::into_raw` gave away above.
        drop(unsafe { Arc::from_raw(context as *const Shared) });
        check(s, err)?;
    }
    let job = unsafe { PsObject::from_raw(out) };
    let for_thread = job.clone();
    let spawned = std::thread::Builder::new().name("pwrs job".to_string()).spawn(move || run(for_thread, shared, body));
    if let Err(e) = spawned {
        let failure = PsError::new(ErrorCategory::ResourceUnavailable, "PwrsJobThread", format!("the job's thread could not start: {e}"));
        finish(&job, Some(&failure));
        return Err(failure);
    }
    Ok(job)
}

/// The job's thread: runs the work, then ends the job with its outcome.
fn run<B: JobBody>(job: PsObject, shared: Arc<Shared>, body: B) {
    let _attached = crate::attach_current_thread();
    let context = JobContext { job, shared, _not_send: PhantomData };
    let failure = match catch_unwind(AssertUnwindSafe(|| body.run(&context))) {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(e),
        Err(payload) => Some(PsError::new(
            ErrorCategory::NotSpecified,
            "PwrsJobPanic",
            format!("the job's work panicked: {}", crate::runtime::panic_message(payload)),
        )),
    };
    finish(&context.job, failure.as_ref());
}

/// Ends the job: Completed, or Failed with `failure` as the reason, its
/// `[id] message` text, or the exception it names when it names one.
fn finish(job: &PsObject, failure: Option<&PsError>) {
    let mut err = PsHandle::NULL;
    let s = match failure.and_then(|e| e.exception.as_deref().map(|type_name| (e, type_name))) {
        Some((e, type_name)) => {
            let text = utf16(&e.to_string());
            let reason = unsafe { PsObject::from_raw(crate::runtime::named_exception(type_name, &text, e)) };
            unsafe { (vtable().job_finish_exception)(job.as_raw(), reason.as_raw(), &mut err) }
        }
        None => {
            let (failed, message, details) = match failure {
                None => (0u8, String::new(), ""),
                Some(e) => (1u8, e.to_string(), details_text(e)),
            };
            let (m, d) = (utf16(&message), utf16(details));
            unsafe { (vtable().job_finish)(job.as_raw(), failed, str16(&m), str16(&d), &mut err) }
        }
    };
    if s != PS_OK {
        eprintln!("PWRS: a job could not be ended: {}", describe(err));
    }
}

/// The job's stop request, from Stop-Job's thread.
unsafe extern "C" fn stop_job(context: *mut c_void) {
    // SAFETY: the job calls this only before `release_job`, with the
    // context `start` gave it, so the reference it owns is alive.
    let shared = unsafe { &*(context as *const Shared) };
    crate::stop::stop(&shared.stopping, &shared.waker);
}

/// Drops the job's reference, once, when it ends.
unsafe extern "C" fn release_job(context: *mut c_void) {
    // SAFETY: the job calls this once, with the context `start` gave it.
    drop(unsafe { Arc::from_raw(context as *const Shared) });
}

/// Whether an invocation of a cmdlet declaring `as_job = "runtime"` runs
/// as a job, read once; whether its job started; and the sender its
/// records go through while the job takes them.
pub struct JobLink<T> {
    asked: Option<bool>,
    started: bool,
    records: Option<Sender<T>>,
}

impl<T> Default for JobLink<T> {
    fn default() -> Self {
        JobLink { asked: None, started: false, records: None }
    }
}

impl<T: Cmdlet> JobLink<T> {
    /// Whether this invocation runs as a job: the caller gave `-AsJob`.
    pub(crate) fn asked(&mut self, ps: &Pipeline<'_>) -> PsResult<bool> {
        match self.asked {
            Some(asked) => Ok(asked),
            None => {
                let asked = ps.as_job()?;
                self.asked = Some(asked);
                Ok(asked)
            }
        }
    }

    /// One phase of the invocation as a job: the first phase to run
    /// starts the job and writes it, each record goes to it, and the end
    /// closes its input so it runs `end_job` and finishes.
    pub(crate) fn phase(&mut self, runner: &JobRunner<T>, cmdlet: &T, phase: pwrs_sys::PsPhase, ps: &Pipeline<'_>) -> PsResult<()> {
        if !self.started {
            self.started = true;
            let (records, job) = (runner.start)(cmdlet, ps)?;
            self.records = Some(records);
            ps.write_object(&job)?;
        }
        match phase {
            pwrs_sys::PS_PHASE_PROCESS => {
                if let Some(records) = &self.records
                    && !(runner.record)(cmdlet, records)
                {
                    self.records = None;
                }
            }
            pwrs_sys::PS_PHASE_END => self.records = None,
            _begin => {}
        }
        Ok(())
    }
}
