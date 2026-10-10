//! Tasks a `#[psmethods]` method hands back to .NET, settled from any
//! thread.
//!
//! A method takes a [`PsTask<T>`] argument and returns `PsResult<()>`;
//! the generated method returns the `Task<T>` (a `Task` for
//! `PsTask<()>`) and takes an optional `CancellationToken`. The method
//! moves the task to whatever thread does the work, and that thread
//! settles it once: [`PsTask::complete`], [`PsTask::fail`] or
//! [`PsTask::cancel`].

use crate::host::vtable;
use crate::pipeline::check;
use crate::{ErrorCategory, IntoPs, PsError, PsObject, PsResult};
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU8, Ordering};
use pwrs_sys::{PsHandle, PsTaskOutcome, PsTaskSlot, PS_TASK_CANCEL, PS_TASK_FAULT, PS_TASK_VALUE};

/// The `Task<T>` a `#[psmethods]` method returns, settled from any
/// thread. Settling consumes it, so a task settles once; a task dropped
/// unsettled, by a panic among other ways, faults with
/// `PwrsTaskDropped` rather than leaving its caller waiting.
///
/// Cancellation is cooperative: the caller's `CancellationToken` sets a
/// flag [`PsTask::is_cancelled`] reads, and the task ends canceled when
/// the module calls [`PsTask::cancel`], which leaves it to decide
/// whether the work in hand finishes first.
pub struct PsTask<T> {
    source: Option<PsObject>,
    cancelled: *const u8,
    _value: PhantomData<fn(T) -> T>,
}

// SAFETY: the source is a handle any thread may settle through, and the
// cancellation byte stays readable until the task is settled, which
// takes the task by value.
unsafe impl<T> Send for PsTask<T> {}

impl<T> PsTask<T> {
    /// The task the generated shell packed into a method's argument
    /// block, holding its own handle to the source.
    ///
    /// # Safety
    /// `slot` came from the argument block of the call being dispatched.
    #[doc(hidden)]
    pub unsafe fn from_slot(slot: PsTaskSlot) -> PsTask<T> {
        let source = unsafe { PsObject::from_raw((vtable().clone_handle)(slot.source)) };
        PsTask { source: Some(source), cancelled: slot.cancelled, _value: PhantomData }
    }

    /// Whether the caller's `CancellationToken` has asked for
    /// cancellation. Reads a byte, with no call into the host.
    pub fn is_cancelled(&self) -> bool {
        if self.cancelled.is_null() {
            return false;
        }
        // SAFETY: the host keeps the byte until the task is settled, and
        // settling consumes the task.
        unsafe { AtomicU8::from_ptr(self.cancelled.cast_mut()) }.load(Ordering::Acquire) != 0
    }

    /// Faults the task with `error`, the error the method would
    /// otherwise have thrown.
    pub fn fail(mut self, error: PsError) {
        self.fault(&error);
    }

    /// Ends the task canceled.
    pub fn cancel(mut self) {
        self.settle(PS_TASK_CANCEL, PsHandle::NULL);
    }

    fn fault(&mut self, error: &PsError) {
        let reason = unsafe { PsObject::from_raw(crate::runtime::error_handle(error)) };
        self.settle(PS_TASK_FAULT, reason.as_raw());
    }

    fn settle(&mut self, outcome: PsTaskOutcome, value: PsHandle) {
        let Some(source) = self.source.take() else { return };
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().task_settle)(source.as_raw(), outcome, value, &mut err) };
        if let Err(e) = check(status, err) {
            eprintln!("pwrs: a task could not be settled: {}", e.message);
        }
    }
}

impl<T: IntoPs> PsTask<T> {
    /// Completes the task with `value`, converted on this thread. A
    /// value that does not convert faults the task with the conversion's
    /// error instead.
    pub fn complete(mut self, value: T) {
        // Completing is a crossing this thread makes on purpose.
        let _attached = crate::host::attach_current_thread();
        match value.into_ps() {
            Ok(obj) => self.settle(PS_TASK_VALUE, obj.as_raw()),
            Err(e) => self.fault(&e),
        }
    }

    /// Completes the task with the value of `result`, or faults it with
    /// the error.
    pub fn finish(self, result: PsResult<T>) {
        match result {
            Ok(value) => self.complete(value),
            Err(e) => self.fail(e),
        }
    }
}

impl<T> Drop for PsTask<T> {
    fn drop(&mut self) {
        if self.source.is_some() {
            self.fault(&PsError::new(ErrorCategory::InvalidOperation, "PwrsTaskDropped", "the module dropped this task without completing it"));
        }
    }
}
