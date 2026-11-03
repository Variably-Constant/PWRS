//! Events, two kinds of them. Engine events a module raises from any
//! thread, which script receives through `Register-EngineEvent`,
//! `Wait-Event` and `Get-Event`, and which a phase reads back from the
//! runspace's queue. And the events of .NET objects: an event of the
//! module's own proxy object raised from Rust, and a Rust callback
//! subscribed to an event of any object.

use crate::host::vtable;
use crate::pipeline::{check, str16, utf16};
use crate::{ErrorCategory, FromPs, IntoPs, PsDateTime, PsError, PsObject, PsResult};
use core::ffi::c_void;
use pwrs_sys::{PsHandle, PsStatus, PS_ERR_MANAGED_EXCEPTION, PS_ERR_NATIVE_PANIC, PS_OK};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

/// The engine events of the runspace a cmdlet ran in, from
/// [`crate::Pipeline::events`]. `Clone`, `Send` and `Sync`: a worker
/// thread, an attached thread or a thread of the module's that runs on after the call
/// returns raises events through it, for as long as it lives.
///
/// An event is raised the way `New-Event` raises one: it is queued, or
/// passed to the actions subscribed to it, before the raise returns, so
/// the events one thread raises arrive in the order it raised them. A
/// `Register-EngineEvent -Action` runs later, on the thread the runspace
/// runs its pipelines on, never on the thread that raised the event.
///
/// [`PsEvents::subscribe`], [`PsEvents::take`] and [`PsEvents::wait`] read
/// the runspace's queue of events, which `Get-Event` reads, through the
/// cmdlet whose `events` made this, so they run on its pipeline thread.
#[derive(Clone, Debug)]
pub struct PsEvents {
    manager: PsObject,
    /// The managed cmdlet whose phase made this, through which the queue
    /// is read; `None` for one no phase made.
    cmdlet: Option<PsObject>,
}

impl PsEvents {
    /// The events `manager`, a `PSEventManager`, raises.
    pub(crate) fn over(manager: PsObject) -> PsEvents {
        PsEvents { manager, cmdlet: None }
    }

    /// The events `manager` raises, whose queue is read through `cmdlet`.
    pub(crate) fn of_cmdlet(manager: PsObject, cmdlet: PsObject) -> PsEvents {
        PsEvents { manager, cmdlet: Some(cmdlet) }
    }

    /// Raises the event `source_identifier` with `message_data`, converted
    /// on this thread, as its `MessageData`, and no sender or arguments.
    pub fn raise(&self, source_identifier: &str, message_data: impl IntoPs) -> PsResult<()> {
        self.raise_with(source_identifier, (), (), message_data)
    }

    /// Raises the event `source_identifier` with a sender, arguments and
    /// `MessageData`, as `New-Event -Sender -EventArguments -MessageData`
    /// does. `()` gives none of any of them. `args` that converts to an
    /// array gives the event its elements as arguments, and anything else
    /// gives it one argument. Each value converts on this thread, and one
    /// that does not convert raises nothing.
    pub fn raise_with(&self, source_identifier: &str, sender: impl IntoPs, args: impl IntoPs, message_data: impl IntoPs) -> PsResult<()> {
        // This thread calls into .NET on purpose here, to raise the
        // event.
        let _attached = crate::host::attach_current_thread();
        let sender = sender.into_ps()?;
        let args = args.into_ps()?;
        let data = message_data.into_ps()?;
        let id = utf16(source_identifier);
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().event_raise)(self.manager.as_raw(), str16(&id), sender.as_raw(), args.as_raw(), data.as_raw(), &mut err) };
        check(status, err)
    }

    /// Subscribes the runspace's queue to the event `event` of `source`:
    /// each raise, on any thread, queues a record under
    /// `source_identifier`, which [`PsEvents::take`] and [`PsEvents::wait`]
    /// read, and `Get-Event` too until they do. The record is shaped as
    /// `Register-ObjectEvent` shapes one: its `Sender` is `source` and its
    /// `SourceArgs` are the delegate's arguments. It is queued on the
    /// raising thread, as [`PsEvents::raise_with`] queues one, so the
    /// records one thread raises stay in their order, an order
    /// `Register-ObjectEvent`, queuing each through the thread pool, loses.
    /// Runs on any thread, and so does the drop that ends it.
    pub fn subscribe(&self, source: &PsObject, event: &str, source_identifier: &str) -> PsResult<PsEventSubscription> {
        let events = self.clone();
        let id = source_identifier.to_string();
        let raised_by = source.clone();
        let forward = source.subscribe(event, move |sender, args| {
            let mut all: Vec<PsObject> = Vec::with_capacity(args.len() + 1);
            if !sender.is_null() {
                all.push(sender.clone());
            }
            all.extend(args.iter().cloned());
            events.raise_with(&id, raised_by.clone(), all, ())
        })?;
        Ok(PsEventSubscription { forward, source_identifier: source_identifier.to_string() })
    }

    /// Takes every record queued under `source_identifier`, oldest first,
    /// out of the queue, as `Get-Event` and then `Remove-Event` do, with
    /// one call of each however many wait: an empty list when none does.
    /// A phase reading a steady stream takes in a loop and waits only when
    /// nothing is queued. Runs on the pipeline thread.
    pub fn take(&self, source_identifier: &str) -> PsResult<Vec<PsEventRecord>> {
        let cmdlet = self.phase_cmdlet("take")?;
        let queued = queued_events(&cmdlet, source_identifier)?;
        if queued.is_empty() {
            return Ok(Vec::new());
        }
        let records = queued.iter().map(PsEventRecord::from_ps).collect::<PsResult<Vec<PsEventRecord>>>()?;
        let taken = crate::PsArray(queued).into_ps()?;
        crate::pipeline::invoke_on(cmdlet.as_raw(), "Remove-Event", &[("ErrorAction", "Ignore".into_ps()?)], Some(&taken))?;
        Ok(records)
    }

    /// Takes the oldest record queued under `source_identifier`, waiting
    /// for one when none is queued: until a raise queues one, the pipeline
    /// stops, or `timeout`, if given, has passed, returning `None` for the
    /// last two. The queue wakes the wait as each record arrives, as it
    /// wakes `Wait-Event`, and the wait looks at the pipeline every 50 ms.
    /// The wait is a .NET wait, as `Wait-Event`'s is, so on a thread of a
    /// single-threaded apartment, which the pipeline thread is, the calls
    /// COM queues for the thread run during it: a COM object made on the
    /// pipeline thread raises its events while a phase waits. Runs on the
    /// pipeline thread.
    pub fn wait(&self, source_identifier: &str, timeout: Option<Duration>) -> PsResult<Option<PsEventRecord>> {
        let cmdlet = self.phase_cmdlet("wait")?;
        let wake = crate::PsType::from_name("System.Threading.AutoResetEvent").new(&[false.into_ps()?])?;
        let waited = self.wait_on(&cmdlet, &wake, source_identifier, timeout);
        let disposed = wake.call("Dispose", &[]);
        let record = waited?;
        disposed?;
        Ok(record)
    }

    /// The loop of [`PsEvents::wait`], waiting on `wake`, which the queue
    /// sets as each record arrives, through a handler removed before this
    /// returns.
    fn wait_on(&self, cmdlet: &PsObject, wake: &PsObject, source_identifier: &str, timeout: Option<Duration>) -> PsResult<Option<PsEventRecord>> {
        let received = self.manager.get("ReceivedEvents")?;
        let set = wake.clone();
        let _wakes = received.subscribe("PSEventReceived", move |_, _| set.call("Set", &[]).map(drop))?;
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            if let Some(first) = queued_events(cmdlet, source_identifier)?.into_iter().next() {
                return Ok(Some(remove_event(cmdlet, &first)?));
            }
            if bool::from_ps(&cmdlet.get("Stopping")?)? {
                return Ok(None);
            }
            let slice = match deadline {
                Some(at) => match at.checked_duration_since(Instant::now()) {
                    Some(left) if !left.is_zero() => left.min(POLL),
                    _passed => return Ok(None),
                },
                None => POLL,
            };
            let millis = i32::try_from(slice.as_millis())
                .map_err(|e| PsError::new(ErrorCategory::InvalidOperation, "PwrsWaitSlice", format!("a wait of {} ms: {e}", slice.as_millis())))?;
            wake.call("WaitOne", &[millis.into_ps()?])?;
        }
    }

    /// The cmdlet whose phase made this, through which `what` reads the
    /// queue.
    fn phase_cmdlet(&self, what: &str) -> PsResult<PsObject> {
        match &self.cmdlet {
            Some(cmdlet) => Ok(cmdlet.clone()),
            None => Err(PsError::new(
                ErrorCategory::InvalidOperation,
                "PwrsNoPhase",
                format!("PsEvents::{what} reads the queue through the cmdlet whose Pipeline::events made the PsEvents, and no phase made this one"),
            )),
        }
    }
}

/// How often [`PsEvents::wait`] looks whether the pipeline is stopping.
const POLL: Duration = Duration::from_millis(50);

/// The `PSEventArgs` queued under `source_identifier`, oldest first.
fn queued_events(cmdlet: &PsObject, source_identifier: &str) -> PsResult<Vec<PsObject>> {
    crate::pipeline::invoke_on(cmdlet.as_raw(), "Get-Event", &[("SourceIdentifier", source_identifier.into_ps()?), ("ErrorAction", "Ignore".into_ps()?)], None)
}

/// Removes `event`, a queued `PSEventArgs`, from the queue, and reads it.
fn remove_event(cmdlet: &PsObject, event: &PsObject) -> PsResult<PsEventRecord> {
    let record = PsEventRecord::from_ps(event)?;
    crate::pipeline::invoke_on(cmdlet.as_raw(), "Remove-Event", &[("EventIdentifier", event.get("EventIdentifier")?), ("ErrorAction", "Ignore".into_ps()?)], None)?;
    Ok(record)
}

/// One event from the runspace's queue, as `Get-Event` shows it: the
/// properties of its `PSEventArgs`.
#[derive(Clone, Debug)]
pub struct PsEventRecord {
    pub source_identifier: String,
    /// The number the runspace gave it, which `Remove-Event
    /// -EventIdentifier` takes.
    pub event_identifier: i32,
    /// The object that raised it.
    pub sender: PsObject,
    /// Every argument the event's delegate was called with, the sender
    /// first for a delegate `(object sender, T e)`.
    pub source_args: Vec<PsObject>,
    /// The `e` of a delegate `(object sender, T e)` whose `T` is an
    /// `EventArgs`, and `$null` otherwise.
    pub source_event_args: PsObject,
    pub message_data: PsObject,
    pub time_generated: PsDateTime,
}

impl FromPs for PsEventRecord {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        Ok(PsEventRecord {
            source_identifier: String::from_ps(&obj.get("SourceIdentifier")?)?,
            event_identifier: i32::from_ps(&obj.get("EventIdentifier")?)?,
            sender: obj.get("Sender")?,
            source_args: <Vec<PsObject>>::from_ps(&obj.get("SourceArgs")?)?,
            source_event_args: obj.get("SourceEventArgs")?,
            message_data: obj.get("MessageData")?,
            time_generated: PsDateTime::from_ps(&obj.get("TimeGenerated")?)?,
        })
    }
}

/// The runspace's queue subscribed to an event, from
/// [`PsEvents::subscribe`]. Dropping it unsubscribes, on any thread; the
/// records already queued stay for [`PsEvents::take`] and `Get-Event`.
#[derive(Debug)]
pub struct PsEventSubscription {
    /// The callback that queues each raise.
    forward: PsSubscription,
    source_identifier: String,
}

impl PsEventSubscription {
    /// The identifier the subscription queues records under.
    pub fn source_identifier(&self) -> &str {
        &self.source_identifier
    }

    /// Unsubscribes now, returning what went wrong, which a drop can only
    /// write to standard error.
    pub fn unsubscribe(self) -> PsResult<()> {
        self.forward.unsubscribe()
    }
}

/// Raises the event `event` of `obj`, an object of a module's proxy
/// class, with `args`: an array's elements, none for `()`, and anything
/// else as the one argument. For a delegate `(object sender, T e)` the
/// object is the sender and the one argument is `e`. A raise made by the
/// thread inside a call into the object is delivered when that call
/// returns; any other runs the handlers on this thread before it returns,
/// and a handler's exception is the `Err`.
pub(crate) fn raise_on(obj: &PsObject, event: &str, args: impl IntoPs) -> PsResult<()> {
    // This thread calls into .NET on purpose here, to run the handlers.
    let _attached = crate::host::attach_current_thread();
    let args = args.into_ps()?;
    let name = utf16(event);
    let mut err = PsHandle::NULL;
    let status = unsafe { (vtable().object_event_raise)(obj.as_raw(), str16(&name), args.as_raw(), &mut err) };
    check(status, err)
}

impl PsObject {
    /// Subscribes `callback` to the event `event` of this object, or to
    /// the static event of that name when this is a type, as C#'s `+=`
    /// does: each raise calls it on the raising thread, before the raise
    /// goes on, with the sender and the arguments. For a delegate
    /// `(object sender, T e)` those are the sender and `[e]`; for any
    /// other delegate the sender is `$null` and the arguments are all of
    /// them. An `Err` or a panic is thrown to the raiser. A delegate that
    /// returns a value or takes a `ref`, `out` or `in` argument is
    /// refused.
    ///
    /// A COM object made late-bound, as `New-Object -ComObject` makes one,
    /// has no .NET events; its events are the methods of the default
    /// source interface its class's type information names, and the
    /// callback gets no sender and the event's arguments. One that returns
    /// a value or takes an out argument is refused, and so is every event
    /// of an object with no such type information. COM raises the events
    /// of an object made on the pipeline thread on that thread, while it
    /// waits on a .NET wait, such as [`PsEvents::wait`] or a `WaitHandle`'s
    /// `WaitOne`. The runtime detaches a callback still on a COM object
    /// when the runspace it was subscribed in closes, and every one left
    /// when the module unloads.
    ///
    /// Dropping the subscription removes the callback and does not wait:
    /// a call already running on another thread finishes with its own
    /// share of the closure, which is dropped after the last call returns.
    pub fn subscribe<F>(&self, event: &str, callback: F) -> PsResult<PsSubscription>
    where
        F: Fn(&PsObject, &[PsObject]) -> PsResult<()> + Send + Sync + 'static,
    {
        let _attached = crate::host::attach_current_thread();
        let name = utf16(event);
        let context = Box::into_raw(Box::new(callback)).cast::<c_void>();
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().object_event_subscribe)(self.as_raw(), str16(&name), call_callback::<F>, context, release_callback::<F>, &mut out, &mut err) };
        if let Err(e) = check(status, err) {
            // The runtime refused it, so the closure is still this call's.
            drop(unsafe { Box::from_raw(context.cast::<F>()) });
            return Err(e);
        }
        Ok(PsSubscription { handle: unsafe { PsObject::from_raw(out) }, event: event.to_string(), ended: false })
    }
}

/// A Rust callback subscribed to an event of a .NET object, from
/// [`PsObject::subscribe`]. Dropping it removes the callback without
/// waiting, on any thread.
#[derive(Debug)]
pub struct PsSubscription {
    handle: PsObject,
    event: String,
    /// `unsubscribe` ended it, so the drop does nothing.
    ended: bool,
}

impl PsSubscription {
    /// Removes the callback now, returning what went wrong, which a drop
    /// can only write to standard error.
    pub fn unsubscribe(mut self) -> PsResult<()> {
        self.ended = true;
        self.end()
    }

    fn end(&self) -> PsResult<()> {
        let _attached = crate::host::attach_current_thread();
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().object_event_unsubscribe)(self.handle.as_raw(), &mut err) };
        check(status, err)
    }
}

impl Drop for PsSubscription {
    fn drop(&mut self) {
        if self.ended {
            return;
        }
        if let Err(e) = self.end() {
            eprintln!("pwrs: removing the callback of the event {} failed: {e}", self.event);
        }
    }
}

/// Runs a subscription's callback for one raise, on the raising thread.
/// The sender and the arguments are the runtime's handles for the call,
/// so each is cloned before Rust stores it.
unsafe extern "C" fn call_callback<F>(context: *mut c_void, sender: PsHandle, args: PsHandle, err: *mut PsHandle) -> PsStatus
where
    F: Fn(&PsObject, &[PsObject]) -> PsResult<()> + Send + Sync + 'static,
{
    let _called_in = crate::host::CalledIn::enter();
    // SAFETY: the runtime passes the context subscribe gave it, and calls
    // this only before it releases it.
    let callback = unsafe { &*context.cast::<F>() };
    let result = catch_unwind(AssertUnwindSafe(|| -> PsResult<()> {
        let sender = if sender.is_null() { PsObject::null() } else { unsafe { PsObject::from_raw((vtable().clone_handle)(sender)) } };
        let args = <Vec<PsObject>>::from_ps(&unsafe { PsObject::from_raw((vtable().clone_handle)(args)) })?;
        callback(&sender, &args)
    }));
    match result {
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

/// Drops a subscription's closure, once, after its last call. A closure
/// that panics as it drops is reported and not unwound into the runtime.
unsafe extern "C" fn release_callback<F>(context: *mut c_void) {
    // SAFETY: the runtime releases the context subscribe gave it, once.
    let callback = unsafe { Box::from_raw(context.cast::<F>()) };
    if catch_unwind(AssertUnwindSafe(move || drop(callback))).is_err() {
        eprintln!("pwrs: an event callback panicked as it was dropped");
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{self, Value};
    use std::sync::Arc;

    #[test]
    fn a_raise_reaches_the_host_with_the_object_the_event_and_the_argument() {
        let _host = testing::install();
        let this = crate::PsThis::from_object(testing::object(Value::Str("draft".to_string())));
        this.raise("Saved", 7i64).expect("the raise");
        let raised = testing::object_raised();
        assert_eq!(raised.len(), 1);
        assert_eq!(raised[0].event, "Saved");
        assert!(matches!(&raised[0].object, Value::Str(s) if s == "draft"), "{:?}", raised[0].object);
        assert!(matches!(raised[0].args, Value::Int(7)), "{:?}", raised[0].args);
    }

    /// A subscription the host refuses gives the closure back, and it is
    /// dropped, so nothing it captured stays alive.
    #[test]
    fn a_refused_subscription_drops_its_callback() {
        let _host = testing::install();
        let held = Arc::new(());
        let kept = held.clone();
        let source = testing::object(Value::Str("source".to_string()));
        let refused = match source.subscribe("Changed", move |_, _| {
            assert!(Arc::strong_count(&kept) > 1);
            Ok(())
        }) {
            Ok(_subscription) => panic!("the fake host has no events to subscribe to"),
            Err(e) => e,
        };
        assert!(refused.to_string().contains("no .NET events"), "{refused}");
        assert_eq!(Arc::strong_count(&held), 1);
    }

    #[test]
    fn the_queue_is_read_only_through_the_events_a_phase_made() {
        let _host = testing::install();
        let events = testing::events("manager");
        let refused = events.take("Hello.Ticks").expect_err("no phase made these events");
        assert!(refused.to_string().contains("PwrsNoPhase"), "{refused}");
        let refused = events.wait("Hello.Ticks", Some(std::time::Duration::from_millis(1))).expect_err("no phase made these events");
        assert!(refused.to_string().contains("PwrsNoPhase"), "{refused}");
    }
}
