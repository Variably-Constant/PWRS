//! Integer reductions and bitwise operations over slices: [`sum_u64`],
//! [`xor_u64`], [`count_ones`], [`min_i64`], [`max_i64`], and the
//! element-wise [`and_u64`], [`or_u64`] and [`and_not_u64`]. Integer
//! addition and XOR give the same total in any order and a minimum is a
//! choice, so every tier returns what the scalar form, the `_scalar` fn
//! beside each, returns.

/// The sum of `values`, wrapping on overflow.
pub fn sum_u64(values: &[u64]) -> u64 {
    tiered::sum_u64(values)
}

/// The scalar form of [`sum_u64`]: the result every tier gives.
pub fn sum_u64_scalar(values: &[u64]) -> u64 {
    values.iter().fold(0u64, |a, &v| a.wrapping_add(v))
}

/// Every value XORed together; 0 for none.
pub fn xor_u64(values: &[u64]) -> u64 {
    tiered::xor_u64(values)
}

/// The scalar form of [`xor_u64`]: the result every tier gives.
pub fn xor_u64_scalar(values: &[u64]) -> u64 {
    values.iter().fold(0u64, |a, &v| a ^ v)
}

/// How many bits are set across `values`.
pub fn count_ones(values: &[u64]) -> u64 {
    tiered::count_ones(values)
}

/// The scalar form of [`count_ones`]: the result every tier gives.
pub fn count_ones_scalar(values: &[u64]) -> u64 {
    values.iter().map(|v| u64::from(v.count_ones())).sum()
}

/// The smallest of `values`, or `None` for none.
pub fn min_i64(values: &[i64]) -> Option<i64> {
    tiered::pick(values, false)
}

/// The scalar form of [`min_i64`]: the result every tier gives.
pub fn min_i64_scalar(values: &[i64]) -> Option<i64> {
    if values.is_empty() { None } else { Some(pick_scalar(values, false)) }
}

/// The largest of `values`, or `None` for none.
pub fn max_i64(values: &[i64]) -> Option<i64> {
    tiered::pick(values, true)
}

/// The scalar form of [`max_i64`]: the result every tier gives.
pub fn max_i64_scalar(values: &[i64]) -> Option<i64> {
    if values.is_empty() { None } else { Some(pick_scalar(values, true)) }
}

/// `out[i] = a[i] & b[i]`, over the shortest of the three.
pub fn and_u64(a: &[u64], b: &[u64], out: &mut [u64]) {
    let n = a.len().min(b.len()).min(out.len());
    tiered::bitwise(Op::And, &a[..n], &b[..n], &mut out[..n]);
}

/// `out[i] = a[i] | b[i]`, over the shortest of the three.
pub fn or_u64(a: &[u64], b: &[u64], out: &mut [u64]) {
    let n = a.len().min(b.len()).min(out.len());
    tiered::bitwise(Op::Or, &a[..n], &b[..n], &mut out[..n]);
}

/// `out[i] = a[i] & !b[i]`, over the shortest of the three.
pub fn and_not_u64(a: &[u64], b: &[u64], out: &mut [u64]) {
    let n = a.len().min(b.len()).min(out.len());
    tiered::bitwise(Op::AndNot, &a[..n], &b[..n], &mut out[..n]);
}

/// The scalar form of [`and_u64`], [`or_u64`] and [`and_not_u64`], over
/// the shortest of the three: the result every tier gives.
pub fn bitwise_scalar(op: Op, a: &[u64], b: &[u64], out: &mut [u64]) {
    for ((o, &x), &y) in out.iter_mut().zip(a).zip(b) {
        *o = op.apply(x, y);
    }
}

/// An element-wise bitwise operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    And,
    Or,
    AndNot,
}

impl Op {
    fn apply(self, x: u64, y: u64) -> u64 {
        match self {
            Op::And => x & y,
            Op::Or => x | y,
            Op::AndNot => x & !y,
        }
    }
}

/// The smallest or, with `largest`, the largest of a non-empty slice, one
/// value at a time.
fn pick_scalar(values: &[i64], largest: bool) -> i64 {
    if largest { values.iter().copied().fold(i64::MIN, i64::max) } else { values.iter().copied().fold(i64::MAX, i64::min) }
}

#[cfg(target_arch = "x86_64")]
mod tiered {
    use super::{x86, Op};
    use crate::cpu::{has, level, Isa, Level};

    // A level is reached only when the CPU and the operating system offer
    // every extension of it; SSE2 is in every x86-64 CPU.
    pub fn sum_u64(v: &[u64]) -> u64 {
        match level() {
            Level::V4 => unsafe { x86::sum_avx512(v) },
            Level::V3 => unsafe { x86::sum_avx2(v) },
            _baseline => unsafe { x86::sum_sse2(v) },
        }
    }

    pub fn xor_u64(v: &[u64]) -> u64 {
        match level() {
            Level::V4 => unsafe { x86::xor_avx512(v) },
            Level::V3 => unsafe { x86::xor_avx2(v) },
            _baseline => unsafe { x86::xor_sse2(v) },
        }
    }

    pub fn count_ones(v: &[u64]) -> u64 {
        match level() {
            // VPOPCNTQ is in no psABI level, so has() checks it on its own.
            Level::V4 if has(Isa::Avx512vpopcntdq) => unsafe { x86::ones_vpopcntq(v) },
            Level::V4 => unsafe { x86::ones_avx512bw(v) },
            Level::V3 => unsafe { x86::ones_avx2(v) },
            Level::V2 => unsafe { x86::ones_popcnt(v) },
            _baseline => super::count_ones_scalar(v),
        }
    }

    /// The smallest or, with `largest`, the largest of `v`, or `None` for
    /// none.
    pub fn pick(v: &[i64], largest: bool) -> Option<i64> {
        if v.is_empty() {
            return None;
        }
        match level() {
            Level::V4 => Some(unsafe { x86::pick_avx512(v, largest) }),
            Level::V3 => Some(unsafe { x86::pick_avx2(v, largest) }),
            Level::V2 => Some(unsafe { x86::pick_sse42(v, largest) }),
            _baseline if largest => super::max_i64_scalar(v),
            _baseline => super::min_i64_scalar(v),
        }
    }

    pub fn bitwise(op: Op, a: &[u64], b: &[u64], out: &mut [u64]) {
        match level() {
            Level::V4 => unsafe { x86::bitwise_avx512(op, a, b, out) },
            Level::V3 => unsafe { x86::bitwise_avx2(op, a, b, out) },
            _baseline => unsafe { x86::bitwise_sse2(op, a, b, out) },
        }
    }
}

// NEON is in every AArch64 CPU, and optional on 32-bit ARM.
#[cfg(any(target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon")))]
mod tiered {
    use super::{neon, Op};
    use crate::simd::arch::has_neon;

    pub fn sum_u64(v: &[u64]) -> u64 {
        if has_neon() { unsafe { neon::sum(v) } } else { super::sum_u64_scalar(v) }
    }

    pub fn xor_u64(v: &[u64]) -> u64 {
        if has_neon() { unsafe { neon::xor(v) } } else { super::xor_u64_scalar(v) }
    }

    pub fn count_ones(v: &[u64]) -> u64 {
        if has_neon() { unsafe { neon::ones(v) } } else { super::count_ones_scalar(v) }
    }

    /// The smallest or, with `largest`, the largest of `v`, or `None` for
    /// none.
    #[cfg(target_arch = "aarch64")]
    pub fn pick(v: &[i64], largest: bool) -> Option<i64> {
        if v.is_empty() { None } else { Some(unsafe { neon::pick(v, largest) }) }
    }

    // ARMv7 has no 64-bit compare. The compiler builds one by moving each
    // lane to core registers and back, which a Cortex-A53 runs slower than
    // the scalar form.
    #[cfg(target_arch = "arm")]
    pub fn pick(v: &[i64], largest: bool) -> Option<i64> {
        if largest { super::max_i64_scalar(v) } else { super::min_i64_scalar(v) }
    }

    pub fn bitwise(op: Op, a: &[u64], b: &[u64], out: &mut [u64]) {
        if has_neon() { unsafe { neon::bitwise(op, a, b, out) } } else { super::bitwise_scalar(op, a, b, out) }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon"))))]
mod tiered {
    use super::Op;

    pub fn sum_u64(v: &[u64]) -> u64 {
        super::sum_u64_scalar(v)
    }

    pub fn xor_u64(v: &[u64]) -> u64 {
        super::xor_u64_scalar(v)
    }

    pub fn count_ones(v: &[u64]) -> u64 {
        super::count_ones_scalar(v)
    }

    pub fn pick(v: &[i64], largest: bool) -> Option<i64> {
        if largest { super::max_i64_scalar(v) } else { super::min_i64_scalar(v) }
    }

    pub fn bitwise(op: Op, a: &[u64], b: &[u64], out: &mut [u64]) {
        super::bitwise_scalar(op, a, b, out)
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::Op;
    use core::arch::x86_64::*;

    /// The x86-64 baseline.
    pub unsafe fn sum_sse2(v: &[u64]) -> u64 {
        // SAFETY: each load reads two values the loop guard checks are in bounds.
        unsafe {
            let mut acc = _mm_setzero_si128();
            let mut i = 0;
            while i + 2 <= v.len() {
                acc = _mm_add_epi64(acc, _mm_loadu_si128(v.as_ptr().add(i).cast()));
                i += 2;
            }
            let mut lanes = [0u64; 2];
            _mm_storeu_si128(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().chain(&v[i..]).fold(0u64, |a, &x| a.wrapping_add(x))
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn sum_avx2(v: &[u64]) -> u64 {
        // SAFETY: as sum_sse2's, four values at a time.
        unsafe {
            let mut acc = _mm256_setzero_si256();
            let mut i = 0;
            while i + 4 <= v.len() {
                acc = _mm256_add_epi64(acc, _mm256_loadu_si256(v.as_ptr().add(i).cast()));
                i += 4;
            }
            let mut lanes = [0u64; 4];
            _mm256_storeu_si256(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().chain(&v[i..]).fold(0u64, |a, &x| a.wrapping_add(x))
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn sum_avx512(v: &[u64]) -> u64 {
        // SAFETY: as sum_sse2's, eight values at a time.
        unsafe {
            let mut acc = _mm512_setzero_si512();
            let mut i = 0;
            while i + 8 <= v.len() {
                acc = _mm512_add_epi64(acc, _mm512_loadu_si512(v.as_ptr().add(i).cast()));
                i += 8;
            }
            let mut lanes = [0u64; 8];
            _mm512_storeu_si512(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().chain(&v[i..]).fold(0u64, |a, &x| a.wrapping_add(x))
        }
    }

    /// The x86-64 baseline.
    pub unsafe fn xor_sse2(v: &[u64]) -> u64 {
        // SAFETY: as sum_sse2's.
        unsafe {
            let mut acc = _mm_setzero_si128();
            let mut i = 0;
            while i + 2 <= v.len() {
                acc = _mm_xor_si128(acc, _mm_loadu_si128(v.as_ptr().add(i).cast()));
                i += 2;
            }
            let mut lanes = [0u64; 2];
            _mm_storeu_si128(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().chain(&v[i..]).fold(0u64, |a, &x| a ^ x)
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn xor_avx2(v: &[u64]) -> u64 {
        // SAFETY: as sum_avx2's.
        unsafe {
            let mut acc = _mm256_setzero_si256();
            let mut i = 0;
            while i + 4 <= v.len() {
                acc = _mm256_xor_si256(acc, _mm256_loadu_si256(v.as_ptr().add(i).cast()));
                i += 4;
            }
            let mut lanes = [0u64; 4];
            _mm256_storeu_si256(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().chain(&v[i..]).fold(0u64, |a, &x| a ^ x)
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn xor_avx512(v: &[u64]) -> u64 {
        // SAFETY: as sum_avx512's.
        unsafe {
            let mut acc = _mm512_setzero_si512();
            let mut i = 0;
            while i + 8 <= v.len() {
                acc = _mm512_xor_si512(acc, _mm512_loadu_si512(v.as_ptr().add(i).cast()));
                i += 8;
            }
            let mut lanes = [0u64; 8];
            _mm512_storeu_si512(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().chain(&v[i..]).fold(0u64, |a, &x| a ^ x)
        }
    }

    /// x86-64-v2: the scalar count, which POPCNT makes one instruction a
    /// value.
    #[target_feature(enable = "popcnt")]
    pub unsafe fn ones_popcnt(v: &[u64]) -> u64 {
        super::count_ones_scalar(v)
    }

    /// Mula's count: each byte's two nibbles looked up in a table of their
    /// bit counts with PSHUFB, the bytes summed into each 64-bit lane with
    /// PSADBW.
    #[target_feature(enable = "avx2")]
    pub unsafe fn ones_avx2(v: &[u64]) -> u64 {
        // SAFETY: each load reads four values the loop guard checks are in
        // bounds.
        unsafe {
            let table = _mm256_setr_epi8(0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4, 0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4);
            let low = _mm256_set1_epi8(0x0F);
            let mut acc = _mm256_setzero_si256();
            let mut i = 0;
            while i + 4 <= v.len() {
                let x = _mm256_loadu_si256(v.as_ptr().add(i).cast());
                let bytes = _mm256_add_epi8(_mm256_shuffle_epi8(table, _mm256_and_si256(x, low)), _mm256_shuffle_epi8(table, _mm256_and_si256(_mm256_srli_epi32::<4>(x), low)));
                acc = _mm256_add_epi64(acc, _mm256_sad_epu8(bytes, _mm256_setzero_si256()));
                i += 4;
            }
            let mut lanes = [0u64; 4];
            _mm256_storeu_si256(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().sum::<u64>() + super::count_ones_scalar(&v[i..])
        }
    }

    /// Mula's count at 512 bits, which needs AVX-512BW's byte shuffle.
    #[target_feature(enable = "avx512f,avx512bw")]
    pub unsafe fn ones_avx512bw(v: &[u64]) -> u64 {
        // SAFETY: each load reads eight values the loop guard checks are in
        // bounds.
        unsafe {
            let table = _mm512_broadcast_i32x4(_mm_setr_epi8(0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4));
            let low = _mm512_set1_epi8(0x0F);
            let mut acc = _mm512_setzero_si512();
            let mut i = 0;
            while i + 8 <= v.len() {
                let x = _mm512_loadu_si512(v.as_ptr().add(i).cast());
                let bytes = _mm512_add_epi8(_mm512_shuffle_epi8(table, _mm512_and_si512(x, low)), _mm512_shuffle_epi8(table, _mm512_and_si512(_mm512_srli_epi32::<4>(x), low)));
                acc = _mm512_add_epi64(acc, _mm512_sad_epu8(bytes, _mm512_setzero_si512()));
                i += 8;
            }
            let mut lanes = [0u64; 8];
            _mm512_storeu_si512(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().sum::<u64>() + super::count_ones_scalar(&v[i..])
        }
    }

    /// AVX-512 VPOPCNTDQ: one instruction counts eight values.
    #[target_feature(enable = "avx512f,avx512vpopcntdq")]
    pub unsafe fn ones_vpopcntq(v: &[u64]) -> u64 {
        // SAFETY: as ones_avx512bw's.
        unsafe {
            let mut acc = _mm512_setzero_si512();
            let mut i = 0;
            while i + 8 <= v.len() {
                acc = _mm512_add_epi64(acc, _mm512_popcnt_epi64(_mm512_loadu_si512(v.as_ptr().add(i).cast())));
                i += 8;
            }
            let mut lanes = [0u64; 8];
            _mm512_storeu_si512(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().sum::<u64>() + super::count_ones_scalar(&v[i..])
        }
    }

    /// x86-64-v2: the smallest or, with `largest`, the largest of a
    /// non-empty slice, through SSE4.2's 64-bit compare and SSE4.1's
    /// blend.
    #[target_feature(enable = "sse4.1,sse4.2")]
    pub unsafe fn pick_sse42(v: &[i64], largest: bool) -> i64 {
        // SAFETY: each load reads two values the loop guard checks are in bounds.
        unsafe {
            let mut best = _mm_set1_epi64x(v[0]);
            let mut i = 0;
            while i + 2 <= v.len() {
                let x = _mm_loadu_si128(v.as_ptr().add(i).cast());
                // Lanes where x is greater: a maximum takes x there and a
                // minimum leaves its best there.
                let gt = _mm_cmpgt_epi64(x, best);
                best = if largest { _mm_blendv_epi8(best, x, gt) } else { _mm_blendv_epi8(x, best, gt) };
                i += 2;
            }
            let mut lanes = [0i64; 2];
            _mm_storeu_si128(lanes.as_mut_ptr().cast(), best);
            fold(lanes.iter().chain(&v[i..]), largest)
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn pick_avx2(v: &[i64], largest: bool) -> i64 {
        // SAFETY: each load reads four values the loop guard checks are in
        // bounds.
        unsafe {
            let mut best = _mm256_set1_epi64x(v[0]);
            let mut i = 0;
            while i + 4 <= v.len() {
                let x = _mm256_loadu_si256(v.as_ptr().add(i).cast());
                let gt = _mm256_cmpgt_epi64(x, best);
                best = if largest { _mm256_blendv_epi8(best, x, gt) } else { _mm256_blendv_epi8(x, best, gt) };
                i += 4;
            }
            let mut lanes = [0i64; 4];
            _mm256_storeu_si256(lanes.as_mut_ptr().cast(), best);
            fold(lanes.iter().chain(&v[i..]), largest)
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn pick_avx512(v: &[i64], largest: bool) -> i64 {
        // SAFETY: each load reads eight values the loop guard checks are in
        // bounds.
        unsafe {
            let mut best = _mm512_set1_epi64(v[0]);
            let mut i = 0;
            while i + 8 <= v.len() {
                let x = _mm512_loadu_si512(v.as_ptr().add(i).cast());
                best = if largest { _mm512_max_epi64(best, x) } else { _mm512_min_epi64(best, x) };
                i += 8;
            }
            let mut lanes = [0i64; 8];
            _mm512_storeu_si512(lanes.as_mut_ptr().cast(), best);
            fold(lanes.iter().chain(&v[i..]), largest)
        }
    }

    fn fold<'a>(values: impl Iterator<Item = &'a i64>, largest: bool) -> i64 {
        if largest { values.copied().fold(i64::MIN, i64::max) } else { values.copied().fold(i64::MAX, i64::min) }
    }

    /// The x86-64 baseline. The three slices have one length.
    pub unsafe fn bitwise_sse2(op: Op, a: &[u64], b: &[u64], out: &mut [u64]) {
        // SAFETY: each load and store covers two values the loop guard
        // checks are in bounds of all three slices.
        unsafe {
            let mut i = 0;
            while i + 2 <= out.len() {
                let x = _mm_loadu_si128(a.as_ptr().add(i).cast());
                let y = _mm_loadu_si128(b.as_ptr().add(i).cast());
                let r = match op {
                    Op::And => _mm_and_si128(x, y),
                    Op::Or => _mm_or_si128(x, y),
                    Op::AndNot => _mm_andnot_si128(y, x),
                };
                _mm_storeu_si128(out.as_mut_ptr().add(i).cast(), r);
                i += 2;
            }
            super::bitwise_scalar(op, &a[i..], &b[i..], &mut out[i..]);
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn bitwise_avx2(op: Op, a: &[u64], b: &[u64], out: &mut [u64]) {
        // SAFETY: as bitwise_sse2's, four values at a time.
        unsafe {
            let mut i = 0;
            while i + 4 <= out.len() {
                let x = _mm256_loadu_si256(a.as_ptr().add(i).cast());
                let y = _mm256_loadu_si256(b.as_ptr().add(i).cast());
                let r = match op {
                    Op::And => _mm256_and_si256(x, y),
                    Op::Or => _mm256_or_si256(x, y),
                    Op::AndNot => _mm256_andnot_si256(y, x),
                };
                _mm256_storeu_si256(out.as_mut_ptr().add(i).cast(), r);
                i += 4;
            }
            super::bitwise_scalar(op, &a[i..], &b[i..], &mut out[i..]);
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn bitwise_avx512(op: Op, a: &[u64], b: &[u64], out: &mut [u64]) {
        // SAFETY: as bitwise_sse2's, eight values at a time.
        unsafe {
            let mut i = 0;
            while i + 8 <= out.len() {
                let x = _mm512_loadu_si512(a.as_ptr().add(i).cast());
                let y = _mm512_loadu_si512(b.as_ptr().add(i).cast());
                let r = match op {
                    Op::And => _mm512_and_si512(x, y),
                    Op::Or => _mm512_or_si512(x, y),
                    Op::AndNot => _mm512_andnot_si512(y, x),
                };
                _mm512_storeu_si512(out.as_mut_ptr().add(i).cast(), r);
                i += 8;
            }
            super::bitwise_scalar(op, &a[i..], &b[i..], &mut out[i..]);
        }
    }
}

#[cfg(any(target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon")))]
mod neon {
    use super::Op;
    use crate::simd::arch::*;

    #[cfg_attr(target_arch = "arm", target_feature(enable = "neon,v7"))]
    pub unsafe fn sum(v: &[u64]) -> u64 {
        // SAFETY: each load reads two values the loop guard checks are in bounds.
        unsafe {
            let mut acc = vdupq_n_u64(0);
            let mut i = 0;
            while i + 2 <= v.len() {
                acc = vaddq_u64(acc, vld1q_u64(v.as_ptr().add(i)));
                i += 2;
            }
            [vgetq_lane_u64::<0>(acc), vgetq_lane_u64::<1>(acc)].iter().chain(&v[i..]).fold(0u64, |a, &x| a.wrapping_add(x))
        }
    }

    #[cfg_attr(target_arch = "arm", target_feature(enable = "neon,v7"))]
    pub unsafe fn xor(v: &[u64]) -> u64 {
        // SAFETY: as sum's.
        unsafe {
            let mut acc = vdupq_n_u64(0);
            let mut i = 0;
            while i + 2 <= v.len() {
                acc = veorq_u64(acc, vld1q_u64(v.as_ptr().add(i)));
                i += 2;
            }
            [vgetq_lane_u64::<0>(acc), vgetq_lane_u64::<1>(acc)].iter().chain(&v[i..]).fold(0u64, |a, &x| a ^ x)
        }
    }

    /// CNT counts each byte's bits, and the sixteen byte counts of two
    /// values are summed each round.
    #[cfg_attr(target_arch = "arm", target_feature(enable = "neon,v7"))]
    pub unsafe fn ones(v: &[u64]) -> u64 {
        // SAFETY: as sum's.
        unsafe {
            let mut total = 0u64;
            let mut i = 0;
            while i + 2 <= v.len() {
                total += u64::from(vaddlvq_u8(vcntq_u8(vreinterpretq_u8_u64(vld1q_u64(v.as_ptr().add(i))))));
                i += 2;
            }
            total + super::count_ones_scalar(&v[i..])
        }
    }

    /// The smallest or, with `largest`, the largest of a non-empty slice:
    /// NEON has no 64-bit minimum, so a compare chooses each lane.
    #[cfg(target_arch = "aarch64")]
    pub unsafe fn pick(v: &[i64], largest: bool) -> i64 {
        // SAFETY: each load reads two values the loop guard checks are in bounds.
        unsafe {
            let mut best = vdupq_n_s64(v[0]);
            let mut i = 0;
            while i + 2 <= v.len() {
                let x = vld1q_s64(v.as_ptr().add(i));
                let take = if largest { vcgtq_s64(x, best) } else { vcltq_s64(x, best) };
                best = vbslq_s64(take, x, best);
                i += 2;
            }
            let lanes = [vgetq_lane_s64::<0>(best), vgetq_lane_s64::<1>(best)];
            let rest = lanes.iter().chain(&v[i..]).copied();
            if largest { rest.fold(i64::MIN, i64::max) } else { rest.fold(i64::MAX, i64::min) }
        }
    }

    /// The three slices have one length.
    #[cfg_attr(target_arch = "arm", target_feature(enable = "neon,v7"))]
    pub unsafe fn bitwise(op: Op, a: &[u64], b: &[u64], out: &mut [u64]) {
        // SAFETY: each load and store covers two values the loop guard
        // checks are in bounds of all three slices.
        unsafe {
            let mut i = 0;
            while i + 2 <= out.len() {
                let x = vld1q_u64(a.as_ptr().add(i));
                let y = vld1q_u64(b.as_ptr().add(i));
                let r = match op {
                    Op::And => vandq_u64(x, y),
                    Op::Or => vorrq_u64(x, y),
                    Op::AndNot => vbicq_u64(x, y),
                };
                vst1q_u64(out.as_mut_ptr().add(i), r);
                i += 2;
            }
            super::bitwise_scalar(op, &a[i..], &b[i..], &mut out[i..]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sum, XOR, bits set, minimum and maximum of one slice.
    type Reduced = (u64, u64, u64, Option<i64>, Option<i64>);
    type Reduce = Box<dyn Fn(&[u64], &[i64]) -> Reduced>;
    type Combine = Box<dyn Fn(Op, &[u64], &[u64], &mut [u64])>;

    /// A minimum or maximum body's result, computed as [`min_i64`] and
    /// [`max_i64`] compute it, only over a slice that is not empty.
    fn picked(s: &[i64], largest: bool, body: &dyn Fn(&[i64], bool) -> i64) -> Option<i64> {
        if s.is_empty() { None } else { Some(body(s, largest)) }
    }

    fn scalar(v: &[u64], s: &[i64]) -> Reduced {
        (sum_u64_scalar(v), xor_u64_scalar(v), count_ones_scalar(v), min_i64_scalar(s), max_i64_scalar(s))
    }

    /// Slices of every length to 70 and a few longer, of random words,
    /// words at and near the signed limits, and runs of one value.
    fn cases() -> Vec<Vec<u64>> {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut out: Vec<Vec<u64>> = (0..70).map(|n| (0..n).map(|_| next()).collect()).collect();
        for n in [255usize, 256, 1031, 4099] {
            out.push((0..n).map(|_| next()).collect());
        }
        let edges = [0u64, 1, u64::MAX, i64::MAX as u64, i64::MIN as u64, (i64::MIN + 1) as u64, (i64::MAX - 1) as u64];
        out.push((0..67).map(|i| edges[i % edges.len()]).collect());
        out.push(vec![u64::MAX; 33]);
        out.push(vec![i64::MIN as u64; 17]);
        out
    }

    /// The bodies this CPU runs, by tier, whatever PWRS_CPU_MAX says; a
    /// tier it lacks is printed as skipped.
    fn bodies() -> Vec<(&'static str, Reduce, Combine)> {
        let mut out: Vec<(&'static str, Reduce, Combine)> = Vec::new();
        // SAFETY in each closure below: a tier is listed only when this CPU
        // offers its instructions.
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            out.push((
                "x86-64",
                Box::new(|v, s| unsafe { (x86::sum_sse2(v), x86::xor_sse2(v), count_ones_scalar(v), picked(s, false, &pick_scalar), picked(s, true, &pick_scalar)) }),
                Box::new(|op, a, b, o| unsafe { x86::bitwise_sse2(op, a, b, o) }),
            ));
            if detected(Isa::Popcnt) && detected(Isa::Sse42) {
                let pick = |s: &[i64], l: bool| unsafe { x86::pick_sse42(s, l) };
                out.push((
                    "x86-64-v2",
                    Box::new(move |v, s| unsafe { (x86::sum_sse2(v), x86::xor_sse2(v), x86::ones_popcnt(v), picked(s, false, &pick), picked(s, true, &pick)) }),
                    Box::new(|op, a, b, o| unsafe { x86::bitwise_sse2(op, a, b, o) }),
                ));
            } else {
                eprintln!("INTS SKIPPED x86-64-v2: this CPU lacks popcnt or sse4.2");
            }
            if detected(Isa::Avx2) {
                let pick = |s: &[i64], l: bool| unsafe { x86::pick_avx2(s, l) };
                out.push((
                    "x86-64-v3",
                    Box::new(move |v, s| unsafe { (x86::sum_avx2(v), x86::xor_avx2(v), x86::ones_avx2(v), picked(s, false, &pick), picked(s, true, &pick)) }),
                    Box::new(|op, a, b, o| unsafe { x86::bitwise_avx2(op, a, b, o) }),
                ));
            } else {
                eprintln!("INTS SKIPPED x86-64-v3: this CPU lacks avx2");
            }
            if detected(Isa::Avx512f) && detected(Isa::Avx512bw) {
                let pick = |s: &[i64], l: bool| unsafe { x86::pick_avx512(s, l) };
                out.push((
                    "x86-64-v4",
                    Box::new(move |v, s| unsafe { (x86::sum_avx512(v), x86::xor_avx512(v), x86::ones_avx512bw(v), picked(s, false, &pick), picked(s, true, &pick)) }),
                    Box::new(|op, a, b, o| unsafe { x86::bitwise_avx512(op, a, b, o) }),
                ));
            } else {
                eprintln!("INTS SKIPPED x86-64-v4: this CPU lacks avx512f or avx512bw");
            }
            if detected(Isa::Avx512f) && detected(Isa::Avx512vpopcntdq) {
                let pick = |s: &[i64], l: bool| unsafe { x86::pick_avx512(s, l) };
                out.push((
                    "avx512vpopcntdq",
                    Box::new(move |v, s| unsafe { (x86::sum_avx512(v), x86::xor_avx512(v), x86::ones_vpopcntq(v), picked(s, false, &pick), picked(s, true, &pick)) }),
                    Box::new(|op, a, b, o| unsafe { x86::bitwise_avx512(op, a, b, o) }),
                ));
            } else {
                eprintln!("INTS SKIPPED avx512vpopcntdq: this CPU lacks it");
            }
        }
        #[cfg(any(target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon")))]
        if crate::simd::arch::neon_detected() {
            #[cfg(target_arch = "aarch64")]
            let pick = |s: &[i64], l: bool| unsafe { neon::pick(s, l) };
            #[cfg(target_arch = "arm")]
            let pick = pick_scalar;
            out.push((
                "neon",
                Box::new(move |v, s| unsafe { (neon::sum(v), neon::xor(v), neon::ones(v), picked(s, false, &pick), picked(s, true, &pick)) }),
                Box::new(|op, a, b, o| unsafe { neon::bitwise(op, a, b, o) }),
            ));
        } else {
            eprintln!("INTS SKIPPED neon: this CPU lacks it");
        }
        out
    }

    #[test]
    fn every_tier_reduces_to_what_the_scalar_form_gives() {
        let cases = cases();
        let bodies = bodies();
        assert!(!bodies.is_empty() || !cfg!(any(target_arch = "x86_64", target_arch = "aarch64")), "no body ran");
        for (tier, reduce, _) in &bodies {
            eprintln!("INTS RUNNING reductions at {tier} over {} slices", cases.len());
            for v in &cases {
                let s: Vec<i64> = v.iter().map(|&x| x as i64).collect();
                assert_eq!(reduce(v, &s), scalar(v, &s), "{tier}: sum, xor, ones, min, max of {} values", v.len());
            }
        }
        for v in &cases {
            let s: Vec<i64> = v.iter().map(|&x| x as i64).collect();
            assert_eq!((sum_u64(v), xor_u64(v), count_ones(v), min_i64(&s), max_i64(&s)), scalar(v, &s));
        }
    }

    #[test]
    fn every_tier_combines_bitmaps_as_the_scalar_form_does() {
        let cases = cases();
        let bodies = bodies();
        for (tier, _, combine) in &bodies {
            eprintln!("INTS RUNNING bitwise at {tier}");
            for pair in cases.windows(2) {
                let n = pair[0].len().min(pair[1].len());
                let (a, b) = (&pair[1][..n], &pair[0][..n]);
                for op in [Op::And, Op::Or, Op::AndNot] {
                    let mut want = vec![0u64; n];
                    bitwise_scalar(op, a, b, &mut want);
                    let mut got = vec![0u64; n];
                    combine(op, a, b, &mut got);
                    assert_eq!(got, want, "{tier}: {op:?} of {n} values");
                }
            }
        }
        let (a, b) = ([0b1100u64, u64::MAX, 7], [0b1010u64, 1]);
        let mut out = [9u64; 3];
        and_not_u64(&a, &b, &mut out);
        assert_eq!(out, [0b0100, u64::MAX - 1, 9]);
    }
}
