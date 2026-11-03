//! Floating-point sums and dot products in one fixed order: [`sum_f64`],
//! [`sum_f32`], [`dot_f64`] and [`dot_f32`].
//!
//! Adding in lanes changes where a sum rounds, so here the order is part
//! of the result. Element `i` goes into running sum `i % K`, which is 8
//! for `f64` and 16 for `f32`, and the K sums are combined by halving: the
//! first half adds the second, until one is left. Every tier has those K
//! sums, in one register or several, and combines them the same way, so
//! every tier returns the bits the scalar form returns, for every input,
//! infinities and signed zeros included. A NaN result is a NaN at every
//! tier, with the payload Rust leaves unspecified for a NaN arithmetic
//! makes. A dot product adds each product rounded on its own, never a
//! fused multiply-add. The result can differ in its last bits from a
//! left-to-right loop's, which is a different order. [`compensated`] uses
//! the same order and records each running sum's rounding error beside it.

pub mod compensated;
mod axpy;
mod prefix;

pub use axpy::{axpy_f32, axpy_f32_scalar, axpy_f64, axpy_f64_scalar};
pub use prefix::{prefix_sum_f32, prefix_sum_f32_scalar, prefix_sum_f64, prefix_sum_f64_scalar};

/// Running sums of `f64`.
pub const F64_LANES: usize = 8;
/// Running sums of `f32`.
pub const F32_LANES: usize = 16;

/// The sum of `values` in this module's order; 0.0 for none.
pub fn sum_f64(values: &[f64]) -> f64 {
    tiered::sum_f64(values)
}

/// The scalar form of [`sum_f64`], in this module's order: the result
/// every tier gives.
pub fn sum_f64_scalar(values: &[f64]) -> f64 {
    let mut lanes = [0.0f64; F64_LANES];
    for (i, v) in values.iter().enumerate() {
        lanes[i % F64_LANES] += v;
    }
    combine_f64(lanes)
}

/// The sum of `values` in this module's order; 0.0 for none.
pub fn sum_f32(values: &[f32]) -> f32 {
    tiered::sum_f32(values)
}

/// The scalar form of [`sum_f32`], in this module's order: the result
/// every tier gives.
pub fn sum_f32_scalar(values: &[f32]) -> f32 {
    let mut lanes = [0.0f32; F32_LANES];
    for (i, v) in values.iter().enumerate() {
        lanes[i % F32_LANES] += v;
    }
    combine_f32(lanes)
}

/// The sum of `a[i] * b[i]` over the shorter slice, in this module's
/// order.
pub fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    tiered::dot_f64(&a[..n], &b[..n])
}

/// The scalar form of [`dot_f64`], in this module's order: the result
/// every tier gives.
pub fn dot_f64_scalar(a: &[f64], b: &[f64]) -> f64 {
    let mut lanes = [0.0f64; F64_LANES];
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        lanes[i % F64_LANES] += x * y;
    }
    combine_f64(lanes)
}

/// The sum of `a[i] * b[i]` over the shorter slice, in this module's
/// order.
pub fn dot_f32(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    tiered::dot_f32(&a[..n], &b[..n])
}

/// The scalar form of [`dot_f32`], in this module's order: the result
/// every tier gives.
pub fn dot_f32_scalar(a: &[f32], b: &[f32]) -> f32 {
    let mut lanes = [0.0f32; F32_LANES];
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        lanes[i % F32_LANES] += x * y;
    }
    combine_f32(lanes)
}

/// The running sums combined by halving: lane `i` adds lane `i + n/2`,
/// for each `n` from the lane count down to 2.
fn combine_f64(mut lanes: [f64; F64_LANES]) -> f64 {
    let mut n = F64_LANES;
    while n > 1 {
        n /= 2;
        for i in 0..n {
            lanes[i] += lanes[i + n];
        }
    }
    lanes[0]
}

fn combine_f32(mut lanes: [f32; F32_LANES]) -> f32 {
    let mut n = F32_LANES;
    while n > 1 {
        n /= 2;
        for i in 0..n {
            lanes[i] += lanes[i + n];
        }
    }
    lanes[0]
}

/// The elements past the last whole block added into their running sums.
fn tail_f64(lanes: &mut [f64; F64_LANES], rest: &[f64]) {
    for (j, v) in rest.iter().enumerate() {
        lanes[j] += v;
    }
}

fn tail_f32(lanes: &mut [f32; F32_LANES], rest: &[f32]) {
    for (j, v) in rest.iter().enumerate() {
        lanes[j] += v;
    }
}

fn tail_dot_f64(lanes: &mut [f64; F64_LANES], a: &[f64], b: &[f64]) {
    for (j, (x, y)) in a.iter().zip(b).enumerate() {
        lanes[j] += x * y;
    }
}

fn tail_dot_f32(lanes: &mut [f32; F32_LANES], a: &[f32], b: &[f32]) {
    for (j, (x, y)) in a.iter().zip(b).enumerate() {
        lanes[j] += x * y;
    }
}

#[cfg(target_arch = "x86_64")]
mod tiered {
    use super::x86;
    use crate::cpu::{level, Level};

    // A level is reached only when the CPU and the operating system offer
    // every extension of it; SSE2 is in every x86-64 CPU.
    pub fn sum_f64(v: &[f64]) -> f64 {
        match level() {
            Level::V4 => unsafe { x86::sum_f64_avx512(v) },
            Level::V3 => unsafe { x86::sum_f64_avx2(v) },
            _baseline => unsafe { x86::sum_f64_sse2(v) },
        }
    }

    pub fn sum_f32(v: &[f32]) -> f32 {
        match level() {
            Level::V4 => unsafe { x86::sum_f32_avx512(v) },
            Level::V3 => unsafe { x86::sum_f32_avx2(v) },
            _baseline => unsafe { x86::sum_f32_sse2(v) },
        }
    }

    pub fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
        match level() {
            Level::V4 => unsafe { x86::dot_f64_avx512(a, b) },
            Level::V3 => unsafe { x86::dot_f64_avx2(a, b) },
            _baseline => unsafe { x86::dot_f64_sse2(a, b) },
        }
    }

    pub fn dot_f32(a: &[f32], b: &[f32]) -> f32 {
        match level() {
            Level::V4 => unsafe { x86::dot_f32_avx512(a, b) },
            Level::V3 => unsafe { x86::dot_f32_avx2(a, b) },
            _baseline => unsafe { x86::dot_f32_sse2(a, b) },
        }
    }
}

// NEON is in every AArch64 CPU.
#[cfg(target_arch = "aarch64")]
mod tiered {
    use super::neon;

    pub fn sum_f64(v: &[f64]) -> f64 {
        unsafe { neon::sum_f64(v) }
    }

    pub fn sum_f32(v: &[f32]) -> f32 {
        unsafe { neon::sum_f32(v) }
    }

    pub fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
        unsafe { neon::dot_f64(a, b) }
    }

    pub fn dot_f32(a: &[f32], b: &[f32]) -> f32 {
        unsafe { neon::dot_f32(a, b) }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod tiered {
    pub fn sum_f64(v: &[f64]) -> f64 {
        super::sum_f64_scalar(v)
    }

    pub fn sum_f32(v: &[f32]) -> f32 {
        super::sum_f32_scalar(v)
    }

    pub fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
        super::dot_f64_scalar(a, b)
    }

    pub fn dot_f32(a: &[f32], b: &[f32]) -> f32 {
        super::dot_f32_scalar(a, b)
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{combine_f32, combine_f64, tail_dot_f32, tail_dot_f64, tail_f32, tail_f64, F32_LANES, F64_LANES};
    use core::arch::x86_64::*;

    /// The x86-64 baseline: four registers of two running sums. The dot
    /// bodies take slices of one length.
    pub unsafe fn sum_f64_sse2(v: &[f64]) -> f64 {
        // SAFETY: each block's loads read eight values the loop guard checks
        // are in bounds, and the stores write the eight-lane array.
        unsafe {
            let mut acc = [_mm_setzero_pd(); 4];
            let blocks = v.len() / F64_LANES;
            for k in 0..blocks {
                let p = v.as_ptr().add(k * F64_LANES);
                for (j, a) in acc.iter_mut().enumerate() {
                    *a = _mm_add_pd(*a, _mm_loadu_pd(p.add(2 * j)));
                }
            }
            let mut lanes = [0.0f64; F64_LANES];
            for (j, a) in acc.iter().enumerate() {
                _mm_storeu_pd(lanes.as_mut_ptr().add(2 * j), *a);
            }
            tail_f64(&mut lanes, &v[blocks * F64_LANES..]);
            combine_f64(lanes)
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn sum_f64_avx2(v: &[f64]) -> f64 {
        // SAFETY: as sum_f64_sse2's, two registers of four.
        unsafe {
            let (mut lo, mut hi) = (_mm256_setzero_pd(), _mm256_setzero_pd());
            let blocks = v.len() / F64_LANES;
            for k in 0..blocks {
                let p = v.as_ptr().add(k * F64_LANES);
                lo = _mm256_add_pd(lo, _mm256_loadu_pd(p));
                hi = _mm256_add_pd(hi, _mm256_loadu_pd(p.add(4)));
            }
            let mut lanes = [0.0f64; F64_LANES];
            _mm256_storeu_pd(lanes.as_mut_ptr(), lo);
            _mm256_storeu_pd(lanes.as_mut_ptr().add(4), hi);
            tail_f64(&mut lanes, &v[blocks * F64_LANES..]);
            combine_f64(lanes)
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn sum_f64_avx512(v: &[f64]) -> f64 {
        // SAFETY: as sum_f64_sse2's, one register of eight.
        unsafe {
            let mut acc = _mm512_setzero_pd();
            let blocks = v.len() / F64_LANES;
            for k in 0..blocks {
                acc = _mm512_add_pd(acc, _mm512_loadu_pd(v.as_ptr().add(k * F64_LANES)));
            }
            let mut lanes = [0.0f64; F64_LANES];
            _mm512_storeu_pd(lanes.as_mut_ptr(), acc);
            tail_f64(&mut lanes, &v[blocks * F64_LANES..]);
            combine_f64(lanes)
        }
    }

    /// The x86-64 baseline: four registers of four running sums.
    pub unsafe fn sum_f32_sse2(v: &[f32]) -> f32 {
        // SAFETY: each block's loads read sixteen values the loop guard
        // checks are in bounds, and the stores write the sixteen-lane array.
        unsafe {
            let mut acc = [_mm_setzero_ps(); 4];
            let blocks = v.len() / F32_LANES;
            for k in 0..blocks {
                let p = v.as_ptr().add(k * F32_LANES);
                for (j, a) in acc.iter_mut().enumerate() {
                    *a = _mm_add_ps(*a, _mm_loadu_ps(p.add(4 * j)));
                }
            }
            let mut lanes = [0.0f32; F32_LANES];
            for (j, a) in acc.iter().enumerate() {
                _mm_storeu_ps(lanes.as_mut_ptr().add(4 * j), *a);
            }
            tail_f32(&mut lanes, &v[blocks * F32_LANES..]);
            combine_f32(lanes)
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn sum_f32_avx2(v: &[f32]) -> f32 {
        // SAFETY: as sum_f32_sse2's, two registers of eight.
        unsafe {
            let (mut lo, mut hi) = (_mm256_setzero_ps(), _mm256_setzero_ps());
            let blocks = v.len() / F32_LANES;
            for k in 0..blocks {
                let p = v.as_ptr().add(k * F32_LANES);
                lo = _mm256_add_ps(lo, _mm256_loadu_ps(p));
                hi = _mm256_add_ps(hi, _mm256_loadu_ps(p.add(8)));
            }
            let mut lanes = [0.0f32; F32_LANES];
            _mm256_storeu_ps(lanes.as_mut_ptr(), lo);
            _mm256_storeu_ps(lanes.as_mut_ptr().add(8), hi);
            tail_f32(&mut lanes, &v[blocks * F32_LANES..]);
            combine_f32(lanes)
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn sum_f32_avx512(v: &[f32]) -> f32 {
        // SAFETY: as sum_f32_sse2's, one register of sixteen.
        unsafe {
            let mut acc = _mm512_setzero_ps();
            let blocks = v.len() / F32_LANES;
            for k in 0..blocks {
                acc = _mm512_add_ps(acc, _mm512_loadu_ps(v.as_ptr().add(k * F32_LANES)));
            }
            let mut lanes = [0.0f32; F32_LANES];
            _mm512_storeu_ps(lanes.as_mut_ptr(), acc);
            tail_f32(&mut lanes, &v[blocks * F32_LANES..]);
            combine_f32(lanes)
        }
    }

    pub unsafe fn dot_f64_sse2(a: &[f64], b: &[f64]) -> f64 {
        // SAFETY: as sum_f64_sse2's, over two slices of one length.
        unsafe {
            let mut acc = [_mm_setzero_pd(); 4];
            let blocks = a.len() / F64_LANES;
            for k in 0..blocks {
                let (p, q) = (a.as_ptr().add(k * F64_LANES), b.as_ptr().add(k * F64_LANES));
                for (j, s) in acc.iter_mut().enumerate() {
                    *s = _mm_add_pd(*s, _mm_mul_pd(_mm_loadu_pd(p.add(2 * j)), _mm_loadu_pd(q.add(2 * j))));
                }
            }
            let mut lanes = [0.0f64; F64_LANES];
            for (j, s) in acc.iter().enumerate() {
                _mm_storeu_pd(lanes.as_mut_ptr().add(2 * j), *s);
            }
            tail_dot_f64(&mut lanes, &a[blocks * F64_LANES..], &b[blocks * F64_LANES..]);
            combine_f64(lanes)
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn dot_f64_avx2(a: &[f64], b: &[f64]) -> f64 {
        // SAFETY: as dot_f64_sse2's, two registers of four.
        unsafe {
            let (mut lo, mut hi) = (_mm256_setzero_pd(), _mm256_setzero_pd());
            let blocks = a.len() / F64_LANES;
            for k in 0..blocks {
                let (p, q) = (a.as_ptr().add(k * F64_LANES), b.as_ptr().add(k * F64_LANES));
                lo = _mm256_add_pd(lo, _mm256_mul_pd(_mm256_loadu_pd(p), _mm256_loadu_pd(q)));
                hi = _mm256_add_pd(hi, _mm256_mul_pd(_mm256_loadu_pd(p.add(4)), _mm256_loadu_pd(q.add(4))));
            }
            let mut lanes = [0.0f64; F64_LANES];
            _mm256_storeu_pd(lanes.as_mut_ptr(), lo);
            _mm256_storeu_pd(lanes.as_mut_ptr().add(4), hi);
            tail_dot_f64(&mut lanes, &a[blocks * F64_LANES..], &b[blocks * F64_LANES..]);
            combine_f64(lanes)
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn dot_f64_avx512(a: &[f64], b: &[f64]) -> f64 {
        // SAFETY: as dot_f64_sse2's, one register of eight.
        unsafe {
            let mut acc = _mm512_setzero_pd();
            let blocks = a.len() / F64_LANES;
            for k in 0..blocks {
                let at = k * F64_LANES;
                acc = _mm512_add_pd(acc, _mm512_mul_pd(_mm512_loadu_pd(a.as_ptr().add(at)), _mm512_loadu_pd(b.as_ptr().add(at))));
            }
            let mut lanes = [0.0f64; F64_LANES];
            _mm512_storeu_pd(lanes.as_mut_ptr(), acc);
            tail_dot_f64(&mut lanes, &a[blocks * F64_LANES..], &b[blocks * F64_LANES..]);
            combine_f64(lanes)
        }
    }

    pub unsafe fn dot_f32_sse2(a: &[f32], b: &[f32]) -> f32 {
        // SAFETY: as sum_f32_sse2's, over two slices of one length.
        unsafe {
            let mut acc = [_mm_setzero_ps(); 4];
            let blocks = a.len() / F32_LANES;
            for k in 0..blocks {
                let (p, q) = (a.as_ptr().add(k * F32_LANES), b.as_ptr().add(k * F32_LANES));
                for (j, s) in acc.iter_mut().enumerate() {
                    *s = _mm_add_ps(*s, _mm_mul_ps(_mm_loadu_ps(p.add(4 * j)), _mm_loadu_ps(q.add(4 * j))));
                }
            }
            let mut lanes = [0.0f32; F32_LANES];
            for (j, s) in acc.iter().enumerate() {
                _mm_storeu_ps(lanes.as_mut_ptr().add(4 * j), *s);
            }
            tail_dot_f32(&mut lanes, &a[blocks * F32_LANES..], &b[blocks * F32_LANES..]);
            combine_f32(lanes)
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn dot_f32_avx2(a: &[f32], b: &[f32]) -> f32 {
        // SAFETY: as dot_f32_sse2's, two registers of eight.
        unsafe {
            let (mut lo, mut hi) = (_mm256_setzero_ps(), _mm256_setzero_ps());
            let blocks = a.len() / F32_LANES;
            for k in 0..blocks {
                let (p, q) = (a.as_ptr().add(k * F32_LANES), b.as_ptr().add(k * F32_LANES));
                lo = _mm256_add_ps(lo, _mm256_mul_ps(_mm256_loadu_ps(p), _mm256_loadu_ps(q)));
                hi = _mm256_add_ps(hi, _mm256_mul_ps(_mm256_loadu_ps(p.add(8)), _mm256_loadu_ps(q.add(8))));
            }
            let mut lanes = [0.0f32; F32_LANES];
            _mm256_storeu_ps(lanes.as_mut_ptr(), lo);
            _mm256_storeu_ps(lanes.as_mut_ptr().add(8), hi);
            tail_dot_f32(&mut lanes, &a[blocks * F32_LANES..], &b[blocks * F32_LANES..]);
            combine_f32(lanes)
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn dot_f32_avx512(a: &[f32], b: &[f32]) -> f32 {
        // SAFETY: as dot_f32_sse2's, one register of sixteen.
        unsafe {
            let mut acc = _mm512_setzero_ps();
            let blocks = a.len() / F32_LANES;
            for k in 0..blocks {
                let at = k * F32_LANES;
                acc = _mm512_add_ps(acc, _mm512_mul_ps(_mm512_loadu_ps(a.as_ptr().add(at)), _mm512_loadu_ps(b.as_ptr().add(at))));
            }
            let mut lanes = [0.0f32; F32_LANES];
            _mm512_storeu_ps(lanes.as_mut_ptr(), acc);
            tail_dot_f32(&mut lanes, &a[blocks * F32_LANES..], &b[blocks * F32_LANES..]);
            combine_f32(lanes)
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::{combine_f32, combine_f64, tail_dot_f32, tail_dot_f64, tail_f32, tail_f64, F32_LANES, F64_LANES};
    use core::arch::aarch64::*;

    /// Four registers of two running sums. The dot bodies take slices of
    /// one length.
    pub unsafe fn sum_f64(v: &[f64]) -> f64 {
        // SAFETY: each block's loads read eight values the loop guard checks
        // are in bounds, and the stores write the eight-lane array.
        unsafe {
            let mut acc = [vdupq_n_f64(0.0); 4];
            let blocks = v.len() / F64_LANES;
            for k in 0..blocks {
                let p = v.as_ptr().add(k * F64_LANES);
                for (j, a) in acc.iter_mut().enumerate() {
                    *a = vaddq_f64(*a, vld1q_f64(p.add(2 * j)));
                }
            }
            let mut lanes = [0.0f64; F64_LANES];
            for (j, a) in acc.iter().enumerate() {
                vst1q_f64(lanes.as_mut_ptr().add(2 * j), *a);
            }
            tail_f64(&mut lanes, &v[blocks * F64_LANES..]);
            combine_f64(lanes)
        }
    }

    /// Four registers of four running sums.
    pub unsafe fn sum_f32(v: &[f32]) -> f32 {
        // SAFETY: as sum_f64's, sixteen values a block.
        unsafe {
            let mut acc = [vdupq_n_f32(0.0); 4];
            let blocks = v.len() / F32_LANES;
            for k in 0..blocks {
                let p = v.as_ptr().add(k * F32_LANES);
                for (j, a) in acc.iter_mut().enumerate() {
                    *a = vaddq_f32(*a, vld1q_f32(p.add(4 * j)));
                }
            }
            let mut lanes = [0.0f32; F32_LANES];
            for (j, a) in acc.iter().enumerate() {
                vst1q_f32(lanes.as_mut_ptr().add(4 * j), *a);
            }
            tail_f32(&mut lanes, &v[blocks * F32_LANES..]);
            combine_f32(lanes)
        }
    }

    /// A product is rounded on its own before it is added: FMUL then FADD,
    /// never FMLA.
    pub unsafe fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
        // SAFETY: as sum_f64's, over two slices of one length.
        unsafe {
            let mut acc = [vdupq_n_f64(0.0); 4];
            let blocks = a.len() / F64_LANES;
            for k in 0..blocks {
                let (p, q) = (a.as_ptr().add(k * F64_LANES), b.as_ptr().add(k * F64_LANES));
                for (j, s) in acc.iter_mut().enumerate() {
                    *s = vaddq_f64(*s, vmulq_f64(vld1q_f64(p.add(2 * j)), vld1q_f64(q.add(2 * j))));
                }
            }
            let mut lanes = [0.0f64; F64_LANES];
            for (j, s) in acc.iter().enumerate() {
                vst1q_f64(lanes.as_mut_ptr().add(2 * j), *s);
            }
            tail_dot_f64(&mut lanes, &a[blocks * F64_LANES..], &b[blocks * F64_LANES..]);
            combine_f64(lanes)
        }
    }

    pub unsafe fn dot_f32(a: &[f32], b: &[f32]) -> f32 {
        // SAFETY: as sum_f32's, over two slices of one length.
        unsafe {
            let mut acc = [vdupq_n_f32(0.0); 4];
            let blocks = a.len() / F32_LANES;
            for k in 0..blocks {
                let (p, q) = (a.as_ptr().add(k * F32_LANES), b.as_ptr().add(k * F32_LANES));
                for (j, s) in acc.iter_mut().enumerate() {
                    *s = vaddq_f32(*s, vmulq_f32(vld1q_f32(p.add(4 * j)), vld1q_f32(q.add(4 * j))));
                }
            }
            let mut lanes = [0.0f32; F32_LANES];
            for (j, s) in acc.iter().enumerate() {
                vst1q_f32(lanes.as_mut_ptr().add(4 * j), *s);
            }
            tail_dot_f32(&mut lanes, &a[blocks * F32_LANES..], &b[blocks * F32_LANES..]);
            combine_f32(lanes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four kernels' results as raw bits, so a NaN compares by its
    /// payload and a signed zero by its sign.
    type Bits = [u64; 4];
    type Body = Box<dyn Fn(&[f64], &[f64], &[f32], &[f32]) -> Bits>;

    fn bits(sd: f64, ss: f32, dd: f64, ds: f32) -> Bits {
        [sd.to_bits(), u64::from(ss.to_bits()), dd.to_bits(), u64::from(ds.to_bits())]
    }

    /// The same bits, or a NaN on both sides: Rust leaves a computed NaN's
    /// payload unspecified. Slots 0 and 2 are `f64`, 1 and 3 `f32`.
    fn agree(got: Bits, want: Bits) -> bool {
        got.iter().zip(&want).enumerate().all(|(slot, (g, w))| {
            let both_nan = if slot % 2 == 0 { f64::from_bits(*g).is_nan() && f64::from_bits(*w).is_nan() } else { f32::from_bits(*g as u32).is_nan() && f32::from_bits(*w as u32).is_nan() };
            g == w || both_nan
        })
    }

    /// The bodies this CPU runs, by tier, whatever PWRS_CPU_MAX says; a
    /// tier it lacks is printed as skipped.
    fn bodies() -> Vec<(&'static str, Body)> {
        let mut out: Vec<(&'static str, Body)> = Vec::new();
        // SAFETY in each closure: a tier is listed only when this CPU
        // offers its instructions, and the pairs have one length.
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            out.push(("x86-64", Box::new(|a, b, c, d| unsafe { bits(x86::sum_f64_sse2(a), x86::sum_f32_sse2(c), x86::dot_f64_sse2(a, b), x86::dot_f32_sse2(c, d)) })));
            if detected(Isa::Avx2) {
                out.push(("x86-64-v3", Box::new(|a, b, c, d| unsafe { bits(x86::sum_f64_avx2(a), x86::sum_f32_avx2(c), x86::dot_f64_avx2(a, b), x86::dot_f32_avx2(c, d)) })));
            } else {
                eprintln!("FLOATS SKIPPED x86-64-v3: this CPU lacks avx2");
            }
            if detected(Isa::Avx512f) {
                out.push(("x86-64-v4", Box::new(|a, b, c, d| unsafe { bits(x86::sum_f64_avx512(a), x86::sum_f32_avx512(c), x86::dot_f64_avx512(a, b), x86::dot_f32_avx512(c, d)) })));
            } else {
                eprintln!("FLOATS SKIPPED x86-64-v4: this CPU lacks avx512f");
            }
        }
        #[cfg(target_arch = "aarch64")]
        out.push(("neon", Box::new(|a, b, c, d| unsafe { bits(neon::sum_f64(a), neon::sum_f32(c), neon::dot_f64(a, b), neon::dot_f32(c, d)) })));
        out
    }

    /// Two `f64` slices and two `f32` slices of one length, the inputs of
    /// one case.
    type Case = (Vec<f64>, Vec<f64>, Vec<f32>, Vec<f32>);

    /// Pairs of slices of every length to 70 and a few longer: values of
    /// mixed sign and magnitude, whose sums round at every step; raw bit
    /// patterns, NaNs, infinities and subnormals among them; and runs of
    /// negative zero.
    pub(super) fn cases() -> Vec<Case> {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut out = Vec::new();
        let lengths: Vec<usize> = (0..70).chain([127, 128, 129, 1000, 4099]).collect();
        for &n in &lengths {
            let mixed = |r: u64| ((r >> 11) as f64 / 9_007_199_254_740_992.0 - 0.5) * 10f64.powi((r % 13) as i32 - 6);
            let a: Vec<f64> = (0..n).map(|_| mixed(next())).collect();
            let b: Vec<f64> = (0..n).map(|_| mixed(next())).collect();
            let c: Vec<f32> = a.iter().map(|&x| x as f32).collect();
            let d: Vec<f32> = b.iter().map(|&x| x as f32).collect();
            out.push((a, b, c, d));
            let raw_a: Vec<f64> = (0..n).map(|_| f64::from_bits(next())).collect();
            let raw_b: Vec<f64> = (0..n).map(|_| f64::from_bits(next())).collect();
            let raw_c: Vec<f32> = (0..n).map(|_| f32::from_bits((next() >> 32) as u32)).collect();
            let raw_d: Vec<f32> = (0..n).map(|_| f32::from_bits((next() >> 32) as u32)).collect();
            out.push((raw_a, raw_b, raw_c, raw_d));
        }
        out.push((vec![-0.0; 37], vec![1.0; 37], vec![-0.0; 37], vec![1.0; 37]));
        out
    }

    #[test]
    fn every_tier_sums_in_the_scalar_order_bit_for_bit() {
        let cases = cases();
        let bodies = bodies();
        assert!(!bodies.is_empty(), "no body ran");
        for (tier, body) in &bodies {
            eprintln!("FLOATS RUNNING sums and dots at {tier} over {} cases", cases.len());
            for (a, b, c, d) in &cases {
                let want = bits(sum_f64_scalar(a), sum_f32_scalar(c), dot_f64_scalar(a, b), dot_f32_scalar(c, d));
                let got = body(a, b, c, d);
                assert!(agree(got, want), "{tier}: {} values gave {got:x?}, the scalar form {want:x?}", a.len());
            }
        }
        for (a, b, c, d) in &cases {
            let want = bits(sum_f64_scalar(a), sum_f32_scalar(c), dot_f64_scalar(a, b), dot_f32_scalar(c, d));
            assert!(agree(bits(sum_f64(a), sum_f32(c), dot_f64(a, b), dot_f32(c, d)), want));
        }
    }

    #[test]
    fn the_order_is_eight_or_sixteen_running_sums_combined_by_halving() {
        // Eight f64 values, one a lane: ((v0 + v4) + (v2 + v6)) + ((v1 + v5) + (v3 + v7)).
        let v = [1e16, 1.0, -1e16, 1.0, 1.0, 1.0, 1.0, 1.0];
        let want = ((v[0] + v[4]) + (v[2] + v[6])) + ((v[1] + v[5]) + (v[3] + v[7]));
        assert_eq!(sum_f64(&v).to_bits(), want.to_bits());
        assert_eq!(sum_f64(&[]), 0.0);
        assert_eq!(dot_f32(&[2.0, 3.0], &[4.0]), 8.0);
    }
}
