//! The process-wide [`HostVTable`] pointer handed over at
//! `pwrs_module_init`, and the checked accessor every other module
//! uses to reach it.

use core::sync::atomic::{AtomicPtr, Ordering};
use pwrs_sys::{HostVTable, PsStatus, PS_ERR_ABI_MISMATCH, PS_OK, PWRS_ABI_VERSION};

static VTABLE: AtomicPtr<HostVTable> = AtomicPtr::new(core::ptr::null_mut());

/// Records the runtime's table. Returns [`PS_ERR_ABI_MISMATCH`] when
/// the runtime implements a different major version or a table
/// smaller than this crate was built against.
///
/// # Safety
/// `table` must point to a table that outlives the module.
pub unsafe fn install(table: *const HostVTable) -> PsStatus {
    if table.is_null() {
        return PS_ERR_ABI_MISMATCH;
    }
    let t = unsafe { &*table };
    if t.version != PWRS_ABI_VERSION || (t.size as usize) < core::mem::size_of::<HostVTable>() {
        return PS_ERR_ABI_MISMATCH;
    }
    VTABLE.store(table as *mut HostVTable, Ordering::Release);
    PS_OK
}

/// The installed table. Panics if `pwrs_module_init` never ran.
#[inline]
pub fn vtable() -> &'static HostVTable {
    let p = VTABLE.load(Ordering::Acquire);
    assert!(!p.is_null(), "pwrs: host vtable not installed; pwrs_module_init was not called");
    unsafe { &*p }
}

/// Whether `PsObject` methods check the thread they run on: in a debug
/// build, and wherever `PWRS_THREAD_CHECK` is `1`, which `cargo pwrs
/// test` sets for its Pester hosts. Off in a release build otherwise,
/// where it costs one load and a branch. Read once per process.
pub(crate) fn thread_check() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        cfg!(debug_assertions)
            || match std::env::var("PWRS_THREAD_CHECK") {
                Ok(v) => v.trim() == "1",
                Err(std::env::VarError::NotPresent) => false,
                Err(std::env::VarError::NotUnicode(_raw)) => false,
            }
    })
}

std::thread_local! {
    static CALLED_IN: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}

/// Marks the current thread as running a call the host made into the
/// module, for as long as it lives. Thread-local storage is touched only
/// while the check is on: in a cdylib it is reached through the dynamic
/// linker, and every pipeline record enters through here.
pub(crate) struct CalledIn {
    marked: bool,
}

impl CalledIn {
    #[inline]
    pub(crate) fn enter() -> CalledIn {
        let marked = thread_check();
        if marked {
            CALLED_IN.with(|c| c.set(c.get() + 1));
        }
        CalledIn { marked }
    }

    /// Marks the thread for the rest of its life: the fake host's test
    /// threads, which call `PsObject` methods with no call coming in.
    pub(crate) fn mark_thread() {
        if thread_check() {
            CALLED_IN.with(|c| c.set(c.get() + 1));
        }
    }
}

impl Drop for CalledIn {
    #[inline]
    fn drop(&mut self) {
        if self.marked {
            CALLED_IN.with(|c| c.set(c.get() - 1));
        }
    }
}

/// Declares the calling thread, one the module started, as a thread that
/// reaches the host on purpose, for as long as the returned guard lives:
/// while the thread check is on, `PsObject` and `PsType` methods called
/// on it run instead of returning `PwrsOffThread`.
///
/// Take it at the start of a thread that drives PowerShell itself, such
/// as one that runs `[powershell]::Create()` in a runspace the module
/// manages, and hold it while the thread makes such calls. The module
/// answers for what those calls run and in which runspace. A worker that
/// only computes Rust values takes none, so the check still catches one
/// that reaches for a `PsObject`. With the check off, as in a release
/// build outside `cargo pwrs test`, the guard changes nothing.
///
/// ```ignore
/// std::thread::spawn(move || {
///     let _attached = pwrs::attach_current_thread();
///     // PsType::call_static, PsObject::call, ... on this thread
/// });
/// ```
pub fn attach_current_thread() -> AttachedThread {
    AttachedThread { _called_in: CalledIn::enter(), _thread: core::marker::PhantomData }
}

/// The mark [`attach_current_thread`] puts on a thread, taken away when
/// it drops. It stays on the thread that took it:
///
/// ```compile_fail,E0277
/// fn to_another_thread<T: Send>(value: T) -> T {
///     value
/// }
/// let attached = to_another_thread(pwrs::attach_current_thread());
/// ```
#[must_use = "the thread counts as called in only while the guard lives"]
pub struct AttachedThread {
    _called_in: CalledIn,
    _thread: core::marker::PhantomData<*const ()>,
}

/// Refuses a `PsObject` method that reaches the host from a thread the
/// host did not call in on, while the check is on. Such a thread is a
/// worker the module started: the call would attach it to the .NET
/// runtime and run PowerShell code on a thread no runspace belongs to.
pub(crate) fn require_called_in(what: &str) -> crate::PsResult<()> {
    if !thread_check() || CALLED_IN.with(|c| c.get()) > 0 {
        return Ok(());
    }
    Err(crate::PsError::new(
        crate::ErrorCategory::InvalidOperation,
        "PwrsOffThread",
        format!("{what} ran on a thread the host did not call into; a worker builds plain Rust values and leaves PsObject to the thread the cmdlet runs on"),
    ))
}

#[cfg(test)]
mod tests {
    use crate::IntoPs;
    use pwrs_sys::PS_TYPE_STRING;

    #[test]
    fn an_attached_worker_reaches_the_host_and_a_released_one_is_checked_again() {
        let _host = crate::testing::install();
        let obj = "abc".into_ps().expect("a string object");
        let worker = std::thread::spawn(move || {
            let held = {
                let _attached = super::attach_current_thread();
                obj.type_tag()
            };
            (held, obj.type_tag())
        });
        let (held, released) = worker.join().expect("the worker");
        assert_eq!(held.expect("the call made while attached"), PS_TYPE_STRING);
        if super::thread_check() {
            let refused = released.expect_err("a released worker is checked again");
            assert_eq!(refused.error_id, "PwrsOffThread");
        } else {
            assert_eq!(released.expect("the check is off"), PS_TYPE_STRING);
        }
    }
}
