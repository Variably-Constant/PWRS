//! The pipeline-thread token and the stream API it unlocks.

use crate::host::vtable;
use crate::{PsError, PsObject, PsResult};
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};
use pwrs_sys::{PsHandle, PsStr16, PsStreamKind, PS_OK, PS_STREAM_DEBUG, PS_STREAM_INFORMATION, PS_STREAM_VERBOSE, PS_STREAM_WARNING};

/// Proof of being on the pipeline thread inside one lifecycle phase.
/// `!Send` and `!Sync` by construction.
pub struct Pipeline<'ps> {
    cmdlet: PsHandle,
    stopping: &'ps AtomicBool,
    /// The instance's UTF-16 buffer, borrowed for the phase.
    scratch: &'ps core::cell::Cell<Vec<u16>>,
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

/// Whether [`Pipeline::par_map`] writes a result when it finishes or
/// when its turn comes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    /// Each result is written as soon as it is ready, so output order
    /// follows completion. This is what `ForEach-Object -Parallel`
    /// does.
    AsReady,
    /// Output order matches input order.
    Input,
}

/// Owned input that worker threads take one item from at a time.
///
/// A worker claims an index with one `fetch_add` and no lock, so a
/// free worker takes the next item and an uneven item slows only the
/// worker holding it.
struct Claim<T> {
    slots: core::cell::UnsafeCell<Vec<Option<T>>>,
    cursor: core::sync::atomic::AtomicUsize,
    len: usize,
}

// SAFETY: `next` hands out each index exactly once, so no two threads
// ever reference the same slot, and `T: Send` carries the item to the
// thread that claimed it.
unsafe impl<T: Send> Sync for Claim<T> {}

impl<T> Claim<T> {
    fn new(items: Vec<T>) -> Self {
        let len = items.len();
        Claim { slots: core::cell::UnsafeCell::new(items.into_iter().map(Some).collect()), cursor: core::sync::atomic::AtomicUsize::new(0), len }
    }

    fn next(&self) -> Option<(usize, T)> {
        let i = self.cursor.fetch_add(1, Ordering::Relaxed);
        if i >= self.len {
            return None;
        }
        // SAFETY: `i` came from a fetch_add, so this call is the only
        // one that will ever see it and holds the sole reference to
        // that slot.
        let slots = unsafe { &mut *self.slots.get() };
        slots[i].take().map(|t| (i, t))
    }

    fn drained(&self) -> bool {
        self.cursor.load(Ordering::Relaxed) >= self.len
    }
}

/// Runs `f` over every item and sends each result with its index,
/// returning once all of them are done. Both pools answer this one
/// shape, so the draining half of [`Pipeline::par_map`] is the same
/// code whichever is compiled in.
#[cfg(not(feature = "parallel"))]
fn run_workers<T, U, F>(items: Vec<T>, f: F, tx: std::sync::mpsc::Sender<(usize, U)>)
where
    T: Send + 'static,
    U: Send + 'static,
    F: Fn(T) -> U + Send + Sync + 'static,
{
    let len = items.len();
    let claim = std::sync::Arc::new(Claim::new(items));
    let f = std::sync::Arc::new(f);
    let width = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(len);
    let mut workers = Vec::with_capacity(width);
    for _ in 0..width {
        let claim = std::sync::Arc::clone(&claim);
        let f = std::sync::Arc::clone(&f);
        let tx = tx.clone();
        workers.push(std::thread::spawn(move || {
            while let Some((i, item)) = claim.next() {
                if tx.send((i, f(item))).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);
    // Every worker is joined before a panic is raised, so none is left
    // running against a caller that has given up. The panic carries to
    // the caller because a worker that died owes results, and a stream
    // short by those items says nothing about it.
    let mut died = None;
    for w in workers {
        if let Err(payload) = w.join() {
            died = Some(payload);
        }
    }
    if let Some(payload) = died {
        std::panic::resume_unwind(payload);
    }
}

/// The `parallel` pool: Flynnel halves the work until a leaf is small
/// enough to run inline, so a worker that finishes early takes from
/// one that has not.
#[cfg(feature = "parallel")]
fn run_workers<T, U, F>(items: Vec<T>, f: F, tx: std::sync::mpsc::Sender<(usize, U)>)
where
    T: Send + 'static,
    U: Send + 'static,
    F: Fn(T) -> U + Send + Sync + 'static,
{
    /// Below this a split costs more than the items it separates.
    const LEAF: usize = 16;

    fn halve<T, U, F>(plan: &flynnel::JobPlan, base: usize, items: &mut [Option<T>], f: &F, tx: &std::sync::mpsc::Sender<(usize, U)>)
    where
        T: Send,
        U: Send,
        F: Fn(T) -> U + Send + Sync,
    {
        if items.len() <= LEAF {
            for (n, slot) in items.iter_mut().enumerate() {
                if let Some(item) = slot.take() {
                    if tx.send((base + n, f(item))).is_err() {
                        return;
                    }
                }
            }
            return;
        }
        let mid = items.len() / 2;
        let (left, right) = items.split_at_mut(mid);
        flynnel::join(plan, || halve(plan, base, left, f, tx), || halve(plan, base + mid, right, f, tx));
    }

    let len = items.len();
    let mut slots: Vec<Option<T>> = items.into_iter().map(Some).collect();
    let plan = flynnel::JobPlan::new(len.next_power_of_two().trailing_zeros() as u8, len as u32);
    halve(&plan, 0, &mut slots, &f, &tx);
    drop(tx);
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
            streams: core::cell::Cell::new(0),
            default_phase: core::cell::Cell::new(false),
            asked: core::cell::Cell::new(false),
            _not_send: PhantomData,
        }
    }

    /// True once the engine called `StopProcessing`.
    #[inline]
    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::Relaxed)
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

    /// `WriteError` or `ThrowTerminatingError` depending on the flag.
    pub fn write_error(&self, e: &PsError) -> PsResult<()> {
        let msg = utf16(&e.message);
        let id = utf16(&e.error_id);
        let target = e.target.as_ref().map(|t| t.as_raw()).unwrap_or(PsHandle::NULL);
        let mut err = PsHandle::NULL;
        let s = unsafe {
            (vtable().write_error)(self.cmdlet, str16(&msg), str16(&id), e.category as u32, target, e.terminating as u8, &mut err)
        };
        check(s, err)
    }

    /// The verbose, debug, warning and information streams. The text
    /// is built in the instance's buffer.
    fn stream(&self, kind: u32, text: &str) -> PsResult<()> {
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

    /// `WriteProgress`; `percent` below zero hides the bar.
    pub fn progress(&self, activity_id: i32, activity: &str, status: &str, percent: i32) -> PsResult<()> {
        let a = utf16(activity);
        let st = utf16(status);
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().write_progress)(self.cmdlet, activity_id, str16(&a), str16(&st), percent, &mut err) };
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
    /// every object it wrote. The command is resolved and invoked in
    /// the current runspace without a script block, so nothing is
    /// parsed. Its non-terminating errors reach this cmdlet's error
    /// stream, as they would had the user run it; a terminating one is
    /// the `Err`.
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

    /// Maps `items` across a worker pool and writes every result to
    /// the output stream from this thread.
    ///
    /// The pipeline token is `!Send`, so `f` cannot capture it and a
    /// worker cannot reach the engine: the parallel half runs over
    /// owned Rust data with no managed object in it, and the writes
    /// happen on the one thread allowed to make them. Workers claim
    /// input by one `fetch_add` on a shared cursor, so a free worker
    /// takes the next item and nothing holds a lock.
    ///
    /// Draining stops when the pipeline is stopping; every worker is
    /// joined before returning. A worker that panics fails the call
    /// with a terminating `PwrsWorkerPanic`, since the items it held
    /// are never written and the stream would otherwise be short
    /// without saying so.
    pub fn par_map<T, U, F>(&self, items: Vec<T>, order: Order, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        U: crate::IntoPs + Send + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        let len = items.len();
        if len == 0 {
            return Ok(());
        }
        let (tx, rx) = std::sync::mpsc::channel::<(usize, U)>();
        let coordinator = std::thread::spawn(move || run_workers(items, f, tx));

        let result = match order {
            Order::AsReady => self.drain_as_ready(&rx),
            Order::Input => self.drain_in_order(&rx, len),
        };
        drop(rx);

        if coordinator.join().is_err() {
            return Err(PsError::new(crate::ErrorCategory::InvalidOperation, "PwrsWorkerPanic", "worker thread panicked").terminating());
        }
        result
    }

    /// [`Pipeline::par_map`] for work whose results are not written.
    /// Stops claiming new items once the pipeline is stopping.
    pub fn par_for_each<T, F>(&self, items: Vec<T>, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        F: Fn(T) + Send + Sync + 'static,
    {
        let len = items.len();
        if len == 0 {
            return Ok(());
        }
        let claim = std::sync::Arc::new(Claim::new(items));
        let f = std::sync::Arc::new(f);
        let halt = std::sync::Arc::new(core::sync::atomic::AtomicBool::new(false));

        let width = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(len);
        let mut workers = Vec::with_capacity(width);
        for _ in 0..width {
            let claim = std::sync::Arc::clone(&claim);
            let f = std::sync::Arc::clone(&f);
            let halt = std::sync::Arc::clone(&halt);
            workers.push(std::thread::spawn(move || {
                while !halt.load(Ordering::Relaxed) {
                    match claim.next() {
                        Some((_, item)) => f(item),
                        None => break,
                    }
                }
            }));
        }

        while !claim.drained() {
            if self.stopping() {
                halt.store(true, Ordering::Relaxed);
                break;
            }
            std::thread::yield_now();
        }

        let mut panicked = false;
        for w in workers {
            if w.join().is_err() {
                panicked = true;
            }
        }
        if panicked {
            return Err(PsError::new(crate::ErrorCategory::InvalidOperation, "PwrsWorkerPanic", "worker thread panicked").terminating());
        }
        Ok(())
    }

    fn drain_as_ready<U: crate::IntoPs>(&self, rx: &std::sync::mpsc::Receiver<(usize, U)>) -> PsResult<()> {
        for (_, item) in rx {
            if self.stopping() {
                break;
            }
            self.write(item)?;
        }
        Ok(())
    }

    /// Holds results that arrived early so the stream keeps input
    /// order. A slow item stalls the ones behind it and the buffer
    /// grows while it does, which is what asking for input order buys.
    fn drain_in_order<U: crate::IntoPs>(&self, rx: &std::sync::mpsc::Receiver<(usize, U)>, len: usize) -> PsResult<()> {
        let mut pending: std::collections::BTreeMap<usize, U> = std::collections::BTreeMap::new();
        let mut next = 0usize;
        for (i, item) in rx {
            if self.stopping() {
                return Ok(());
            }
            pending.insert(i, item);
            while let Some(ready) = pending.remove(&next) {
                self.write(ready)?;
                next += 1;
                if self.stopping() {
                    return Ok(());
                }
            }
            if next == len {
                break;
            }
        }
        Ok(())
    }

    /// Runs `work` on a new thread while this thread forwards every
    /// value it sends to the output stream, in order. Draining stops
    /// when the pipeline is stopping, which is noticed while the worker
    /// is silent too; the worker is always joined, so one that neither
    /// sends nor watches for the stop keeps Ctrl+C waiting until it
    /// returns. [`Pipeline::stream_from_thread_until`] hands the worker
    /// the stop as well.
    pub fn stream_from_thread<T, F>(&self, work: F) -> PsResult<()>
    where
        T: crate::IntoPs + Send + 'static,
        F: FnOnce(std::sync::mpsc::Sender<T>) + Send + 'static,
    {
        self.stream_from_thread_until(move |tx, _stop| work(tx))
    }

    /// [`Pipeline::stream_from_thread`], with a [`StopSignal`] the worker
    /// checks between steps of work that sends nothing for a long time,
    /// so a stopped pipeline gets the thread back promptly.
    pub fn stream_from_thread_until<T, F>(&self, work: F) -> PsResult<()>
    where
        T: crate::IntoPs + Send + 'static,
        F: FnOnce(std::sync::mpsc::Sender<T>, StopSignal) + Send + 'static,
    {
        use std::sync::mpsc::RecvTimeoutError;
        let halt = StopSignal(std::sync::Arc::new(AtomicBool::new(false)));
        let (tx, rx) = std::sync::mpsc::channel::<T>();
        let signal = halt.clone();
        let worker = std::thread::spawn(move || work(tx, signal));
        let mut result = Ok(());
        loop {
            if self.stopping() {
                break;
            }
            match rx.recv_timeout(STOP_POLL) {
                Ok(item) => {
                    if let Err(e) = self.write(item) {
                        result = Err(e);
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        halt.0.store(true, Ordering::Relaxed);
        drop(rx);
        if worker.join().is_err() {
            return Err(PsError::new(crate::ErrorCategory::InvalidOperation, "PwrsWorkerPanic", "worker thread panicked").terminating());
        }
        result
    }
}

/// How long the pipeline thread waits on a silent worker before it looks
/// at the stop flag again: short enough that Ctrl+C feels immediate.
const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// The stop of the pipeline a worker serves, which the worker may keep
/// and check from its own thread. Set once the pipeline thread has seen
/// a stop, a failed write, or the worker's channel close.
#[derive(Clone, Debug)]
pub struct StopSignal(std::sync::Arc<AtomicBool>);

impl StopSignal {
    /// True once the worker should give up and return.
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
