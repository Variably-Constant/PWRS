//! Byte search, counting, selection and classes. At every tier, for every
//! input, [`find`] returns the position [`find_scalar`] returns, [`count`]
//! the total [`count_scalar`] returns, and [`nth_byte`] and
//! [`nth_byte_back`] the positions their scalar forms return. A
//! [`ByteClass`] finds the runs of a set of ASCII bytes that a lexer
//! reads, under the same contract.
//!
//! The tiers read 16 bytes at a time on the x86-64 baseline and on NEON,
//! 32 under x86-64-v3 and 64 under x86-64-v4. A search compares the
//! needle's first byte across a whole vector, turns a candidate away on its
//! last byte, and compares what is left in full, so it finds the leftmost
//! match the scalar search finds. A selection counts each vector's matches
//! and passes over the vector while it has fewer than are still wanted,
//! then takes the position from the compare's mask in the vector that has
//! the one wanted.

mod class;

pub use class::ByteClass;

/// The leftmost position at which `needle` occurs in `haystack`, or
/// `None`. An empty needle matches at position 0.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    find_tiered(haystack, needle)
}

/// The scalar form of [`find`]: the result every tier gives.
pub fn find_scalar(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    tail(haystack, needle, 0)
}

/// How many times `byte` occurs in `haystack`.
pub fn count(haystack: &[u8], byte: u8) -> usize {
    count_tiered(haystack, byte)
}

/// The scalar form of [`count`]: the result every tier gives.
pub fn count_scalar(haystack: &[u8], byte: u8) -> usize {
    haystack.iter().filter(|&&b| b == byte).count()
}

/// The position of the `n`th occurrence of `byte` in `haystack`, counting
/// from one at the start, or `None` where it occurs fewer than `n` times or
/// `n` is zero. The first `n` lines of a text end at its `n`th newline.
pub fn nth_byte(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    nth_tiered(haystack, byte, n)
}

/// The scalar form of [`nth_byte`]: the result every tier gives.
pub fn nth_byte_scalar(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    haystack.iter().enumerate().filter(|&(_, &b)| b == byte).nth(n - 1).map(|(i, _)| i)
}

/// The position of the `n`th occurrence of `byte` in `haystack`, counting
/// from one at the end, or `None` where it occurs fewer than `n` times or
/// `n` is zero. It reads only the bytes after the occurrence it returns,
/// and the vector it is in, so the lines at the end of a long text are
/// found without reading the rest.
pub fn nth_byte_back(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    nth_back_tiered(haystack, byte, n)
}

/// The scalar form of [`nth_byte_back`]: the result every tier gives.
pub fn nth_byte_back_scalar(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    haystack.iter().enumerate().rev().filter(|&(_, &b)| b == byte).nth(n - 1).map(|(i, _)| i)
}

#[cfg(target_arch = "x86_64")]
fn find_tiered(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    use crate::cpu::{level, Level};
    // A level is reached only when the CPU and the operating system offer
    // every extension of it, and SSE2 is in every x86-64 CPU.
    match level() {
        Level::V4 => unsafe { x86::find_avx512(haystack, needle) },
        Level::V3 => unsafe { x86::find_avx2(haystack, needle) },
        _baseline => unsafe { x86::find_sse2(haystack, needle) },
    }
}

#[cfg(target_arch = "x86_64")]
fn count_tiered(haystack: &[u8], byte: u8) -> usize {
    use crate::cpu::{level, Level};
    match level() {
        Level::V4 => unsafe { x86::count_avx512(haystack, byte) },
        Level::V3 => unsafe { x86::count_avx2(haystack, byte) },
        _baseline => unsafe { x86::count_sse2(haystack, byte) },
    }
}

#[cfg(target_arch = "x86_64")]
fn nth_tiered(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
    use crate::cpu::{level, Level};
    match level() {
        Level::V4 => unsafe { x86::nth_avx512(haystack, byte, n) },
        Level::V3 => unsafe { x86::nth_avx2(haystack, byte, n) },
        _baseline => unsafe { x86::nth_sse2(haystack, byte, n) },
    }
}

#[cfg(target_arch = "x86_64")]
fn nth_back_tiered(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
    use crate::cpu::{level, Level};
    match level() {
        Level::V4 => unsafe { x86::nth_back_avx512(haystack, byte, n) },
        Level::V3 => unsafe { x86::nth_back_avx2(haystack, byte, n) },
        _baseline => unsafe { x86::nth_back_sse2(haystack, byte, n) },
    }
}

// NEON is in every AArch64 CPU.
#[cfg(target_arch = "aarch64")]
fn find_tiered(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    unsafe { neon::find(haystack, needle) }
}

#[cfg(target_arch = "aarch64")]
fn count_tiered(haystack: &[u8], byte: u8) -> usize {
    unsafe { neon::count(haystack, byte) }
}

#[cfg(target_arch = "aarch64")]
fn nth_tiered(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
    unsafe { neon::nth(haystack, byte, n) }
}

#[cfg(target_arch = "aarch64")]
fn nth_back_tiered(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
    unsafe { neon::nth_back(haystack, byte, n) }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn find_tiered(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    find_scalar(haystack, needle)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn count_tiered(haystack: &[u8], byte: u8) -> usize {
    count_scalar(haystack, byte)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn nth_tiered(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
    nth_byte_scalar(haystack, byte, n)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn nth_back_tiered(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
    nth_byte_back_scalar(haystack, byte, n)
}

/// The first position from `from` at which `needle`, which is not empty and
/// no longer than `haystack`, occurs.
fn tail(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let last = haystack.len() - needle.len();
    (from..=last).find(|&i| haystack[i] == needle[0] && &haystack[i..i + needle.len()] == needle)
}

/// The first candidate in `mask` at which the whole needle occurs. Candidate
/// `j` is position `at + j`, marked by bit `j << shift`.
fn verify(haystack: &[u8], needle: &[u8], at: usize, mut mask: u64, shift: u32) -> Option<usize> {
    let last = haystack.len() - needle.len();
    while mask != 0 {
        let pos = at + (mask.trailing_zeros() >> shift) as usize;
        if pos > last {
            return None;
        }
        if &haystack[pos..pos + needle.len()] == needle {
            return Some(pos);
        }
        mask &= mask - 1;
    }
    None
}

/// The position of the `k`th set bit of `mask`, counting from one at the
/// lowest; `mask` has at least `k` set bits.
#[inline]
fn nth_set_bit(mut mask: u64, k: usize) -> u32 {
    for _ in 1..k {
        mask &= mask - 1;
    }
    mask.trailing_zeros()
}

/// The position of the `k`th set bit of `mask`, counting from one at the
/// highest; `mask` has at least `k` set bits.
#[inline]
fn nth_set_bit_from_top(mut mask: u64, k: usize) -> u32 {
    for _ in 1..k {
        mask ^= 1 << (63 - mask.leading_zeros());
    }
    63 - mask.leading_zeros()
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{count_scalar, nth_byte_back_scalar, nth_byte_scalar, nth_set_bit, nth_set_bit_from_top, tail, verify};
    use core::arch::x86_64::*;

    // The x86-64-v3 and v4 bodies that count a vector's matches enable
    // POPCNT, which every level from x86-64-v2 includes.

    /// The x86-64 baseline. The needle is not empty and no longer than the
    /// haystack, as for every search body.
    pub unsafe fn find_sse2(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        // SAFETY: each load reads 16 bytes from an offset its guard checks is
        // 16 or more short of the end.
        unsafe {
            let n = needle.len();
            let first = _mm_set1_epi8(needle[0] as i8);
            let final_byte = _mm_set1_epi8(needle[n - 1] as i8);
            let mut i = 0;
            while i + 16 <= haystack.len() {
                let mut mask = _mm_movemask_epi8(_mm_cmpeq_epi8(_mm_loadu_si128(haystack.as_ptr().add(i).cast()), first)) as u32 as u64;
                if mask != 0 && i + n - 1 + 16 <= haystack.len() {
                    mask &= _mm_movemask_epi8(_mm_cmpeq_epi8(_mm_loadu_si128(haystack.as_ptr().add(i + n - 1).cast()), final_byte)) as u32 as u64;
                }
                if let Some(pos) = verify(haystack, needle, i, mask, 0) {
                    return Some(pos);
                }
                i += 16;
            }
            tail(haystack, needle, i)
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn find_avx2(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        // SAFETY: as find_sse2's, 32 bytes at a time.
        unsafe {
            let n = needle.len();
            let first = _mm256_set1_epi8(needle[0] as i8);
            let final_byte = _mm256_set1_epi8(needle[n - 1] as i8);
            let mut i = 0;
            while i + 32 <= haystack.len() {
                let mut mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(_mm256_loadu_si256(haystack.as_ptr().add(i).cast()), first)) as u32 as u64;
                if mask != 0 && i + n - 1 + 32 <= haystack.len() {
                    mask &= _mm256_movemask_epi8(_mm256_cmpeq_epi8(_mm256_loadu_si256(haystack.as_ptr().add(i + n - 1).cast()), final_byte)) as u32 as u64;
                }
                if let Some(pos) = verify(haystack, needle, i, mask, 0) {
                    return Some(pos);
                }
                i += 32;
            }
            tail(haystack, needle, i)
        }
    }

    #[target_feature(enable = "avx512f,avx512bw")]
    pub unsafe fn find_avx512(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        // SAFETY: as find_sse2's, 64 bytes at a time.
        unsafe {
            let n = needle.len();
            let first = _mm512_set1_epi8(needle[0] as i8);
            let final_byte = _mm512_set1_epi8(needle[n - 1] as i8);
            let mut i = 0;
            while i + 64 <= haystack.len() {
                let mut mask = _mm512_cmpeq_epi8_mask(_mm512_loadu_si512(haystack.as_ptr().add(i).cast()), first);
                if mask != 0 && i + n - 1 + 64 <= haystack.len() {
                    mask &= _mm512_cmpeq_epi8_mask(_mm512_loadu_si512(haystack.as_ptr().add(i + n - 1).cast()), final_byte);
                }
                if let Some(pos) = verify(haystack, needle, i, mask, 0) {
                    return Some(pos);
                }
                i += 64;
            }
            tail(haystack, needle, i)
        }
    }

    /// The x86-64 baseline.
    pub unsafe fn count_sse2(haystack: &[u8], byte: u8) -> usize {
        // SAFETY: each load reads 16 bytes the loop guard checks are in bounds.
        unsafe {
            let wanted = _mm_set1_epi8(byte as i8);
            let mut count = 0usize;
            let mut i = 0;
            while i + 16 <= haystack.len() {
                count += (_mm_movemask_epi8(_mm_cmpeq_epi8(_mm_loadu_si128(haystack.as_ptr().add(i).cast()), wanted)) as u32).count_ones() as usize;
                i += 16;
            }
            count + count_scalar(&haystack[i..], byte)
        }
    }

    #[target_feature(enable = "avx2,popcnt")]
    pub unsafe fn count_avx2(haystack: &[u8], byte: u8) -> usize {
        // SAFETY: as count_sse2's, 32 bytes at a time.
        unsafe {
            let wanted = _mm256_set1_epi8(byte as i8);
            let mut count = 0usize;
            let mut i = 0;
            while i + 32 <= haystack.len() {
                count += (_mm256_movemask_epi8(_mm256_cmpeq_epi8(_mm256_loadu_si256(haystack.as_ptr().add(i).cast()), wanted)) as u32).count_ones() as usize;
                i += 32;
            }
            count + count_scalar(&haystack[i..], byte)
        }
    }

    #[target_feature(enable = "avx512f,avx512bw,popcnt")]
    pub unsafe fn count_avx512(haystack: &[u8], byte: u8) -> usize {
        // SAFETY: as count_sse2's, 64 bytes at a time.
        unsafe {
            let wanted = _mm512_set1_epi8(byte as i8);
            let mut count = 0usize;
            let mut i = 0;
            while i + 64 <= haystack.len() {
                count += _mm512_cmpeq_epi8_mask(_mm512_loadu_si512(haystack.as_ptr().add(i).cast()), wanted).count_ones() as usize;
                i += 64;
            }
            count + count_scalar(&haystack[i..], byte)
        }
    }

    /// The x86-64 baseline. `n` is at least one, as for every selection
    /// body.
    pub unsafe fn nth_sse2(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
        // SAFETY: each load reads 16 bytes the loop guard checks are in bounds.
        unsafe {
            let wanted = _mm_set1_epi8(byte as i8);
            let mut left = n;
            let mut i = 0;
            while i + 16 <= haystack.len() {
                let mask = _mm_movemask_epi8(_mm_cmpeq_epi8(_mm_loadu_si128(haystack.as_ptr().add(i).cast()), wanted)) as u32 as u64;
                let here = mask.count_ones() as usize;
                if here >= left {
                    return Some(i + nth_set_bit(mask, left) as usize);
                }
                left -= here;
                i += 16;
            }
            nth_byte_scalar(&haystack[i..], byte, left).map(|at| i + at)
        }
    }

    #[target_feature(enable = "avx2,popcnt")]
    pub unsafe fn nth_avx2(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
        // SAFETY: as nth_sse2's, 32 bytes at a time.
        unsafe {
            let wanted = _mm256_set1_epi8(byte as i8);
            let mut left = n;
            let mut i = 0;
            while i + 32 <= haystack.len() {
                let mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(_mm256_loadu_si256(haystack.as_ptr().add(i).cast()), wanted)) as u32 as u64;
                let here = mask.count_ones() as usize;
                if here >= left {
                    return Some(i + nth_set_bit(mask, left) as usize);
                }
                left -= here;
                i += 32;
            }
            nth_byte_scalar(&haystack[i..], byte, left).map(|at| i + at)
        }
    }

    #[target_feature(enable = "avx512f,avx512bw,popcnt")]
    pub unsafe fn nth_avx512(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
        // SAFETY: as nth_sse2's, 64 bytes at a time.
        unsafe {
            let wanted = _mm512_set1_epi8(byte as i8);
            let mut left = n;
            let mut i = 0;
            while i + 64 <= haystack.len() {
                let mask = _mm512_cmpeq_epi8_mask(_mm512_loadu_si512(haystack.as_ptr().add(i).cast()), wanted);
                let here = mask.count_ones() as usize;
                if here >= left {
                    return Some(i + nth_set_bit(mask, left) as usize);
                }
                left -= here;
                i += 64;
            }
            nth_byte_scalar(&haystack[i..], byte, left).map(|at| i + at)
        }
    }

    /// The x86-64 baseline.
    pub unsafe fn nth_back_sse2(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
        // SAFETY: each load reads the 16 bytes before `end`, which the loop
        // guard checks is 16 or more and which starts at the haystack's end.
        unsafe {
            let wanted = _mm_set1_epi8(byte as i8);
            let mut left = n;
            let mut end = haystack.len();
            while end >= 16 {
                let start = end - 16;
                let mask = _mm_movemask_epi8(_mm_cmpeq_epi8(_mm_loadu_si128(haystack.as_ptr().add(start).cast()), wanted)) as u32 as u64;
                let here = mask.count_ones() as usize;
                if here >= left {
                    return Some(start + nth_set_bit_from_top(mask, left) as usize);
                }
                left -= here;
                end = start;
            }
            nth_byte_back_scalar(&haystack[..end], byte, left)
        }
    }

    #[target_feature(enable = "avx2,popcnt")]
    pub unsafe fn nth_back_avx2(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
        // SAFETY: as nth_back_sse2's, 32 bytes at a time.
        unsafe {
            let wanted = _mm256_set1_epi8(byte as i8);
            let mut left = n;
            let mut end = haystack.len();
            while end >= 32 {
                let start = end - 32;
                let mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(_mm256_loadu_si256(haystack.as_ptr().add(start).cast()), wanted)) as u32 as u64;
                let here = mask.count_ones() as usize;
                if here >= left {
                    return Some(start + nth_set_bit_from_top(mask, left) as usize);
                }
                left -= here;
                end = start;
            }
            nth_byte_back_scalar(&haystack[..end], byte, left)
        }
    }

    #[target_feature(enable = "avx512f,avx512bw,popcnt")]
    pub unsafe fn nth_back_avx512(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
        // SAFETY: as nth_back_sse2's, 64 bytes at a time.
        unsafe {
            let wanted = _mm512_set1_epi8(byte as i8);
            let mut left = n;
            let mut end = haystack.len();
            while end >= 64 {
                let start = end - 64;
                let mask = _mm512_cmpeq_epi8_mask(_mm512_loadu_si512(haystack.as_ptr().add(start).cast()), wanted);
                let here = mask.count_ones() as usize;
                if here >= left {
                    return Some(start + nth_set_bit_from_top(mask, left) as usize);
                }
                left -= here;
                end = start;
            }
            nth_byte_back_scalar(&haystack[..end], byte, left)
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::{count_scalar, nth_byte_back_scalar, nth_byte_scalar, nth_set_bit, nth_set_bit_from_top, tail, verify};
    use core::arch::aarch64::*;

    /// A compare's 16 lanes, each 0 or 0xFF, as four bits a lane: NEON has
    /// no byte movemask, and a narrowing shift packs the lanes this way.
    /// Lane `j` sets bit `4 * j + 3`.
    pub(super) unsafe fn lanes(eq: uint8x16_t) -> u64 {
        // SAFETY: register operations only.
        unsafe { vget_lane_u64::<0>(vreinterpret_u64_u8(vshrn_n_u16::<4>(vreinterpretq_u16_u8(eq)))) & 0x8888_8888_8888_8888 }
    }

    /// NEON, which every AArch64 CPU has. The needle is not empty and no
    /// longer than the haystack.
    pub unsafe fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        // SAFETY: each load reads 16 bytes from an offset its guard checks is
        // 16 or more short of the end.
        unsafe {
            let n = needle.len();
            let first = vdupq_n_u8(needle[0]);
            let final_byte = vdupq_n_u8(needle[n - 1]);
            let mut i = 0;
            while i + 16 <= haystack.len() {
                let mut eq = vceqq_u8(vld1q_u8(haystack.as_ptr().add(i)), first);
                if vmaxvq_u8(eq) != 0 && i + n - 1 + 16 <= haystack.len() {
                    eq = vandq_u8(eq, vceqq_u8(vld1q_u8(haystack.as_ptr().add(i + n - 1)), final_byte));
                }
                if let Some(pos) = verify(haystack, needle, i, lanes(eq), 2) {
                    return Some(pos);
                }
                i += 16;
            }
            tail(haystack, needle, i)
        }
    }

    pub unsafe fn count(haystack: &[u8], byte: u8) -> usize {
        // SAFETY: each load reads 16 bytes the loop guard checks are in bounds.
        unsafe {
            let wanted = vdupq_n_u8(byte);
            let mut count = 0usize;
            let mut i = 0;
            while i + 16 <= haystack.len() {
                // A compare's 0xFF subtracted adds one to a lane, which is
                // summed before it can pass 255.
                let mut lanes = vdupq_n_u8(0);
                let mut rounds = 0;
                while rounds < 255 && i + 16 <= haystack.len() {
                    lanes = vsubq_u8(lanes, vceqq_u8(vld1q_u8(haystack.as_ptr().add(i)), wanted));
                    i += 16;
                    rounds += 1;
                }
                count += usize::from(vaddlvq_u8(lanes));
            }
            count + count_scalar(&haystack[i..], byte)
        }
    }

    /// `n` is at least one, as for every selection body. A match in lane
    /// `j` is bit `4 * j + 3` of `lanes`, one bit a lane, so the mask
    /// counts the lanes and a bit's position over four is its lane.
    pub unsafe fn nth(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
        // SAFETY: each load reads 16 bytes the loop guard checks are in bounds.
        unsafe {
            let wanted = vdupq_n_u8(byte);
            let mut left = n;
            let mut i = 0;
            while i + 16 <= haystack.len() {
                let mask = lanes(vceqq_u8(vld1q_u8(haystack.as_ptr().add(i)), wanted));
                let here = mask.count_ones() as usize;
                if here >= left {
                    return Some(i + (nth_set_bit(mask, left) >> 2) as usize);
                }
                left -= here;
                i += 16;
            }
            nth_byte_scalar(&haystack[i..], byte, left).map(|at| i + at)
        }
    }

    pub unsafe fn nth_back(haystack: &[u8], byte: u8, n: usize) -> Option<usize> {
        // SAFETY: each load reads the 16 bytes before `end`, which the loop
        // guard checks is 16 or more and which starts at the haystack's end.
        unsafe {
            let wanted = vdupq_n_u8(byte);
            let mut left = n;
            let mut end = haystack.len();
            while end >= 16 {
                let start = end - 16;
                let mask = lanes(vceqq_u8(vld1q_u8(haystack.as_ptr().add(start)), wanted));
                let here = mask.count_ones() as usize;
                if here >= left {
                    return Some(start + (nth_set_bit_from_top(mask, left) >> 2) as usize);
                }
                left -= here;
                end = start;
            }
            nth_byte_back_scalar(&haystack[..end], byte, left)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Find = unsafe fn(&[u8], &[u8]) -> Option<usize>;
    type Count = unsafe fn(&[u8], u8) -> usize;
    type Nth = unsafe fn(&[u8], u8, usize) -> Option<usize>;

    /// One tier's bodies.
    struct TierBodies {
        tier: &'static str,
        find: Find,
        count: Count,
        nth: Nth,
        nth_back: Nth,
    }

    /// Every body this CPU runs, by tier, whatever `PWRS_CPU_MAX` says; a
    /// tier it lacks is printed as skipped.
    fn bodies() -> Vec<TierBodies> {
        let mut out = Vec::new();
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            out.push(TierBodies { tier: "x86-64", find: x86::find_sse2, count: x86::count_sse2, nth: x86::nth_sse2, nth_back: x86::nth_back_sse2 });
            if detected(Isa::Avx2) && detected(Isa::Popcnt) {
                out.push(TierBodies { tier: "x86-64-v3", find: x86::find_avx2, count: x86::count_avx2, nth: x86::nth_avx2, nth_back: x86::nth_back_avx2 });
            } else {
                eprintln!("BYTES SKIPPED x86-64-v3: this CPU lacks avx2 or popcnt");
            }
            if detected(Isa::Avx512f) && detected(Isa::Avx512bw) && detected(Isa::Popcnt) {
                out.push(TierBodies { tier: "x86-64-v4", find: x86::find_avx512, count: x86::count_avx512, nth: x86::nth_avx512, nth_back: x86::nth_back_avx512 });
            } else {
                eprintln!("BYTES SKIPPED x86-64-v4: this CPU lacks avx512f, avx512bw or popcnt");
            }
        }
        #[cfg(target_arch = "aarch64")]
        out.push(TierBodies { tier: "neon", find: neon::find, count: neon::count, nth: neon::nth, nth_back: neon::nth_back });
        out
    }

    /// Haystacks over two, four and 256 byte values, so matches are common
    /// and rare, at lengths on both sides of every vector width, with needles
    /// taken from the haystack and drawn at random.
    fn cases() -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut out = Vec::new();
        for len in [1usize, 2, 15, 16, 17, 31, 32, 33, 47, 63, 64, 65, 79, 127, 128, 129, 200, 1000, 4099] {
            for values in [2u64, 4, 256] {
                let haystack: Vec<u8> = (0..len).map(|_| (next() % values) as u8).collect();
                for needle_len in [1usize, 2, 3, 8, 17, 33, 65] {
                    if needle_len <= len {
                        let at = (next() as usize) % (len - needle_len + 1);
                        out.push((haystack.clone(), haystack[at..at + needle_len].to_vec()));
                    }
                    out.push((haystack.clone(), (0..needle_len).map(|_| (next() % values) as u8).collect()));
                }
            }
        }
        out
    }

    #[test]
    fn every_tier_finds_what_the_scalar_search_finds() {
        let cases = cases();
        let bodies = bodies();
        assert!(!bodies.is_empty(), "no search body ran");
        for body in &bodies {
            let tier = body.tier;
            eprintln!("BYTES RUNNING find at {tier} over {} cases", cases.len());
            for (haystack, needle) in &cases {
                if needle.len() > haystack.len() {
                    continue;
                }
                // SAFETY: bodies() lists only bodies this CPU runs.
                let got = unsafe { (body.find)(haystack, needle) };
                assert_eq!(got, find_scalar(haystack, needle), "{tier}: a {}-byte needle in {} bytes", needle.len(), haystack.len());
            }
        }
        for (haystack, needle) in &cases {
            assert_eq!(find(haystack, needle), find_scalar(haystack, needle));
        }
        assert_eq!(find(b"abc", b""), Some(0));
        assert_eq!(find(b"ab", b"abc"), None);
        assert_eq!(find(b"", b"a"), None);
    }

    #[test]
    fn every_tier_counts_what_the_scalar_count_counts() {
        let cases = cases();
        let bodies = bodies();
        assert!(!bodies.is_empty(), "no count body ran");
        // Long runs of one byte value, so a NEON lane would pass 255 in one
        // block if it were not summed first.
        let runs: Vec<Vec<u8>> = [0usize, 1, 4080, 4096, 70_000].iter().map(|&n| vec![7u8; n]).collect();
        for body in &bodies {
            let tier = body.tier;
            eprintln!("BYTES RUNNING count at {tier}");
            for haystack in cases.iter().map(|c| &c.0).chain(runs.iter()) {
                for byte in [0u8, 1, 3, 7, 255] {
                    // SAFETY: bodies() lists only bodies this CPU runs.
                    let got = unsafe { (body.count)(haystack, byte) };
                    assert_eq!(got, count_scalar(haystack, byte), "{tier}: byte {byte} in {} bytes", haystack.len());
                }
            }
        }
        for haystack in runs.iter() {
            assert_eq!(count(haystack, 7), haystack.len());
        }
    }

    #[test]
    fn every_tier_selects_what_the_scalar_selection_selects() {
        let cases = cases();
        // Runs of one value, where every byte matches, so a selection ends
        // in the first vector, in the last, or past the end.
        let runs: Vec<Vec<u8>> = [0usize, 1, 16, 63, 64, 65, 1000].iter().map(|&n| vec![7u8; n]).collect();
        let mut haystacks: Vec<&Vec<u8>> = cases.iter().map(|c| &c.0).collect();
        haystacks.dedup();
        haystacks.extend(runs.iter());
        let bodies = bodies();
        assert!(!bodies.is_empty(), "no selection body ran");
        for body in &bodies {
            let tier = body.tier;
            eprintln!("BYTES RUNNING nth_byte and nth_byte_back at {tier} over {} haystacks", haystacks.len());
            for haystack in &haystacks {
                for byte in [0u8, 1, 3, 7, 255] {
                    let total = count_scalar(haystack, byte);
                    for n in [1, 2, 3, 15, 16, 17, 31, 32, 33, 63, 64, 65, total / 2, total.saturating_sub(1), total, total + 1, usize::MAX] {
                        if n == 0 {
                            continue;
                        }
                        // SAFETY: bodies() lists only bodies this CPU runs,
                        // and `n` is at least one.
                        let (forward, back) = unsafe { ((body.nth)(haystack, byte, n), (body.nth_back)(haystack, byte, n)) };
                        assert_eq!(forward, nth_byte_scalar(haystack, byte, n), "{tier}: occurrence {n} of {byte} in {} bytes", haystack.len());
                        assert_eq!(back, nth_byte_back_scalar(haystack, byte, n), "{tier}: occurrence {n} from the end of {byte} in {} bytes", haystack.len());
                    }
                }
            }
        }
        for haystack in &haystacks {
            for n in [0, 1, 2, 40] {
                assert_eq!(nth_byte(haystack, 1, n), nth_byte_scalar(haystack, 1, n));
                assert_eq!(nth_byte_back(haystack, 1, n), nth_byte_back_scalar(haystack, 1, n));
            }
        }
        let text = b"ab\ncd\nef\n";
        assert_eq!(nth_byte(text, b'\n', 2), Some(5));
        assert_eq!(nth_byte(text, b'\n', 4), None);
        assert_eq!(nth_byte_back(text, b'\n', 1), Some(8));
        assert_eq!(nth_byte_back(text, b'\n', 3), Some(2));
        assert_eq!(nth_byte(text, b'\n', 0), None);
        assert_eq!(nth_byte_back(text, b'\n', 0), None);
    }
}
