//! A proxy object a cmdlet receives, reached in place.
//!
//! A parameter of type [`PsProxy<T>`] is declared with `T`'s CLR type,
//! so the engine's binder accepts only a `T` and refuses anything else
//! before the cmdlet runs, whether the object arrives down the pipeline,
//! from a variable, or through commands that know nothing of it. The
//! value stays behind the object: [`PsProxy::with`] and
//! [`PsProxy::with_mut`] lend it to a closure under the object's gate,
//! which its property reads and method calls take too.

use crate::class::{PsClassMeta, PsTyped};
use crate::host::vtable;
use crate::pipeline::check;
use crate::{ErrorCategory, FromPs, IntoPs, PsError, PsObject, PsResult};
use core::ffi::c_void;
use core::marker::PhantomData;
use pwrs_sys::PsHandle;

/// A `#[psclass(proxy)]` object of class `T`, as a parameter or a
/// [`FromPs`] conversion receives it. Writing it writes the same object.
pub struct PsProxy<T> {
    obj: PsObject,
    _class: PhantomData<fn() -> T>,
}

impl<T: PsClassMeta> PsProxy<T> {
    /// Lends the value to `f` as a shared reader. Fails when the object
    /// has been disposed, was made by an earlier load of the module, is
    /// another class or another module's, or is inside a `&mut self`
    /// method or a `with_mut` borrow on this thread; it nests inside a
    /// `&self` method, a property read or another `with`.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> PsResult<R> {
        let lent = Lent::enter::<T>(&self.obj, "PsProxy::with", false)?;
        // SAFETY: the gate is held as shared until `lent` drops, so no
        // method or borrow that changes the value runs meanwhile, and the
        // class id check makes the pointer a live `T`.
        Ok(f(unsafe { &*(lent.instance as *const T) }))
    }

    /// Lends the value to `f` to change, holding the gate exclusively:
    /// fails while any call into the object runs on this thread, and
    /// refuses every other entry until `f` returns. A class declared
    /// with `native_bytes` is asked for its bytes again afterwards, so
    /// the figure the garbage collector holds follows the change.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> PsResult<R> {
        let lent = Lent::enter::<T>(&self.obj, "PsProxy::with_mut", true)?;
        // SAFETY: the gate is held exclusively until `lent` drops, so
        // this is the only reference to the value.
        Ok(f(unsafe { &mut *(lent.instance as *mut T) }))
    }

    /// The object itself.
    pub fn object(&self) -> &PsObject {
        &self.obj
    }
}

/// Holds no object, so [`PsProxy::with`] on it fails: what a parameter
/// the engine never assigned keeps.
impl<T> Default for PsProxy<T> {
    fn default() -> Self {
        PsProxy { obj: PsObject::default(), _class: PhantomData }
    }
}

impl<T: PsClassMeta> FromPs for PsProxy<T> {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        if T::MODE != "proxy" {
            return Err(PsError::new(
                ErrorCategory::InvalidType,
                "PwrsNotAProxy",
                format!("{} is a {} class, and PsProxy holds a proxy class", T::NAME, T::MODE),
            ));
        }
        if obj.is_null() {
            return Err(PsError::new(ErrorCategory::InvalidData, "PwrsNullObject", format!("a {} cannot be read from $null", T::NAME)));
        }
        Ok(PsProxy { obj: obj.clone(), _class: PhantomData })
    }
}

impl<T> IntoPs for PsProxy<T> {
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(self.obj)
    }
}

impl<T: PsTyped> PsTyped for PsProxy<T> {
    const CLR_NAME: &'static str = T::CLR_NAME;
    const VALUE_TYPE: bool = false;
}

/// One entry into an object's gate, left when dropped. It holds a raw
/// pointer and so is not `Send`: it drops on the thread that entered,
/// which the gate requires. An exclusive entry is the one that may
/// change the value, so `changed` on exit is what `exclusive` was on
/// entry.
struct Lent<'a> {
    obj: &'a PsObject,
    instance: *mut c_void,
    changed: bool,
}

impl<'a> Lent<'a> {
    fn enter<T: PsClassMeta>(obj: &'a PsObject, what: &str, exclusive: bool) -> PsResult<Self> {
        crate::host::require_called_in(what)?;
        let class_id = T::class_id()?;
        let mut instance: *mut c_void = core::ptr::null_mut();
        let mut err = PsHandle::NULL;
        let enter = if exclusive { vtable().proxy_enter } else { vtable().proxy_enter_shared };
        let status = unsafe { enter(obj.as_raw(), class_id, &mut instance, &mut err) };
        check(status, err)?;
        Ok(Lent { obj, instance, changed: exclusive })
    }
}

impl Drop for Lent<'_> {
    fn drop(&mut self) {
        unsafe { (vtable().proxy_exit)(self.obj.as_raw(), u8::from(self.changed)) };
    }
}
