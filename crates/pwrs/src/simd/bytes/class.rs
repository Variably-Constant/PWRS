//! [`ByteClass`]: a set of ASCII bytes, or every byte outside one, and the
//! runs of it a lexer reads, a whole vector of bytes classified at a time.

use core::ops::Range;

/// A set of ASCII bytes, or every byte outside one, classified a vector at
/// a time. Each scan returns, at every tier and for every input, what its
/// scalar form returns.
///
/// The set is stored as the two 16-entry tables a byte shuffle indexes. The
/// low-nibble table's entry `l` has bit `h` set where the byte `h << 4 | l`
/// is in the set, and the high-nibble table's entry `h` is bit `h` for the
/// eight ASCII high nibbles and empty above them, so a byte is in the set
/// where its two entries share a bit. A shuffle for each nibble and a
/// bitwise and classify 16, 32 or 64 bytes whatever the set contains. The
/// tiers are SSSE3 from x86-64-v2, AVX2 at v3, AVX-512BW at v4 and NEON's
/// TBL; below x86-64-v2, which has no byte shuffle, the scalar forms run.
///
/// ```
/// use pwrs::simd::bytes::ByteClass;
///
/// let line = b"let total_42 = 7;";
/// assert_eq!(ByteClass::WORD.run(&line[4..]), 8);
/// assert_eq!(ByteClass::DIGIT.find(line), Some(10));
/// assert_eq!(ByteClass::WHITESPACE.complement().runs(line, 1), [0..3, 4..12, 13..14, 15..17]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ByteClass {
    lo: [u8; 16],
    complement: bool,
}

/// The high-nibble table: entry `h` is bit `h` for the eight ASCII high
/// nibbles and empty for the eight above, so a byte at or above 0x80 meets
/// no low-nibble entry.
const HIGH: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 0, 0, 0, 0, 0, 0, 0, 0];

impl ByteClass {
    /// ASCII whitespace as [`u8::is_ascii_whitespace`] has it: tab, line
    /// feed, form feed, carriage return and space.
    pub const WHITESPACE: ByteClass = ByteClass::of(b"\t\n\x0C\r ");

    /// The ASCII digits `0` to `9`.
    pub const DIGIT: ByteClass = ByteClass::of(b"0123456789");

    /// The ASCII letters and digits and `_`, the bytes of an identifier in
    /// most languages.
    pub const WORD: ByteClass = ByteClass::of(b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz");

    /// The class of the bytes in `members`, or `None` where one is at or
    /// above 0x80.
    pub const fn new(members: &[u8]) -> Option<ByteClass> {
        let mut lo = [0u8; 16];
        let mut i = 0;
        while i < members.len() {
            let byte = members[i];
            if byte >= 0x80 {
                return None;
            }
            lo[(byte & 0x0F) as usize] |= 1 << (byte >> 4);
            i += 1;
        }
        Some(ByteClass { lo, complement: false })
    }

    /// [`ByteClass::new`] for the constants, where a byte at or above 0x80
    /// fails the build.
    const fn of(members: &[u8]) -> ByteClass {
        match ByteClass::new(members) {
            Some(class) => class,
            None => panic!("a byte class is built from ASCII bytes"),
        }
    }

    /// Every byte this class does not contain.
    pub const fn complement(self) -> ByteClass {
        ByteClass { lo: self.lo, complement: !self.complement }
    }

    /// Whether `byte` is in the class.
    pub const fn contains(&self, byte: u8) -> bool {
        (self.lo[(byte & 0x0F) as usize] & HIGH[(byte >> 4) as usize] != 0) != self.complement
    }

    /// The length of the run of class bytes `haystack` starts with.
    pub fn run(&self, haystack: &[u8]) -> usize {
        match first_tiered(self, haystack, false) {
            Some(end) => end,
            None => haystack.len(),
        }
    }

    /// The scalar form of [`ByteClass::run`]: the result every tier gives.
    pub fn run_scalar(&self, haystack: &[u8]) -> usize {
        haystack.iter().take_while(|&&b| self.contains(b)).count()
    }

    /// The position of the first class byte in `haystack`, or `None`.
    pub fn find(&self, haystack: &[u8]) -> Option<usize> {
        first_tiered(self, haystack, true)
    }

    /// The scalar form of [`ByteClass::find`]: the result every tier gives.
    pub fn find_scalar(&self, haystack: &[u8]) -> Option<usize> {
        haystack.iter().position(|&b| self.contains(b))
    }

    /// The length of the longest run of class bytes in `haystack`.
    pub fn longest_run(&self, haystack: &[u8]) -> usize {
        longest_tiered(self, haystack)
    }

    /// The scalar form of [`ByteClass::longest_run`]: the result every tier
    /// gives.
    pub fn longest_run_scalar(&self, haystack: &[u8]) -> usize {
        longest_scalar(self, haystack, usize::MAX)
    }

    /// Whether `haystack` has a run of at least `least` class bytes. It
    /// reads, a vector at a time, only as far as the first such run.
    pub fn has_run_of(&self, haystack: &[u8], least: usize) -> bool {
        least == 0 || has_run_tiered(self, haystack, least)
    }

    /// The scalar form of [`ByteClass::has_run_of`]: the result every tier
    /// gives.
    pub fn has_run_of_scalar(&self, haystack: &[u8], least: usize) -> bool {
        least == 0 || longest_scalar(self, haystack, least) >= least
    }

    /// The maximal runs of class bytes in `haystack` at least `least` long,
    /// and never empty, as ascending ranges. The words of a text are the
    /// runs of [`ByteClass::WHITESPACE`]'s complement.
    pub fn runs(&self, haystack: &[u8], least: usize) -> Vec<Range<usize>> {
        runs_tiered(self, haystack, least)
    }

    /// The scalar form of [`ByteClass::runs`]: the result every tier gives.
    pub fn runs_scalar(&self, haystack: &[u8], least: usize) -> Vec<Range<usize>> {
        let least = least.max(1);
        let mut out = Vec::new();
        let mut start = 0;
        for (i, &b) in haystack.iter().enumerate() {
            if !self.contains(b) {
                if i - start >= least {
                    out.push(start..i);
                }
                start = i + 1;
            }
        }
        if haystack.len() - start >= least {
            out.push(start..haystack.len());
        }
        out
    }
}

/// The first position whose byte is in the class where `member` is true,
/// and outside it where false.
fn first_scalar(class: &ByteClass, haystack: &[u8], member: bool) -> Option<usize> {
    if member {
        class.find_scalar(haystack)
    } else {
        Some(class.run_scalar(haystack)).filter(|&end| end < haystack.len())
    }
}

/// The longest run of class bytes, returned as soon as one reaches `stop`.
fn longest_scalar(class: &ByteClass, haystack: &[u8], stop: usize) -> usize {
    let (mut longest, mut run) = (0, 0);
    for &b in haystack {
        run = if class.contains(b) { run + 1 } else { 0 };
        longest = longest.max(run);
        if longest >= stop {
            return longest;
        }
    }
    longest
}

#[cfg(target_arch = "x86_64")]
fn first_tiered(class: &ByteClass, haystack: &[u8], member: bool) -> Option<usize> {
    use crate::cpu::{level, Level};
    // A level is reached only when the CPU and the operating system offer
    // every extension of it and of each level below it.
    match level() {
        Level::V4 => unsafe { x86::first_avx512(class, haystack, member) },
        Level::V3 => unsafe { x86::first_avx2(class, haystack, member) },
        Level::V2 => unsafe { x86::first_ssse3(class, haystack, member) },
        _baseline => first_scalar(class, haystack, member),
    }
}

#[cfg(target_arch = "x86_64")]
fn longest_tiered(class: &ByteClass, haystack: &[u8]) -> usize {
    use crate::cpu::{level, Level};
    match level() {
        Level::V4 => unsafe { x86::longest_avx512(class, haystack, usize::MAX) },
        Level::V3 => unsafe { x86::longest_avx2(class, haystack, usize::MAX) },
        Level::V2 => unsafe { x86::longest_ssse3(class, haystack, usize::MAX) },
        _baseline => class.longest_run_scalar(haystack),
    }
}

/// Whether a run of class bytes reaches `least`, which is not 0. A vector
/// tier stops reading at the first such run.
#[cfg(target_arch = "x86_64")]
fn has_run_tiered(class: &ByteClass, haystack: &[u8], least: usize) -> bool {
    use crate::cpu::{level, Level};
    match level() {
        Level::V4 => unsafe { x86::longest_avx512(class, haystack, least) >= least },
        Level::V3 => unsafe { x86::longest_avx2(class, haystack, least) >= least },
        Level::V2 => unsafe { x86::longest_ssse3(class, haystack, least) >= least },
        _baseline => class.has_run_of_scalar(haystack, least),
    }
}

#[cfg(target_arch = "x86_64")]
fn runs_tiered(class: &ByteClass, haystack: &[u8], least: usize) -> Vec<Range<usize>> {
    use crate::cpu::{level, Level};
    match level() {
        Level::V4 => unsafe { x86::runs_avx512(class, haystack, least) },
        Level::V3 => unsafe { x86::runs_avx2(class, haystack, least) },
        Level::V2 => unsafe { x86::runs_ssse3(class, haystack, least) },
        _baseline => class.runs_scalar(haystack, least),
    }
}

// NEON is in every AArch64 CPU.
#[cfg(target_arch = "aarch64")]
fn first_tiered(class: &ByteClass, haystack: &[u8], member: bool) -> Option<usize> {
    unsafe { neon::first(class, haystack, member) }
}

#[cfg(target_arch = "aarch64")]
fn longest_tiered(class: &ByteClass, haystack: &[u8]) -> usize {
    unsafe { neon::longest(class, haystack, usize::MAX) }
}

#[cfg(target_arch = "aarch64")]
fn has_run_tiered(class: &ByteClass, haystack: &[u8], least: usize) -> bool {
    unsafe { neon::longest(class, haystack, least) >= least }
}

#[cfg(target_arch = "aarch64")]
fn runs_tiered(class: &ByteClass, haystack: &[u8], least: usize) -> Vec<Range<usize>> {
    unsafe { neon::runs(class, haystack, least) }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn first_tiered(class: &ByteClass, haystack: &[u8], member: bool) -> Option<usize> {
    first_scalar(class, haystack, member)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn longest_tiered(class: &ByteClass, haystack: &[u8]) -> usize {
    class.longest_run_scalar(haystack)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn has_run_tiered(class: &ByteClass, haystack: &[u8], least: usize) -> bool {
    class.has_run_of_scalar(haystack, least)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn runs_tiered(class: &ByteClass, haystack: &[u8], least: usize) -> Vec<Range<usize>> {
    class.runs_scalar(haystack, least)
}

/// How a tier's mask spells a block of `width` bytes: lane `j`'s bit sits
/// where a shift right by `shift` gives `j`, and `all` is every lane's bit.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[derive(Clone, Copy)]
struct Lanes {
    width: usize,
    shift: u32,
    all: u64,
}

// The scans every tier runs over its own masks. `hits(i)` gives the lanes
// of the `width` bytes from `i` whose two table entries share a bit, and
// is called only where those bytes are in bounds; for a complement it gives
// the lanes that do not.

/// The first position whose byte is in the class where `member` is true,
/// and outside it where false.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[inline(always)]
fn scan_first(class: &ByteClass, haystack: &[u8], member: bool, lanes: Lanes, mut hits: impl FnMut(usize) -> u64) -> Option<usize> {
    let flip = if class.complement == member { lanes.all } else { 0 };
    let mut i = 0;
    while i + lanes.width <= haystack.len() {
        let wanted = hits(i) ^ flip;
        if wanted != 0 {
            return Some(i + (wanted.trailing_zeros() >> lanes.shift) as usize);
        }
        i += lanes.width;
    }
    first_scalar(class, &haystack[i..], member).map(|at| i + at)
}

/// The longest run of class bytes, returned once one reaches `stop`, which
/// may be a block after the scalar form would return it.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[inline(always)]
fn scan_longest(class: &ByteClass, haystack: &[u8], stop: usize, lanes: Lanes, mut hits: impl FnMut(usize) -> u64) -> usize {
    // The lanes outside the class, each of which ends a run.
    let flip = if class.complement { 0 } else { lanes.all };
    let (mut longest, mut run, mut i) = (0usize, 0usize, 0usize);
    while i + lanes.width <= haystack.len() {
        let breaks = hits(i) ^ flip;
        if breaks == 0 {
            run += lanes.width;
        } else {
            let first = (breaks.trailing_zeros() >> lanes.shift) as usize;
            longest = longest.max(run + first);
            // A run between two breaks is no longer than the block's class
            // bytes, so the walk is taken only where they could pass it.
            if lanes.width - breaks.count_ones() as usize > longest {
                let mut prev = first;
                let mut rest = breaks & (breaks - 1);
                while rest != 0 {
                    let next = (rest.trailing_zeros() >> lanes.shift) as usize;
                    longest = longest.max(next - prev - 1);
                    prev = next;
                    rest &= rest - 1;
                }
            }
            run = lanes.width - 1 - ((63 - breaks.leading_zeros()) >> lanes.shift) as usize;
        }
        longest = longest.max(run);
        if longest >= stop {
            return longest;
        }
        i += lanes.width;
    }
    for &b in &haystack[i..] {
        run = if class.contains(b) { run + 1 } else { 0 };
        longest = longest.max(run);
        if longest >= stop {
            return longest;
        }
    }
    longest
}

/// The maximal runs of class bytes at least `least` long.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[inline(always)]
fn scan_runs(class: &ByteClass, haystack: &[u8], least: usize, lanes: Lanes, mut hits: impl FnMut(usize) -> u64) -> Vec<Range<usize>> {
    let least = least.max(1);
    let flip = if class.complement { 0 } else { lanes.all };
    let mut out = Vec::new();
    let (mut start, mut i) = (0usize, 0usize);
    while i + lanes.width <= haystack.len() {
        let breaks = hits(i) ^ flip;
        if breaks != 0 {
            let first = (breaks.trailing_zeros() >> lanes.shift) as usize;
            if i + first - start >= least {
                out.push(start..i + first);
            }
            // A run between two breaks needs `least` class bytes, so a
            // block with fewer has none to walk for.
            if lanes.width - breaks.count_ones() as usize >= least {
                let mut prev = first;
                let mut rest = breaks & (breaks - 1);
                while rest != 0 {
                    let next = (rest.trailing_zeros() >> lanes.shift) as usize;
                    if next - prev > least {
                        out.push(i + prev + 1..i + next);
                    }
                    prev = next;
                    rest &= rest - 1;
                }
            }
            start = i + 1 + ((63 - breaks.leading_zeros()) >> lanes.shift) as usize;
        }
        i += lanes.width;
    }
    for (j, &b) in haystack.iter().enumerate().skip(i) {
        if !class.contains(b) {
            if j - start >= least {
                out.push(start..j);
            }
            start = j + 1;
        }
    }
    if haystack.len() - start >= least {
        out.push(start..haystack.len());
    }
    out
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{scan_first, scan_longest, scan_runs, ByteClass, Lanes, HIGH};
    use core::arch::x86_64::*;
    use core::ops::Range;

    const L16: Lanes = Lanes { width: 16, shift: 0, all: 0xFFFF };
    const L32: Lanes = Lanes { width: 32, shift: 0, all: 0xFFFF_FFFF };
    const L64: Lanes = Lanes { width: 64, shift: 0, all: u64::MAX };

    // Each tier's bodies enable the bit instructions its level includes for
    // the scans' counts and positions: POPCNT from x86-64-v2, BMI1 and LZCNT
    // from v3. In each, the tables are 16 bytes and `hits` reads the block
    // the scan's loop guard checks is in bounds.

    /// The lanes of the 16 bytes at `at` whose table entries share a bit.
    #[target_feature(enable = "ssse3")]
    #[inline]
    unsafe fn hits_ssse3(lo: __m128i, high: __m128i, at: *const u8) -> u64 {
        // SAFETY: the caller has checked that the 16 bytes are in bounds.
        unsafe {
            let v = _mm_loadu_si128(at.cast());
            let nibble = _mm_set1_epi8(0x0F);
            // A 16-bit shift brings each byte's high nibble down, with the
            // byte above's low nibble over it in the lower byte of a pair,
            // which the mask clears.
            let met = _mm_and_si128(_mm_shuffle_epi8(lo, _mm_and_si128(v, nibble)), _mm_shuffle_epi8(high, _mm_and_si128(_mm_srli_epi16::<4>(v), nibble)));
            u64::from(_mm_movemask_epi8(_mm_cmpeq_epi8(met, _mm_setzero_si128())) as u32 ^ 0xFFFF)
        }
    }

    #[target_feature(enable = "avx2")]
    #[inline]
    unsafe fn hits_avx2(lo: __m256i, high: __m256i, at: *const u8) -> u64 {
        // SAFETY: the caller has checked that the 32 bytes are in bounds.
        unsafe {
            let v = _mm256_loadu_si256(at.cast());
            let nibble = _mm256_set1_epi8(0x0F);
            let met = _mm256_and_si256(_mm256_shuffle_epi8(lo, _mm256_and_si256(v, nibble)), _mm256_shuffle_epi8(high, _mm256_and_si256(_mm256_srli_epi16::<4>(v), nibble)));
            u64::from(!(_mm256_movemask_epi8(_mm256_cmpeq_epi8(met, _mm256_setzero_si256())) as u32))
        }
    }

    #[target_feature(enable = "avx512f,avx512bw")]
    #[inline]
    unsafe fn hits_avx512(lo: __m512i, high: __m512i, at: *const u8) -> u64 {
        // SAFETY: the caller has checked that the 64 bytes are in bounds.
        unsafe {
            let v = _mm512_loadu_si512(at.cast());
            let nibble = _mm512_set1_epi8(0x0F);
            _mm512_test_epi8_mask(_mm512_shuffle_epi8(lo, _mm512_and_si512(v, nibble)), _mm512_shuffle_epi8(high, _mm512_and_si512(_mm512_srli_epi16::<4>(v), nibble)))
        }
    }

    /// The tables as 16-byte vectors, which the wider tiers broadcast to
    /// each 128-bit lane, since a shuffle reads within its lane.
    #[target_feature(enable = "ssse3")]
    #[inline]
    unsafe fn tables_128(class: &ByteClass) -> (__m128i, __m128i) {
        // SAFETY: each table is 16 bytes.
        unsafe { (_mm_loadu_si128(class.lo.as_ptr().cast()), _mm_loadu_si128(HIGH.as_ptr().cast())) }
    }

    #[target_feature(enable = "ssse3,popcnt")]
    pub unsafe fn first_ssse3(class: &ByteClass, haystack: &[u8], member: bool) -> Option<usize> {
        unsafe {
            let (lo, high) = tables_128(class);
            scan_first(class, haystack, member, L16, |i| hits_ssse3(lo, high, haystack.as_ptr().add(i)))
        }
    }

    #[target_feature(enable = "ssse3,popcnt")]
    pub unsafe fn longest_ssse3(class: &ByteClass, haystack: &[u8], stop: usize) -> usize {
        unsafe {
            let (lo, high) = tables_128(class);
            scan_longest(class, haystack, stop, L16, |i| hits_ssse3(lo, high, haystack.as_ptr().add(i)))
        }
    }

    #[target_feature(enable = "ssse3,popcnt")]
    pub unsafe fn runs_ssse3(class: &ByteClass, haystack: &[u8], least: usize) -> Vec<Range<usize>> {
        unsafe {
            let (lo, high) = tables_128(class);
            scan_runs(class, haystack, least, L16, |i| hits_ssse3(lo, high, haystack.as_ptr().add(i)))
        }
    }

    #[target_feature(enable = "avx2,bmi1,lzcnt,popcnt")]
    pub unsafe fn first_avx2(class: &ByteClass, haystack: &[u8], member: bool) -> Option<usize> {
        unsafe {
            let (lo, high) = tables_128(class);
            let (lo, high) = (_mm256_broadcastsi128_si256(lo), _mm256_broadcastsi128_si256(high));
            scan_first(class, haystack, member, L32, |i| hits_avx2(lo, high, haystack.as_ptr().add(i)))
        }
    }

    #[target_feature(enable = "avx2,bmi1,lzcnt,popcnt")]
    pub unsafe fn longest_avx2(class: &ByteClass, haystack: &[u8], stop: usize) -> usize {
        unsafe {
            let (lo, high) = tables_128(class);
            let (lo, high) = (_mm256_broadcastsi128_si256(lo), _mm256_broadcastsi128_si256(high));
            scan_longest(class, haystack, stop, L32, |i| hits_avx2(lo, high, haystack.as_ptr().add(i)))
        }
    }

    #[target_feature(enable = "avx2,bmi1,lzcnt,popcnt")]
    pub unsafe fn runs_avx2(class: &ByteClass, haystack: &[u8], least: usize) -> Vec<Range<usize>> {
        unsafe {
            let (lo, high) = tables_128(class);
            let (lo, high) = (_mm256_broadcastsi128_si256(lo), _mm256_broadcastsi128_si256(high));
            scan_runs(class, haystack, least, L32, |i| hits_avx2(lo, high, haystack.as_ptr().add(i)))
        }
    }

    #[target_feature(enable = "avx512f,avx512bw,bmi1,lzcnt,popcnt")]
    pub unsafe fn first_avx512(class: &ByteClass, haystack: &[u8], member: bool) -> Option<usize> {
        unsafe {
            let (lo, high) = tables_128(class);
            let (lo, high) = (_mm512_broadcast_i32x4(lo), _mm512_broadcast_i32x4(high));
            scan_first(class, haystack, member, L64, |i| hits_avx512(lo, high, haystack.as_ptr().add(i)))
        }
    }

    #[target_feature(enable = "avx512f,avx512bw,bmi1,lzcnt,popcnt")]
    pub unsafe fn longest_avx512(class: &ByteClass, haystack: &[u8], stop: usize) -> usize {
        unsafe {
            let (lo, high) = tables_128(class);
            let (lo, high) = (_mm512_broadcast_i32x4(lo), _mm512_broadcast_i32x4(high));
            scan_longest(class, haystack, stop, L64, |i| hits_avx512(lo, high, haystack.as_ptr().add(i)))
        }
    }

    #[target_feature(enable = "avx512f,avx512bw,bmi1,lzcnt,popcnt")]
    pub unsafe fn runs_avx512(class: &ByteClass, haystack: &[u8], least: usize) -> Vec<Range<usize>> {
        unsafe {
            let (lo, high) = tables_128(class);
            let (lo, high) = (_mm512_broadcast_i32x4(lo), _mm512_broadcast_i32x4(high));
            scan_runs(class, haystack, least, L64, |i| hits_avx512(lo, high, haystack.as_ptr().add(i)))
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::super::neon::lanes;
    use super::{scan_first, scan_longest, scan_runs, ByteClass, Lanes, HIGH};
    use core::arch::aarch64::*;
    use core::ops::Range;

    /// `lanes` sets bit `4 * j + 3` for lane `j`.
    const L16: Lanes = Lanes { width: 16, shift: 2, all: 0x8888_8888_8888_8888 };

    // In each body the tables are 16 bytes and `hits` reads the block the
    // scan's loop guard checks is in bounds.

    /// The lanes of the 16 bytes at `at` whose table entries share a bit.
    /// Both indexes are under 16, where TBL reads the table.
    #[inline]
    unsafe fn hits(lo: uint8x16_t, high: uint8x16_t, at: *const u8) -> u64 {
        // SAFETY: the caller has checked that the 16 bytes are in bounds.
        unsafe {
            let v = vld1q_u8(at);
            lanes(vtstq_u8(vqtbl1q_u8(lo, vandq_u8(v, vdupq_n_u8(0x0F))), vqtbl1q_u8(high, vshrq_n_u8::<4>(v))))
        }
    }

    pub unsafe fn first(class: &ByteClass, haystack: &[u8], member: bool) -> Option<usize> {
        unsafe {
            let (lo, high) = (vld1q_u8(class.lo.as_ptr()), vld1q_u8(HIGH.as_ptr()));
            scan_first(class, haystack, member, L16, |i| hits(lo, high, haystack.as_ptr().add(i)))
        }
    }

    pub unsafe fn longest(class: &ByteClass, haystack: &[u8], stop: usize) -> usize {
        unsafe {
            let (lo, high) = (vld1q_u8(class.lo.as_ptr()), vld1q_u8(HIGH.as_ptr()));
            scan_longest(class, haystack, stop, L16, |i| hits(lo, high, haystack.as_ptr().add(i)))
        }
    }

    pub unsafe fn runs(class: &ByteClass, haystack: &[u8], least: usize) -> Vec<Range<usize>> {
        unsafe {
            let (lo, high) = (vld1q_u8(class.lo.as_ptr()), vld1q_u8(HIGH.as_ptr()));
            scan_runs(class, haystack, least, L16, |i| hits(lo, high, haystack.as_ptr().add(i)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type First = unsafe fn(&ByteClass, &[u8], bool) -> Option<usize>;
    type Longest = unsafe fn(&ByteClass, &[u8], usize) -> usize;
    type Runs = unsafe fn(&ByteClass, &[u8], usize) -> Vec<Range<usize>>;

    /// One tier's bodies.
    struct TierBodies {
        tier: &'static str,
        first: First,
        longest: Longest,
        runs: Runs,
    }

    /// Every body this CPU runs, by tier, whatever `PWRS_CPU_MAX` says; a
    /// tier it lacks is printed as skipped.
    fn bodies() -> Vec<TierBodies> {
        let mut out = Vec::new();
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            let bits = detected(Isa::Bmi1) && detected(Isa::Lzcnt) && detected(Isa::Popcnt);
            if detected(Isa::Ssse3) && detected(Isa::Popcnt) {
                out.push(TierBodies { tier: "x86-64-v2", first: x86::first_ssse3, longest: x86::longest_ssse3, runs: x86::runs_ssse3 });
            } else {
                eprintln!("BYTECLASS SKIPPED x86-64-v2: this CPU lacks ssse3 or popcnt");
            }
            if detected(Isa::Avx2) && bits {
                out.push(TierBodies { tier: "x86-64-v3", first: x86::first_avx2, longest: x86::longest_avx2, runs: x86::runs_avx2 });
            } else {
                eprintln!("BYTECLASS SKIPPED x86-64-v3: this CPU lacks avx2, bmi1, lzcnt or popcnt");
            }
            if detected(Isa::Avx512f) && detected(Isa::Avx512bw) && bits {
                out.push(TierBodies { tier: "x86-64-v4", first: x86::first_avx512, longest: x86::longest_avx512, runs: x86::runs_avx512 });
            } else {
                eprintln!("BYTECLASS SKIPPED x86-64-v4: this CPU lacks avx512f, avx512bw, bmi1, lzcnt or popcnt");
            }
        }
        #[cfg(target_arch = "aarch64")]
        out.push(TierBodies { tier: "neon", first: neon::first, longest: neon::longest, runs: neon::runs });
        out
    }

    /// The ready-made classes, three more, and the complement of each.
    fn classes() -> Vec<(String, ByteClass)> {
        let ascii: Vec<u8> = (0..0x80).collect();
        let named = [
            ("word", ByteClass::WORD),
            ("whitespace", ByteClass::WHITESPACE),
            ("digit", ByteClass::DIGIT),
            ("hex", ByteClass::new(b"0123456789abcdef").expect("the hex digits are ASCII")),
            ("empty", ByteClass::new(b"").expect("a class may be empty")),
            ("ascii", ByteClass::new(&ascii).expect("0 to 0x7F are ASCII")),
        ];
        named.iter().flat_map(|(name, class)| [(name.to_string(), *class), (format!("not {name}"), class.complement())]).collect()
    }

    /// Inputs made of runs of one kind of byte at a time, up to 80 long so
    /// that they cross every block edge, inputs of bytes drawn from all 256
    /// values, at lengths on both sides of every vector width, and one byte
    /// value throughout.
    fn haystacks() -> Vec<Vec<u8>> {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let kinds: [&[u8]; 5] = [b"abcXYZ_", b"0123456789", b" \t\n\r\x0C", b"+-=;.,\x0B", &[0x80, 0xC3, 0xA9, 0xFF]];
        let mut out = Vec::new();
        for len in [0usize, 1, 2, 15, 16, 17, 31, 32, 33, 63, 64, 65, 100, 127, 128, 129, 200, 1000, 4099] {
            let mut text = Vec::with_capacity(len);
            while text.len() < len {
                let kind = kinds[(next() % kinds.len() as u64) as usize];
                let run = 1 + (next() % 80) as usize;
                for _ in 0..run.min(len - text.len()) {
                    text.push(kind[(next() % kind.len() as u64) as usize]);
                }
            }
            out.push(text);
            out.push((0..len).map(|_| next() as u8).collect());
        }
        for len in [64usize, 65, 200] {
            out.push(vec![b'a'; len]);
            out.push(vec![b' '; len]);
        }
        out
    }

    #[test]
    fn the_classes_hold_what_their_definitions_hold() {
        let classes = classes();
        let hex = ByteClass::new(b"0123456789abcdef").expect("the hex digits are ASCII");
        for b in 0..=255u8 {
            assert_eq!(ByteClass::WORD.contains(b), b.is_ascii_alphanumeric() || b == b'_', "word: byte {b}");
            assert_eq!(ByteClass::WHITESPACE.contains(b), b.is_ascii_whitespace(), "whitespace: byte {b}");
            assert_eq!(ByteClass::DIGIT.contains(b), b.is_ascii_digit(), "digit: byte {b}");
            assert_eq!(hex.contains(b), b.is_ascii_digit() || (b'a'..=b'f').contains(&b), "hex: byte {b}");
            for (name, class) in &classes {
                assert_eq!(class.complement().contains(b), !class.contains(b), "{name}: byte {b}");
            }
        }
        assert_eq!(ByteClass::new(&[b'a', 0x80]), None);
        assert_eq!(ByteClass::WORD.complement().complement(), ByteClass::WORD);
    }

    #[test]
    fn every_tier_scans_what_the_scalar_scans_find() {
        let classes = classes();
        let haystacks = haystacks();
        let bodies = bodies();
        assert!(!bodies.is_empty(), "no class body ran");
        for body in &bodies {
            let tier = body.tier;
            eprintln!("BYTECLASS RUNNING at {tier} over {} classes and {} haystacks", classes.len(), haystacks.len());
            for (name, class) in &classes {
                for haystack in &haystacks {
                    let len = haystack.len();
                    // SAFETY in each call: bodies() lists only bodies this
                    // CPU runs.
                    for member in [true, false] {
                        let got = unsafe { (body.first)(class, haystack, member) };
                        assert_eq!(got, first_scalar(class, haystack, member), "{tier}: the first byte {} {name} in {len} bytes", if member { "in" } else { "outside" });
                    }
                    let got = unsafe { (body.longest)(class, haystack, usize::MAX) };
                    assert_eq!(got, class.longest_run_scalar(haystack), "{tier}: the longest {name} run in {len} bytes");
                    for least in [1usize, 2, 3, 15, 16, 17, 33, 64, 65, 81] {
                        let got = unsafe { (body.longest)(class, haystack, least) } >= least;
                        assert_eq!(got, class.has_run_of_scalar(haystack, least), "{tier}: a {name} run of {least} in {len} bytes");
                    }
                    for least in [0usize, 1, 2, 3, 7, 15, 16, 17, 31, 32, 33, 64, 65] {
                        let got = unsafe { (body.runs)(class, haystack, least) };
                        assert_eq!(got, class.runs_scalar(haystack, least), "{tier}: the {name} runs of {least} in {len} bytes");
                    }
                }
            }
        }
        for (name, class) in &classes {
            for haystack in &haystacks {
                assert_eq!(class.run(haystack), class.run_scalar(haystack), "{name}");
                assert_eq!(class.find(haystack), class.find_scalar(haystack), "{name}");
                assert_eq!(class.longest_run(haystack), class.longest_run_scalar(haystack), "{name}");
                for least in [0usize, 1, 5, 64] {
                    assert_eq!(class.has_run_of(haystack, least), class.has_run_of_scalar(haystack, least), "{name}");
                    assert_eq!(class.runs(haystack, least), class.runs_scalar(haystack, least), "{name}");
                }
            }
        }
        assert!(ByteClass::DIGIT.has_run_of(b"", 0));
        assert!(!ByteClass::DIGIT.has_run_of(b"12a3", 3));
        assert_eq!(ByteClass::WORD.run(b"abc"), 3);
        assert_eq!(ByteClass::WORD.find(b"  "), None);
    }
}
