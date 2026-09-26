//! Owned handle to a managed object.

use crate::host::vtable;
use pwrs_sys::PsHandle;

/// An owned `GCHandle`. `Send` and `Sync` because a `GCHandle` may be
/// released from any thread. The methods that reach the host belong on
/// a thread the host called the module on, which a worker the module
/// started is not; a debug build and `cargo pwrs test` refuse them
/// there with `PwrsOffThread`. The cmdlet stream API is thread-affine
/// as well, and requires a [`crate::Pipeline`].
pub struct PsObject {
    raw: PsHandle,
}

unsafe impl Send for PsObject {}
unsafe impl Sync for PsObject {}

impl Default for PsObject {
    fn default() -> Self {
        PsObject::null()
    }
}

impl PsObject {
    /// Takes ownership of a handle the runtime allocated.
    ///
    /// # Safety
    /// `raw` must be a live `GCHandle` not owned elsewhere.
    pub unsafe fn from_raw(raw: PsHandle) -> Self {
        PsObject { raw }
    }

    pub fn null() -> Self {
        PsObject { raw: PsHandle::NULL }
    }

    pub fn is_null(&self) -> bool {
        self.raw.is_null()
    }

    pub fn as_raw(&self) -> PsHandle {
        self.raw
    }

    /// Gives up ownership; the caller must free the handle.
    pub fn into_raw(self) -> PsHandle {
        let raw = self.raw;
        core::mem::forget(self);
        raw
    }
}

impl Clone for PsObject {
    fn clone(&self) -> Self {
        if self.raw.is_null() {
            return PsObject::null();
        }
        PsObject { raw: unsafe { (vtable().clone_handle)(self.raw) } }
    }
}

impl Drop for PsObject {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { (vtable().free_handle)(self.raw) }
        }
    }
}

/// `new PSObject()` with `type_name` inserted as its `PSTypeName`.
pub fn new_psobject(type_name: &str) -> PsObject {
    let u: Vec<u16> = crate::text::to_utf16(type_name);
    let s = pwrs_sys::PsStr16 { ptr: u.as_ptr(), len: u.len() };
    unsafe { PsObject::from_raw((vtable().psobject_new)(s)) }
}

/// Adds a note property to a `PSObject`.
pub fn add_note(obj: &PsObject, name: &str, value: PsObject) -> crate::PsResult<()> {
    let u: Vec<u16> = crate::text::to_utf16(name);
    let s = pwrs_sys::PsStr16 { ptr: u.as_ptr(), len: u.len() };
    let mut err = PsHandle::NULL;
    let status = unsafe { (vtable().psobject_add_note)(obj.as_raw(), s, value.as_raw(), &mut err) };
    crate::pipeline::check(status, err)
}

/// Reads a property off a `PSObject` by name, note properties
/// included. A name the object does not carry is an error, not `$null`.
///
/// This reads the shell's property bag, which [`add_note`] writes to
/// and a `PSCustomObject` carries. [`PsObject::get`] goes to the
/// underlying .NET object and does not see a note.
pub fn property(obj: &PsObject, name: &str) -> crate::PsResult<PsObject> {
    let u: Vec<u16> = crate::text::to_utf16(name);
    let s = pwrs_sys::PsStr16 { ptr: u.as_ptr(), len: u.len() };
    let mut out = PsHandle::NULL;
    let mut err = PsHandle::NULL;
    let status = unsafe { (vtable().psobject_get_property)(obj.as_raw(), s, &mut out, &mut err) };
    crate::pipeline::check(status, err)?;
    Ok(unsafe { PsObject::from_raw(out) })
}

impl core::fmt::Debug for PsObject {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PsObject({:p})", self.raw.0)
    }
}
