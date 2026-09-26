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
    let t = &*table;
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
