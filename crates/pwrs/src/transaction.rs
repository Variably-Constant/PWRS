//! PowerShell transactions. A cmdlet declaring `supports_transactions`,
//! or a provider declaring the `Transactions` capability, enlists Rust
//! work in the transaction its command runs in, so `Complete-Transaction`
//! commits it and `Undo-Transaction` rolls it back.
//!
//! Only Windows PowerShell 5.1 gives commands `-UseTransaction`.
//! PowerShell 7 has `Start-Transaction` and the other transaction cmdlets
//! on Windows, and none of them on Linux or FreeBSD, and it gives no
//! compiled cmdlet or provider `-UseTransaction` anywhere, so there a
//! command never runs in a transaction.

use crate::host::vtable;
use crate::pipeline::check;
use crate::{FromPs, PsObject, PsResult};
use core::ffi::c_void;
use pwrs_sys::{PsHandle, PsStatus, PS_ERR_MANAGED_EXCEPTION, PS_ERR_NATIVE_PANIC, PS_OK};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// The PowerShell transaction a command runs in, from
/// [`Pipeline::transaction`](crate::Pipeline::transaction) or a provider
/// call's [`Call::transaction`](crate::Call::transaction): the
/// `System.Transactions.Transaction` behind it, which work is enlisted in.
#[derive(Clone, Debug)]
pub struct PsTransaction {
    object: PsObject,
}

impl PsTransaction {
    pub(crate) fn from_object(object: PsObject) -> PsTransaction {
        PsTransaction { object }
    }

    /// The `System.Transactions.Transaction`.
    pub fn object(&self) -> &PsObject {
        &self.object
    }

    /// The transaction's `TransactionInformation.LocalIdentifier`, the
    /// same for every command that runs in it, so work can be recorded per
    /// transaction.
    pub fn id(&self) -> PsResult<String> {
        String::from_ps(&self.object.get("TransactionInformation")?.get("LocalIdentifier")?)
    }

    /// Enlists `enlistment` in the transaction. When the transaction is
    /// completed, each enlistment's [`Enlistment::prepare`] runs, and then
    /// exactly one of [`Enlistment::commit`] and [`Enlistment::rollback`]:
    /// commit when every enlistment prepared, and rollback otherwise, for
    /// the enlistment that refused too. A transaction rolled back without
    /// being completed, by `Undo-Transaction` or a timeout, runs rollback
    /// alone. Each runs on the thread that completes or rolls back the
    /// transaction: the pipeline thread of `Complete-Transaction` or
    /// `Undo-Transaction`, or a timer's for a timeout, so none of them can
    /// write to a pipeline. A transaction that has ended refuses the
    /// enlistment, which is dropped.
    pub fn enlist<E: Enlistment>(&self, enlistment: E) -> PsResult<()> {
        let context = Box::into_raw(Box::new(enlistment)).cast::<c_void>();
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().transaction_enlist)(self.object.as_raw(), prepare_callback::<E>, end_callback::<E>, context, &mut err) };
        if let Err(e) = check(status, err) {
            // The runtime refused it, so the enlistment is still this call's.
            drop(unsafe { Box::from_raw(context.cast::<E>()) });
            return Err(e);
        }
        Ok(())
    }

    /// Enlists two closures under [`PsTransaction::enlist`]'s terms, with
    /// no prepare: `commit` runs when the transaction commits and
    /// `rollback` when it rolls back.
    pub fn on_complete<C, R>(&self, commit: C, rollback: R) -> PsResult<()>
    where
        C: FnOnce() + Send + 'static,
        R: FnOnce() + Send + 'static,
    {
        self.enlist(OnComplete { commit, rollback })
    }
}

/// Work enlisted in a PowerShell transaction by [`PsTransaction::enlist`].
pub trait Enlistment: Send + 'static {
    /// Called when the transaction is completed, before any enlistment
    /// commits. `Ok` lets it commit. An `Err` rolls the whole transaction
    /// back, and `Complete-Transaction` fails with a
    /// `TransactionAbortedException` whose inner exception is the error.
    /// The default lets it commit.
    fn prepare(&mut self) -> PsResult<()> {
        Ok(())
    }

    /// The transaction committed.
    fn commit(self);

    /// The transaction rolled back.
    fn rollback(self);
}

/// The two closures [`PsTransaction::on_complete`] enlists.
struct OnComplete<C, R> {
    commit: C,
    rollback: R,
}

impl<C, R> Enlistment for OnComplete<C, R>
where
    C: FnOnce() + Send + 'static,
    R: FnOnce() + Send + 'static,
{
    fn commit(self) {
        (self.commit)()
    }

    fn rollback(self) {
        (self.rollback)()
    }
}

/// Runs an enlistment's prepare, on the thread completing the transaction.
unsafe extern "C" fn prepare_callback<E: Enlistment>(context: *mut c_void, err: *mut PsHandle) -> PsStatus {
    let _called_in = crate::host::CalledIn::enter();
    // SAFETY: the runtime passes the context enlist gave it, before the
    // end that takes it.
    let enlistment = unsafe { &mut *context.cast::<E>() };
    match catch_unwind(AssertUnwindSafe(|| enlistment.prepare())) {
        Ok(Ok(())) => PS_OK,
        Ok(Err(e)) => {
            if !err.is_null() {
                unsafe { *err = crate::runtime::error_handle(&e) };
            }
            PS_ERR_MANAGED_EXCEPTION
        }
        Err(payload) => {
            if !err.is_null() {
                unsafe { *err = crate::runtime::string_handle(&crate::runtime::panic_message(payload)) };
            }
            PS_ERR_NATIVE_PANIC
        }
    }
}

/// Runs an enlistment's commit or rollback, once, and drops it. A panic
/// in either is reported and not unwound into the runtime.
unsafe extern "C" fn end_callback<E: Enlistment>(context: *mut c_void, committed: u8) {
    let _called_in = crate::host::CalledIn::enter();
    // SAFETY: the runtime ends the context enlist gave it, once.
    let enlistment = unsafe { *Box::from_raw(context.cast::<E>()) };
    let ended = catch_unwind(AssertUnwindSafe(move || if committed != 0 { enlistment.commit() } else { enlistment.rollback() }));
    if ended.is_err() {
        eprintln!("pwrs: an enlistment panicked in its {}", if committed != 0 { "commit" } else { "rollback" });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;
    use crate::{ErrorCategory, PsError};
    use std::sync::{Arc, Mutex};

    /// What the enlistments of one test did, in order.
    type Record = Arc<Mutex<Vec<String>>>;

    /// An enlistment that records each call, and refuses at prepare or
    /// panics there when its test sets it to.
    struct Recorder {
        name: &'static str,
        record: Record,
        refuse: bool,
        panic: bool,
    }

    impl Recorder {
        fn new(name: &'static str, record: &Record) -> Recorder {
            Recorder { name, record: record.clone(), refuse: false, panic: false }
        }

        fn note(&self, what: &str) {
            self.record.lock().expect("the record").push(format!("{what} {}", self.name));
        }
    }

    impl Enlistment for Recorder {
        fn prepare(&mut self) -> PsResult<()> {
            self.note("prepare");
            if self.panic {
                panic!("{} cannot tell", self.name);
            }
            if self.refuse {
                return Err(PsError::new(ErrorCategory::ResourceUnavailable, "Refused", format!("{} cannot commit", self.name)));
            }
            Ok(())
        }

        fn commit(self) {
            self.note("commit");
        }

        fn rollback(self) {
            self.note("rollback");
        }
    }

    impl Drop for Recorder {
        fn drop(&mut self) {
            self.note("drop");
        }
    }

    fn taken(record: &Record) -> Vec<String> {
        core::mem::take(&mut *record.lock().expect("the record"))
    }

    #[test]
    fn every_enlistment_prepares_and_then_commits_and_on_complete_has_no_prepare() {
        let _host = testing::install();
        let record = Record::default();
        let tx = testing::start_transaction();
        tx.enlist(Recorder::new("a", &record)).expect("enlisted");
        let (commits, rollbacks) = (record.clone(), record.clone());
        tx.on_complete(move || commits.lock().expect("the record").push("commit b".into()), move || rollbacks.lock().expect("the record").push("rollback b".into())).expect("enlisted");
        tx.enlist(Recorder::new("c", &record)).expect("enlisted");
        testing::complete_transaction().expect("committed");
        assert_eq!(taken(&record), ["prepare a", "prepare c", "commit a", "drop a", "commit b", "commit c", "drop c"]);
    }

    #[test]
    fn a_refused_prepare_rolls_back_every_enlistment_the_refusing_one_first_and_is_the_error() {
        let _host = testing::install();
        let record = Record::default();
        let tx = testing::start_transaction();
        tx.enlist(Recorder::new("a", &record)).expect("enlisted");
        let mut refusing = Recorder::new("b", &record);
        refusing.refuse = true;
        tx.enlist(refusing).expect("enlisted");
        tx.enlist(Recorder::new("c", &record)).expect("enlisted");
        let refusal = testing::complete_transaction().expect_err("refused");
        assert!(refusal.to_string().contains("b cannot commit"), "{refusal}");
        assert_eq!(taken(&record), ["prepare a", "prepare b", "rollback b", "drop b", "rollback a", "drop a", "rollback c", "drop c"]);
    }

    #[test]
    fn a_prepare_that_panics_refuses_with_the_panic_and_rolls_back() {
        let _host = testing::install();
        let record = Record::default();
        let tx = testing::start_transaction();
        let mut panicking = Recorder::new("a", &record);
        panicking.panic = true;
        tx.enlist(panicking).expect("enlisted");
        let refusal = testing::complete_transaction().expect_err("refused");
        assert!(refusal.to_string().contains("a cannot tell"), "{refusal}");
        assert_eq!(taken(&record), ["prepare a", "rollback a", "drop a"]);
    }

    #[test]
    fn undo_rolls_each_enlistment_back_with_no_prepare() {
        let _host = testing::install();
        let record = Record::default();
        let tx = testing::start_transaction();
        tx.enlist(Recorder::new("a", &record)).expect("enlisted");
        tx.enlist(Recorder::new("b", &record)).expect("enlisted");
        testing::undo_transaction();
        assert_eq!(taken(&record), ["rollback a", "drop a", "rollback b", "drop b"]);
    }

    #[test]
    fn an_ended_transaction_refuses_an_enlistment_which_is_dropped() {
        let _host = testing::install();
        let record = Record::default();
        let first = testing::start_transaction();
        testing::complete_transaction().expect("nothing to refuse");
        let _second = testing::start_transaction();
        let refused = first.enlist(Recorder::new("late", &record)).expect_err("refused");
        assert!(refused.to_string().contains("has ended"), "{refused}");
        assert_eq!(taken(&record), ["drop late"]);
    }

    #[test]
    fn the_id_is_the_transactions_local_identifier() {
        let _host = testing::install();
        assert_eq!(testing::start_transaction().id().expect("an id"), "fake:1");
        testing::undo_transaction();
        assert_eq!(testing::start_transaction().id().expect("an id"), "fake:2");
    }
}
