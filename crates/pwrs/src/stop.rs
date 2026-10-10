//! A waker the pipeline's stop runs, so a call blocked on something of
//! its own wakes on Ctrl+C instead of polling [`crate::Pipeline::stopping`].
//!
//! One slot per cmdlet instance holds at most one waker. Whoever swaps a
//! waker out of the slot owns it: the stop runs it, or the guard frees
//! it, so a waker runs at most once and is never freed while it runs.
//! The stop's store to the flag and a registration's read of it are
//! both sequentially consistent, so a waker registered as the stop
//! arrives is run by one side or the other.

use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::{ErrorCategory, PsError, PsResult};

type Waker = Box<dyn FnOnce() + Send>;

/// The slot an instance keeps its stop waker in.
pub struct StopWakerSlot(AtomicPtr<Waker>);

impl StopWakerSlot {
    pub const fn new() -> StopWakerSlot {
        StopWakerSlot(AtomicPtr::new(null_mut()))
    }

    /// The waker the slot holds, which the caller now owns.
    fn take(&self) -> Option<Waker> {
        let taken = self.0.swap(null_mut(), Ordering::SeqCst);
        if taken.is_null() {
            None
        } else {
            // SAFETY: a pointer in the slot came from Box::into_raw in
            // `register`, and the swap handed it to this caller alone.
            Some(*unsafe { Box::from_raw(taken) })
        }
    }

    /// Puts `waker` in an empty slot. False when the slot holds one.
    fn register(&self, waker: Waker) -> bool {
        let boxed = Box::into_raw(Box::new(waker));
        if self.0.compare_exchange(null_mut(), boxed, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            return true;
        }
        // SAFETY: the exchange failed, so the slot never held `boxed`.
        drop(unsafe { Box::from_raw(boxed) });
        false
    }
}

impl Default for StopWakerSlot {
    fn default() -> Self {
        StopWakerSlot::new()
    }
}

impl Drop for StopWakerSlot {
    fn drop(&mut self) {
        drop(self.take());
    }
}

/// Keeps a waker registered for the pipeline's stop until it drops.
/// From [`crate::Pipeline::on_stop`].
#[must_use = "the waker is unregistered when the guard drops"]
pub struct StopWaker<'ps> {
    slot: &'ps StopWakerSlot,
}

impl Drop for StopWaker<'_> {
    fn drop(&mut self) {
        drop(self.slot.take());
    }
}

/// Sets `stopping` and runs the waker `slot` holds, on this thread.
pub(crate) fn stop(stopping: &AtomicBool, slot: &StopWakerSlot) {
    stopping.store(true, Ordering::SeqCst);
    if let Some(waker) = slot.take() {
        run(waker);
    }
}

/// Registers `waker` in `slot`, running it at once when `stopping` is
/// already set.
pub(crate) fn on_stop<'ps>(stopping: &AtomicBool, slot: &'ps StopWakerSlot, waker: Waker) -> PsResult<StopWaker<'ps>> {
    if !slot.register(waker) {
        return Err(PsError::new(
            ErrorCategory::InvalidOperation,
            "PwrsStopWakerHeld",
            "a stop waker is already registered for this call; one waker can make every wake the call needs",
        ));
    }
    if stopping.load(Ordering::SeqCst)
        && let Some(waker) = slot.take()
    {
        run(waker);
    }
    Ok(StopWaker { slot })
}

/// Runs a waker. The stop arrives through a native export, which nothing
/// may unwind across, so a waker's panic is caught and reported on
/// standard error.
fn run(waker: Waker) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(waker)) {
        eprintln!("PWRS: a stop waker panicked: {}", crate::runtime::panic_message(payload));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    /// Counts its runs and its drops.
    #[derive(Clone, Default)]
    struct Tally {
        runs: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
    }

    struct Dropped(Arc<AtomicUsize>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Tally {
        fn waker(&self) -> Waker {
            let runs = self.runs.clone();
            let dropped = Dropped(self.drops.clone());
            Box::new(move || {
                runs.fetch_add(1, Ordering::SeqCst);
                drop(dropped);
            })
        }

        fn runs(&self) -> usize {
            self.runs.load(Ordering::SeqCst)
        }

        fn drops(&self) -> usize {
            self.drops.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn a_waker_registered_before_the_stop_runs_once_on_the_stop() {
        let stopping = AtomicBool::new(false);
        let slot = StopWakerSlot::new();
        let tally = Tally::default();
        let guard = on_stop(&stopping, &slot, tally.waker()).expect("an empty slot takes a waker");
        assert_eq!(tally.runs(), 0, "the waker ran before the stop");
        stop(&stopping, &slot);
        assert_eq!(tally.runs(), 1);
        stop(&stopping, &slot);
        assert_eq!(tally.runs(), 1, "a second stop ran the waker again");
        drop(guard);
        assert_eq!(tally.drops(), 1);
    }

    #[test]
    fn a_waker_registered_after_the_stop_runs_at_registration() {
        let stopping = AtomicBool::new(false);
        let slot = StopWakerSlot::new();
        stop(&stopping, &slot);
        let tally = Tally::default();
        let guard = on_stop(&stopping, &slot, tally.waker()).expect("an empty slot takes a waker");
        assert_eq!(tally.runs(), 1);
        drop(guard);
        assert_eq!(tally.runs(), 1);
        assert_eq!(tally.drops(), 1);
    }

    #[test]
    fn a_second_waker_is_refused_while_the_first_is_held_and_taken_after() {
        let stopping = AtomicBool::new(false);
        let slot = StopWakerSlot::new();
        let first = Tally::default();
        let second = Tally::default();
        let guard = on_stop(&stopping, &slot, first.waker()).expect("an empty slot takes a waker");
        match on_stop(&stopping, &slot, second.waker()) {
            Ok(_) => panic!("a second waker was taken while the first was held"),
            Err(e) => assert_eq!(e.error_id, "PwrsStopWakerHeld"),
        }
        assert_eq!(second.drops(), 1, "the refused waker was not freed");
        drop(guard);
        let again = on_stop(&stopping, &slot, second.waker()).expect("the slot is empty once the first guard drops");
        stop(&stopping, &slot);
        assert_eq!(first.runs(), 0, "a dropped guard's waker ran");
        assert_eq!(second.runs(), 1);
        drop(again);
    }

    #[test]
    fn a_dropped_guard_frees_its_waker_without_running_it() {
        let stopping = AtomicBool::new(false);
        let slot = StopWakerSlot::new();
        let tally = Tally::default();
        drop(on_stop(&stopping, &slot, tally.waker()).expect("an empty slot takes a waker"));
        stop(&stopping, &slot);
        assert_eq!(tally.runs(), 0);
        assert_eq!(tally.drops(), 1);
    }

    #[test]
    fn a_slot_dropped_holding_a_waker_frees_it() {
        let stopping = AtomicBool::new(false);
        let tally = Tally::default();
        let slot = StopWakerSlot::new();
        assert!(slot.register(tally.waker()));
        drop(slot);
        assert_eq!(tally.runs(), 0);
        assert_eq!(tally.drops(), 1);
        assert!(!stopping.load(Ordering::SeqCst));
    }

    #[test]
    fn a_stop_racing_a_dropped_guard_runs_the_waker_at_most_once_and_frees_it_once() {
        for round in 0..20_000 {
            let stopping = Arc::new(AtomicBool::new(false));
            let slot = Arc::new(StopWakerSlot::new());
            let tally = Tally::default();
            let guard = on_stop(&stopping, &slot, tally.waker()).expect("an empty slot takes a waker");
            let stopper = {
                let stopping = stopping.clone();
                let slot = slot.clone();
                std::thread::spawn(move || stop(&stopping, &slot))
            };
            drop(guard);
            stopper.join().expect("the stopping thread panicked");
            assert!(tally.runs() <= 1, "round {round}: the waker ran {} times", tally.runs());
            assert_eq!(tally.drops(), 1, "round {round}: the waker was freed {} times", tally.drops());
        }
    }

    #[test]
    fn a_registration_racing_the_stop_runs_the_waker_exactly_once() {
        for round in 0..20_000 {
            let stopping = Arc::new(AtomicBool::new(false));
            let slot = Arc::new(StopWakerSlot::new());
            let tally = Tally::default();
            let stopper = {
                let stopping = stopping.clone();
                let slot = slot.clone();
                std::thread::spawn(move || stop(&stopping, &slot))
            };
            let guard = on_stop(&stopping, &slot, tally.waker()).expect("an empty slot takes a waker");
            stopper.join().expect("the stopping thread panicked");
            assert_eq!(tally.runs(), 1, "round {round}: the waker ran {} times", tally.runs());
            drop(guard);
            assert_eq!(tally.drops(), 1, "round {round}");
        }
    }

    #[test]
    fn a_pipeline_token_registers_in_its_instance_slot() {
        let stopping = AtomicBool::new(false);
        let scratch = core::cell::Cell::new(Vec::new());
        let slot = StopWakerSlot::new();
        let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) }.with_stop_waker(&slot);
        let tally = Tally::default();
        let guard = ps.on_stop(tally.waker()).expect("the instance slot takes a waker");
        stop(&stopping, &slot);
        assert_eq!(tally.runs(), 1);
        assert!(ps.stopping());
        drop(guard);
        assert_eq!(tally.drops(), 1);
    }

    #[test]
    fn a_pipeline_token_without_a_slot_refuses_a_waker() {
        let stopping = AtomicBool::new(false);
        let scratch = core::cell::Cell::new(Vec::new());
        let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
        let tally = Tally::default();
        match ps.on_stop(tally.waker()) {
            Ok(_) => panic!("a token without a slot took a waker"),
            Err(e) => assert_eq!(e.error_id, "PwrsStopWakerUnavailable"),
        }
        assert_eq!(tally.drops(), 1, "the refused waker was not freed");
    }

    #[test]
    fn a_waker_that_panics_does_not_unwind_out_of_the_stop() {
        let stopping = AtomicBool::new(false);
        let slot = StopWakerSlot::new();
        let guard = on_stop(&stopping, &slot, Box::new(|| panic!("the waker failed on purpose"))).expect("an empty slot takes a waker");
        stop(&stopping, &slot);
        assert!(stopping.load(Ordering::SeqCst));
        drop(guard);
    }
}
