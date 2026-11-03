//! L1 distances: [`l1_u8`] and [`l1_u32`], the sum of the absolute
//! differences of two slices' elements over the shorter slice. At every
//! tier, for every input, each returns what its scalar form returns: every
//! difference is exact, and the sum is stored in `u64` lanes, which wrap only
//! past 2^64 as the scalar form's wrapping sum does, so the order the lanes
//! add in does not change it.
//!
//! The bytes' differences are summed eight to a lane by SSE2's `psadbw`,
//! 16, 32 and 64 bytes a step through SSE2, AVX2 and AVX-512BW, and by
//! NEON's absolute difference with pairwise widening adds. A `u32`
//! difference is the larger value less the smaller, which works over all
//! of `u32` where a signed subtraction would not.

/// The sum of `|a[i] - b[i]|` over the shorter slice.
pub fn l1_u8(a: &[u8], b: &[u8]) -> u64 {
    let n = a.len().min(b.len());
    tiered::l1_u8(&a[..n], &b[..n])
}

/// The scalar form of [`l1_u8`]: the result every tier gives.
pub fn l1_u8_scalar(a: &[u8], b: &[u8]) -> u64 {
    a.iter().zip(b).fold(0u64, |sum, (&x, &y)| sum.wrapping_add(u64::from(x.abs_diff(y))))
}

/// The sum of `|a[i] - b[i]|` over the shorter slice.
pub fn l1_u32(a: &[u32], b: &[u32]) -> u64 {
    let n = a.len().min(b.len());
    tiered::l1_u32(&a[..n], &b[..n])
}

/// The scalar form of [`l1_u32`]: the result every tier gives.
pub fn l1_u32_scalar(a: &[u32], b: &[u32]) -> u64 {
    a.iter().zip(b).fold(0u64, |sum, (&x, &y)| sum.wrapping_add(u64::from(x.abs_diff(y))))
}

#[cfg(target_arch = "x86_64")]
mod tiered {
    use super::x86;
    use crate::cpu::{level, Level};

    // A level is reached only when the CPU and the operating system offer
    // every extension of it; SSE2 is in every x86-64 CPU.
    pub fn l1_u8(a: &[u8], b: &[u8]) -> u64 {
        match level() {
            Level::V4 => unsafe { x86::l1_u8_avx512(a, b) },
            Level::V3 => unsafe { x86::l1_u8_avx2(a, b) },
            _baseline => unsafe { x86::l1_u8_sse2(a, b) },
        }
    }

    pub fn l1_u32(a: &[u32], b: &[u32]) -> u64 {
        match level() {
            Level::V4 => unsafe { x86::l1_u32_avx512(a, b) },
            Level::V3 => unsafe { x86::l1_u32_avx2(a, b) },
            _baseline => unsafe { x86::l1_u32_sse2(a, b) },
        }
    }
}

// NEON is in every AArch64 CPU.
#[cfg(target_arch = "aarch64")]
mod tiered {
    use super::neon;

    pub fn l1_u8(a: &[u8], b: &[u8]) -> u64 {
        unsafe { neon::l1_u8(a, b) }
    }

    pub fn l1_u32(a: &[u32], b: &[u32]) -> u64 {
        unsafe { neon::l1_u32(a, b) }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod tiered {
    pub fn l1_u8(a: &[u8], b: &[u8]) -> u64 {
        super::l1_u8_scalar(a, b)
    }

    pub fn l1_u32(a: &[u32], b: &[u32]) -> u64 {
        super::l1_u32_scalar(a, b)
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{l1_u32_scalar, l1_u8_scalar};
    use core::arch::x86_64::*;

    // Each body takes slices of one length, and each load reads from an
    // offset its loop guard checks is a whole vector short of the end.

    /// The x86-64 baseline.
    pub unsafe fn l1_u8_sse2(a: &[u8], b: &[u8]) -> u64 {
        // SAFETY: as above.
        unsafe {
            let mut acc = _mm_setzero_si128();
            let mut i = 0;
            while i + 16 <= a.len() {
                acc = _mm_add_epi64(acc, _mm_sad_epu8(_mm_loadu_si128(a.as_ptr().add(i).cast()), _mm_loadu_si128(b.as_ptr().add(i).cast())));
                i += 16;
            }
            let mut lanes = [0u64; 2];
            _mm_storeu_si128(lanes.as_mut_ptr().cast(), acc);
            lanes[0].wrapping_add(lanes[1]).wrapping_add(l1_u8_scalar(&a[i..], &b[i..]))
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn l1_u8_avx2(a: &[u8], b: &[u8]) -> u64 {
        // SAFETY: as above.
        unsafe {
            let mut acc = _mm256_setzero_si256();
            let mut i = 0;
            while i + 32 <= a.len() {
                acc = _mm256_add_epi64(acc, _mm256_sad_epu8(_mm256_loadu_si256(a.as_ptr().add(i).cast()), _mm256_loadu_si256(b.as_ptr().add(i).cast())));
                i += 32;
            }
            let mut lanes = [0u64; 4];
            _mm256_storeu_si256(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().fold(l1_u8_scalar(&a[i..], &b[i..]), |sum, &lane| sum.wrapping_add(lane))
        }
    }

    #[target_feature(enable = "avx512f,avx512bw")]
    pub unsafe fn l1_u8_avx512(a: &[u8], b: &[u8]) -> u64 {
        // SAFETY: as above.
        unsafe {
            let mut acc = _mm512_setzero_si512();
            let mut i = 0;
            while i + 64 <= a.len() {
                acc = _mm512_add_epi64(acc, _mm512_sad_epu8(_mm512_loadu_si512(a.as_ptr().add(i).cast()), _mm512_loadu_si512(b.as_ptr().add(i).cast())));
                i += 64;
            }
            (_mm512_reduce_add_epi64(acc) as u64).wrapping_add(l1_u8_scalar(&a[i..], &b[i..]))
        }
    }

    /// The x86-64 baseline, which has no unsigned 32-bit compare: the
    /// difference is negated where `b` is the larger, which a signed
    /// compare of both values with the sign bit flipped finds.
    pub unsafe fn l1_u32_sse2(a: &[u32], b: &[u32]) -> u64 {
        // SAFETY: as above.
        unsafe {
            let (bias, zero) = (_mm_set1_epi32(i32::MIN), _mm_setzero_si128());
            let mut acc = _mm_setzero_si128();
            let mut i = 0;
            while i + 4 <= a.len() {
                let (x, y) = (_mm_loadu_si128(a.as_ptr().add(i).cast()), _mm_loadu_si128(b.as_ptr().add(i).cast()));
                let below = _mm_cmpgt_epi32(_mm_xor_si128(y, bias), _mm_xor_si128(x, bias));
                let d = _mm_sub_epi32(_mm_xor_si128(_mm_sub_epi32(x, y), below), below);
                acc = _mm_add_epi64(acc, _mm_add_epi64(_mm_unpacklo_epi32(d, zero), _mm_unpackhi_epi32(d, zero)));
                i += 4;
            }
            let mut lanes = [0u64; 2];
            _mm_storeu_si128(lanes.as_mut_ptr().cast(), acc);
            lanes[0].wrapping_add(lanes[1]).wrapping_add(l1_u32_scalar(&a[i..], &b[i..]))
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn l1_u32_avx2(a: &[u32], b: &[u32]) -> u64 {
        // SAFETY: as above.
        unsafe {
            let mut acc = _mm256_setzero_si256();
            let mut i = 0;
            while i + 8 <= a.len() {
                let (x, y) = (_mm256_loadu_si256(a.as_ptr().add(i).cast()), _mm256_loadu_si256(b.as_ptr().add(i).cast()));
                let d = _mm256_sub_epi32(_mm256_max_epu32(x, y), _mm256_min_epu32(x, y));
                let wide = _mm256_add_epi64(_mm256_cvtepu32_epi64(_mm256_castsi256_si128(d)), _mm256_cvtepu32_epi64(_mm256_extracti128_si256::<1>(d)));
                acc = _mm256_add_epi64(acc, wide);
                i += 8;
            }
            let mut lanes = [0u64; 4];
            _mm256_storeu_si256(lanes.as_mut_ptr().cast(), acc);
            lanes.iter().fold(l1_u32_scalar(&a[i..], &b[i..]), |sum, &lane| sum.wrapping_add(lane))
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn l1_u32_avx512(a: &[u32], b: &[u32]) -> u64 {
        // SAFETY: as above.
        unsafe {
            let mut acc = _mm512_setzero_si512();
            let mut i = 0;
            while i + 16 <= a.len() {
                let (x, y) = (_mm512_loadu_si512(a.as_ptr().add(i).cast()), _mm512_loadu_si512(b.as_ptr().add(i).cast()));
                let d = _mm512_sub_epi32(_mm512_max_epu32(x, y), _mm512_min_epu32(x, y));
                let wide = _mm512_add_epi64(_mm512_cvtepu32_epi64(_mm512_castsi512_si256(d)), _mm512_cvtepu32_epi64(_mm512_extracti64x4_epi64::<1>(d)));
                acc = _mm512_add_epi64(acc, wide);
                i += 16;
            }
            (_mm512_reduce_add_epi64(acc) as u64).wrapping_add(l1_u32_scalar(&a[i..], &b[i..]))
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::{l1_u32_scalar, l1_u8_scalar};
    use core::arch::aarch64::*;

    // Each body takes slices of one length, and each load reads from an
    // offset its loop guard checks is a whole vector short of the end.

    /// A step adds at most 510 to each 16-bit lane, so the lanes are
    /// widened into the 64-bit sums every 128 steps, before they could
    /// pass 2^16.
    pub unsafe fn l1_u8(a: &[u8], b: &[u8]) -> u64 {
        // SAFETY: as above.
        unsafe {
            let mut total = vdupq_n_u64(0);
            let mut i = 0;
            while i + 16 <= a.len() {
                let mut lanes = vdupq_n_u16(0);
                let mut steps = 0;
                while steps < 128 && i + 16 <= a.len() {
                    lanes = vpadalq_u8(lanes, vabdq_u8(vld1q_u8(a.as_ptr().add(i)), vld1q_u8(b.as_ptr().add(i))));
                    i += 16;
                    steps += 1;
                }
                total = vpadalq_u32(total, vpaddlq_u16(lanes));
            }
            vaddvq_u64(total).wrapping_add(l1_u8_scalar(&a[i..], &b[i..]))
        }
    }

    pub unsafe fn l1_u32(a: &[u32], b: &[u32]) -> u64 {
        // SAFETY: as above.
        unsafe {
            let mut total = vdupq_n_u64(0);
            let mut i = 0;
            while i + 4 <= a.len() {
                total = vpadalq_u32(total, vabdq_u32(vld1q_u32(a.as_ptr().add(i)), vld1q_u32(b.as_ptr().add(i))));
                i += 4;
            }
            vaddvq_u64(total).wrapping_add(l1_u32_scalar(&a[i..], &b[i..]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type L1U8 = unsafe fn(&[u8], &[u8]) -> u64;
    type L1U32 = unsafe fn(&[u32], &[u32]) -> u64;

    /// Every body this CPU runs, by tier, whatever `PWRS_CPU_MAX` says; a
    /// tier it lacks is printed as skipped.
    fn bodies() -> Vec<(&'static str, L1U8, L1U32)> {
        let mut out: Vec<(&'static str, L1U8, L1U32)> = Vec::new();
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            out.push(("x86-64", x86::l1_u8_sse2, x86::l1_u32_sse2));
            if detected(Isa::Avx2) {
                out.push(("x86-64-v3", x86::l1_u8_avx2, x86::l1_u32_avx2));
            } else {
                eprintln!("DISTANCE SKIPPED x86-64-v3: this CPU lacks avx2");
            }
            if detected(Isa::Avx512f) && detected(Isa::Avx512bw) {
                out.push(("x86-64-v4", x86::l1_u8_avx512, x86::l1_u32_avx512));
            } else {
                eprintln!("DISTANCE SKIPPED x86-64-v4: this CPU lacks avx512f or avx512bw");
            }
        }
        #[cfg(target_arch = "aarch64")]
        out.push(("neon", neon::l1_u8, neon::l1_u32));
        out
    }

    const EDGES: [u32; 6] = [0, 1, i32::MAX as u32, 1 << 31, u32::MAX - 1, u32::MAX];

    /// Two slices of one length, the inputs of one distance.
    type Pairs<T> = Vec<(Vec<T>, Vec<T>)>;

    /// Pairs of every length to 70 and a few longer, of values drawn from
    /// all of each type and from its edges, and the largest difference
    /// throughout, long enough that NEON widens its byte sums many times.
    fn pairs() -> (Pairs<u8>, Pairs<u32>) {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let (mut bytes, mut words) = (Vec::new(), Vec::new());
        for n in (0..70).chain([127, 128, 129, 1000, 4099]) {
            bytes.push(((0..n).map(|_| next() as u8).collect(), (0..n).map(|_| next() as u8).collect()));
            bytes.push(((0..n).map(|_| if next() % 2 == 0 { 0 } else { 255 }).collect(), (0..n).map(|_| if next() % 2 == 0 { 0 } else { 255 }).collect()));
            words.push(((0..n).map(|_| next() as u32).collect(), (0..n).map(|_| next() as u32).collect()));
            words.push(((0..n).map(|_| EDGES[(next() % 6) as usize]).collect(), (0..n).map(|_| EDGES[(next() % 6) as usize]).collect()));
        }
        bytes.push((vec![255; 70_001], vec![0; 70_001]));
        words.push((vec![u32::MAX; 4099], vec![0; 4099]));
        (bytes, words)
    }

    #[test]
    fn every_tier_sums_the_differences_the_scalar_form_sums() {
        let (bytes, words) = pairs();
        let bodies = bodies();
        assert!(!bodies.is_empty(), "no distance body ran");
        for (tier, l1_u8_body, l1_u32_body) in &bodies {
            eprintln!("DISTANCE RUNNING l1_u8 and l1_u32 at {tier} over {} and {} pairs", bytes.len(), words.len());
            for (a, b) in &bytes {
                // SAFETY: bodies() lists only bodies this CPU runs, and the
                // pairs have one length.
                let got = unsafe { l1_u8_body(a, b) };
                assert_eq!(got, l1_u8_scalar(a, b), "{tier}: {} bytes", a.len());
            }
            for (a, b) in &words {
                // SAFETY: as above.
                let got = unsafe { l1_u32_body(a, b) };
                assert_eq!(got, l1_u32_scalar(a, b), "{tier}: {} words", a.len());
            }
        }
        for (a, b) in &bytes {
            assert_eq!(l1_u8(a, b), l1_u8_scalar(a, b));
        }
        for (a, b) in &words {
            assert_eq!(l1_u32(a, b), l1_u32_scalar(a, b));
        }
        assert_eq!(l1_u8(&[255; 70_001], &[0; 70_001]), 70_001 * 255);
        assert_eq!(l1_u32(&[u32::MAX; 4099], &[0; 4099]), 4099 * u64::from(u32::MAX));
        assert_eq!(l1_u32(&[0, 1 << 31], &[1 << 31, 0, 7]), 1 << 32);
        assert_eq!(l1_u8(&[], &[1, 2]), 0);
    }
}
