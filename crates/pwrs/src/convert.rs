//! Conversions between Rust values and managed objects.
//!
//! The parameter binder has already coerced inputs to the CLR type the
//! shell declared, so [`FromPs`] reads a known type back out.
//! [`IntoPs`] builds output objects; `ENUMERATE` says whether the
//! pipeline should unroll the result.

use crate::host::vtable;
use crate::pinned::Primitive;
use crate::{ErrorCategory, PsError, PsObject, PsResult};
use pwrs_sys::{
    PsHandle, PsStr16, PsTypeTag, PS_OK, PS_TYPE_BOOL, PS_TYPE_F32, PS_TYPE_F64, PS_TYPE_I16, PS_TYPE_I32, PS_TYPE_I64, PS_TYPE_I8,
    PS_TYPE_OBJECT, PS_TYPE_STRING, PS_TYPE_U16, PS_TYPE_U32, PS_TYPE_U64, PS_TYPE_U8,
};

pub trait FromPs: Sized {
    fn from_ps(obj: &PsObject) -> PsResult<Self>;

    /// A `Vec<Self>` from an array, `IList` or `IEnumerable`; `$null`
    /// is empty. The default reads each element through the handle
    /// entries; a primitive type copies a typed array of its own kind
    /// through one pin and reads anything else element by element.
    fn vec_from_ps(obj: &PsObject) -> PsResult<Vec<Self>> {
        elements(obj)
    }
}

pub trait IntoPs: Sized {
    /// True when `write` should enumerate the produced object.
    const ENUMERATE: bool = false;
    /// Element type of the CLR array a `Vec<Self>` becomes.
    const TYPE_TAG: PsTypeTag = PS_TYPE_OBJECT;
    fn into_ps(self) -> PsResult<PsObject>;

    /// The array a `Vec<Self>` becomes. The default sets each element
    /// through the handle entries; a primitive type fills a typed
    /// array through one pin.
    fn vec_into_ps(v: Vec<Self>) -> PsResult<PsObject> {
        vec_to_array(v)
    }

    /// Writes this value to the pipeline. The default builds an
    /// object and writes the handle; scalars override it to use a
    /// direct vtable entry, skipping the handle entirely, and
    /// `Option<T>` hands a `Some` to `T`'s override.
    fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
        let obj = self.into_ps()?;
        if Self::ENUMERATE {
            ps.write_enumerated(&obj)
        } else {
            ps.write_object(&obj)
        }
    }
}

/// Writes a `Vec<T>` as one array object instead of enumerating it.
pub struct PsArray<T>(pub Vec<T>);

pub(crate) fn conv_err(what: &str, err: PsHandle) -> PsError {
    if !err.is_null() {
        unsafe { (vtable().free_handle)(err) };
    }
    PsError::new(ErrorCategory::InvalidType, "PwrsConversionError", format!("cannot convert value to {what}"))
}

impl FromPs for PsObject {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        Ok(obj.clone())
    }
}
impl IntoPs for PsObject {
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(self)
    }
}

/// The element tag of a typed array, `PS_TYPE_OBJECT` for anything
/// else.
pub(crate) fn element_tag(obj: &PsObject) -> PsResult<PsTypeTag> {
    let mut tag = PS_TYPE_OBJECT;
    let mut err = PsHandle::NULL;
    let s = unsafe { (vtable().array_element_tag)(obj.as_raw(), &mut tag, &mut err) };
    if s == PS_OK { Ok(tag) } else { Err(conv_err("array", err)) }
}

/// A typed array of `T` copied through one pin; any other collection
/// read element by element.
fn pinned_or_elements<T: Primitive + FromPs + IntoPs>(obj: &PsObject) -> PsResult<Vec<T>> {
    if obj.is_null() {
        return Ok(Vec::new());
    }
    if element_tag(obj)? == T::TYPE_TAG { crate::fallible::copy_of(&obj.pin_tagged::<T>()?) } else { elements(obj) }
}

macro_rules! int_conv {
    ($($t:ty => $tag:expr, $new:ident),*) => {$(
        impl FromPs for $t {
            fn from_ps(obj: &PsObject) -> PsResult<Self> {
                let v = i64::from_ps(obj)?;
                <$t>::try_from(v).map_err(|overflow| {
                    PsError::new(ErrorCategory::InvalidData, "PwrsConversionError", format!("value does not fit {}: {overflow}", stringify!($t)))
                })
            }
            fn vec_from_ps(obj: &PsObject) -> PsResult<Vec<Self>> {
                pinned_or_elements::<$t>(obj)
            }
        }
        impl IntoPs for $t {
            const TYPE_TAG: PsTypeTag = $tag;
            fn into_ps(self) -> PsResult<PsObject> {
                Ok(unsafe { PsObject::from_raw((vtable().$new)(self)) })
            }
            fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
                // Not write_i64: that is one crossing fewer and would
                // put an Int64 on the pipeline. The engine types an
                // operator's answer by its operands' widths, so the
                // width has to survive the write as well as the
                // conversion.
                ps.write_object(&self.into_ps()?)
            }
            fn vec_into_ps(v: Vec<Self>) -> PsResult<PsObject> {
                PsObject::from_slice(&v)
            }
        }
    )*};
}
int_conv!(
    i8 => PS_TYPE_I8, i8_new,
    i16 => PS_TYPE_I16, i16_new,
    i32 => PS_TYPE_I32, i32_new,
    u8 => PS_TYPE_U8, u8_new,
    u16 => PS_TYPE_U16, u16_new,
    u32 => PS_TYPE_U32, u32_new
);

impl FromPs for i64 {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut v = 0i64;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().i64_read)(obj.as_raw(), &mut v, &mut err) };
        if s == PS_OK { Ok(v) } else { Err(conv_err("Int64", err)) }
    }
    fn vec_from_ps(obj: &PsObject) -> PsResult<Vec<Self>> {
        pinned_or_elements::<i64>(obj)
    }
}
impl IntoPs for i64 {
    const TYPE_TAG: PsTypeTag = PS_TYPE_I64;
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(unsafe { PsObject::from_raw((vtable().i64_new)(self)) })
    }
    fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
        ps.write_i64(self)
    }
    fn vec_into_ps(v: Vec<Self>) -> PsResult<PsObject> {
        PsObject::from_slice(&v)
    }
}

impl FromPs for u64 {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut v = 0u64;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().u64_read)(obj.as_raw(), &mut v, &mut err) };
        if s == PS_OK { Ok(v) } else { Err(conv_err("UInt64", err)) }
    }
    fn vec_from_ps(obj: &PsObject) -> PsResult<Vec<Self>> {
        pinned_or_elements::<u64>(obj)
    }
}
impl IntoPs for u64 {
    const TYPE_TAG: PsTypeTag = PS_TYPE_U64;
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(unsafe { PsObject::from_raw((vtable().u64_new)(self)) })
    }
    fn vec_into_ps(v: Vec<Self>) -> PsResult<PsObject> {
        PsObject::from_slice(&v)
    }
}

impl FromPs for usize {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let v = u64::from_ps(obj)?;
        usize::try_from(v).map_err(|overflow| PsError::new(ErrorCategory::InvalidData, "PwrsConversionError", format!("value does not fit usize: {overflow}")))
    }
}
impl IntoPs for usize {
    const TYPE_TAG: PsTypeTag = PS_TYPE_U64;
    fn into_ps(self) -> PsResult<PsObject> {
        (self as u64).into_ps()
    }
}

impl FromPs for isize {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let v = i64::from_ps(obj)?;
        isize::try_from(v).map_err(|overflow| PsError::new(ErrorCategory::InvalidData, "PwrsConversionError", format!("value does not fit isize: {overflow}")))
    }
}
impl IntoPs for isize {
    const TYPE_TAG: PsTypeTag = PS_TYPE_I64;
    fn into_ps(self) -> PsResult<PsObject> {
        (self as i64).into_ps()
    }
    fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
        ps.write_i64(self as i64)
    }
}

impl FromPs for f64 {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut v = 0f64;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().f64_read)(obj.as_raw(), &mut v, &mut err) };
        if s == PS_OK { Ok(v) } else { Err(conv_err("Double", err)) }
    }
    fn vec_from_ps(obj: &PsObject) -> PsResult<Vec<Self>> {
        pinned_or_elements::<f64>(obj)
    }
}
impl IntoPs for f64 {
    const TYPE_TAG: PsTypeTag = PS_TYPE_F64;
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(unsafe { PsObject::from_raw((vtable().f64_new)(self)) })
    }
    fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
        ps.write_f64(self)
    }
    fn vec_into_ps(v: Vec<Self>) -> PsResult<PsObject> {
        PsObject::from_slice(&v)
    }
}
impl FromPs for f32 {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        f64::from_ps(obj).map(|v| v as f32)
    }
    fn vec_from_ps(obj: &PsObject) -> PsResult<Vec<Self>> {
        pinned_or_elements::<f32>(obj)
    }
}
impl IntoPs for f32 {
    const TYPE_TAG: PsTypeTag = PS_TYPE_F32;
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(unsafe { PsObject::from_raw((vtable().f32_new)(self)) })
    }
    fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
        // A Single widened to Double does not round-trip: 0.1f as a
        // Double prints 0.100000001490116, so the width has to survive
        // the write.
        ps.write_object(&self.into_ps()?)
    }
    fn vec_into_ps(v: Vec<Self>) -> PsResult<PsObject> {
        PsObject::from_slice(&v)
    }
}

impl FromPs for bool {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut v = 0u8;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().bool_read)(obj.as_raw(), &mut v, &mut err) };
        if s == PS_OK { Ok(v != 0) } else { Err(conv_err("Boolean", err)) }
    }
}
impl IntoPs for bool {
    const TYPE_TAG: PsTypeTag = PS_TYPE_BOOL;
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(unsafe { PsObject::from_raw((vtable().bool_new)(self as u8)) })
    }
    fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
        ps.write_bool(self)
    }
}

impl FromPs for String {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut buf = vec![0u16; 256];
        let mut len = 0usize;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().string_read)(obj.as_raw(), buf.as_mut_ptr(), buf.len(), &mut len, &mut err) };
        if s != PS_OK {
            return Err(conv_err("String", err));
        }
        if len > buf.len() {
            crate::fallible::grow_to(&mut buf, len, 0)?;
            let s = unsafe { (vtable().string_read)(obj.as_raw(), buf.as_mut_ptr(), buf.len(), &mut len, &mut err) };
            if s != PS_OK {
                return Err(conv_err("String", err));
            }
        }
        crate::text::try_from_utf16(&buf[..len])
    }
}
impl IntoPs for String {
    const TYPE_TAG: PsTypeTag = PS_TYPE_STRING;
    fn into_ps(self) -> PsResult<PsObject> {
        self.as_str().into_ps()
    }
    fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
        ps.write_str(&self)
    }
}
impl IntoPs for &str {
    const TYPE_TAG: PsTypeTag = PS_TYPE_STRING;
    fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
        ps.write_str(self)
    }
    fn into_ps(self) -> PsResult<PsObject> {
        let h = crate::text::with_utf16(self, |u| unsafe { (vtable().string_new)(PsStr16 { ptr: u.as_ptr(), len: u.len() }) });
        Ok(unsafe { PsObject::from_raw(h) })
    }
}

impl FromPs for std::path::PathBuf {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        String::from_ps(obj).map(std::path::PathBuf::from)
    }
}
impl IntoPs for std::path::PathBuf {
    const TYPE_TAG: PsTypeTag = PS_TYPE_STRING;
    fn into_ps(self) -> PsResult<PsObject> {
        self.to_string_lossy().as_ref().into_ps()
    }
    fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
        ps.write_str(&self.to_string_lossy())
    }
}

impl<T: FromPs> FromPs for Option<T> {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        if obj.is_null() { Ok(None) } else { T::from_ps(obj).map(Some) }
    }
}
impl<T: IntoPs> IntoPs for Option<T> {
    const ENUMERATE: bool = T::ENUMERATE;
    fn into_ps(self) -> PsResult<PsObject> {
        match self {
            Some(v) => v.into_ps(),
            None => Ok(PsObject::null()),
        }
    }
    fn write_to(self, ps: &crate::Pipeline<'_>) -> PsResult<()> {
        match self {
            Some(v) => v.write_to(ps),
            None => ps.write_object(&PsObject::null()),
        }
    }
}

/// Every element of an array, `IList` or `IEnumerable` through the
/// handle entries; `$null` is empty.
fn elements<T: FromPs>(obj: &PsObject) -> PsResult<Vec<T>> {
    if obj.is_null() {
        return Ok(Vec::new());
    }
    let mut len = 0usize;
    let mut err = PsHandle::NULL;
    let s = unsafe { (vtable().array_len)(obj.as_raw(), &mut len, &mut err) };
    if s != PS_OK {
        return Err(conv_err("array", err));
    }
    let mut out = crate::fallible::vec_with_capacity(len)?;
    for i in 0..len {
        let mut h = PsHandle::NULL;
        let s = unsafe { (vtable().array_get)(obj.as_raw(), i, &mut h, &mut err) };
        if s != PS_OK {
            return Err(conv_err("array element", err));
        }
        let item = unsafe { PsObject::from_raw(h) };
        out.push(T::from_ps(&item)?);
    }
    Ok(out)
}

impl<T: FromPs> FromPs for Vec<T> {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        T::vec_from_ps(obj)
    }
}

fn vec_to_array<T: IntoPs>(v: Vec<T>) -> PsResult<PsObject> {
    let mut out = PsHandle::NULL;
    let mut err = PsHandle::NULL;
    let s = unsafe { (vtable().array_new)(T::TYPE_TAG, v.len(), &mut out, &mut err) };
    if s != PS_OK {
        return Err(conv_err("object[]", err));
    }
    let arr = unsafe { PsObject::from_raw(out) };
    for (i, item) in v.into_iter().enumerate() {
        let obj = item.into_ps()?;
        let s = unsafe { (vtable().array_set)(arr.as_raw(), i, obj.as_raw(), &mut err) };
        if s != PS_OK {
            return Err(conv_err("array element", err));
        }
    }
    Ok(arr)
}

impl<T: IntoPs> IntoPs for Vec<T> {
    const ENUMERATE: bool = true;
    fn into_ps(self) -> PsResult<PsObject> {
        T::vec_into_ps(self)
    }
}

impl<T: IntoPs> IntoPs for PsArray<T> {
    const ENUMERATE: bool = false;
    fn into_ps(self) -> PsResult<PsObject> {
        T::vec_into_ps(self.0)
    }
}
