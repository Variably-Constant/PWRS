//! Work off the pipeline thread.
//!
//! Only the pipeline thread may write to the engine, so every record a
//! worker thread makes crosses to it over a channel and is written
//! there. [`Pipeline::stream_from_thread`] runs one thread whose values
//! are written as output; [`Pipeline::stream_from_worker`] runs one that
//! writes every stream through a [`Worker`]; [`Pipeline::par_map`] and
//! [`Pipeline::par_for_each`] run owned items across a pool, and
//! [`Pipeline::workers`] sets how many run at once.

use crate::{ErrorCategory, IntoPs, Pipeline, Progress, PsError, PsResult};
use core::convert::Infallible;
use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicBool, Ordering};
use pwrs_sys::{PsStreamKind, PS_STREAM_DEBUG, PS_STREAM_INFORMATION, PS_STREAM_VERBOSE, PS_STREAM_WARNING};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::Arc;

/// How long the pipeline thread waits on a silent worker before it
/// looks at the stop flag again: short enough that Ctrl+C feels
/// immediate.
const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(50);

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

/// The stop of the pipeline a worker serves, which the worker may keep
/// and check from its own thread. Set once the pipeline thread has seen
/// a stop, a failed write, a terminating error, a worker's panic, or
/// the end of the work, and when the call that started the worker
/// returns.
#[derive(Clone, Debug)]
pub struct StopSignal(Arc<AtomicBool>);

impl StopSignal {
    fn new() -> Self {
        StopSignal(Arc::new(AtomicBool::new(false)))
    }

    /// True once the worker should give up and return.
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn set(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// The flag itself, for a pool that reads it before each item.
    fn flag(&self) -> &AtomicBool {
        &self.0
    }
}

/// A record a worker hands the pipeline thread.
enum Sent<T> {
    Output(T),
    Error(PsError),
    Stream(PsStreamKind, String),
    Progress(Progress),
}

impl Sent<Infallible> {
    /// The same record on a channel whose output is `X`; a record of
    /// this type is never output.
    fn widen<X>(self) -> Sent<X> {
        match self {
            Sent::Output(never) => match never {},
            Sent::Error(error) => Sent::Error(error),
            Sent::Stream(kind, text) => Sent::Stream(kind, text),
            Sent::Progress(record) => Sent::Progress(record),
        }
    }
}

/// A worker thread's way to the pipeline that started it.
///
/// It sends records for the error, warning, verbose, debug, information
/// and progress streams, and output where [`Pipeline::stream_from_worker`]
/// hands it, to the pipeline thread, which writes each on its stream in
/// the order they were sent. Of the progress records for one activity
/// that are waiting to be written together, only the latest is written,
/// so a worker that reports faster than the host draws adds no writes.
///
/// Each send answers whether the pipeline took the record: false once
/// [`Worker::stopping`] is true, and the worker should then return.
/// Clone it to hand one to each thread; a clone kept after the work
/// returns does not hold the call open.
pub struct Worker<T = Infallible> {
    sink: Arc<dyn Fn(Sent<T>) -> bool + Send + Sync>,
    stop: StopSignal,
}

impl<T> Clone for Worker<T> {
    fn clone(&self) -> Self {
        Worker { sink: Arc::clone(&self.sink), stop: self.stop.clone() }
    }
}

impl<T> Worker<T> {
    fn new(stop: StopSignal, sink: impl Fn(Sent<T>) -> bool + Send + Sync + 'static) -> Self {
        Worker { sink: Arc::new(sink), stop }
    }

    fn send(&self, sent: Sent<T>) -> bool {
        !self.stop.is_set() && (self.sink)(sent)
    }

    /// Writes `value` to the output stream.
    pub fn write(&self, value: T) -> bool {
        self.send(Sent::Output(value))
    }

    /// Writes `error` to the error stream. A terminating error ends the
    /// call that started the worker, with that error, once the pipeline
    /// thread reaches it.
    pub fn write_error(&self, error: PsError) -> bool {
        self.send(Sent::Error(error))
    }

    /// Writes `text` to the warning stream.
    pub fn warning(&self, text: impl Into<String>) -> bool {
        self.send(Sent::Stream(PS_STREAM_WARNING, text.into()))
    }

    /// Writes `text` to the verbose stream.
    pub fn verbose(&self, text: impl Into<String>) -> bool {
        self.send(Sent::Stream(PS_STREAM_VERBOSE, text.into()))
    }

    /// Writes `text` to the debug stream.
    pub fn debug(&self, text: impl Into<String>) -> bool {
        self.send(Sent::Stream(PS_STREAM_DEBUG, text.into()))
    }

    /// Writes `text` to the information stream.
    pub fn information(&self, text: impl Into<String>) -> bool {
        self.send(Sent::Stream(PS_STREAM_INFORMATION, text.into()))
    }

    /// Writes `record` to the progress stream.
    pub fn write_progress(&self, record: Progress) -> bool {
        self.send(Sent::Progress(record))
    }

    /// True once the worker should stop: the pipeline is stopping, a
    /// write failed, a terminating error was written, a worker of the
    /// same call panicked, or the call has returned.
    pub fn stopping(&self) -> bool {
        self.stop.is_set()
    }
}

/// The parallel helpers at a set number of workers, from
/// [`Pipeline::workers`]: at most that many items run at once. The
/// default pool starts that many threads for the call, or one per item
/// when there are fewer items. Under the `parallel` feature they are
/// tasks on Flynnel's pool, which also runs no more at once than it has
/// workers.
pub struct Workers<'a, 'ps> {
    ps: &'a Pipeline<'ps>,
    count: NonZeroUsize,
}

impl Workers<'_, '_> {
    /// The most items that run at once.
    pub fn count(&self) -> usize {
        self.count.get()
    }

    /// [`Pipeline::par_map`] at this count.
    pub fn par_map<T, U, F>(&self, items: Vec<T>, order: Order, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        U: IntoPs + Send + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        self.ps.par_map_in(Some(self.count), items, order, move |item, _worker: &Worker| f(item))
    }

    /// [`Pipeline::par_map_with`] at this count.
    pub fn par_map_with<T, U, F>(&self, items: Vec<T>, order: Order, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        U: IntoPs + Send + 'static,
        F: Fn(T, &Worker) -> U + Send + Sync + 'static,
    {
        self.ps.par_map_in(Some(self.count), items, order, f)
    }

    /// [`Pipeline::par_for_each`] at this count.
    pub fn par_for_each<T, F>(&self, items: Vec<T>, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        F: Fn(T) + Send + Sync + 'static,
    {
        self.ps.par_for_each_in(Some(self.count), items, move |item, _worker: &Worker| f(item))
    }

    /// [`Pipeline::par_for_each_with`] at this count.
    pub fn par_for_each_with<T, F>(&self, items: Vec<T>, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        F: Fn(T, &Worker) + Send + Sync + 'static,
    {
        self.ps.par_for_each_in(Some(self.count), items, f)
    }
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
}

/// Sets the flag when its thread unwinds through it, so the other
/// workers start no item after one has panicked.
struct HaltOnPanic<'a>(&'a AtomicBool);

impl Drop for HaltOnPanic<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.store(true, Ordering::Relaxed);
        }
    }
}

/// Runs `job` on the items `claim` hands out, one at a time, until none
/// is left or `halt` is set, which is read before each item starts.
fn claim_loop<T, J: Fn(usize, T)>(claim: &Claim<T>, halt: &AtomicBool, job: &J) {
    let _halts = HaltOnPanic(halt);
    while !halt.load(Ordering::Relaxed) {
        match claim.next() {
            Some((i, item)) => job(i, item),
            None => return,
        }
    }
}

/// The workers a helper runs when the caller names no count: the
/// machine's available parallelism, or one where the system cannot
/// report it.
#[cfg(not(feature = "parallel"))]
fn default_width() -> usize {
    match std::thread::available_parallelism() {
        Ok(n) => n.get(),
        Err(_unreported) => 1,
    }
}

/// Runs `job` over every item on std threads: `width` of them, or the
/// machine's parallelism, and never more than there are items. `halt`
/// is read before each item starts, so once it is set no further item
/// does. Returns when every thread has finished, and raises a panic
/// from one of them after the rest have stopped.
#[cfg(not(feature = "parallel"))]
fn run_pool<T: Send, J: Fn(usize, T) + Sync>(items: Vec<T>, width: Option<NonZeroUsize>, halt: &AtomicBool, job: &J) {
    let len = items.len();
    let width = match width {
        Some(n) => n.get(),
        None => default_width(),
    }
    .min(len);
    let claim = Claim::new(items);
    std::thread::scope(|s| {
        for _ in 0..width {
            s.spawn(|| claim_loop(&claim, halt, job));
        }
    });
}

/// The `parallel` pool. With no count, Flynnel halves the items until a
/// leaf is small enough to run inline, so a worker that finishes early
/// takes from one that has not. With a count, that many tasks on
/// Flynnel's pool each take one item at a time. `halt` is read before
/// each item starts.
#[cfg(feature = "parallel")]
fn run_pool<T: Send, J: Fn(usize, T) + Sync>(items: Vec<T>, width: Option<NonZeroUsize>, halt: &AtomicBool, job: &J) {
    /// Below this a split costs more than the items it separates.
    const LEAF: usize = 16;

    fn halve<T: Send, J: Fn(usize, T) + Sync>(plan: &flynnel::JobPlan, base: usize, items: &mut [Option<T>], halt: &AtomicBool, job: &J) {
        if halt.load(Ordering::Relaxed) {
            return;
        }
        if items.len() <= LEAF {
            let _halts = HaltOnPanic(halt);
            for (n, slot) in items.iter_mut().enumerate() {
                if halt.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(item) = slot.take() {
                    job(base + n, item);
                }
            }
            return;
        }
        let mid = items.len() / 2;
        let (left, right) = items.split_at_mut(mid);
        flynnel::join(plan, || halve(plan, base, left, halt, job), || halve(plan, base + mid, right, halt, job));
    }

    /// Runs `work` in `n` tasks on the pool.
    fn fan<W: Fn() + Sync>(plan: &flynnel::JobPlan, n: usize, work: &W) {
        if n <= 1 {
            work();
            return;
        }
        let half = n / 2;
        flynnel::join(plan, || fan(plan, half, work), || fan(plan, n - half, work));
    }

    let len = items.len();
    let plan = flynnel::JobPlan::new(len.next_power_of_two().trailing_zeros() as u8, len as u32);
    match width {
        None => {
            let mut slots: Vec<Option<T>> = items.into_iter().map(Some).collect();
            halve(&plan, 0, &mut slots, halt, job);
        }
        Some(n) => {
            let claim = Claim::new(items);
            fan(&plan, n.get().min(len), &|| claim_loop(&claim, halt, job));
        }
    }
}

/// The error a call returns when one of its worker threads panicked:
/// the items it held are never written, and the stream would otherwise
/// be short without saying so.
fn worker_panic() -> PsError {
    PsError::new(ErrorCategory::InvalidOperation, "PwrsWorkerPanic", "worker thread panicked").terminating()
}

impl<'ps> Pipeline<'ps> {
    /// Runs `work` on a new thread while this thread forwards every
    /// value it sends to the output stream, in order. Draining stops
    /// when the pipeline is stopping, which is noticed while the worker
    /// is silent too; the worker is always joined, so one that neither
    /// sends nor watches for the stop keeps Ctrl+C waiting until it
    /// returns. [`Pipeline::stream_from_thread_until`] hands the worker
    /// the stop as well, and [`Pipeline::stream_from_worker`] every
    /// stream.
    pub fn stream_from_thread<T, F>(&self, work: F) -> PsResult<()>
    where
        T: IntoPs + Send + 'static,
        F: FnOnce(std::sync::mpsc::Sender<T>) + Send + 'static,
    {
        self.stream_from_thread_until(move |tx, _stop| work(tx))
    }

    /// [`Pipeline::stream_from_thread`], with a [`StopSignal`] the worker
    /// checks between steps of work that sends nothing for a long time,
    /// so a stopped pipeline gets the thread back promptly.
    pub fn stream_from_thread_until<T, F>(&self, work: F) -> PsResult<()>
    where
        T: IntoPs + Send + 'static,
        F: FnOnce(std::sync::mpsc::Sender<T>, StopSignal) + Send + 'static,
    {
        let halt = StopSignal::new();
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
        halt.set();
        drop(rx);
        if worker.join().is_err() {
            return Err(worker_panic());
        }
        result
    }

    /// Runs `work` on a new thread with a [`Worker`] through which it
    /// writes output and every other stream, and on this thread writes
    /// each record it sends, in the order sent. Draining stops when the
    /// pipeline is stopping, which is noticed while the worker is silent
    /// too, and [`Worker::stopping`] then turns true. A terminating error
    /// the worker writes ends the call with that error. The worker is
    /// always joined.
    ///
    /// ```ignore
    /// ps.stream_from_worker(move |w| {
    ///     for (n, file) in files.iter().enumerate() {
    ///         if !w.write_progress(Progress::new(1, "Hashing", format!("{n} of {}", files.len()))) {
    ///             return;
    ///         }
    ///         match hash(file) {
    ///             Ok(sum) => w.write(sum),
    ///             Err(e) => w.write_error(PsError::new(ErrorCategory::ReadError, "HashFailed", e.to_string())),
    ///         };
    ///     }
    /// })
    /// ```
    pub fn stream_from_worker<T, F>(&self, work: F) -> PsResult<()>
    where
        T: IntoPs + Send + 'static,
        F: FnOnce(Worker<T>) + Send + 'static,
    {
        let stop = StopSignal::new();
        let (tx, rx) = std::sync::mpsc::channel::<Sent<T>>();
        let worker = Worker::new(stop.clone(), move |sent| tx.send(sent).is_ok());
        let thread = std::thread::spawn(move || work(worker));
        let result = self.drain(&rx, &|| thread.is_finished(), &mut |value| self.write(value));
        stop.set();
        drop(rx);
        if thread.join().is_err() {
            return Err(worker_panic());
        }
        result
    }

    /// Maps `items` across a worker pool and writes every result to
    /// the output stream from this thread.
    ///
    /// The pipeline token is `!Send`, so `f` cannot capture it and a
    /// worker cannot reach the engine: the parallel half runs over
    /// owned Rust data with no managed object in it, and the writes
    /// happen on the one thread allowed to make them. The pool is as
    /// wide as the machine's available parallelism, or as
    /// [`Pipeline::workers`] sets.
    ///
    /// Draining stops when the pipeline is stopping, which is noticed
    /// while no result arrives too, and no item starts after it; every
    /// worker is joined before returning, so an item that runs long
    /// holds the stop until it ends, and [`Pipeline::par_map_with`]
    /// hands `f` the stop to check. A worker that panics fails the call
    /// with a terminating `PwrsWorkerPanic`, since the items it held are
    /// never written and the stream would otherwise be short without
    /// saying so, and no item starts after it either.
    pub fn par_map<T, U, F>(&self, items: Vec<T>, order: Order, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        U: IntoPs + Send + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        self.par_map_in(None, items, order, move |item, _worker: &Worker| f(item))
    }

    /// [`Pipeline::par_map`], with a [`Worker`] handed to `f` for the
    /// other streams and for the stop, so a long item can report its
    /// progress and give up once the pipeline stops. The results keep
    /// the order asked for; what `f` sends through the worker is
    /// written as it arrives.
    pub fn par_map_with<T, U, F>(&self, items: Vec<T>, order: Order, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        U: IntoPs + Send + 'static,
        F: Fn(T, &Worker) -> U + Send + Sync + 'static,
    {
        self.par_map_in(None, items, order, f)
    }

    /// [`Pipeline::par_map`] for work whose results are not written,
    /// on the same pool. This thread waits without spinning, and stops
    /// waiting when the pipeline is stopping.
    pub fn par_for_each<T, F>(&self, items: Vec<T>, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        F: Fn(T) + Send + Sync + 'static,
    {
        self.par_for_each_in(None, items, move |item, _worker: &Worker| f(item))
    }

    /// [`Pipeline::par_for_each`], with a [`Worker`] handed to `f` for
    /// the other streams and for the stop.
    pub fn par_for_each_with<T, F>(&self, items: Vec<T>, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        F: Fn(T, &Worker) + Send + Sync + 'static,
    {
        self.par_for_each_in(None, items, f)
    }

    /// The parallel helpers with at most `count` items running at once,
    /// for work that waits on disks or the network more than it
    /// computes, or that must not take the whole machine. Any count from
    /// one up; zero is refused with `PwrsWorkerCount`.
    ///
    /// ```ignore
    /// ps.workers(self.threads as usize)?.par_for_each_with(files, |file, w| copy(file, w))?;
    /// ```
    pub fn workers(&self, count: usize) -> PsResult<Workers<'_, 'ps>> {
        match NonZeroUsize::new(count) {
            Some(count) => Ok(Workers { ps: self, count }),
            None => Err(PsError::new(ErrorCategory::InvalidArgument, "PwrsWorkerCount", "a parallel helper needs at least one worker, and 0 was asked for")),
        }
    }

    fn par_map_in<T, U, F>(&self, width: Option<NonZeroUsize>, items: Vec<T>, order: Order, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        U: IntoPs + Send + 'static,
        F: Fn(T, &Worker) -> U + Send + Sync + 'static,
    {
        if items.is_empty() {
            return Ok(());
        }
        let stop = StopSignal::new();
        let (tx, rx) = std::sync::mpsc::channel::<Sent<(usize, U)>>();
        let side = tx.clone();
        let worker = Worker::new(stop.clone(), move |sent: Sent<Infallible>| side.send(sent.widen()).is_ok());
        let halt = stop.clone();
        let coordinator = std::thread::spawn(move || {
            let job = |i: usize, item: T| {
                let value = f(item, &worker);
                if tx.send(Sent::Output((i, value))).is_err() {
                    halt.set();
                }
            };
            run_pool(items, width, halt.flag(), &job);
        });

        let finished = || coordinator.is_finished();
        let result = match order {
            Order::AsReady => self.drain(&rx, &finished, &mut |(_, value)| self.write(value)),
            Order::Input => {
                // Results that arrived early wait here so the stream
                // keeps input order: a slow item stalls the ones behind
                // it, and this grows while it does.
                let mut pending = std::collections::BTreeMap::new();
                let mut next = 0usize;
                self.drain(&rx, &finished, &mut |(i, value)| {
                    pending.insert(i, value);
                    while let Some(ready) = pending.remove(&next) {
                        if self.stopping() {
                            return Ok(());
                        }
                        self.write(ready)?;
                        next += 1;
                    }
                    Ok(())
                })
            }
        };
        stop.set();
        drop(rx);
        if coordinator.join().is_err() {
            return Err(worker_panic());
        }
        result
    }

    fn par_for_each_in<T, F>(&self, width: Option<NonZeroUsize>, items: Vec<T>, f: F) -> PsResult<()>
    where
        T: Send + 'static,
        F: Fn(T, &Worker) + Send + Sync + 'static,
    {
        if items.is_empty() {
            return Ok(());
        }
        let stop = StopSignal::new();
        let (tx, rx) = std::sync::mpsc::channel::<Sent<Infallible>>();
        let worker = Worker::new(stop.clone(), move |sent| tx.send(sent).is_ok());
        let halt = stop.clone();
        let coordinator = std::thread::spawn(move || {
            let job = |_: usize, item: T| f(item, &worker);
            run_pool(items, width, halt.flag(), &job);
        });
        let result = self.drain(&rx, &|| coordinator.is_finished(), &mut |never| match never {});
        stop.set();
        drop(rx);
        if coordinator.join().is_err() {
            return Err(worker_panic());
        }
        result
    }

    /// Writes what workers send on `rx` until every sender is gone, or
    /// `finished` answers true and nothing is waiting, or the pipeline
    /// is stopping. Output goes to `output`. The records waiting when
    /// the thread wakes are written together, in the order sent, and of
    /// the progress records among them for one activity only the last.
    /// A terminating error ends the drain as its `Err`.
    fn drain<X>(&self, rx: &Receiver<Sent<X>>, finished: &dyn Fn() -> bool, output: &mut dyn FnMut(X) -> PsResult<()>) -> PsResult<()> {
        loop {
            if self.stopping() {
                return Ok(());
            }
            let first = match rx.recv_timeout(STOP_POLL) {
                Ok(sent) => sent,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
                Err(RecvTimeoutError::Timeout) => {
                    // A worker handle kept after the work returned holds
                    // the channel open, so the end of the work is read
                    // from the thread that ran it.
                    if !finished() {
                        continue;
                    }
                    match rx.try_recv() {
                        Ok(sent) => sent,
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(()),
                    }
                }
            };
            let mut batch = vec![first];
            while !self.stopping() {
                match rx.try_recv() {
                    Ok(sent) => batch.push(sent),
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }
            self.write_sent(batch, output)?;
        }
    }

    /// Writes `batch` in order, leaving out each progress record that a
    /// later one in the batch for the same activity replaces.
    fn write_sent<X>(&self, batch: Vec<Sent<X>>, output: &mut dyn FnMut(X) -> PsResult<()>) -> PsResult<()> {
        let mut latest = std::collections::HashMap::new();
        for (i, sent) in batch.iter().enumerate() {
            if let Sent::Progress(record) = sent {
                latest.insert(record.activity_id, i);
            }
        }
        for (i, sent) in batch.into_iter().enumerate() {
            if self.stopping() {
                return Ok(());
            }
            match sent {
                Sent::Output(value) => output(value)?,
                Sent::Error(error) if error.terminating => return Err(error),
                Sent::Error(error) => self.write_error(&error)?,
                Sent::Stream(kind, text) => self.stream(kind, &text)?,
                Sent::Progress(record) => {
                    if latest.get(&record.activity_id) == Some(&i) {
                        self.write_progress(&record)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{self, Value};

    #[test]
    fn of_the_progress_waiting_for_one_activity_only_the_latest_is_written() {
        let _host = testing::install();
        let stopping = AtomicBool::new(false);
        let scratch = core::cell::Cell::new(Vec::new());
        let ps = unsafe { Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
        testing::take_output();
        testing::take_progress();

        let at = |id: i32, percent: i32| Sent::Progress(Progress::new(id, "Copying", format!("{percent}%")).with_percent(percent));
        let batch: Vec<Sent<i64>> = vec![at(1, 10), Sent::Output(7), at(2, 50), at(1, 20), at(1, 30), Sent::Output(8), at(2, 60)];
        ps.write_sent(batch, &mut |value| ps.write(value)).expect("write");

        let written: Vec<(i32, i32)> = testing::take_progress().iter().map(|p| (p.activity_id, p.percent_complete)).collect();
        assert_eq!(written, vec![(1, 30), (2, 60)], "the last record for each activity, in the order sent");
        let out: Vec<i64> = testing::take_output()
            .into_iter()
            .map(|v| match v {
                Value::Int(n) => n,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(out, vec![7, 8], "output is never left out");
    }
}
