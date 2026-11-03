//! A proxy object a cmdlet receives, reached in place.
//!
//! A parameter of type [`PsProxy<T>`] is declared with `T`'s CLR type,
//! so the engine's binder accepts only a `T` and refuses anything else
//! before the cmdlet runs, whether the object arrives down the pipeline,
//! from a variable, or through commands that only pass it along. The
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
        // SAFETY: the gate is entered as shared until `lent` ends, so no
        // method or borrow that changes the value runs meanwhile, and the
        // class id check makes the pointer a live `T`.
        let result = f(unsafe { &*(lent.instance as *const T) });
        lent.exit()?;
        Ok(result)
    }

    /// Lends the value to `f` to change, with the gate entered
    /// exclusively: fails while any call into the object runs on this
    /// thread, and refuses every other entry until `f` returns. A class
    /// declared with `native_bytes` has its byte count read again
    /// afterward, so the figure reported to the garbage collector
    /// follows the change.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> PsResult<R> {
        let lent = Lent::enter::<T>(&self.obj, "PsProxy::with_mut", true)?;
        // SAFETY: the gate is entered exclusively until `lent` ends, so
        // this is the only reference to the value.
        let result = f(unsafe { &mut *(lent.instance as *mut T) });
        lent.exit()?;
        Ok(result)
    }

    /// The object itself.
    pub fn object(&self) -> &PsObject {
        &self.obj
    }

    /// Raises the event `event` of the object, as [`PsThis::raise`] does.
    /// Inside [`PsProxy::with`] or [`PsProxy::with_mut`] the raise is
    /// delivered when the borrow ends, and that call returns a handler's
    /// exception.
    pub fn raise(&self, event: &str, args: impl IntoPs) -> PsResult<()> {
        crate::events::raise_on(&self.obj, event, args)
    }
}

/// The object a `#[psmethods]` method runs on. A method takes it as an
/// argument `this: &PsThis`, which the method's .NET signature does not
/// have, and the generated method passes the object itself.
///
/// [`PsThis::raise`] raises the object's events. A clone is `Send` and
/// `Sync`, so a worker thread the method starts can take one and raise
/// after the call returns; the object stays alive as long as the clone
/// does.
#[derive(Clone, Debug)]
pub struct PsThis {
    obj: PsObject,
}

impl PsThis {
    /// The object a generated method passes, as its handle.
    #[doc(hidden)]
    pub fn from_object(obj: PsObject) -> PsThis {
        PsThis { obj }
    }

    /// The object itself.
    pub fn object(&self) -> &PsObject {
        &self.obj
    }

    /// Raises the event `event` of the object with `args`: an array's
    /// elements, none for `()`, and anything else as the one argument,
    /// each converted to its parameter's type as PowerShell converts it.
    /// For a delegate `(object sender, T e)`, such as `EventHandler` or
    /// `PropertyChangedEventHandler`, the object is the sender and the one
    /// argument is `e`, `EventArgs.Empty` when `()` gives none to an
    /// `EventHandler`. A raise made during a call into the object, as from
    /// the method that was given this, is delivered once that call
    /// returns, so a handler can read the object, and a handler's
    /// exception is then the call's. Any other raise runs the handlers on
    /// this thread before it returns, and a handler's exception is the
    /// `Err`. A script block handler runs only on the thread of a runspace,
    /// and raised from another one it fails with "There is no Runspace
    /// available to run scripts in this thread".
    pub fn raise(&self, event: &str, args: impl IntoPs) -> PsResult<()> {
        crate::events::raise_on(&self.obj, event, args)
    }
}

/// Has no object, so [`PsProxy::with`] on it fails: the value of a
/// parameter the engine never assigned.
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
                format!("{} is a {} class, and PsProxy takes a proxy class", T::NAME, T::MODE),
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

/// One entry into an object's gate, left by [`Lent::exit`], or when
/// dropped, as by a panic, which delivers none of the raises made during
/// it. It has a raw pointer field and so is not `Send`: it ends on the
/// thread that entered, which the gate requires. An exclusive entry is
/// the one that may change the value, so `changed` on exit is what
/// `exclusive` was on entry.
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

    /// Leaves the gate and delivers the raises made during the entry
    /// when it was this thread's last; a handler's exception is the `Err`.
    fn exit(self) -> PsResult<()> {
        let lent = core::mem::ManuallyDrop::new(self);
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().proxy_exit_raising)(lent.obj.as_raw(), u8::from(lent.changed), &mut err) };
        check(status, err)
    }
}

impl Drop for Lent<'_> {
    fn drop(&mut self) {
        unsafe { (vtable().proxy_exit)(self.obj.as_raw(), u8::from(self.changed)) };
    }
}
