//! Primitive buffers passed without a copy in both directions: borrows
//! of managed arrays, and native memory given to the engine as
//! `Memory<T>` or `ReadOnlyMemory<T>`, from buffers PWRS allocates and
//! from memory whose life the module manages some other way.

use crate::host::vtable;
use crate::pipeline::check;
use crate::{ErrorCategory, FromPs, IntoPs, PsError, PsObject, PsResult};
use core::alloc::Layout;
use core::cell::RefCell;
use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr::NonNull;
use pwrs_sys::{PsHandle, PsPinned};
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The element types of managed primitive arrays.
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
// is false for it. The element is the four words in memory order, not
// the order Decimal.GetBits reports.
unsafe impl Primitive for crate::PsDecimalBits {}

/// A pinned managed array viewed as a slice. The GC cannot move the
/// array while this lives; the pin is released on drop, so it must
/// not outlive the lifecycle phase that created it. While it lives,
/// another pin of the same array on this thread is refused, so the
/// thread never has two views of one array that can write.
pub struct Pinned<'a, T: Primitive> {
    raw: PsPinned,
    /// Whether the array is in this thread's list of pinned arrays,
    /// which the drop takes it off.
    listed: bool,
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
        if self.listed {
            unlist_pin(self.raw.data as usize);
        }
        unsafe { (vtable().array_unpin)(self.raw) }
    }
}

impl<'a, T: Primitive> Pinned<'a, T> {
    /// A view of a pin the host returned, released on drop. It is not
    /// listed among the thread's pinned arrays: its one caller pins a
    /// string, which nothing writes through.
    ///
    /// # Safety
    /// `raw` came from a host pin entry that succeeded, its `data` is
    /// valid for `len` elements of `T`, and nothing else unpins it.
    pub(crate) unsafe fn from_raw(raw: PsPinned) -> Self {
        Pinned { raw, listed: false, _borrow: PhantomData }
    }
}

std::thread_local! {
    /// The address of the first element of every array pinned on this
    /// thread and not yet released.
    static PINNED_HERE: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Lists the array whose first element is at `data` as pinned on this
/// thread, or refuses it when a view of that array already lives here.
fn list_pin(data: usize) -> PsResult<()> {
    let listed = PINNED_HERE.try_with(|pins| -> PsResult<()> {
        let mut pins = pins.borrow_mut();
        if pins.contains(&data) {
            return Err(PsError::new(
                ErrorCategory::ResourceBusy,
                "PwrsArrayPinned",
                "this array is already pinned on this thread, by a view that is still in use, as when one array is passed for two arguments",
            ));
        }
        pins.try_reserve(1)?;
        pins.push(data);
        Ok(())
    });
    match listed {
        Ok(result) => result,
        Err(_exiting) => Err(PsError::new(ErrorCategory::InvalidOperation, "PwrsArrayPinned", "this thread is exiting, and an array cannot be pinned on it")),
    }
}

/// Takes the array whose first element is at `data` off this thread's
/// list of pinned arrays. A thread that is exiting has dropped its list,
/// so there is nothing to take it off.
fn unlist_pin(data: usize) {
    let unlisted = PINNED_HERE.try_with(|pins| {
        let mut pins = pins.borrow_mut();
        if let Some(at) = pins.iter().position(|&p| p == data) {
            pins.swap_remove(at);
        }
    });
    match unlisted {
        Ok(()) => {}
        Err(_list_dropped) => {}
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
    /// when its element type is not `T`, or when the array is already
    /// pinned on this thread by a view that is still alive.
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

    /// Pins without reading the array's element type, for the callers
    /// that have already checked it: a `Vec<T>` read that has just
    /// compared the tag, a [`PsArrayMut`] whose conversion compared it,
    /// and an array this crate allocated as `T::TYPE_TAG`. The width is
    /// still checked, and a pin of an array already pinned on this
    /// thread is refused. An empty array is not listed, since no view
    /// of it reaches any memory.
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
        let listed = raw.len != 0;
        if listed
            && let Err(refused) = list_pin(raw.data as usize)
        {
            unsafe { (vtable().array_unpin)(raw) };
            return Err(refused);
        }
        Ok(Pinned { raw, listed, _borrow: PhantomData })
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
            copy_into(&mut pinned, data);
        }
        Ok(arr)
    }
}

/// A managed `T[]` that a method or a cmdlet reads and writes in place.
/// The shell declares the argument or parameter as `T[]`, and
/// [`PsArrayMut::pin`] reaches the array the engine passed, with no
/// copy. An argument that is already a `T[]` is passed as itself, so a
/// change shows in the caller's array; the engine converts any other
/// value to a new `T[]` first. The conversion refuses `$null`. Writing
/// one writes the same array, as one object.
pub struct PsArrayMut<T: Primitive> {
    array: PsObject,
    _elements: PhantomData<T>,
}

impl<T: Primitive + IntoPs> PsArrayMut<T> {
    /// Pins the array and borrows it, as [`PsObject::pin`] does: the
    /// view is a slice to read, and through `DerefMut` a slice to write.
    /// The view borrows this value mutably, so the value has one view
    /// at a time, and another pin of the same array on this thread,
    /// through another value or [`PsObject::pin`], is refused while it
    /// lives.
    pub fn pin(&mut self) -> PsResult<Pinned<'_, T>> {
        crate::host::require_called_in("PsArrayMut::pin")?;
        if T::TYPE_TAG == pwrs_sys::PS_TYPE_DECIMAL {
            decimal_layout_holds()?;
        }
        self.array.pin_tagged()
    }
}

impl<T: Primitive> PsArrayMut<T> {
    /// The array itself.
    pub fn object(&self) -> &PsObject {
        &self.array
    }
}

/// Has no array, so [`PsArrayMut::pin`] on it fails: the value of a
/// parameter the engine never assigned.
impl<T: Primitive> Default for PsArrayMut<T> {
    fn default() -> Self {
        PsArrayMut { array: PsObject::default(), _elements: PhantomData }
    }
}

/// Takes the array the engine passed once its elements are found to be
/// `T`. The array is pinned only by [`PsArrayMut::pin`].
impl<T: Primitive + IntoPs> FromPs for PsArrayMut<T> {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        if obj.is_null() {
            return Err(PsError::new(ErrorCategory::InvalidData, "PwrsNullObject", "the array passed is $null"));
        }
        let tag = crate::convert::element_tag(obj)?;
        if tag != T::TYPE_TAG {
            return Err(PsError::new(
                ErrorCategory::InvalidType,
                "PwrsPinElementType",
                format!("array elements are type tag {tag}, expected {}", T::TYPE_TAG),
            ));
        }
        Ok(PsArrayMut { array: obj.clone(), _elements: PhantomData })
    }
}

impl<T: Primitive> IntoPs for PsArrayMut<T> {
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(self.array)
    }
}

impl<T: Primitive> core::fmt::Debug for PsArrayMut<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PsArrayMut({:?})", self.array)
    }
}

/// The span within which a load and an earlier store whose addresses
/// agree in their low 12 bits are taken to touch the same bytes.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
const PAGE: usize = 4096;

/// The size from which [`copy_into`] picks its direction.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
const LARGE_COPY: usize = 1 << 20;

/// Copies `src` into `dst`, which must have the same length.
///
/// On x86 and x86-64, a copy of 1 MiB or more whose destination starts
/// less than 2 KiB after the source, each measured from the start of its
/// own 4 KiB page, goes from the last byte to the first with
/// non-temporal stores. memcpy takes every other copy. For 32 MiB from a
/// heap block into a .NET 10 array, in twelve pwsh 7.6.7 processes on a
/// Ryzen 9 7900X, memcpy took 1.7 to 8.9 ms and this copy 1.6 to 2.1 ms.
fn copy_into<T: Primitive>(dst: &mut [T], src: &[T]) {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    if dst.len() == src.len() && copies_down(dst.as_ptr() as usize, src.as_ptr() as usize, core::mem::size_of_val(src)) {
        // SAFETY: both are as long as `src` and, as a unique and a
        // shared borrow, do not overlap; every x86-64 processor has SSE2,
        // and so does every 32-bit one PowerShell runs on.
        unsafe { copy_down_streaming(dst.as_mut_ptr().cast::<u8>(), src.as_ptr().cast::<u8>(), core::mem::size_of_val(src)) };
        return;
    }
    dst.copy_from_slice(src);
}

/// Whether [`copy_into`] copies `bytes` bytes from the address `src` to
/// the address `dst` last byte first.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
fn copies_down(dst: usize, src: usize, bytes: usize) -> bool {
    let ahead = dst.wrapping_sub(src) % PAGE;
    bytes >= LARGE_COPY && ahead != 0 && ahead < PAGE / 2
}

/// Copies `len` bytes from `src` to `dst`, last byte first: ordinary
/// stores for the bytes after the last 16-byte aligned destination and
/// before the first, 16-byte loads and non-temporal stores between, then
/// a store fence.
///
/// # Safety
/// `dst` and `src` are valid for `len` bytes and do not overlap, and the
/// processor has SSE2.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
unsafe fn copy_down_streaming(dst: *mut u8, src: *const u8, len: usize) {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::{__m128i, _mm_loadu_si128, _mm_sfence, _mm_stream_si128};
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::{__m128i, _mm_loadu_si128, _mm_sfence, _mm_stream_si128};
    let head = ((16 - (dst as usize & 15)) & 15).min(len);
    let end = head + ((len - head) & !15);
    unsafe {
        let mut at = len;
        while at > end {
            at -= 1;
            *dst.add(at) = *src.add(at);
        }
        while at >= head + 64 {
            at -= 64;
            let s = src.add(at).cast::<__m128i>();
            let d = dst.add(at).cast::<__m128i>();
            let (x3, x2, x1, x0) = (_mm_loadu_si128(s.add(3)), _mm_loadu_si128(s.add(2)), _mm_loadu_si128(s.add(1)), _mm_loadu_si128(s));
            _mm_stream_si128(d.add(3), x3);
            _mm_stream_si128(d.add(2), x2);
            _mm_stream_si128(d.add(1), x1);
            _mm_stream_si128(d, x0);
        }
        while at > head {
            at -= 16;
            _mm_stream_si128(dst.add(at).cast::<__m128i>(), _mm_loadu_si128(src.add(at).cast::<__m128i>()));
        }
        while at > 0 {
            at -= 1;
            *dst.add(at) = *src.add(at);
        }
        _mm_sfence();
    }
}

/// A flag that refuses the memory views tied to it once it is set: a
/// view not yet passed to the engine is refused when it is, with
/// `ObjectDisposedException`, and on .NET a view already passed throws
/// it from every later `GetSpan` and `Pin`, so `ToArray`, `CopyTo` and
/// every other read and write through the view fail. A span taken
/// before the flag was set goes on working; the memory under it stays
/// valid while the view's owner lives, which is until the view is
/// released. Clones share one flag, and it is never cleared.
#[derive(Clone, Debug, Default)]
pub struct PsRevocation(Arc<AtomicBool>);

impl PsRevocation {
    /// A flag not yet set.
    pub fn new() -> PsRevocation {
        PsRevocation::default()
    }

    /// Sets the flag.
    pub fn revoke(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether the flag is set.
    pub fn is_revoked(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// The byte the host reads. An `AtomicBool` has the size, alignment
    /// and bit validity of a `bool`, so it reads as 0 or 1.
    fn byte(&self) -> *const u8 {
        Arc::as_ptr(&self.0).cast::<u8>()
    }
}

/// A buffer allocated by Rust and passed to the engine as a `Memory<T>`
/// over the same allocation on .NET, or copied into a `T[]` on .NET
/// Framework. Fill it in place through `DerefMut`, or make it from a
/// slice or a `Vec` with one copy. `into_ps` gives the allocation to the
/// engine: on .NET it is freed when its managed memory manager is
/// disposed or collected, and on .NET Framework the copy is made and the
/// allocation freed before the call returns. A value never passed to the
/// engine frees its allocation on drop. [`PsMemory::read_only`] makes it
/// a `ReadOnlyMemory<T>` instead, and [`PsMemory::revocable`] ties it to
/// a [`PsRevocation`].
pub struct PsMemory<T: Primitive> {
    base: NonNull<u8>,
    len: usize,
    layout: Layout,
    read_only: bool,
    revocation: Option<PsRevocation>,
    _elements: PhantomData<T>,
}

// SAFETY: a `PsMemory` is the only owner of its allocation, as a
// `Vec<T>` is of its own.
unsafe impl<T: Primitive + Send> Send for PsMemory<T> {}
unsafe impl<T: Primitive + Sync> Sync for PsMemory<T> {}

/// The length is stored at the start of the allocation; the elements
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
        Ok(PsMemory { base, len, layout, read_only: false, revocation: None, _elements: PhantomData })
    }

    /// A buffer filled with a copy of `data`.
    pub fn from_slice(data: &[T]) -> PsResult<PsMemory<T>> {
        let mut m = PsMemory::zeroed(data.len())?;
        copy_into(&mut m, data);
        Ok(m)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Makes the buffer a `ReadOnlyMemory<T>` when it is passed to the
    /// engine on .NET. On .NET Framework it is a `T[]` copy either way.
    pub fn read_only(mut self) -> PsMemory<T> {
        self.read_only = true;
        self
    }

    /// Ties the buffer, once passed to the engine, to `revocation`.
    pub fn revocable(mut self, revocation: &PsRevocation) -> PsMemory<T> {
        self.revocation = Some(revocation.clone());
        self
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

impl<T: Primitive + Send + Sync + 'static + crate::IntoPs> crate::IntoPs for PsMemory<T> {
    fn into_ps(mut self) -> PsResult<PsObject> {
        let (data, len, read_only) = (self.data(), self.len, self.read_only);
        let revocation = self.revocation.take();
        // The buffer moves into the owner; the allocation it points at
        // stays where it is.
        unsafe { hand_over::<T>(data, len, read_only, revocation, Box::new(self)) }
    }
}

/// A view of memory PWRS did not allocate, such as a mapped file, passed
/// to the engine as a `Memory<T>` or `ReadOnlyMemory<T>` over that memory
/// on .NET, or copied into a `T[]` on .NET Framework.
///
/// The view comes with an owner, a value that stops the memory being
/// freed, such as an `Arc` of a mapping. The owner lives until the engine
/// releases the view, which on .NET is when the view's manager is
/// disposed or collected and on .NET Framework is once the copy is
/// made, and then drops it on whatever thread releases the view, the
/// finalizer's among them. [`PsMemoryView::revocable`] ties the view to
/// a [`PsRevocation`].
pub struct PsMemoryView<T: Primitive> {
    ptr: *mut T,
    len: usize,
    read_only: bool,
    owner: Box<dyn Any + Send + Sync>,
    revocation: Option<PsRevocation>,
}

// SAFETY: the memory is valid while `owner` lives, on whichever thread
// has the view, as the constructors require, and `owner` is itself
// `Send + Sync`.
unsafe impl<T: Primitive + Send> Send for PsMemoryView<T> {}
unsafe impl<T: Primitive + Sync> Sync for PsMemoryView<T> {}

impl<T: Primitive> PsMemoryView<T> {
    /// A view the engine may write through: a `Memory<T>` on .NET.
    ///
    /// # Safety
    /// For as long as `owner` lives, `ptr` is aligned and valid for reads
    /// and writes of `len` elements of `T`, and that memory is neither
    /// freed nor moved. Managed code may write through the view from any
    /// thread until it is released, so the module has no Rust
    /// reference to that memory meanwhile, and reads and writes it
    /// through raw pointers or atomics.
    pub unsafe fn writable<O: Send + Sync + 'static>(ptr: *mut T, len: usize, owner: O) -> PsMemoryView<T> {
        PsMemoryView { ptr, len, read_only: false, owner: Box::new(owner), revocation: None }
    }

    /// A view the engine only reads: a `ReadOnlyMemory<T>` on .NET.
    ///
    /// # Safety
    /// For as long as `owner` lives, `ptr` is aligned and valid for reads
    /// of `len` elements of `T`, and that memory is neither freed nor
    /// moved.
    pub unsafe fn read_only<O: Send + Sync + 'static>(ptr: *const T, len: usize, owner: O) -> PsMemoryView<T> {
        PsMemoryView { ptr: ptr.cast_mut(), len, read_only: true, owner: Box::new(owner), revocation: None }
    }

    /// Ties the view, once passed to the engine, to `revocation`.
    pub fn revocable(mut self, revocation: &PsRevocation) -> PsMemoryView<T> {
        self.revocation = Some(revocation.clone());
        self
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<T: Primitive + crate::IntoPs> crate::IntoPs for PsMemoryView<T> {
    fn into_ps(self) -> PsResult<PsObject> {
        let PsMemoryView { ptr, len, read_only, owner, revocation } = self;
        unsafe { hand_over::<T>(ptr, len, read_only, revocation, owner) }
    }
}

/// What a view's context points at: the value that stops its memory being freed
/// and the revocation whose byte the host reads, both dropped when the
/// host releases the view.
struct ViewOwner {
    _owner: Box<dyn Any + Send + Sync>,
    _revocation: Option<PsRevocation>,
}

/// Releases a view: drops its owner, on whatever thread the host calls
/// this from. An owner that panics as it drops is reported and not
/// unwound into the host.
unsafe extern "C" fn release_view(context: *mut c_void) {
    let owner = unsafe { Box::from_raw(context.cast::<ViewOwner>()) };
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(owner))).is_err() {
        eprintln!("pwrs: a memory view's owner panicked as it was dropped");
    }
}

/// Passes `len` elements at `ptr` to the host as a view, and `owner`
/// with them: the host drops it when it releases the view, and this
/// drops it when the host refuses the view.
///
/// # Safety
/// `ptr` is aligned and valid for reads of `len` elements of `T`, and
/// for writes too unless `read_only`, for as long as `owner` lives.
unsafe fn hand_over<T: Primitive + crate::IntoPs>(
    ptr: *mut T,
    len: usize,
    read_only: bool,
    revocation: Option<PsRevocation>,
    owner: Box<dyn Any + Send + Sync>,
) -> PsResult<PsObject> {
    let refused = revocation.as_ref().map_or(core::ptr::null(), PsRevocation::byte);
    let context = Box::into_raw(Box::new(ViewOwner { _owner: owner, _revocation: revocation }));
    let mut out = PsHandle::NULL;
    let mut err = PsHandle::NULL;
    let status = unsafe {
        (vtable().memory_view_borrow)(
            T::TYPE_TAG,
            ptr.cast::<c_void>(),
            len,
            u8::from(read_only),
            refused,
            context.cast::<c_void>(),
            release_view,
            &mut out,
            &mut err,
        )
    };
    match check(status, err) {
        Ok(()) => Ok(unsafe { PsObject::from_raw(out) }),
        Err(e) => {
            drop(unsafe { Box::from_raw(context) });
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{copy_into, Primitive};

    /// The index of the first element of `buf` whose address sits at
    /// least `offset` bytes into its 4 KiB page.
    fn index_at<T>(buf: &[T], offset: usize) -> usize {
        let skip = (offset + 4096 - buf.as_ptr() as usize % 4096) % 4096;
        skip.div_ceil(core::mem::size_of::<T>())
    }

    /// Copies with `copy_into` for each pair of page offsets, at lengths
    /// on both sides of 1 MiB, and checks every element copied and the
    /// elements around the destination untouched. Returns how many of the
    /// copies went last byte first and how many first byte first, as
    /// `copies_down` returns on x86 and x86-64; elsewhere both are zero.
    fn copies_whole<T: Primitive + PartialEq>(element: impl Fn(usize) -> T, sentinel: T, offsets: &[(usize, usize)]) -> (usize, usize) {
        let size = core::mem::size_of::<T>();
        let large = (1 << 20) / size;
        let page = 4096 / size;
        let directions = &mut (0, 0);
        for &(src_at, dst_at) in offsets {
            for len in [large - 1, large, large + 37] {
                let src_buf: Vec<T> = (0..len + page + 1).map(&element).collect();
                let mut dst_buf = vec![sentinel; len + 2 * page + 1];
                let s = index_at(&src_buf, src_at);
                let d = index_at(&dst_buf, dst_at) + page;
                let src = &src_buf[s..s + len];
                let place = format!("source +{src_at}, destination +{dst_at}, {len} elements of {size} bytes");
                #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
                {
                    let (to, from) = (dst_buf[d..].as_ptr() as usize, src.as_ptr() as usize);
                    let ahead = to.wrapping_sub(from) % 4096;
                    let down = super::copies_down(to, from, len * size);
                    assert_eq!(down, len * size >= 1 << 20 && ahead != 0 && ahead < 2048, "{place}");
                    if down { directions.0 += 1 } else { directions.1 += 1 }
                }
                copy_into(&mut dst_buf[d..d + len], src);
                assert!(dst_buf[d..d + len] == *src, "{place}: the copy differs");
                assert!(dst_buf[..d].iter().chain(&dst_buf[d + len..]).all(|e| *e == sentinel), "{place}: an element outside the destination changed");
            }
        }
        *directions
    }

    #[test]
    fn a_copy_is_whole_at_every_page_offset_in_either_direction() {
        let spread = |i: usize| (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let counts = [
            copies_whole(|i| (spread(i) >> 56) as u8, 0xA5, &[(0, 0), (0, 8), (0, 88), (3, 91), (5, 2047), (0, 2048), (88, 0), (91, 3), (0, 4095)]),
            copies_whole(|i| (spread(i) >> 48) as u16, 0xA5A5, &[(0, 0), (0, 2), (0, 88), (6, 1030), (0, 2046), (0, 2048), (88, 0), (2, 0)]),
            copies_whole(spread, u64::MAX, &[(0, 0), (0, 8), (0, 88), (8, 2040), (0, 2048), (88, 0)]),
            copies_whole(|i| i as f64 * 0.5 - 7.25, f64::MAX, &[(0, 8), (0, 88), (88, 0)]),
        ];
        let (down, up) = counts.iter().fold((0, 0), |(d, u), (cd, cu)| (d + cd, u + cu));
        if cfg!(any(target_arch = "x86_64", target_arch = "x86")) {
            assert!(down > 0 && up > 0, "{down} copies went last byte first and {up} first byte first; the cases must reach both");
        }
    }
}
