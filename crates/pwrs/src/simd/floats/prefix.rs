//! Inclusive prefix sums in one fixed order: [`prefix_sum_f32`] and
//! [`prefix_sum_f64`] turn each value into the sum of it and every value
//! before it.
//!
//! Adding in lanes changes where a running sum rounds, so here the order
//! is part of the result. The values are taken in blocks of 16 `f32` or 8
//! `f64` from the start. Within a block each value adds the value one place
//! before it, then two, then four, and for `f32` eight, all from the values
//! the step before left, and `-0.0` where there is none, which leaves a
//! value as it is; then each value of the block adds the last sum of the
//! block before, `-0.0` for the first. A final short block takes the same
//! steps over the values it has. Every tier uses that order, so each
//! writes the scalar form's bits, for every input; a NaN is a NaN at every
//! tier, with the payload Rust leaves unspecified for a NaN arithmetic
//! makes. The sums can differ in their last bits from a left-to-right
//! running sum's, which is a different order.

use super::{F32_LANES, F64_LANES};

/// Turns `values` into its inclusive prefix sums, in this module's order.
pub fn prefix_sum_f32(values: &mut [f32]) {
    tiered::prefix_f32(values);
}

/// The scalar form of [`prefix_sum_f32`], in this module's order: the
/// result every tier gives.
pub fn prefix_sum_f32_scalar(values: &mut [f32]) {
    let mut carry = -0.0;
    for block in values.chunks_mut(F32_LANES) {
        carry = block_f32(block, carry);
    }
}

/// Turns `values` into its inclusive prefix sums, in this module's order.
pub fn prefix_sum_f64(values: &mut [f64]) {
    tiered::prefix_f64(values);
}

/// The scalar form of [`prefix_sum_f64`], in this module's order: the
/// result every tier gives.
pub fn prefix_sum_f64_scalar(values: &mut [f64]) {
    let mut carry = -0.0;
    for block in values.chunks_mut(F64_LANES) {
        carry = block_f64(block, carry);
    }
}

/// One block's steps, a block of at most 16, then the block before's last
/// sum added; returns this block's last sum. Each step runs from the top
/// down, so a value adds one the step has not yet changed.
fn block_f32(block: &mut [f32], carry: f32) -> f32 {
    let mut shift = 1;
    while shift < F32_LANES {
        for i in (0..block.len()).rev() {
            block[i] += if i >= shift { block[i - shift] } else { -0.0 };
        }
        shift <<= 1;
    }
    for v in block.iter_mut() {
        *v += carry;
    }
    block[block.len() - 1]
}

/// [`block_f32`] for a block of at most 8 `f64`.
fn block_f64(block: &mut [f64], carry: f64) -> f64 {
    let mut shift = 1;
    while shift < F64_LANES {
        for i in (0..block.len()).rev() {
            block[i] += if i >= shift { block[i - shift] } else { -0.0 };
        }
        shift <<= 1;
    }
    for v in block.iter_mut() {
        *v += carry;
    }
    block[block.len() - 1]
}

#[cfg(target_arch = "x86_64")]
mod tiered {
    use super::x86;
    use crate::cpu::{level, Level};

    // A level is reached only when the CPU and the operating system offer
    // every extension of it; SSE2 is in every x86-64 CPU.
    pub fn prefix_f32(v: &mut [f32]) {
        match level() {
            Level::V4 => unsafe { x86::prefix_f32_avx512(v) },
            Level::V3 => unsafe { x86::prefix_f32_avx2(v) },
            _baseline => unsafe { x86::prefix_f32_sse2(v) },
        }
    }

    pub fn prefix_f64(v: &mut [f64]) {
        match level() {
            Level::V4 => unsafe { x86::prefix_f64_avx512(v) },
            Level::V3 => unsafe { x86::prefix_f64_avx2(v) },
            _baseline => unsafe { x86::prefix_f64_sse2(v) },
        }
    }
}

// NEON is in every AArch64 CPU.
#[cfg(target_arch = "aarch64")]
mod tiered {
    use super::neon;

    pub fn prefix_f32(v: &mut [f32]) {
        unsafe { neon::prefix_f32(v) }
    }

    pub fn prefix_f64(v: &mut [f64]) {
        unsafe { neon::prefix_f64(v) }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod tiered {
    pub fn prefix_f32(v: &mut [f32]) {
        super::prefix_sum_f32_scalar(v);
    }

    pub fn prefix_f64(v: &mut [f64]) {
        super::prefix_sum_f64_scalar(v);
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{block_f32, block_f64, F32_LANES, F64_LANES};
    use core::arch::x86_64::*;

    // Each body loads and stores whole blocks, which the loop guard checks
    // are in bounds, and leaves a final short block to the scalar steps with
    // the carry it reached. A shifted register takes `-0.0` into the lanes
    // with nothing that far before them.

    /// The x86-64 baseline: a block of 16 in four registers.
    pub unsafe fn prefix_f32_sse2(values: &mut [f32]) {
        // SAFETY: as above.
        unsafe {
            let nz = _mm_set1_ps(-0.0);
            let mut carry = nz;
            let blocks = values.len() / F32_LANES;
            for b in 0..blocks {
                let p = values.as_mut_ptr().add(b * F32_LANES);
                let mut r = [_mm_loadu_ps(p), _mm_loadu_ps(p.add(4)), _mm_loadu_ps(p.add(8)), _mm_loadu_ps(p.add(12))];
                // One place: lane 3 of the register before, then lanes 0 to 2.
                let before = r;
                for k in 0..4 {
                    let prev = if k == 0 { nz } else { before[k - 1] };
                    let t = _mm_shuffle_ps::<0b01_00_11_11>(prev, before[k]);
                    r[k] = _mm_add_ps(before[k], _mm_shuffle_ps::<0b10_01_10_00>(t, before[k]));
                }
                // Two places: lanes 2 and 3 of the register before, then 0 and 1.
                let before = r;
                for k in 0..4 {
                    let prev = if k == 0 { nz } else { before[k - 1] };
                    r[k] = _mm_add_ps(before[k], _mm_shuffle_ps::<0b01_00_11_10>(prev, before[k]));
                }
                let before = r;
                for k in 0..4 {
                    r[k] = _mm_add_ps(before[k], if k == 0 { nz } else { before[k - 1] });
                }
                let before = r;
                for k in 0..4 {
                    r[k] = _mm_add_ps(before[k], if k < 2 { nz } else { before[k - 2] });
                }
                for (k, v) in r.iter_mut().enumerate() {
                    *v = _mm_add_ps(*v, carry);
                    _mm_storeu_ps(p.add(4 * k), *v);
                }
                carry = _mm_shuffle_ps::<0b11_11_11_11>(r[3], r[3]);
            }
            let rest = &mut values[blocks * F32_LANES..];
            if !rest.is_empty() {
                block_f32(rest, _mm_cvtss_f32(carry));
            }
        }
    }

    /// A block of 16 in two registers.
    #[target_feature(enable = "avx2")]
    pub unsafe fn prefix_f32_avx2(values: &mut [f32]) {
        // SAFETY: as above.
        unsafe {
            let nz = _mm256_set1_ps(-0.0);
            let by1 = _mm256_setr_epi32(0, 0, 1, 2, 3, 4, 5, 6);
            let from1 = _mm256_set1_epi32(7);
            let by2 = _mm256_setr_epi32(0, 0, 0, 1, 2, 3, 4, 5);
            let from2 = _mm256_setr_epi32(6, 7, 0, 0, 0, 0, 0, 0);
            let by4 = _mm256_setr_epi32(0, 0, 0, 0, 0, 1, 2, 3);
            let from4 = _mm256_setr_epi32(4, 5, 6, 7, 0, 0, 0, 0);
            let mut carry = nz;
            let blocks = values.len() / F32_LANES;
            for b in 0..blocks {
                let p = values.as_mut_ptr().add(b * F32_LANES);
                let (mut lo, mut hi) = (_mm256_loadu_ps(p), _mm256_loadu_ps(p.add(8)));
                let (s_lo, s_hi) = (_mm256_blend_ps::<0b0000_0001>(_mm256_permutevar8x32_ps(lo, by1), nz), _mm256_blend_ps::<0b0000_0001>(_mm256_permutevar8x32_ps(hi, by1), _mm256_permutevar8x32_ps(lo, from1)));
                (lo, hi) = (_mm256_add_ps(lo, s_lo), _mm256_add_ps(hi, s_hi));
                let (s_lo, s_hi) = (_mm256_blend_ps::<0b0000_0011>(_mm256_permutevar8x32_ps(lo, by2), nz), _mm256_blend_ps::<0b0000_0011>(_mm256_permutevar8x32_ps(hi, by2), _mm256_permutevar8x32_ps(lo, from2)));
                (lo, hi) = (_mm256_add_ps(lo, s_lo), _mm256_add_ps(hi, s_hi));
                let (s_lo, s_hi) = (_mm256_blend_ps::<0b0000_1111>(_mm256_permutevar8x32_ps(lo, by4), nz), _mm256_blend_ps::<0b0000_1111>(_mm256_permutevar8x32_ps(hi, by4), _mm256_permutevar8x32_ps(lo, from4)));
                (lo, hi) = (_mm256_add_ps(lo, s_lo), _mm256_add_ps(hi, s_hi));
                (lo, hi) = (_mm256_add_ps(lo, nz), _mm256_add_ps(hi, lo));
                (lo, hi) = (_mm256_add_ps(lo, carry), _mm256_add_ps(hi, carry));
                _mm256_storeu_ps(p, lo);
                _mm256_storeu_ps(p.add(8), hi);
                carry = _mm256_permutevar8x32_ps(hi, from1);
            }
            let rest = &mut values[blocks * F32_LANES..];
            if !rest.is_empty() {
                block_f32(rest, _mm256_cvtss_f32(carry));
            }
        }
    }

    /// Lanes `i >= 16 - IMM` of the block take the value `16 - IMM` places
    /// before, the rest `-0.0`.
    #[target_feature(enable = "avx512f")]
    #[inline]
    fn shifted_f32<const IMM: i32>(x: __m512, nz: __m512) -> __m512 {
        _mm512_castsi512_ps(_mm512_alignr_epi32::<IMM>(_mm512_castps_si512(x), _mm512_castps_si512(nz)))
    }

    /// A block of 16 in one register.
    #[target_feature(enable = "avx512f")]
    pub unsafe fn prefix_f32_avx512(values: &mut [f32]) {
        // SAFETY: as above.
        unsafe {
            let nz = _mm512_set1_ps(-0.0);
            let mut carry = nz;
            let blocks = values.len() / F32_LANES;
            for b in 0..blocks {
                let p = values.as_mut_ptr().add(b * F32_LANES);
                let mut x = _mm512_loadu_ps(p);
                x = _mm512_add_ps(x, shifted_f32::<15>(x, nz));
                x = _mm512_add_ps(x, shifted_f32::<14>(x, nz));
                x = _mm512_add_ps(x, shifted_f32::<12>(x, nz));
                x = _mm512_add_ps(x, shifted_f32::<8>(x, nz));
                x = _mm512_add_ps(x, carry);
                _mm512_storeu_ps(p, x);
                carry = _mm512_permutexvar_ps(_mm512_set1_epi32(15), x);
            }
            let rest = &mut values[blocks * F32_LANES..];
            if !rest.is_empty() {
                block_f32(rest, _mm512_cvtss_f32(carry));
            }
        }
    }

    /// The x86-64 baseline: a block of 8 in four registers.
    pub unsafe fn prefix_f64_sse2(values: &mut [f64]) {
        // SAFETY: as above.
        unsafe {
            let nz = _mm_set1_pd(-0.0);
            let mut carry = nz;
            let blocks = values.len() / F64_LANES;
            for b in 0..blocks {
                let p = values.as_mut_ptr().add(b * F64_LANES);
                let mut r = [_mm_loadu_pd(p), _mm_loadu_pd(p.add(2)), _mm_loadu_pd(p.add(4)), _mm_loadu_pd(p.add(6))];
                // One place: lane 1 of the register before, then lane 0.
                let before = r;
                for k in 0..4 {
                    let prev = if k == 0 { nz } else { before[k - 1] };
                    r[k] = _mm_add_pd(before[k], _mm_shuffle_pd::<0b01>(prev, before[k]));
                }
                let before = r;
                for k in 0..4 {
                    r[k] = _mm_add_pd(before[k], if k == 0 { nz } else { before[k - 1] });
                }
                let before = r;
                for k in 0..4 {
                    r[k] = _mm_add_pd(before[k], if k < 2 { nz } else { before[k - 2] });
                }
                for (k, v) in r.iter_mut().enumerate() {
                    *v = _mm_add_pd(*v, carry);
                    _mm_storeu_pd(p.add(2 * k), *v);
                }
                carry = _mm_unpackhi_pd(r[3], r[3]);
            }
            let rest = &mut values[blocks * F64_LANES..];
            if !rest.is_empty() {
                block_f64(rest, _mm_cvtsd_f64(carry));
            }
        }
    }

    /// A block of 8 in two registers.
    #[target_feature(enable = "avx2")]
    pub unsafe fn prefix_f64_avx2(values: &mut [f64]) {
        // SAFETY: as above.
        unsafe {
            let nz = _mm256_set1_pd(-0.0);
            let mut carry = nz;
            let blocks = values.len() / F64_LANES;
            for b in 0..blocks {
                let p = values.as_mut_ptr().add(b * F64_LANES);
                let (mut lo, mut hi) = (_mm256_loadu_pd(p), _mm256_loadu_pd(p.add(4)));
                // One place: lanes 0, 0, 1, 2 with lane 0 from lane 3 of the
                // register before.
                let s_lo = _mm256_blend_pd::<0b0001>(_mm256_permute4x64_pd::<0b10_01_00_00>(lo), nz);
                let s_hi = _mm256_blend_pd::<0b0001>(_mm256_permute4x64_pd::<0b10_01_00_00>(hi), _mm256_permute4x64_pd::<0b11_11_11_11>(lo));
                (lo, hi) = (_mm256_add_pd(lo, s_lo), _mm256_add_pd(hi, s_hi));
                // Two places: lanes 0, 0, 0, 1 with lanes 0 and 1 from lanes 2
                // and 3 of the register before.
                let s_lo = _mm256_blend_pd::<0b0011>(_mm256_permute4x64_pd::<0b01_00_00_00>(lo), nz);
                let s_hi = _mm256_blend_pd::<0b0011>(_mm256_permute4x64_pd::<0b01_00_00_00>(hi), _mm256_permute4x64_pd::<0b11_10_11_10>(lo));
                (lo, hi) = (_mm256_add_pd(lo, s_lo), _mm256_add_pd(hi, s_hi));
                (lo, hi) = (_mm256_add_pd(lo, nz), _mm256_add_pd(hi, lo));
                (lo, hi) = (_mm256_add_pd(lo, carry), _mm256_add_pd(hi, carry));
                _mm256_storeu_pd(p, lo);
                _mm256_storeu_pd(p.add(4), hi);
                carry = _mm256_permute4x64_pd::<0b11_11_11_11>(hi);
            }
            let rest = &mut values[blocks * F64_LANES..];
            if !rest.is_empty() {
                block_f64(rest, _mm256_cvtsd_f64(carry));
            }
        }
    }

    /// Lanes `i >= 8 - IMM` of the block take the value `8 - IMM` places
    /// before, the rest `-0.0`.
    #[target_feature(enable = "avx512f")]
    #[inline]
    fn shifted_f64<const IMM: i32>(x: __m512d, nz: __m512d) -> __m512d {
        _mm512_castsi512_pd(_mm512_alignr_epi64::<IMM>(_mm512_castpd_si512(x), _mm512_castpd_si512(nz)))
    }

    /// A block of 8 in one register.
    #[target_feature(enable = "avx512f")]
    pub unsafe fn prefix_f64_avx512(values: &mut [f64]) {
        // SAFETY: as above.
        unsafe {
            let nz = _mm512_set1_pd(-0.0);
            let mut carry = nz;
            let blocks = values.len() / F64_LANES;
            for b in 0..blocks {
                let p = values.as_mut_ptr().add(b * F64_LANES);
                let mut x = _mm512_loadu_pd(p);
                x = _mm512_add_pd(x, shifted_f64::<7>(x, nz));
                x = _mm512_add_pd(x, shifted_f64::<6>(x, nz));
                x = _mm512_add_pd(x, shifted_f64::<4>(x, nz));
                x = _mm512_add_pd(x, carry);
                _mm512_storeu_pd(p, x);
                carry = _mm512_permutexvar_pd(_mm512_set1_epi64(7), x);
            }
            let rest = &mut values[blocks * F64_LANES..];
            if !rest.is_empty() {
                block_f64(rest, _mm512_cvtsd_f64(carry));
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::{block_f32, block_f64, F32_LANES, F64_LANES};
    use core::arch::aarch64::*;

    // As the x86 bodies: whole blocks loaded and stored within the loop
    // guard, a final short block left to the scalar steps, and `-0.0` taken
    // into the lanes with nothing that far before them.

    /// A block of 16 in four registers.
    pub unsafe fn prefix_f32(values: &mut [f32]) {
        // SAFETY: as above.
        unsafe {
            let nz = vdupq_n_f32(-0.0);
            let mut carry = nz;
            let blocks = values.len() / F32_LANES;
            for b in 0..blocks {
                let p = values.as_mut_ptr().add(b * F32_LANES);
                let mut r = [vld1q_f32(p), vld1q_f32(p.add(4)), vld1q_f32(p.add(8)), vld1q_f32(p.add(12))];
                let before = r;
                for k in 0..4 {
                    let prev = if k == 0 { nz } else { before[k - 1] };
                    r[k] = vaddq_f32(before[k], vextq_f32::<3>(prev, before[k]));
                }
                let before = r;
                for k in 0..4 {
                    let prev = if k == 0 { nz } else { before[k - 1] };
                    r[k] = vaddq_f32(before[k], vextq_f32::<2>(prev, before[k]));
                }
                let before = r;
                for k in 0..4 {
                    r[k] = vaddq_f32(before[k], if k == 0 { nz } else { before[k - 1] });
                }
                let before = r;
                for k in 0..4 {
                    r[k] = vaddq_f32(before[k], if k < 2 { nz } else { before[k - 2] });
                }
                for (k, v) in r.iter_mut().enumerate() {
                    *v = vaddq_f32(*v, carry);
                    vst1q_f32(p.add(4 * k), *v);
                }
                carry = vdupq_laneq_f32::<3>(r[3]);
            }
            let rest = &mut values[blocks * F32_LANES..];
            if !rest.is_empty() {
                block_f32(rest, vgetq_lane_f32::<0>(carry));
            }
        }
    }

    /// A block of 8 in four registers.
    pub unsafe fn prefix_f64(values: &mut [f64]) {
        // SAFETY: as above.
        unsafe {
            let nz = vdupq_n_f64(-0.0);
            let mut carry = nz;
            let blocks = values.len() / F64_LANES;
            for b in 0..blocks {
                let p = values.as_mut_ptr().add(b * F64_LANES);
                let mut r = [vld1q_f64(p), vld1q_f64(p.add(2)), vld1q_f64(p.add(4)), vld1q_f64(p.add(6))];
                let before = r;
                for k in 0..4 {
                    let prev = if k == 0 { nz } else { before[k - 1] };
                    r[k] = vaddq_f64(before[k], vextq_f64::<1>(prev, before[k]));
                }
                let before = r;
                for k in 0..4 {
                    r[k] = vaddq_f64(before[k], if k == 0 { nz } else { before[k - 1] });
                }
                let before = r;
                for k in 0..4 {
                    r[k] = vaddq_f64(before[k], if k < 2 { nz } else { before[k - 2] });
                }
                for (k, v) in r.iter_mut().enumerate() {
                    *v = vaddq_f64(*v, carry);
                    vst1q_f64(p.add(2 * k), *v);
                }
                carry = vdupq_laneq_f64::<1>(r[3]);
            }
            let rest = &mut values[blocks * F64_LANES..];
            if !rest.is_empty() {
                block_f64(rest, vgetq_lane_f64::<0>(carry));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type F32Body = unsafe fn(&mut [f32]);
    type F64Body = unsafe fn(&mut [f64]);

    /// Every body this CPU runs, by tier, whatever `PWRS_CPU_MAX` says; a
    /// tier it lacks is printed as skipped.
    fn bodies() -> Vec<(&'static str, F32Body, F64Body)> {
        let mut out: Vec<(&'static str, F32Body, F64Body)> = Vec::new();
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            out.push(("x86-64", x86::prefix_f32_sse2, x86::prefix_f64_sse2));
            if detected(Isa::Avx2) {
                out.push(("x86-64-v3", x86::prefix_f32_avx2, x86::prefix_f64_avx2));
            } else {
                eprintln!("PREFIX SKIPPED x86-64-v3: this CPU lacks avx2");
            }
            if detected(Isa::Avx512f) {
                out.push(("x86-64-v4", x86::prefix_f32_avx512, x86::prefix_f64_avx512));
            } else {
                eprintln!("PREFIX SKIPPED x86-64-v4: this CPU lacks avx512f");
            }
        }
        #[cfg(target_arch = "aarch64")]
        out.push(("neon", neon::prefix_f32, neon::prefix_f64));
        out
    }

    /// The same bits, or a NaN on both sides: Rust leaves a computed NaN's
    /// payload unspecified.
    fn same_f32(got: &[f32], want: &[f32]) -> bool {
        got.iter().zip(want).all(|(g, w)| g.to_bits() == w.to_bits() || (g.is_nan() && w.is_nan()))
    }

    fn same_f64(got: &[f64], want: &[f64]) -> bool {
        got.iter().zip(want).all(|(g, w)| g.to_bits() == w.to_bits() || (g.is_nan() && w.is_nan()))
    }

    #[test]
    fn every_tier_writes_the_scalar_forms_bits() {
        let mut cases = super::super::tests::cases();
        let mut zeros = (vec![-0.0f64; 37], vec![0.0f64; 37], vec![-0.0f32; 37], vec![0.0f32; 37]);
        zeros.0[20] = 0.0;
        zeros.2[20] = 0.0;
        cases.push(zeros);
        let bodies = bodies();
        assert!(!bodies.is_empty(), "no prefix body ran");
        for (tier, f32_body, f64_body) in &bodies {
            eprintln!("PREFIX RUNNING prefix_sum_f32 and prefix_sum_f64 at {tier} over {} cases", cases.len());
            for (a, _, c, _) in &cases {
                let (mut got, mut want) = (c.clone(), c.clone());
                // SAFETY: bodies() lists only bodies this CPU runs.
                unsafe { f32_body(&mut got) };
                prefix_sum_f32_scalar(&mut want);
                assert!(same_f32(&got, &want), "{tier}: {} f32 values gave {got:?}, the scalar form {want:?}", c.len());
                let (mut got, mut want) = (a.clone(), a.clone());
                // SAFETY: as above.
                unsafe { f64_body(&mut got) };
                prefix_sum_f64_scalar(&mut want);
                assert!(same_f64(&got, &want), "{tier}: {} f64 values gave {got:?}, the scalar form {want:?}", a.len());
            }
        }
        for (a, _, c, _) in &cases {
            let (mut got, mut want) = (c.clone(), c.clone());
            prefix_sum_f32(&mut got);
            prefix_sum_f32_scalar(&mut want);
            assert!(same_f32(&got, &want));
            let (mut got, mut want) = (a.clone(), a.clone());
            prefix_sum_f64(&mut got);
            prefix_sum_f64_scalar(&mut want);
            assert!(same_f64(&got, &want));
        }
    }

    #[test]
    fn the_order_is_a_tree_in_each_block_and_a_carry_between() {
        // Whole numbers add exactly in any order.
        let mut ones = vec![1.0f64; 40];
        prefix_sum_f64(&mut ones);
        assert!(ones.iter().enumerate().all(|(i, &v)| v == (i + 1) as f64));
        // Within a block, value 3 is (v2 + v3) + (v0 + v1): here 0.0, where
        // a running sum from the left gives 1.0.
        let mut v = [1.0f64, 1e16, -1e16, 1.0];
        prefix_sum_f64(&mut v);
        assert_eq!(v, [1.0, 1e16, 1.0, 0.0]);
        // A value with nothing before it is left as it is, sign of zero
        // included.
        let mut z = [-0.0f32];
        prefix_sum_f32(&mut z);
        assert_eq!(z[0].to_bits(), (-0.0f32).to_bits());
        let mut empty: [f64; 0] = [];
        prefix_sum_f64(&mut empty);
    }
}
