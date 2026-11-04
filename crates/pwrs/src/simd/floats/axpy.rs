//! `y[i] += a * x[i]` over the shorter slice, the product rounded on its
//! own before it is added, never a fused multiply-add: [`axpy_f64`] and
//! [`axpy_f32`]. Each element is computed alone, so every tier writes the
//! bits the scalar form writes, for every input; a NaN is a NaN at every
//! tier, with the payload Rust leaves unspecified for a NaN arithmetic
//! makes.

/// `y[i] += a * x[i]` over the shorter slice.
pub fn axpy_f64(y: &mut [f64], a: f64, x: &[f64]) {
    let n = y.len().min(x.len());
    tiered::axpy_f64(&mut y[..n], a, &x[..n]);
}

/// The scalar form of [`axpy_f64`]: the result every tier gives.
pub fn axpy_f64_scalar(y: &mut [f64], a: f64, x: &[f64]) {
    for (yi, &xi) in y.iter_mut().zip(x) {
        *yi += a * xi;
    }
}

/// `y[i] += a * x[i]` over the shorter slice.
pub fn axpy_f32(y: &mut [f32], a: f32, x: &[f32]) {
    let n = y.len().min(x.len());
    tiered::axpy_f32(&mut y[..n], a, &x[..n]);
}

/// The scalar form of [`axpy_f32`]: the result every tier gives.
pub fn axpy_f32_scalar(y: &mut [f32], a: f32, x: &[f32]) {
    for (yi, &xi) in y.iter_mut().zip(x) {
        *yi += a * xi;
    }
}

#[cfg(target_arch = "x86_64")]
mod tiered {
    use super::x86;
    use crate::cpu::{level, Level};

    // A level is reached only when the CPU and the operating system offer
    // every extension of it; SSE2 is in every x86-64 CPU.
    pub fn axpy_f64(y: &mut [f64], a: f64, x: &[f64]) {
        match level() {
            Level::V4 => unsafe { x86::axpy_f64_avx512(y, a, x) },
            Level::V3 => unsafe { x86::axpy_f64_avx(y, a, x) },
            _baseline => unsafe { x86::axpy_f64_sse2(y, a, x) },
        }
    }

    pub fn axpy_f32(y: &mut [f32], a: f32, x: &[f32]) {
        match level() {
            Level::V4 => unsafe { x86::axpy_f32_avx512(y, a, x) },
            Level::V3 => unsafe { x86::axpy_f32_avx(y, a, x) },
            _baseline => unsafe { x86::axpy_f32_sse2(y, a, x) },
        }
    }
}

// NEON is in every AArch64 CPU.
#[cfg(target_arch = "aarch64")]
mod tiered {
    use super::neon;

    pub fn axpy_f64(y: &mut [f64], a: f64, x: &[f64]) {
        unsafe { neon::axpy_f64(y, a, x) }
    }

    pub fn axpy_f32(y: &mut [f32], a: f32, x: &[f32]) {
        unsafe { neon::axpy_f32(y, a, x) }
    }
}

// NEON is optional on 32-bit ARM, and has no f64 lanes.
#[cfg(all(target_arch = "arm", feature = "arm-neon"))]
mod tiered {
    use crate::cpu::arm32::{has, Feature};

    pub fn axpy_f64(y: &mut [f64], a: f64, x: &[f64]) {
        super::axpy_f64_scalar(y, a, x);
    }

    pub fn axpy_f32(y: &mut [f32], a: f32, x: &[f32]) {
        if has(Feature::Neon) { unsafe { super::super::arm::axpy_f32(y, a, x) } } else { super::axpy_f32_scalar(y, a, x) }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon"))))]
mod tiered {
    pub fn axpy_f64(y: &mut [f64], a: f64, x: &[f64]) {
        super::axpy_f64_scalar(y, a, x);
    }

    pub fn axpy_f32(y: &mut [f32], a: f32, x: &[f32]) {
        super::axpy_f32_scalar(y, a, x);
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{axpy_f32_scalar, axpy_f64_scalar};
    use core::arch::x86_64::*;

    // Each body takes slices of one length; each load and store reaches a
    // whole vector from an offset its loop guard checks is that far short
    // of the end. A multiply then an add, which the compiler never fuses.

    /// The x86-64 baseline.
    pub unsafe fn axpy_f64_sse2(y: &mut [f64], a: f64, x: &[f64]) {
        // SAFETY: as above.
        unsafe {
            let k = _mm_set1_pd(a);
            let mut i = 0;
            while i + 2 <= y.len() {
                let p = y.as_mut_ptr().add(i);
                _mm_storeu_pd(p, _mm_add_pd(_mm_loadu_pd(p), _mm_mul_pd(k, _mm_loadu_pd(x.as_ptr().add(i)))));
                i += 2;
            }
            axpy_f64_scalar(&mut y[i..], a, &x[i..]);
        }
    }

    /// AVX, from x86-64-v3.
    #[target_feature(enable = "avx")]
    pub unsafe fn axpy_f64_avx(y: &mut [f64], a: f64, x: &[f64]) {
        // SAFETY: as above.
        unsafe {
            let k = _mm256_set1_pd(a);
            let mut i = 0;
            while i + 4 <= y.len() {
                let p = y.as_mut_ptr().add(i);
                _mm256_storeu_pd(p, _mm256_add_pd(_mm256_loadu_pd(p), _mm256_mul_pd(k, _mm256_loadu_pd(x.as_ptr().add(i)))));
                i += 4;
            }
            axpy_f64_scalar(&mut y[i..], a, &x[i..]);
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn axpy_f64_avx512(y: &mut [f64], a: f64, x: &[f64]) {
        // SAFETY: as above.
        unsafe {
            let k = _mm512_set1_pd(a);
            let mut i = 0;
            while i + 8 <= y.len() {
                let p = y.as_mut_ptr().add(i);
                _mm512_storeu_pd(p, _mm512_add_pd(_mm512_loadu_pd(p), _mm512_mul_pd(k, _mm512_loadu_pd(x.as_ptr().add(i)))));
                i += 8;
            }
            axpy_f64_scalar(&mut y[i..], a, &x[i..]);
        }
    }

    /// The x86-64 baseline.
    pub unsafe fn axpy_f32_sse2(y: &mut [f32], a: f32, x: &[f32]) {
        // SAFETY: as above.
        unsafe {
            let k = _mm_set1_ps(a);
            let mut i = 0;
            while i + 4 <= y.len() {
                let p = y.as_mut_ptr().add(i);
                _mm_storeu_ps(p, _mm_add_ps(_mm_loadu_ps(p), _mm_mul_ps(k, _mm_loadu_ps(x.as_ptr().add(i)))));
                i += 4;
            }
            axpy_f32_scalar(&mut y[i..], a, &x[i..]);
        }
    }

    /// AVX, from x86-64-v3.
    #[target_feature(enable = "avx")]
    pub unsafe fn axpy_f32_avx(y: &mut [f32], a: f32, x: &[f32]) {
        // SAFETY: as above.
        unsafe {
            let k = _mm256_set1_ps(a);
            let mut i = 0;
            while i + 8 <= y.len() {
                let p = y.as_mut_ptr().add(i);
                _mm256_storeu_ps(p, _mm256_add_ps(_mm256_loadu_ps(p), _mm256_mul_ps(k, _mm256_loadu_ps(x.as_ptr().add(i)))));
                i += 8;
            }
            axpy_f32_scalar(&mut y[i..], a, &x[i..]);
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn axpy_f32_avx512(y: &mut [f32], a: f32, x: &[f32]) {
        // SAFETY: as above.
        unsafe {
            let k = _mm512_set1_ps(a);
            let mut i = 0;
            while i + 16 <= y.len() {
                let p = y.as_mut_ptr().add(i);
                _mm512_storeu_ps(p, _mm512_add_ps(_mm512_loadu_ps(p), _mm512_mul_ps(k, _mm512_loadu_ps(x.as_ptr().add(i)))));
                i += 16;
            }
            axpy_f32_scalar(&mut y[i..], a, &x[i..]);
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::{axpy_f32_scalar, axpy_f64_scalar};
    use core::arch::aarch64::*;

    // As the x86 bodies: FMUL then FADD, never FMLA.

    pub unsafe fn axpy_f64(y: &mut [f64], a: f64, x: &[f64]) {
        // SAFETY: each load and store reaches two values from an offset the
        // loop guard checks is two short of the end, in slices of one length.
        unsafe {
            let k = vdupq_n_f64(a);
            let mut i = 0;
            while i + 2 <= y.len() {
                let p = y.as_mut_ptr().add(i);
                vst1q_f64(p, vaddq_f64(vld1q_f64(p), vmulq_f64(k, vld1q_f64(x.as_ptr().add(i)))));
                i += 2;
            }
            axpy_f64_scalar(&mut y[i..], a, &x[i..]);
        }
    }

    pub unsafe fn axpy_f32(y: &mut [f32], a: f32, x: &[f32]) {
        // SAFETY: as axpy_f64's, four values at a time.
        unsafe {
            let k = vdupq_n_f32(a);
            let mut i = 0;
            while i + 4 <= y.len() {
                let p = y.as_mut_ptr().add(i);
                vst1q_f32(p, vaddq_f32(vld1q_f32(p), vmulq_f32(k, vld1q_f32(x.as_ptr().add(i)))));
                i += 4;
            }
            axpy_f32_scalar(&mut y[i..], a, &x[i..]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type F64Body = unsafe fn(&mut [f64], f64, &[f64]);
    type F32Body = unsafe fn(&mut [f32], f32, &[f32]);

    /// Every body this CPU runs, by tier, whatever `PWRS_CPU_MAX` says; a
    /// tier it lacks is printed as skipped.
    #[cfg_attr(target_arch = "aarch64", allow(clippy::vec_init_then_push))]
    fn bodies() -> Vec<(&'static str, F64Body, F32Body)> {
        let mut out: Vec<(&'static str, F64Body, F32Body)> = Vec::new();
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            out.push(("x86-64", x86::axpy_f64_sse2, x86::axpy_f32_sse2));
            if detected(Isa::Avx) {
                out.push(("x86-64-v3", x86::axpy_f64_avx, x86::axpy_f32_avx));
            } else {
                eprintln!("AXPY SKIPPED x86-64-v3: this CPU lacks avx");
            }
            if detected(Isa::Avx512f) {
                out.push(("x86-64-v4", x86::axpy_f64_avx512, x86::axpy_f32_avx512));
            } else {
                eprintln!("AXPY SKIPPED x86-64-v4: this CPU lacks avx512f");
            }
        }
        #[cfg(target_arch = "aarch64")]
        out.push(("neon", neon::axpy_f64, neon::axpy_f32));
        #[cfg(all(target_arch = "arm", feature = "arm-neon"))]
        if crate::cpu::arm32::detected(crate::cpu::arm32::Feature::Neon) {
            out.push(("neon", axpy_f64_scalar, super::super::arm::axpy_f32));
        } else {
            eprintln!("AXPY SKIPPED neon: this CPU lacks it");
        }
        out
    }

    /// The same bits, or a NaN on both sides: Rust leaves a computed NaN's
    /// payload unspecified.
    fn same_f64(got: &[f64], want: &[f64]) -> bool {
        got.iter().zip(want).all(|(g, w)| g.to_bits() == w.to_bits() || (g.is_nan() && w.is_nan()))
    }

    fn same_f32(got: &[f32], want: &[f32]) -> bool {
        got.iter().zip(want).all(|(g, w)| g.to_bits() == w.to_bits() || (g.is_nan() && w.is_nan()))
    }

    #[test]
    fn every_tier_writes_the_scalar_forms_bits() {
        let cases = super::super::tests::cases();
        let bodies = bodies();
        assert!(!bodies.is_empty() || !cfg!(any(target_arch = "x86_64", target_arch = "aarch64")), "no axpy body ran");
        let scales = [2.5, -1.0, 0.0, -0.0, 1e300, f64::MIN_POSITIVE, f64::INFINITY, f64::NAN];
        for (tier, f64_body, f32_body) in &bodies {
            eprintln!("AXPY RUNNING axpy_f64 and axpy_f32 at {tier} over {} cases", cases.len());
            for (a, b, c, d) in &cases {
                for &k in &scales {
                    let (mut got, mut want) = (b.clone(), b.clone());
                    // SAFETY: bodies() lists only bodies this CPU runs, and
                    // the pairs have one length.
                    unsafe { f64_body(&mut got, k, a) };
                    axpy_f64_scalar(&mut want, k, a);
                    assert!(same_f64(&got, &want), "{tier}: {} f64 values times {k:e}", a.len());
                    let (mut got, mut want) = (d.clone(), d.clone());
                    // SAFETY: as above.
                    unsafe { f32_body(&mut got, k as f32, c) };
                    axpy_f32_scalar(&mut want, k as f32, c);
                    assert!(same_f32(&got, &want), "{tier}: {} f32 values times {k:e}", c.len());
                }
            }
        }
        for (a, b, c, d) in &cases {
            let (mut got, mut want) = (b.clone(), b.clone());
            axpy_f64(&mut got, 0.1, a);
            axpy_f64_scalar(&mut want, 0.1, a);
            assert!(same_f64(&got, &want));
            let (mut got, mut want) = (d.clone(), d.clone());
            axpy_f32(&mut got, 0.1, c);
            axpy_f32_scalar(&mut want, 0.1, c);
            assert!(same_f32(&got, &want));
        }
        // The product rounds before the add: 0.1 * 10 is 1.0, so 1 - 1 is 0,
        // where a fused multiply-add would leave the product's error.
        let mut y = [-1.0f64, 5.0];
        axpy_f64(&mut y, 0.1, &[10.0]);
        assert_eq!(y, [0.0, 5.0]);
    }
}
