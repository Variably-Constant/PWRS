//! Engine events a module raises from any thread, which script receives
//! through `Register-EngineEvent`, `Wait-Event` and `Get-Event`.

use crate::host::vtable;
use crate::pipeline::{check, str16, utf16};
use crate::{IntoPs, PsObject, PsResult};
use pwrs_sys::PsHandle;

/// The engine events of the runspace a cmdlet ran in, from
/// [`crate::Pipeline::events`]. `Clone`, `Send` and `Sync`: a worker
/// thread, an attached thread or a thread the module keeps after the call
/// returns raises events through it, for as long as it lives.
///
/// An event is raised the way `New-Event` raises one: it is queued, or
/// handed to the actions subscribed to it, before the raise returns, so
/// the events one thread raises arrive in the order it raised them. A
/// `Register-EngineEvent -Action` runs later, on the thread the runspace
/// runs its pipelines on, never on the thread that raised the event.
#[derive(Clone, Debug)]
pub struct PsEvents {
    manager: PsObject,
}

impl PsEvents {
    /// The events `manager`, a `PSEventManager`, raises.
    pub(crate) fn over(manager: PsObject) -> PsEvents {
        PsEvents { manager }
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
        // Raising is a crossing this thread makes on purpose.
        let _attached = crate::host::attach_current_thread();
        let sender = sender.into_ps()?;
        let args = args.into_ps()?;
        let data = message_data.into_ps()?;
        let id = utf16(source_identifier);
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().event_raise)(self.manager.as_raw(), str16(&id), sender.as_raw(), args.as_raw(), data.as_raw(), &mut err) };
        check(status, err)
    }
}
