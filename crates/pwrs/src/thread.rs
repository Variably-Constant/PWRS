//! Threads a module starts for a resource that belongs to one thread: a
//! window, a COM object, a handle that is released where it was made.
//!
//! [`enter_sta`] puts such a thread in a single-threaded COM apartment
//! on Windows, and does nothing elsewhere. [`apartment`] says which
//! apartment the calling thread is in.

use crate::PsResult;
use core::marker::PhantomData;

/// A thread's COM apartment, as `CoGetApartmentType` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apartment {
    /// A single-threaded apartment of the thread's own (`APTTYPE_STA`).
    Sta,
    /// The process's main single-threaded apartment (`APTTYPE_MAINSTA`).
    MainSta,
    /// The multithreaded apartment, joined by this thread (`APTTYPE_MTA`).
    Mta,
    /// The multithreaded apartment of a thread that never initialized
    /// COM, in a process where another thread did (`APTTYPE_MTA` with
    /// `APTTYPEQUALIFIER_IMPLICIT_MTA`).
    ImplicitMta,
    /// The neutral apartment (`APTTYPE_NA`).
    Neutral,
    /// COM is not initialized on the thread, nor the multithreaded
    /// apartment anywhere in the process (`CO_E_NOTINITIALIZED`).
    NotInitialized,
}

/// The calling thread's COM apartment, or `None` where there is no COM,
/// which is everywhere but Windows.
pub fn apartment() -> Option<Apartment> {
    imp::apartment()
}

/// Puts the calling thread in a single-threaded COM apartment for as
/// long as the returned guard lives, on Windows: `CoInitializeEx` with
/// `COINIT_APARTMENTTHREADED` now and `CoUninitialize` when the guard
/// drops, on this thread. Elsewhere the guard does nothing.
///
/// Take it at the start of a thread the module starts for a window or
/// a COM object, and hold it for the thread's life. A thread already
/// single-threaded keeps its apartment. One already in the multithreaded
/// apartment is refused with `PwrsApartmentChanged`, since COM cannot
/// move a thread from one apartment to another; the host's own pipeline
/// thread is single-threaded or multithreaded depending on how the host
/// was started, so a resource that needs one kind of apartment gets a
/// thread of its own.
pub fn enter_sta() -> PsResult<StaGuard> {
    imp::enter_sta()?;
    Ok(StaGuard { _thread: PhantomData })
}

/// The apartment [`enter_sta`] entered, left when it drops. It stays on
/// the thread that took it:
///
/// ```compile_fail,E0277
/// fn to_another_thread<T: Send>(value: T) -> T {
///     value
/// }
/// let guard = to_another_thread(pwrs::thread::enter_sta().expect("an apartment"));
/// ```
#[must_use = "the thread stays in the apartment only while the guard lives"]
pub struct StaGuard {
    _thread: PhantomData<*const ()>,
}

impl Drop for StaGuard {
    fn drop(&mut self) {
        imp::leave();
    }
}

#[cfg(windows)]
mod imp {
    use super::Apartment;
    use crate::{ErrorCategory, PsError, PsResult};
    use core::ffi::c_void;

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
        fn CoUninitialize();
        fn CoGetApartmentType(apt_type: *mut i32, qualifier: *mut i32) -> i32;
    }

    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    const S_OK: i32 = 0;
    const S_FALSE: i32 = 1;
    const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;
    const APTTYPE_STA: i32 = 0;
    const APTTYPE_MTA: i32 = 1;
    const APTTYPE_NA: i32 = 2;
    const APTTYPE_MAINSTA: i32 = 3;
    const APTTYPEQUALIFIER_IMPLICIT_MTA: i32 = 1;

    pub fn enter_sta() -> PsResult<()> {
        // S_FALSE is a thread already single-threaded, and still owes the
        // CoUninitialize the guard makes.
        match unsafe { CoInitializeEx(core::ptr::null_mut(), COINIT_APARTMENTTHREADED) } {
            S_OK | S_FALSE => Ok(()),
            RPC_E_CHANGED_MODE => Err(PsError::new(
                ErrorCategory::InvalidOperation,
                "PwrsApartmentChanged",
                "this thread is in the multithreaded COM apartment, and COM cannot move it to a single-threaded one; start a thread of its own for the resource",
            )),
            hr => Err(PsError::new(ErrorCategory::InvalidOperation, "PwrsComInitialize", format!("CoInitializeEx failed with 0x{:08X}", hr as u32))),
        }
    }

    pub fn leave() {
        unsafe { CoUninitialize() }
    }

    pub fn apartment() -> Option<Apartment> {
        let mut apt_type = -1;
        let mut qualifier = 0;
        if unsafe { CoGetApartmentType(&mut apt_type, &mut qualifier) } != S_OK {
            return Some(Apartment::NotInitialized);
        }
        Some(match (apt_type, qualifier) {
            (APTTYPE_STA, _) => Apartment::Sta,
            (APTTYPE_MAINSTA, _) => Apartment::MainSta,
            (APTTYPE_MTA, APTTYPEQUALIFIER_IMPLICIT_MTA) => Apartment::ImplicitMta,
            (APTTYPE_MTA, _) => Apartment::Mta,
            (APTTYPE_NA, _) => Apartment::Neutral,
            _other => Apartment::NotInitialized,
        })
    }

    /// Puts the calling thread in the multithreaded apartment, for a test
    /// of the refusal; the thread is left there.
    #[cfg(test)]
    pub fn enter_mta_for_test() {
        const COINIT_MULTITHREADED: u32 = 0x0;
        let hr = unsafe { CoInitializeEx(core::ptr::null_mut(), COINIT_MULTITHREADED) };
        assert!(hr == S_OK || hr == S_FALSE, "CoInitializeEx(MTA) failed with 0x{:08X}", hr as u32);
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Apartment, PsResult};

    pub fn enter_sta() -> PsResult<()> {
        Ok(())
    }

    pub fn leave() {}

    pub fn apartment() -> Option<Apartment> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{apartment, enter_sta, Apartment};

    /// Single-threaded: a thread's own apartment, or the process's main
    /// one, which the first thread of a process to enter one gets.
    fn single_threaded(seen: Option<Apartment>) -> bool {
        matches!(seen, Some(Apartment::Sta | Apartment::MainSta))
    }

    /// A fresh thread enters a single-threaded apartment through the
    /// guard and leaves it when the guard drops; off Windows nothing
    /// changes and there is no apartment to report. Other tests of this
    /// process join and leave the multithreaded apartment while this
    /// runs, so what the thread reads outside the guard is only checked
    /// to be no single-threaded apartment.
    #[test]
    fn a_thread_is_single_threaded_while_the_guard_lives() {
        let seen = std::thread::spawn(|| {
            let before = apartment();
            let inside = {
                let _sta = enter_sta().expect("a single-threaded apartment");
                apartment()
            };
            (before, inside, apartment())
        })
        .join()
        .expect("the thread");
        if cfg!(windows) {
            assert!(single_threaded(seen.1), "{seen:?}");
            assert!(!single_threaded(seen.0) && !single_threaded(seen.2), "{seen:?}");
        } else {
            assert_eq!(seen, (None, None, None));
        }
    }

    /// Entering twice nests, and the thread stays single-threaded until
    /// the last guard drops.
    #[test]
    fn a_second_guard_on_the_thread_nests() {
        let seen = std::thread::spawn(|| {
            let outer = enter_sta().expect("the first");
            let inner = enter_sta().expect("the second");
            drop(inner);
            let after_inner = apartment();
            drop(outer);
            after_inner
        })
        .join()
        .expect("the thread");
        if cfg!(windows) {
            assert!(single_threaded(seen), "{seen:?}");
        } else {
            assert_eq!(seen, None);
        }
    }

    /// A thread already in the multithreaded apartment is refused.
    #[cfg(windows)]
    #[test]
    fn a_multithreaded_thread_is_refused() {
        let refused = std::thread::spawn(|| {
            super::imp::enter_mta_for_test();
            enter_sta().err()
        })
        .join()
        .expect("the thread")
        .expect("refused");
        assert_eq!(refused.error_id, "PwrsApartmentChanged");
    }
}
