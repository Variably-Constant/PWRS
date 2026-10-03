//! Dynamic access to any .NET object from Rust through the engine's
//! own member binder: properties, instance methods, static methods,
//! and constructors by type name. They run on a thread the host called
//! the module on, not necessarily the pipeline thread; a worker the
//! module started must not make them, and while the thread check is on
//! (`crate::host::thread_check`) one that does gets `PwrsOffThread`.

use crate::host::vtable;
use crate::pipeline::check;
use crate::{PsObject, PsResult};
use pwrs_sys::{PsHandle, PsStr16, PsTypeTag, PS_TYPE_OBJECT};

#[inline]
pub(crate) fn utf16(s: &str) -> Vec<u16> {
    crate::text::to_utf16(s)
}

#[inline]
pub(crate) fn str16(v: &[u16]) -> PsStr16 {
    PsStr16 { ptr: v.as_ptr(), len: v.len() }
}

/// An `object[]` holding clones of `args`.
pub fn args_array(args: &[PsObject]) -> PsResult<PsObject> {
    let mut out = PsHandle::NULL;
    let mut err = PsHandle::NULL;
    let status = unsafe { (vtable().array_new)(PS_TYPE_OBJECT, args.len(), &mut out, &mut err) };
    check(status, err)?;
    let arr = unsafe { PsObject::from_raw(out) };
    for (i, a) in args.iter().enumerate() {
        let status = unsafe { (vtable().array_set)(arr.as_raw(), i, a.as_raw(), &mut err) };
        check(status, err)?;
    }
    Ok(arr)
}

impl PsObject {
    /// Reads a property or field by name.
    pub fn get(&self, name: &str) -> PsResult<PsObject> {
        crate::host::require_called_in("PsObject::get")?;
        let n = utf16(name);
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().dyn_get)(self.as_raw(), str16(&n), &mut out, &mut err) };
        check(status, err)?;
        Ok(unsafe { PsObject::from_raw(out) })
    }

    /// Writes a property by name.
    pub fn set(&self, name: &str, value: &PsObject) -> PsResult<()> {
        crate::host::require_called_in("PsObject::set")?;
        let n = utf16(name);
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().dyn_set)(self.as_raw(), str16(&n), value.as_raw(), &mut err) };
        check(status, err)
    }

    /// Invokes an instance method by name with the engine's overload
    /// resolution.
    pub fn call(&self, name: &str, args: &[PsObject]) -> PsResult<PsObject> {
        crate::host::require_called_in("PsObject::call")?;
        let n = utf16(name);
        let arr = args_array(args)?;
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().dyn_call)(self.as_raw(), str16(&n), arr.as_raw(), &mut out, &mut err) };
        check(status, err)?;
        Ok(unsafe { PsObject::from_raw(out) })
    }

    /// Full CLR type name of the underlying object.
    pub fn type_name(&self) -> PsResult<String> {
        let ty = self.call("GetType", &[])?;
        let name = ty.get("FullName")?;
        <String as crate::FromPs>::from_ps(&name)
    }

    /// The tag of this object's own type, `PS_TYPE_OBJECT` for a type
    /// outside the tag vocabulary.
    ///
    /// One crossing answering a `u32`, against three and a string
    /// compare for [`PsObject::type_name`]. A caller dispatching on a
    /// type it does not know at compile time matches on this first and
    /// falls back to the name only for `PS_TYPE_OBJECT`.
    pub fn type_tag(&self) -> PsResult<PsTypeTag> {
        crate::host::require_called_in("PsObject::type_tag")?;
        let mut tag = PS_TYPE_OBJECT;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().object_type_tag)(self.as_raw(), &mut tag, &mut err) };
        check(status, err)?;
        Ok(tag)
    }
}

/// A .NET type addressed by name, resolved by the engine's type
/// resolver (so `int`, `System.IO.File`, and `[Math]`-style names all
/// work).
pub struct PsType {
    name: String,
}

impl PsType {
    pub fn from_name(name: impl Into<String>) -> PsType {
        PsType { name: name.into() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Invokes a public static method.
    pub fn call_static(&self, method: &str, args: &[PsObject]) -> PsResult<PsObject> {
        crate::host::require_called_in("PsType::call_static")?;
        let t = utf16(&self.name);
        let m = utf16(method);
        let arr = args_array(args)?;
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().dyn_call_static)(str16(&t), str16(&m), arr.as_raw(), &mut out, &mut err) };
        check(status, err)?;
        Ok(unsafe { PsObject::from_raw(out) })
    }

    /// Invokes a public constructor of the CLR type and returns the
    /// instance as an object. Named after the operation, not the
    /// receiver.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(&self, args: &[PsObject]) -> PsResult<PsObject> {
        crate::host::require_called_in("PsType::new")?;
        let t = utf16(&self.name);
        let arr = args_array(args)?;
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().dyn_new)(str16(&t), arr.as_raw(), &mut out, &mut err) };
        check(status, err)?;
        Ok(unsafe { PsObject::from_raw(out) })
    }
}
