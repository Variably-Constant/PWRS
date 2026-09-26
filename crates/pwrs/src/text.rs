//! UTF-16 and UTF-8 conversions on the parameter and output path.
//!
//! Every string parameter is read through [`from_utf16`] and every
//! string written through [`to_utf16`]. Both take an ASCII fast path:
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

/// Runs `f` with a UTF-16 copy of `s` built in `slot`. The buffer is
/// taken out of the slot for the call and put back after, so a callee
/// that re-enters this with the same slot gets its own.
#[inline]
pub fn with_utf16_in<R>(slot: &core::cell::Cell<Vec<u16>>, s: &str, f: impl FnOnce(&[u16]) -> R) -> R {
    let mut buf = slot.take();
    buf.clear();
    to_utf16_into(s, &mut buf);
    let r = f(&buf);
    slot.set(buf);
    r
}

/// Runs `f` with a thread-local UTF-16 copy of `s`, for the paths that
/// have no instance to borrow a slot from.
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
    SCRATCH.with(|slot| with_utf16_in(slot, s, f))
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
    from_utf16(core::slice::from_raw_parts(ptr, len))
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
    try_from_utf16(core::slice::from_raw_parts(ptr, len))
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
    use super::{from_utf16, to_utf16, try_from_utf16};

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
}
