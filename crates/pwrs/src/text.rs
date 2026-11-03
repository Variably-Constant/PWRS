//! UTF-16 and UTF-8 conversions on the parameter and output path.
//!
//! A string parameter is read through [`from_utf16`] or one of its
//! fallible forms, and a string written is converted by
//! [`with_utf16_in`] or [`with_utf16`]. Each takes an ASCII fast path:
//! a single widening or narrowing pass over the bytes, written as a
//! slice loop LLVM vectorizes. Anything outside ASCII goes to std.

/// `s` as UTF-16 code units.
#[inline]
pub fn to_utf16(s: &str) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    to_utf16_into(s, &mut out);
    out
}

/// Appends `s` as UTF-16 code units to `out`.
#[inline]
pub fn to_utf16_into(s: &str, out: &mut Vec<u16>) {
    let bytes = s.as_bytes();
    if !bytes.is_ascii() {
        out.extend(s.encode_utf16());
        return;
    }
    out.reserve(bytes.len());
    let start = out.len();
    let spare = out.spare_capacity_mut();
    for (slot, &b) in spare.iter_mut().zip(bytes) {
        slot.write(u16::from(b));
    }
    // Every slot from `start` to `start + bytes.len()` was written by
    // the loop above.
    unsafe { out.set_len(start + bytes.len()) };
}

/// The longest string, in UTF-8 bytes, that [`with_utf16_in`] and
/// [`with_utf16`] convert on the stack. UTF-16 never takes more units
/// than UTF-8 takes bytes, so the stack buffer has room for this many units.
const INLINE_UTF16: usize = 512;

/// Runs `f` with a UTF-16 copy of `s`. A string of at most 512 UTF-8
/// bytes is converted on the stack, so writing it allocates nothing, a
/// new instance's first write included. A longer one is built in
/// `slot`, whose buffer is taken out of the slot for the call and put
/// back after, so a callee that re-enters this with the same slot gets
/// its own.
#[inline]
pub fn with_utf16_in<R>(slot: &core::cell::Cell<Vec<u16>>, s: &str, f: impl FnOnce(&[u16]) -> R) -> R {
    if s.len() <= INLINE_UTF16 {
        return with_utf16_inline(s, f);
    }
    let mut buf = slot.take();
    buf.clear();
    to_utf16_into(s, &mut buf);
    let r = f(&buf);
    slot.set(buf);
    r
}

/// Runs `f` with a thread-local UTF-16 copy of `s`, for the paths that
/// have no instance to borrow a slot from. A string of at most 512 UTF-8
/// bytes is converted on the stack and never reaches the thread-local.
///
/// A `thread_local!` in a cdylib is reached through `__tls_get_addr`,
/// an indirect call into the dynamic linker, on every access. The
/// slot is therefore read once here and the paths that run per
/// pipeline record take their buffer from the instance instead.
#[inline]
pub fn with_utf16<R>(s: &str, f: impl FnOnce(&[u16]) -> R) -> R {
    thread_local! {
        static SCRATCH: core::cell::Cell<Vec<u16>> = const { core::cell::Cell::new(Vec::new()) };
    }
    if s.len() <= INLINE_UTF16 {
        return with_utf16_inline(s, f);
    }
    SCRATCH.with(|slot| with_utf16_in(slot, s, f))
}

/// Runs `f` with a UTF-16 copy of `s`, at most [`INLINE_UTF16`] bytes,
/// converted into a buffer on the stack.
#[inline]
fn with_utf16_inline<R>(s: &str, f: impl FnOnce(&[u16]) -> R) -> R {
    debug_assert!(s.len() <= INLINE_UTF16);
    let mut buf = [core::mem::MaybeUninit::<u16>::uninit(); INLINE_UTF16];
    let bytes = s.as_bytes();
    let n = if bytes.is_ascii() {
        for (slot, &b) in buf.iter_mut().zip(bytes) {
            slot.write(u16::from(b));
        }
        bytes.len()
    } else {
        buf.iter_mut().zip(s.encode_utf16()).fold(0, |n, (slot, u)| {
            slot.write(u);
            n + 1
        })
    };
    // SAFETY: the loops wrote the first `n` slots, and none of `s` was
    // cut off, since its UTF-16 takes no more units than its UTF-8
    // takes bytes and the buffer has a unit for every byte.
    f(unsafe { core::slice::from_raw_parts(buf.as_ptr().cast::<u16>(), n) })
}

/// The string behind `units`; unpaired surrogates become U+FFFD.
#[inline]
pub fn from_utf16(units: &[u16]) -> String {
    if !is_ascii_utf16(units) {
        return String::from_utf16_lossy(units);
    }
    narrow_ascii(units, Vec::with_capacity(units.len()))
}

/// [`from_utf16`] with every allocation reserved first, so a string too
/// large for the allocator is a `PwrsOutOfMemory` error record rather
/// than the end of the host process. Text outside ASCII is measured in
/// one decoding pass and written in a second, into exactly that much.
#[inline]
pub fn try_from_utf16(units: &[u16]) -> crate::PsResult<String> {
    if !is_ascii_utf16(units) {
        let decoded = || char::decode_utf16(units.iter().copied()).map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER));
        let bytes = decoded().map(char::len_utf8).sum();
        let mut out = String::new();
        out.try_reserve_exact(bytes)?;
        out.extend(decoded());
        return Ok(out);
    }
    let mut out: Vec<u8> = Vec::new();
    out.try_reserve_exact(units.len())?;
    Ok(narrow_ascii(units, out))
}

/// `units`, every one below 0x80, as a string written into `out`,
/// which is empty and has room for all of them.
#[inline]
fn narrow_ascii(units: &[u16], mut out: Vec<u8>) -> String {
    debug_assert!(out.is_empty() && out.capacity() >= units.len());
    let spare = out.spare_capacity_mut();
    for (slot, &u) in spare.iter_mut().zip(units) {
        slot.write(u as u8);
    }
    // Every slot up to `units.len()` was written by the loop above,
    // and every unit is below 0x80, so the bytes are valid UTF-8.
    unsafe {
        out.set_len(units.len());
        String::from_utf8_unchecked(out)
    }
}

/// The string behind a borrowed `(ptr, len)` UTF-16 buffer; null or
/// empty reads as the empty string.
///
/// # Safety
/// `ptr` is valid for `len` reads when it is non-null.
#[inline]
pub unsafe fn from_str16(ptr: *const u16, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    from_utf16(unsafe { core::slice::from_raw_parts(ptr, len) })
}

/// [`from_str16`] through [`try_from_utf16`].
///
/// # Safety
/// `ptr` is valid for `len` reads when it is non-null.
#[inline]
pub unsafe fn try_from_str16(ptr: *const u16, len: usize) -> crate::PsResult<String> {
    if ptr.is_null() || len == 0 {
        return Ok(String::new());
    }
    try_from_utf16(unsafe { core::slice::from_raw_parts(ptr, len) })
}

/// [`try_from_str16`] into `dst`, in the buffer it already has: a string
/// parameter bound again for every pipeline record makes one allocation
/// for all of them rather than one per record. `dst` ends with the text
/// [`try_from_str16`] returns, and an allocation the allocator refuses is
/// the same `PwrsOutOfMemory` error. A buffer more than four times the
/// text, and over 4 KiB, is given up first, so one long value does not
/// leave its memory in use for the records after it.
///
/// # Safety
/// `ptr` is valid for `len` reads when it is non-null.
#[inline]
pub unsafe fn try_assign_str16(dst: &mut String, ptr: *const u16, len: usize) -> crate::PsResult<()> {
    dst.clear();
    if ptr.is_null() || len == 0 {
        return Ok(());
    }
    let units = unsafe { core::slice::from_raw_parts(ptr, len) };
    let ascii = is_ascii_utf16(units);
    let decoded = || char::decode_utf16(units.iter().copied()).map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER));
    let bytes = if ascii { units.len() } else { decoded().map(char::len_utf8).sum() };
    if dst.capacity() > bytes.saturating_mul(4).max(4096) {
        *dst = String::new();
    }
    dst.try_reserve_exact(bytes)?;
    if !ascii {
        dst.extend(decoded());
        return Ok(());
    }
    // SAFETY: every unit is below 0x80, so each byte written is ASCII and
    // the string stays UTF-8; the loop writes `bytes` bytes into the room
    // reserved for them, and the length covers exactly those.
    let buf = unsafe { dst.as_mut_vec() };
    for (slot, &u) in buf.spare_capacity_mut().iter_mut().zip(units) {
        slot.write(u as u8);
    }
    unsafe { buf.set_len(bytes) };
    Ok(())
}

/// True when every unit is below 0x80. An OR-reduction, one pass.
#[inline]
fn is_ascii_utf16(units: &[u16]) -> bool {
    let mut acc: u16 = 0;
    for &u in units {
        acc |= u;
    }
    acc < 0x80
}

#[cfg(test)]
mod tests {
    use super::{from_utf16, to_utf16, try_assign_str16, try_from_str16, try_from_utf16};

    /// Reads `units` into `dst` as a bound parameter is read.
    fn assign(dst: &mut String, units: &[u16]) {
        unsafe { try_assign_str16(dst, units.as_ptr(), units.len()) }.expect("small");
    }

    #[test]
    fn an_assigned_string_is_what_a_fresh_read_gives() {
        let mut dst = String::from("left from the record before, which is longer");
        for s in ["", "Hello, PowerShell!", "héllo wörld ✓ 𝄞", "ascii then ü", "x"] {
            let u = to_utf16(s);
            assign(&mut dst, &u);
            assert_eq!(dst, unsafe { try_from_str16(u.as_ptr(), u.len()) }.expect("small"), "{s}");
        }
        for units in [&[0xD800u16][..], &[0x41, 0xDC00, 0x42]] {
            assign(&mut dst, units);
            assert_eq!(dst, from_utf16(units));
        }
        unsafe { try_assign_str16(&mut dst, core::ptr::null(), 0) }.expect("null");
        assert_eq!(dst, "", "a $null string reads as empty");
    }

    #[test]
    fn a_shorter_string_reuses_the_buffer_and_a_far_smaller_one_gives_up_a_large_one() {
        let mut dst = String::new();
        assign(&mut dst, &to_utf16(&"a".repeat(100)));
        let (ptr, capacity) = (dst.as_ptr(), dst.capacity());
        assign(&mut dst, &to_utf16("Ada"));
        assert_eq!((dst.as_ptr(), dst.capacity()), (ptr, capacity), "a record's string took a new allocation");
        assert_eq!(dst, "Ada");
        assign(&mut dst, &to_utf16(&"b".repeat(100_000)));
        assign(&mut dst, &to_utf16("Bob"));
        assert_eq!(dst, "Bob");
        assert!(dst.capacity() <= 4096, "a short string still had a {} byte buffer", dst.capacity());
    }

    #[test]
    fn the_fallible_read_gives_what_the_infallible_one_does() {
        for s in ["", "Hello, PowerShell! 0123456789", "héllo wörld ✓ 𝄞", "ascii then ü"] {
            let u = to_utf16(s);
            assert_eq!(try_from_utf16(&u).expect("small"), from_utf16(&u), "{s}");
        }
        for units in [&[0xD800u16][..], &[0x41, 0xDC00, 0x42], &[0xD83D, 0xDE00, 0xD800]] {
            let got = try_from_utf16(units).expect("small");
            assert_eq!(got, from_utf16(units));
            assert_eq!(got.capacity(), got.len(), "reserved exactly what the text needs");
        }
    }

    #[test]
    fn ascii_round_trips() {
        let s = "Hello, PowerShell! 0123456789";
        let u = to_utf16(s);
        assert_eq!(u, s.encode_utf16().collect::<Vec<u16>>());
        assert_eq!(from_utf16(&u), s);
    }

    #[test]
    fn non_ascii_round_trips() {
        let s = "héllo wörld ✓ 𝄞";
        let u = to_utf16(s);
        assert_eq!(u, s.encode_utf16().collect::<Vec<u16>>());
        assert_eq!(from_utf16(&u), s);
    }

    #[test]
    fn empty_and_lone_surrogate() {
        assert_eq!(to_utf16(""), Vec::<u16>::new());
        assert_eq!(from_utf16(&[]), "");
        assert_eq!(from_utf16(&[0xD800]), "\u{FFFD}");
    }

    #[test]
    fn a_write_converts_the_same_on_the_stack_and_in_a_buffer() {
        let slot = core::cell::Cell::new(Vec::new());
        let two_byte = "é".repeat(super::INLINE_UTF16 / 2);
        let four_byte = "𝄞".repeat(super::INLINE_UTF16 / 4);
        let over = "y".repeat(super::INLINE_UTF16 + 1);
        for s in ["", "Hello, PowerShell!", "héllo wörld ✓ 𝄞", two_byte.as_str(), four_byte.as_str(), over.as_str()] {
            let want: Vec<u16> = s.encode_utf16().collect();
            assert_eq!(super::with_utf16_in(&slot, s, |u| u.to_vec()), want, "{s}");
            assert_eq!(super::with_utf16(s, |u| u.to_vec()), want, "{s}");
        }
    }

    #[test]
    fn a_short_write_leaves_the_slot_unallocated_and_a_long_one_leaves_its_buffer_there() {
        let slot = core::cell::Cell::new(Vec::new());
        super::with_utf16_in(&slot, &"x".repeat(super::INLINE_UTF16), |_| ());
        assert_eq!(slot.take().capacity(), 0, "a short write allocated");
        super::with_utf16_in(&slot, &"x".repeat(super::INLINE_UTF16 + 1), |_| ());
        assert!(slot.take().capacity() > super::INLINE_UTF16, "a long write left no buffer for the next");
    }

    #[test]
    fn a_write_made_inside_a_write_gets_its_own_buffer() {
        let slot = core::cell::Cell::new(Vec::new());
        let outer = "outer ".repeat(100);
        let inner = "inner ".repeat(100);
        let (o, i, s) = super::with_utf16_in(&slot, &outer, |o| {
            let i = super::with_utf16_in(&slot, &inner, |u| u.to_vec());
            let s = super::with_utf16_in(&slot, "short", |u| u.to_vec());
            (o.to_vec(), i, s)
        });
        assert_eq!(o, outer.encode_utf16().collect::<Vec<u16>>());
        assert_eq!(i, inner.encode_utf16().collect::<Vec<u16>>());
        assert_eq!(s, "short".encode_utf16().collect::<Vec<u16>>());
    }
}
