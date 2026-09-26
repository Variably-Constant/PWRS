//! Zero-copy crossings of primitive buffers in both directions:
//! borrows of managed arrays, and Rust-owned buffers handed to the
//! engine as `Memory<T>`.

use crate::host::vtable;
use crate::pipeline::check;
use crate::{ErrorCategory, IntoPs, PsError, PsObject, PsResult};
use core::alloc::Layout;
use core::ffi::c_void;
use core::marker::PhantomData;
use core::mem::ManuallyDrop;
use core::ptr::NonNull;
use pwrs_sys::{PsHandle, PsPinned};

/// Element types a managed primitive array can hold.
///
/// # Safety
/// An implementor's layout must match the CLR primitive of the same
/// width bit for bit, because a pinned view reinterprets the managed
/// array's memory as a slice of the implementor.
pub unsafe trait Primitive: Copy {}
unsafe impl Primitive for u8 {}
unsafe impl Primitive for i8 {}
unsafe impl Primitive for u16 {}
unsafe impl Primitive for i16 {}
unsafe impl Primitive for u32 {}
unsafe impl Primitive for i32 {}
unsafe impl Primitive for u64 {}
unsafe impl Primitive for i64 {}
unsafe impl Primitive for f32 {}
unsafe impl Primitive for f64 {}
// A Decimal[] pins like a primitive array even though Type.IsPrimitive
// is false for it. The element is the four words in MEMORY order, not
// the order Decimal.GetBits reports.
unsafe impl Primitive for crate::PsDecimalBits {}

/// A pinned managed array viewed as a slice. The GC cannot move the
/// array while this lives; the pin is released on drop, so it must
/// not outlive the lifecycle phase that created it.
pub struct Pinned<'a, T: Primitive> {
    raw: PsPinned,
    _borrow: PhantomData<&'a mut [T]>,
}

impl<'a, T: Primitive> core::ops::Deref for Pinned<'a, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        if self.raw.len == 0 {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(self.raw.data as *const T, self.raw.len) }
    }
}

impl<'a, T: Primitive> core::ops::DerefMut for Pinned<'a, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        if self.raw.len == 0 {
            return &mut [];
        }
        unsafe { core::slice::from_raw_parts_mut(self.raw.data as *mut T, self.raw.len) }
    }
}

impl<'a, T: Primitive> Drop for Pinned<'a, T> {
    fn drop(&mut self) {
        unsafe { (vtable().array_unpin)(self.raw) }
    }
}

/// Whether this host lays a `System.Decimal` out as `PsDecimalBits`
/// says. Proved against `Decimal.GetBits` on the first decimal pin in
/// a process and cached; later pins read the verdict.
///
/// The field order is internal to the runtime. It is `flags, hi, lo,
/// mid` on x64 Windows under .NET 10 and .NET Framework 4.8, and
/// unread on every other target.
fn decimal_layout_holds() -> PsResult<()> {
    static VERDICT: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    VERDICT
        .get_or_init(|| {
            // Four words that are distinct and individually
            // identifiable, so any permutation shows up as a
            // mismatch rather than coinciding.
            let want = crate::PsDecimal::from_bits(0x0100_0001, 0x0200_0002, 0x0300_0003, 2 << 16);
            let probe = || -> PsResult<crate::PsDecimalBits> {
                let one = want.into_ps()?;
                let array = {
                    let mut out = PsHandle::NULL;
                    let mut err = PsHandle::NULL;
                    let s = unsafe {
                        (vtable().array_new)(pwrs_sys::PS_TYPE_DECIMAL, 1, &mut out, &mut err)
                    };
                    check(s, err)?;
                    unsafe { PsObject::from_raw(out) }
                };
                let mut err = PsHandle::NULL;
                let s = unsafe { (vtable().array_set)(array.as_raw(), 0, one.as_raw(), &mut err) };
                check(s, err)?;
                let words = {
                    let pinned = array.pin_tagged::<crate::PsDecimalBits>()?;
                    pinned.to_vec()
                };
                Ok(words[0])
            };
            match probe() {
                Ok(got) if crate::PsDecimal::from(got) == want => Ok(()),
                Ok(got) => Err(format!(
                    "this host orders a Decimal's words as {got:?}, not flags, hi, lo, mid; \
                     pinning a Decimal[] here would reinterpret every element"
                )),
                Err(e) => Err(format!("the Decimal layout could not be checked: {}", e.message)),
            }
        })
        .clone()
        .map_err(|message| {
            PsError::new(ErrorCategory::InvalidOperation, "PwrsDecimalLayout", message)
        })
}

impl PsObject {
    /// Pins a managed `T[]` and borrows it. Fails when the object is
    /// not a primitive array, when its element size differs from `T`,
    /// or when its element type is not `T`.
    ///
    /// The element type is checked as well as its width, so an
    /// `Int64[]` does not pin as `f64`. Same-width types whose bits
    /// mean different things would otherwise be reinterpreted rather
    /// than read, and the pin would report no error.
    pub fn pin<T: Primitive + crate::IntoPs>(&self) -> PsResult<Pinned<'_, T>> {
        crate::host::require_called_in("PsObject::pin")?;
        if T::TYPE_TAG == pwrs_sys::PS_TYPE_DECIMAL {
            decimal_layout_holds()?;
        }
        let tag = crate::convert::element_tag(self)?;
        if tag != T::TYPE_TAG {
            return Err(PsError::new(
                ErrorCategory::InvalidType,
                "PwrsPinElementType",
                format!("array elements are type tag {tag}, expected {}", T::TYPE_TAG),
            ));
        }
        self.pin_tagged()
    }

    /// Pins without asking the array its element type, for the two
    /// callers that already know it: a `Vec<T>` read that has just
    /// compared the tag, and an array this crate allocated as
    /// `T::TYPE_TAG`. The width is still checked.
    pub(crate) fn pin_tagged<T: Primitive>(&self) -> PsResult<Pinned<'_, T>> {
        let mut raw = PsPinned { data: core::ptr::null_mut(), len: 0, elem_size: 0, pin: PsHandle::NULL };
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().array_pin)(self.as_raw(), &mut raw, &mut err) };
        check(status, err)?;
        if raw.elem_size as usize != core::mem::size_of::<T>() {
            unsafe { (vtable().array_unpin)(raw) };
            return Err(PsError::new(
                ErrorCategory::InvalidType,
                "PwrsPinElementSize",
                format!("array elements are {} bytes, expected {}", raw.elem_size, core::mem::size_of::<T>()),
            ));
        }
        Ok(Pinned { raw, _borrow: PhantomData })
    }

    /// A new managed `T[]` filled from `data`, written through one pin.
    pub fn from_slice<T: Primitive + crate::IntoPs>(data: &[T]) -> PsResult<PsObject> {
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().array_new)(T::TYPE_TAG, data.len(), &mut out, &mut err) };
        check(status, err)?;
        let arr = unsafe { PsObject::from_raw(out) };
        if !data.is_empty() {
            let mut pinned = arr.pin_tagged::<T>()?;
            pinned.copy_from_slice(data);
        }
        Ok(arr)
    }
}

/// A Rust-owned buffer handed to the engine as a `Memory<T>` over the
/// same allocation on .NET, or copied into a `T[]` on .NET Framework.
/// Fill it in place through `DerefMut`, or make it from a slice or a
/// `Vec` with one copy. `into_ps` hands the allocation over: on .NET
/// the managed owner's collection frees it, on .NET Framework the copy
/// is made and the allocation freed before the call returns. A value
/// never handed over frees its allocation on drop.
pub struct PsMemory<T: Primitive> {
    base: NonNull<u8>,
    len: usize,
    layout: Layout,
    _elements: PhantomData<T>,
}

/// The length is kept at the start of the allocation; the elements
/// follow at the first offset aligned for `T`.
const fn data_offset<T>() -> usize {
    let align = core::mem::align_of::<T>();
    core::mem::size_of::<usize>().div_ceil(align) * align
}

fn buffer_layout<T>(len: usize) -> PsResult<Layout> {
    let align = core::mem::align_of::<T>().max(core::mem::align_of::<usize>());
    let bytes = match len.checked_mul(core::mem::size_of::<T>()).and_then(|b| b.checked_add(data_offset::<T>())) {
        Some(b) => b,
        None => return Err(PsError::new(ErrorCategory::InvalidArgument, "PwrsBufferSize", format!("{len} elements do not fit in memory"))),
    };
    Layout::from_size_align(bytes, align).map_err(|e| PsError::new(ErrorCategory::InvalidArgument, "PwrsBufferSize", format!("{len} elements: {e}")))
}

impl<T: Primitive> PsMemory<T> {
    /// A zero-filled buffer of `len` elements.
    pub fn zeroed(len: usize) -> PsResult<PsMemory<T>> {
        let layout = buffer_layout::<T>(len)?;
        let raw = unsafe { std::alloc::alloc_zeroed(layout) };
        let base = match NonNull::new(raw) {
            Some(p) => p,
            None => {
                return Err(PsError::new(ErrorCategory::ResourceUnavailable, "PwrsOutOfMemory", format!("cannot allocate {} bytes", layout.size())));
            }
        };
        unsafe { base.as_ptr().cast::<usize>().write(len) };
        Ok(PsMemory { base, len, layout, _elements: PhantomData })
    }

    /// A buffer holding a copy of `data`.
    pub fn from_slice(data: &[T]) -> PsResult<PsMemory<T>> {
        let mut m = PsMemory::zeroed(data.len())?;
        m.copy_from_slice(data);
        Ok(m)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn data(&self) -> *mut T {
        unsafe { self.base.as_ptr().add(data_offset::<T>()).cast::<T>() }
    }
}

impl<T: Primitive> core::ops::Deref for PsMemory<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        unsafe { core::slice::from_raw_parts(self.data(), self.len) }
    }
}

impl<T: Primitive> core::ops::DerefMut for PsMemory<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe { core::slice::from_raw_parts_mut(self.data(), self.len) }
    }
}

impl<T: Primitive> Drop for PsMemory<T> {
    fn drop(&mut self) {
        unsafe { std::alloc::dealloc(self.base.as_ptr(), self.layout) }
    }
}

impl<T: Primitive> TryFrom<Vec<T>> for PsMemory<T> {
    type Error = PsError;
    fn try_from(v: Vec<T>) -> PsResult<Self> {
        PsMemory::from_slice(&v)
    }
}

/// Frees a buffer `into_ps` handed over: the length sits ahead of the
/// data and the layout follows from it.
unsafe extern "C" fn free_buffer<T: Primitive>(data: *mut c_void) {
    let base = data.cast::<u8>().sub(data_offset::<T>());
    let len = base.cast::<usize>().read();
    match buffer_layout::<T>(len) {
        Ok(layout) => std::alloc::dealloc(base, layout),
        Err(e) => eprintln!("pwrs: a memory view's buffer of {len} elements was not freed: {e}"),
    }
}

impl<T: Primitive + crate::IntoPs> crate::IntoPs for PsMemory<T> {
    fn into_ps(self) -> PsResult<PsObject> {
        let this = ManuallyDrop::new(self);
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().memory_view_new)(T::TYPE_TAG, this.data().cast::<c_void>(), this.len, free_buffer::<T>, &mut out, &mut err) };
        match check(status, err) {
            Ok(()) => Ok(unsafe { PsObject::from_raw(out) }),
            Err(e) => {
                unsafe { std::alloc::dealloc(this.base.as_ptr(), this.layout) };
                Err(e)
            }
        }
    }
}
