//! Compensated sums and dot products of `f64`, in the floats module's
//! order: element `i` into running sum `i % 8`, the eight combined by
//! halving. Each running sum records beside it the exact rounding error of
//! every addition into it (Knuth's TwoSum), and a dot product each
//! product's own rounding error, which a fused multiply-add gives exactly,
//! so the result is nearly as accurate as one computed with twice the
//! precision and rounded once (Ogita, Rump and Oishi's Sum2 and Dot2).
//!
//! The running sums are [`super::sum_f64`]'s and [`super::dot_f64`]'s
//! own, and their result is returned with the errors added; where that
//! result is an infinity or a NaN, it is returned as it is. Every tier
//! returns the scalar form's bits, for every input, with the payload of a
//! NaN arithmetic makes left unspecified, as Rust leaves it.
//!
//! The sums run on SSE2, AVX2, AVX-512 and NEON. The dot products run
//! where the CPU has a fused multiply-add, from x86-64-v3 and on every
//! AArch64 CPU, and below x86-64-v3 the scalar form runs, whose
//! [`f64::mul_add`] rounds once as the instruction does.

use super::F64_LANES;

/// The compensated sum of `values` in this module's order; 0.0 for none.
pub fn sum_f64(values: &[f64]) -> f64 {
    tiered::sum_f64(values)
}

/// The scalar form of [`sum_f64`], in this module's order: the result
/// every tier gives.
pub fn sum_f64_scalar(values: &[f64]) -> f64 {
    let (mut sums, mut errs) = ([0.0f64; F64_LANES], [0.0f64; F64_LANES]);
    for (i, &v) in values.iter().enumerate() {
        let j = i % F64_LANES;
        let (s, e) = two_sum(sums[j], v);
        sums[j] = s;
        errs[j] += e;
    }
    combine(sums, errs)
}

/// The compensated sum of `a[i] * b[i]` over the shorter slice, in this
/// module's order.
pub fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    tiered::dot_f64(&a[..n], &b[..n])
}

/// The scalar form of [`dot_f64`], in this module's order: the result
/// every tier gives.
pub fn dot_f64_scalar(a: &[f64], b: &[f64]) -> f64 {
    let (mut sums, mut errs) = ([0.0f64; F64_LANES], [0.0f64; F64_LANES]);
    for (i, (&x, &y)) in a.iter().zip(b).enumerate() {
        let j = i % F64_LANES;
        let p = x * y;
        let (s, q) = two_sum(sums[j], p);
        sums[j] = s;
        errs[j] += q + x.mul_add(y, -p);
    }
    combine(sums, errs)
}

/// `a + b` and its rounding error, exactly where the sum does not
/// overflow: Knuth's TwoSum, which needs no ordering of `a` and `b`.
#[inline(always)]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// The running sums combined by halving, each pair's sum with its own
/// rounding error, and the errors added to the result where it is finite.
fn combine(mut sums: [f64; F64_LANES], mut errs: [f64; F64_LANES]) -> f64 {
    let mut n = F64_LANES;
    while n > 1 {
        n /= 2;
        for i in 0..n {
            let (s, e) = two_sum(sums[i], sums[i + n]);
            sums[i] = s;
            errs[i] = (errs[i] + errs[i + n]) + e;
        }
    }
    if sums[0].is_finite() { sums[0] + errs[0] } else { sums[0] }
}

/// The elements past the last whole block added into their running sums.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn tail_sum(sums: &mut [f64; F64_LANES], errs: &mut [f64; F64_LANES], rest: &[f64]) {
    for (j, &v) in rest.iter().enumerate() {
        let (s, e) = two_sum(sums[j], v);
        sums[j] = s;
        errs[j] += e;
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn tail_dot(sums: &mut [f64; F64_LANES], errs: &mut [f64; F64_LANES], a: &[f64], b: &[f64]) {
    for (j, (&x, &y)) in a.iter().zip(b).enumerate() {
        let p = x * y;
        let (s, q) = two_sum(sums[j], p);
        sums[j] = s;
        errs[j] += q + x.mul_add(y, -p);
    }
}

#[cfg(target_arch = "x86_64")]
mod tiered {
    use super::x86;
    use crate::cpu::{level, Level};

    // A level is reached only when the CPU and the operating system offer
    // every extension of it and of each level below it; SSE2 is in every
    // x86-64 CPU, and FMA first in x86-64-v3.
    pub fn sum_f64(v: &[f64]) -> f64 {
        match level() {
            Level::V4 => unsafe { x86::sum_avx512(v) },
            Level::V3 => unsafe { x86::sum_avx2(v) },
            _baseline => unsafe { x86::sum_sse2(v) },
        }
    }

    pub fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
        match level() {
            Level::V4 => unsafe { x86::dot_avx512(a, b) },
            Level::V3 => unsafe { x86::dot_avx2(a, b) },
            _below_v3 => super::dot_f64_scalar(a, b),
        }
    }
}

// NEON, with its fused multiply-add, is in every AArch64 CPU.
#[cfg(target_arch = "aarch64")]
mod tiered {
    use super::neon;

    pub fn sum_f64(v: &[f64]) -> f64 {
        unsafe { neon::sum(v) }
    }

    pub fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
        unsafe { neon::dot(a, b) }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod tiered {
    pub fn sum_f64(v: &[f64]) -> f64 {
        super::sum_f64_scalar(v)
    }

    pub fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
        super::dot_f64_scalar(a, b)
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{combine, tail_dot, tail_sum, F64_LANES};
    use core::arch::x86_64::*;

    // In each body, each block's loads read eight values the loop guard
    // checks are in bounds, the stores write the eight-lane arrays, and the dot
    // bodies take slices of one length. A product's rounding error is
    // `x * y - p` with one rounding, `fmsub`, which is the scalar form's
    // `x.mul_add(y, -p)`: IEEE subtraction is the addition of the negation.

    /// TwoSum on each lane, as the scalar form's.
    #[inline(always)]
    unsafe fn two_sum_sse2(a: __m128d, b: __m128d) -> (__m128d, __m128d) {
        // SAFETY: SSE2 is in every x86-64 CPU.
        unsafe {
            let s = _mm_add_pd(a, b);
            let bb = _mm_sub_pd(s, a);
            (s, _mm_add_pd(_mm_sub_pd(a, _mm_sub_pd(s, bb)), _mm_sub_pd(b, bb)))
        }
    }

    #[target_feature(enable = "avx")]
    #[inline]
    unsafe fn two_sum_avx(a: __m256d, b: __m256d) -> (__m256d, __m256d) {
        let s = _mm256_add_pd(a, b);
        let bb = _mm256_sub_pd(s, a);
        (s, _mm256_add_pd(_mm256_sub_pd(a, _mm256_sub_pd(s, bb)), _mm256_sub_pd(b, bb)))
    }

    #[target_feature(enable = "avx512f")]
    #[inline]
    unsafe fn two_sum_avx512(a: __m512d, b: __m512d) -> (__m512d, __m512d) {
        let s = _mm512_add_pd(a, b);
        let bb = _mm512_sub_pd(s, a);
        (s, _mm512_add_pd(_mm512_sub_pd(a, _mm512_sub_pd(s, bb)), _mm512_sub_pd(b, bb)))
    }

    /// The x86-64 baseline: four registers of two running sums, and four
    /// of their errors.
    pub unsafe fn sum_sse2(v: &[f64]) -> f64 {
        unsafe {
            let (mut sums, mut errs) = ([_mm_setzero_pd(); 4], [_mm_setzero_pd(); 4]);
            let blocks = v.len() / F64_LANES;
            for k in 0..blocks {
                let p = v.as_ptr().add(k * F64_LANES);
                for (j, (s, e)) in sums.iter_mut().zip(errs.iter_mut()).enumerate() {
                    let (sum, err) = two_sum_sse2(*s, _mm_loadu_pd(p.add(2 * j)));
                    *s = sum;
                    *e = _mm_add_pd(*e, err);
                }
            }
            let (mut s_lanes, mut e_lanes) = ([0.0f64; F64_LANES], [0.0f64; F64_LANES]);
            for j in 0..4 {
                _mm_storeu_pd(s_lanes.as_mut_ptr().add(2 * j), sums[j]);
                _mm_storeu_pd(e_lanes.as_mut_ptr().add(2 * j), errs[j]);
            }
            tail_sum(&mut s_lanes, &mut e_lanes, &v[blocks * F64_LANES..]);
            combine(s_lanes, e_lanes)
        }
    }

    /// Two registers of four running sums, and two of their errors.
    #[target_feature(enable = "avx2")]
    pub unsafe fn sum_avx2(v: &[f64]) -> f64 {
        unsafe {
            let (mut sums, mut errs) = ([_mm256_setzero_pd(); 2], [_mm256_setzero_pd(); 2]);
            let blocks = v.len() / F64_LANES;
            for k in 0..blocks {
                let p = v.as_ptr().add(k * F64_LANES);
                for (j, (s, e)) in sums.iter_mut().zip(errs.iter_mut()).enumerate() {
                    let (sum, err) = two_sum_avx(*s, _mm256_loadu_pd(p.add(4 * j)));
                    *s = sum;
                    *e = _mm256_add_pd(*e, err);
                }
            }
            let (mut s_lanes, mut e_lanes) = ([0.0f64; F64_LANES], [0.0f64; F64_LANES]);
            for j in 0..2 {
                _mm256_storeu_pd(s_lanes.as_mut_ptr().add(4 * j), sums[j]);
                _mm256_storeu_pd(e_lanes.as_mut_ptr().add(4 * j), errs[j]);
            }
            tail_sum(&mut s_lanes, &mut e_lanes, &v[blocks * F64_LANES..]);
            combine(s_lanes, e_lanes)
        }
    }

    /// One register of eight running sums, and one of their errors.
    #[target_feature(enable = "avx512f")]
    pub unsafe fn sum_avx512(v: &[f64]) -> f64 {
        unsafe {
            let (mut sums, mut errs) = (_mm512_setzero_pd(), _mm512_setzero_pd());
            let blocks = v.len() / F64_LANES;
            for k in 0..blocks {
                let (sum, err) = two_sum_avx512(sums, _mm512_loadu_pd(v.as_ptr().add(k * F64_LANES)));
                sums = sum;
                errs = _mm512_add_pd(errs, err);
            }
            let (mut s_lanes, mut e_lanes) = ([0.0f64; F64_LANES], [0.0f64; F64_LANES]);
            _mm512_storeu_pd(s_lanes.as_mut_ptr(), sums);
            _mm512_storeu_pd(e_lanes.as_mut_ptr(), errs);
            tail_sum(&mut s_lanes, &mut e_lanes, &v[blocks * F64_LANES..]);
            combine(s_lanes, e_lanes)
        }
    }

    #[target_feature(enable = "avx2,fma")]
    pub unsafe fn dot_avx2(a: &[f64], b: &[f64]) -> f64 {
        unsafe {
            let (mut sums, mut errs) = ([_mm256_setzero_pd(); 2], [_mm256_setzero_pd(); 2]);
            let blocks = a.len() / F64_LANES;
            for k in 0..blocks {
                let (pa, pb) = (a.as_ptr().add(k * F64_LANES), b.as_ptr().add(k * F64_LANES));
                for (j, (s, e)) in sums.iter_mut().zip(errs.iter_mut()).enumerate() {
                    let (x, y) = (_mm256_loadu_pd(pa.add(4 * j)), _mm256_loadu_pd(pb.add(4 * j)));
                    let p = _mm256_mul_pd(x, y);
                    let (sum, q) = two_sum_avx(*s, p);
                    *s = sum;
                    *e = _mm256_add_pd(*e, _mm256_add_pd(q, _mm256_fmsub_pd(x, y, p)));
                }
            }
            let (mut s_lanes, mut e_lanes) = ([0.0f64; F64_LANES], [0.0f64; F64_LANES]);
            for j in 0..2 {
                _mm256_storeu_pd(s_lanes.as_mut_ptr().add(4 * j), sums[j]);
                _mm256_storeu_pd(e_lanes.as_mut_ptr().add(4 * j), errs[j]);
            }
            tail_dot(&mut s_lanes, &mut e_lanes, &a[blocks * F64_LANES..], &b[blocks * F64_LANES..]);
            combine(s_lanes, e_lanes)
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn dot_avx512(a: &[f64], b: &[f64]) -> f64 {
        unsafe {
            let (mut sums, mut errs) = (_mm512_setzero_pd(), _mm512_setzero_pd());
            let blocks = a.len() / F64_LANES;
            for k in 0..blocks {
                let (x, y) = (_mm512_loadu_pd(a.as_ptr().add(k * F64_LANES)), _mm512_loadu_pd(b.as_ptr().add(k * F64_LANES)));
                let p = _mm512_mul_pd(x, y);
                let (sum, q) = two_sum_avx512(sums, p);
                sums = sum;
                errs = _mm512_add_pd(errs, _mm512_add_pd(q, _mm512_fmsub_pd(x, y, p)));
            }
            let (mut s_lanes, mut e_lanes) = ([0.0f64; F64_LANES], [0.0f64; F64_LANES]);
            _mm512_storeu_pd(s_lanes.as_mut_ptr(), sums);
            _mm512_storeu_pd(e_lanes.as_mut_ptr(), errs);
            tail_dot(&mut s_lanes, &mut e_lanes, &a[blocks * F64_LANES..], &b[blocks * F64_LANES..]);
            combine(s_lanes, e_lanes)
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::{combine, tail_dot, tail_sum, F64_LANES};
    use core::arch::aarch64::*;

    // In each body, each block's loads read eight values the loop guard
    // checks are in bounds, the stores write the eight-lane arrays, and the dot
    // body takes slices of one length.

    /// TwoSum on each lane, as the scalar form's.
    #[inline(always)]
    unsafe fn two_sum(a: float64x2_t, b: float64x2_t) -> (float64x2_t, float64x2_t) {
        // SAFETY: NEON is in every AArch64 CPU.
        unsafe {
            let s = vaddq_f64(a, b);
            let bb = vsubq_f64(s, a);
            (s, vaddq_f64(vsubq_f64(a, vsubq_f64(s, bb)), vsubq_f64(b, bb)))
        }
    }

    /// Four registers of two running sums, and four of their errors.
    pub unsafe fn sum(v: &[f64]) -> f64 {
        unsafe {
            let (mut sums, mut errs) = ([vdupq_n_f64(0.0); 4], [vdupq_n_f64(0.0); 4]);
            let blocks = v.len() / F64_LANES;
            for k in 0..blocks {
                let p = v.as_ptr().add(k * F64_LANES);
                for (j, (s, e)) in sums.iter_mut().zip(errs.iter_mut()).enumerate() {
                    let (sum, err) = two_sum(*s, vld1q_f64(p.add(2 * j)));
                    *s = sum;
                    *e = vaddq_f64(*e, err);
                }
            }
            let (mut s_lanes, mut e_lanes) = ([0.0f64; F64_LANES], [0.0f64; F64_LANES]);
            for j in 0..4 {
                vst1q_f64(s_lanes.as_mut_ptr().add(2 * j), sums[j]);
                vst1q_f64(e_lanes.as_mut_ptr().add(2 * j), errs[j]);
            }
            tail_sum(&mut s_lanes, &mut e_lanes, &v[blocks * F64_LANES..]);
            combine(s_lanes, e_lanes)
        }
    }

    /// A product's rounding error is FMLA's `-p + x * y`, with one
    /// rounding, which is the scalar form's `x.mul_add(y, -p)`.
    pub unsafe fn dot(a: &[f64], b: &[f64]) -> f64 {
        unsafe {
            let (mut sums, mut errs) = ([vdupq_n_f64(0.0); 4], [vdupq_n_f64(0.0); 4]);
            let blocks = a.len() / F64_LANES;
            for k in 0..blocks {
                let (pa, pb) = (a.as_ptr().add(k * F64_LANES), b.as_ptr().add(k * F64_LANES));
                for (j, (s, e)) in sums.iter_mut().zip(errs.iter_mut()).enumerate() {
                    let (x, y) = (vld1q_f64(pa.add(2 * j)), vld1q_f64(pb.add(2 * j)));
                    let p = vmulq_f64(x, y);
                    let (sum, q) = two_sum(*s, p);
                    *s = sum;
                    *e = vaddq_f64(*e, vaddq_f64(q, vfmaq_f64(vnegq_f64(p), x, y)));
                }
            }
            let (mut s_lanes, mut e_lanes) = ([0.0f64; F64_LANES], [0.0f64; F64_LANES]);
            for j in 0..4 {
                vst1q_f64(s_lanes.as_mut_ptr().add(2 * j), sums[j]);
                vst1q_f64(e_lanes.as_mut_ptr().add(2 * j), errs[j]);
            }
            tail_dot(&mut s_lanes, &mut e_lanes, &a[blocks * F64_LANES..], &b[blocks * F64_LANES..]);
            combine(s_lanes, e_lanes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Sum = unsafe fn(&[f64]) -> f64;
    type Dot = unsafe fn(&[f64], &[f64]) -> f64;

    /// The same bits, or a NaN on both sides: Rust leaves a computed NaN's
    /// payload unspecified.
    fn same(got: f64, want: f64) -> bool {
        got.to_bits() == want.to_bits() || (got.is_nan() && want.is_nan())
    }

    /// The sum bodies this CPU runs, by tier, whatever `PWRS_CPU_MAX` says;
    /// a tier it lacks is printed as skipped.
    #[cfg_attr(not(any(target_arch = "x86_64", target_arch = "aarch64")), allow(unused_mut))]
    #[cfg_attr(target_arch = "aarch64", allow(clippy::vec_init_then_push))]
    fn sum_bodies() -> Vec<(&'static str, Sum)> {
        let mut out: Vec<(&'static str, Sum)> = Vec::new();
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            out.push(("x86-64", x86::sum_sse2));
            if detected(Isa::Avx2) {
                out.push(("x86-64-v3", x86::sum_avx2));
            } else {
                eprintln!("COMPENSATED SKIPPED sums at x86-64-v3: this CPU lacks avx2");
            }
            if detected(Isa::Avx512f) {
                out.push(("x86-64-v4", x86::sum_avx512));
            } else {
                eprintln!("COMPENSATED SKIPPED sums at x86-64-v4: this CPU lacks avx512f");
            }
        }
        #[cfg(target_arch = "aarch64")]
        out.push(("neon", neon::sum));
        out
    }

    /// The dot bodies this CPU runs, as [`sum_bodies`]'s.
    #[cfg_attr(not(any(target_arch = "x86_64", target_arch = "aarch64")), allow(unused_mut))]
    #[cfg_attr(target_arch = "aarch64", allow(clippy::vec_init_then_push))]
    fn dot_bodies() -> Vec<(&'static str, Dot)> {
        let mut out: Vec<(&'static str, Dot)> = Vec::new();
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            if detected(Isa::Avx2) && detected(Isa::Fma) {
                out.push(("x86-64-v3", x86::dot_avx2));
            } else {
                eprintln!("COMPENSATED SKIPPED dots at x86-64-v3: this CPU lacks avx2 or fma");
            }
            if detected(Isa::Avx512f) {
                out.push(("x86-64-v4", x86::dot_avx512));
            } else {
                eprintln!("COMPENSATED SKIPPED dots at x86-64-v4: this CPU lacks avx512f");
            }
        }
        #[cfg(target_arch = "aarch64")]
        out.push(("neon", neon::dot));
        out
    }

    #[test]
    fn every_tier_returns_the_scalar_forms_bits() {
        let cases = super::super::tests::cases();
        let sums = sum_bodies();
        assert!(!sums.is_empty() || !cfg!(any(target_arch = "x86_64", target_arch = "aarch64")), "no sum body ran");
        for (tier, body) in &sums {
            eprintln!("COMPENSATED RUNNING sums at {tier} over {} cases", cases.len());
            for (a, ..) in &cases {
                // SAFETY: sum_bodies() lists only bodies this CPU runs.
                let got = unsafe { body(a) };
                let want = sum_f64_scalar(a);
                assert!(same(got, want), "{tier}: {} values summed to {got:e} ({:x}), the scalar form {want:e} ({:x})", a.len(), got.to_bits(), want.to_bits());
            }
        }
        for (tier, body) in &dot_bodies() {
            eprintln!("COMPENSATED RUNNING dots at {tier} over {} cases", cases.len());
            for (a, b, ..) in &cases {
                // SAFETY: dot_bodies() lists only bodies this CPU runs, and
                // the pairs have one length.
                let got = unsafe { body(a, b) };
                let want = dot_f64_scalar(a, b);
                assert!(same(got, want), "{tier}: a dot of {} pairs gave {got:e} ({:x}), the scalar form {want:e} ({:x})", a.len(), got.to_bits(), want.to_bits());
            }
        }
        for (a, b, ..) in &cases {
            assert!(same(sum_f64(a), sum_f64_scalar(a)));
            assert!(same(dot_f64(a, b), dot_f64_scalar(a, b)));
            // An infinite or NaN plain result is returned as it is.
            let (plain_sum, plain_dot) = (super::super::sum_f64(a), super::super::dot_f64(a, b));
            if !plain_sum.is_finite() {
                assert!(same(sum_f64(a), plain_sum));
            }
            if !plain_dot.is_finite() {
                assert!(same(dot_f64(a, b), plain_dot));
            }
        }
    }

    /// The spacing of doubles at `x`.
    fn ulp(x: f64) -> f64 {
        let m = x.abs();
        f64::from_bits(m.to_bits() + 1) - m
    }

    #[test]
    fn a_compensated_result_is_within_a_unit_in_the_last_place_of_the_exact_one() {
        // Values k * 2^e with |k| < 2^30 and |e| <= 30, whose exact sum is
        // an integer count of 2^-30 that fits in an i128; `as f64` rounds it
        // to nearest, and the scaling back is exact.
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let scale = 2f64.powi(-30);
        for n in [1usize, 7, 8, 9, 64, 65, 1000, 4099] {
            let terms: Vec<(i64, i32)> = (0..n).map(|_| ((next() as i64) >> 34, (next() % 61) as i32 - 30)).collect();
            let values: Vec<f64> = terms.iter().map(|&(k, e)| k as f64 * 2f64.powi(e)).collect();
            let exact: i128 = terms.iter().map(|&(k, e)| i128::from(k) << (e + 30)).sum();
            let exact = exact as f64 * scale;
            let got = sum_f64(&values);
            assert!((got - exact).abs() <= ulp(exact), "{n} values summed to {got:e}, exactly {exact:e}");
            // Products of k * 2^e with |k| < 2^20 and |e| <= 15, an integer
            // count of 2^-30 apiece.
            let pairs: Vec<((i64, i32), (i64, i32))> = (0..n).map(|_| (((next() as i64) >> 44, (next() % 31) as i32 - 15), ((next() as i64) >> 44, (next() % 31) as i32 - 15))).collect();
            let (xs, ys): (Vec<f64>, Vec<f64>) = pairs.iter().map(|&((k1, e1), (k2, e2))| (k1 as f64 * 2f64.powi(e1), k2 as f64 * 2f64.powi(e2))).unzip();
            let exact: i128 = pairs.iter().map(|&((k1, e1), (k2, e2))| (i128::from(k1) * i128::from(k2)) << (e1 + e2 + 30)).sum();
            let exact = exact as f64 * scale;
            let got = dot_f64(&xs, &ys);
            assert!((got - exact).abs() <= ulp(exact), "a dot of {n} pairs gave {got:e}, exactly {exact:e}");
        }
        // One running sum loses the 1.0 to 1e16's rounding, and the plain
        // order then cancels to 0.0; the error it recorded is the exact result.
        let mut v = vec![0.0f64; 17];
        (v[0], v[8], v[16]) = (1e16, 1.0, -1e16);
        assert_eq!(super::super::sum_f64(&v), 0.0);
        assert_eq!(sum_f64(&v), 1.0);
        // 0.1 * 10 is 1.0 after rounding, and its error is not lost.
        let exact_error = 0.1f64.mul_add(10.0, -1.0);
        assert_eq!(dot_f64(&[0.1, -1.0], &[10.0, 1.0]), exact_error);
        assert_eq!(sum_f64(&[]), 0.0);
        assert_eq!(dot_f64(&[2.0, 3.0], &[4.0]), 8.0);
    }
}
