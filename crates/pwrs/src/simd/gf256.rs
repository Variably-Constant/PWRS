//! Arithmetic in GF(256) with the polynomial `x^8 + x^4 + x^3 + x^2 + 1`
//! (`0x11D`), the field Reed-Solomon erasure codes work in: [`mul`] and
//! [`mul_add`], which adds a byte slice times a constant into another.
//! Multiplying by a constant is linear over GF(2), so a table of each
//! nibble's product, or the 8x8 bit matrix GFNI's affine instruction
//! applies, gives the field product exactly: every tier returns what
//! [`mul_add_scalar`] returns.
//!
//! The tiers look the two nibbles up with SSSE3 from x86-64-v2, AVX2 at
//! v3 and AVX-512BW at v4, multiply through GFNI where the CPU has it,
//! which no psABI level includes, and look up with NEON's TBL on AArch64.

/// `exp[i] = 2^i` for `i` to 509, so a product's logarithms index it
/// without a reduction, and `log[x]` for `x` from 1.
const TABLES: ([u8; 512], [u8; 256]) = tables();

const fn tables() -> ([u8; 512], [u8; 256]) {
    let mut exp = [0u8; 512];
    let mut log = [0u8; 256];
    let mut x: u16 = 1;
    let mut i = 0;
    while i < 255 {
        exp[i] = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11D;
        }
        i += 1;
    }
    while i < 512 {
        exp[i] = exp[i - 255];
        i += 1;
    }
    (exp, log)
}

/// `a` times `b` in GF(256).
pub fn mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    let (exp, log) = &TABLES;
    exp[log[a as usize] as usize + log[b as usize] as usize]
}

/// `out[i] ^= coef * src[i]` in GF(256), over the shorter slice.
pub fn mul_add(out: &mut [u8], src: &[u8], coef: u8) {
    let n = out.len().min(src.len());
    if coef == 0 || n == 0 {
        return;
    }
    tiered::mul_add(&mut out[..n], &src[..n], coef);
}

/// The scalar form of [`mul_add`]: the result every tier gives.
pub fn mul_add_scalar(out: &mut [u8], src: &[u8], coef: u8) {
    for (o, &s) in out.iter_mut().zip(src) {
        *o ^= mul(coef, s);
    }
}

/// The products of `coef` with each low nibble and each high nibble, which
/// XORed together give its product with a byte.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon")))]
fn nibble_tables(coef: u8) -> ([u8; 16], [u8; 16]) {
    let mut lo = [0u8; 16];
    let mut hi = [0u8; 16];
    for i in 0..16u8 {
        lo[i as usize] = mul(coef, i);
        hi[i as usize] = mul(coef, i << 4);
    }
    (lo, hi)
}

/// The 8x8 bit matrix GF2P8AFFINEQB applies to multiply by `coef`: output
/// bit `i` is the parity of byte `7 - i` of the matrix ANDed with the
/// input, so that byte's bit `j` is bit `i` of `coef * 2^j`.
#[cfg(any(target_arch = "x86_64", test))]
fn affine_matrix(coef: u8) -> u64 {
    let mut m = 0u64;
    for i in 0..8 {
        let mut row = 0u8;
        for j in 0..8 {
            if (mul(coef, 1 << j) >> i) & 1 == 1 {
                row |= 1 << j;
            }
        }
        m |= u64::from(row) << (8 * (7 - i));
    }
    m
}

#[cfg(target_arch = "x86_64")]
mod tiered {
    use super::x86;
    use crate::cpu::{has, level, Isa, Level};

    // A level is reached only when the CPU and the operating system offer
    // every extension of it; GFNI is in none, so has() checks it on its own.
    pub fn mul_add(out: &mut [u8], src: &[u8], coef: u8) {
        match level() {
            Level::V4 if has(Isa::Gfni) => unsafe { x86::mul_add_gfni512(out, src, coef) },
            Level::V3 if has(Isa::Gfni) => unsafe { x86::mul_add_gfni256(out, src, coef) },
            Level::V4 => unsafe { x86::mul_add_avx512(out, src, coef) },
            Level::V3 => unsafe { x86::mul_add_avx2(out, src, coef) },
            Level::V2 => unsafe { x86::mul_add_ssse3(out, src, coef) },
            _baseline => super::mul_add_scalar(out, src, coef),
        }
    }
}

// NEON is in every AArch64 CPU, and optional on 32-bit ARM.
#[cfg(any(target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon")))]
mod tiered {
    pub fn mul_add(out: &mut [u8], src: &[u8], coef: u8) {
        if crate::simd::arch::has_neon() { unsafe { super::neon::mul_add(out, src, coef) } } else { super::mul_add_scalar(out, src, coef) }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon"))))]
mod tiered {
    pub fn mul_add(out: &mut [u8], src: &[u8], coef: u8) {
        super::mul_add_scalar(out, src, coef)
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{affine_matrix, mul_add_scalar, nibble_tables};
    use core::arch::x86_64::*;

    /// x86-64-v2. The two slices have one length, as for every body.
    #[target_feature(enable = "ssse3")]
    pub unsafe fn mul_add_ssse3(out: &mut [u8], src: &[u8], coef: u8) {
        let (lo, hi) = nibble_tables(coef);
        // SAFETY: each load and store covers 16 bytes the loop guard checks
        // are in bounds of both slices.
        unsafe {
            let (lo, hi) = (_mm_loadu_si128(lo.as_ptr().cast()), _mm_loadu_si128(hi.as_ptr().cast()));
            let mask = _mm_set1_epi8(0x0F);
            let mut i = 0;
            while i + 16 <= out.len() {
                let s = _mm_loadu_si128(src.as_ptr().add(i).cast());
                let product = _mm_xor_si128(_mm_shuffle_epi8(lo, _mm_and_si128(s, mask)), _mm_shuffle_epi8(hi, _mm_and_si128(_mm_srli_epi16::<4>(s), mask)));
                let o = out.as_mut_ptr().add(i).cast();
                _mm_storeu_si128(o, _mm_xor_si128(_mm_loadu_si128(o), product));
                i += 16;
            }
            mul_add_scalar(&mut out[i..], &src[i..], coef);
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn mul_add_avx2(out: &mut [u8], src: &[u8], coef: u8) {
        let (lo, hi) = nibble_tables(coef);
        // SAFETY: as mul_add_ssse3's, 32 bytes at a time.
        unsafe {
            let lo = _mm256_broadcastsi128_si256(_mm_loadu_si128(lo.as_ptr().cast()));
            let hi = _mm256_broadcastsi128_si256(_mm_loadu_si128(hi.as_ptr().cast()));
            let mask = _mm256_set1_epi8(0x0F);
            let mut i = 0;
            while i + 32 <= out.len() {
                let s = _mm256_loadu_si256(src.as_ptr().add(i).cast());
                let product = _mm256_xor_si256(_mm256_shuffle_epi8(lo, _mm256_and_si256(s, mask)), _mm256_shuffle_epi8(hi, _mm256_and_si256(_mm256_srli_epi16::<4>(s), mask)));
                let o = out.as_mut_ptr().add(i).cast();
                _mm256_storeu_si256(o, _mm256_xor_si256(_mm256_loadu_si256(o), product));
                i += 32;
            }
            mul_add_scalar(&mut out[i..], &src[i..], coef);
        }
    }

    #[target_feature(enable = "avx512f,avx512bw")]
    pub unsafe fn mul_add_avx512(out: &mut [u8], src: &[u8], coef: u8) {
        let (lo, hi) = nibble_tables(coef);
        // SAFETY: as mul_add_ssse3's, 64 bytes at a time.
        unsafe {
            let lo = _mm512_broadcast_i32x4(_mm_loadu_si128(lo.as_ptr().cast()));
            let hi = _mm512_broadcast_i32x4(_mm_loadu_si128(hi.as_ptr().cast()));
            let mask = _mm512_set1_epi8(0x0F);
            let mut i = 0;
            while i + 64 <= out.len() {
                let s = _mm512_loadu_si512(src.as_ptr().add(i).cast());
                let product = _mm512_xor_si512(_mm512_shuffle_epi8(lo, _mm512_and_si512(s, mask)), _mm512_shuffle_epi8(hi, _mm512_and_si512(_mm512_srli_epi16::<4>(s), mask)));
                let o = out.as_mut_ptr().add(i).cast();
                _mm512_storeu_si512(o, _mm512_xor_si512(_mm512_loadu_si512(o), product));
                i += 64;
            }
            mul_add_scalar(&mut out[i..], &src[i..], coef);
        }
    }

    /// GFNI at 256 bits: one affine instruction multiplies 32 bytes.
    #[target_feature(enable = "gfni,avx2")]
    pub unsafe fn mul_add_gfni256(out: &mut [u8], src: &[u8], coef: u8) {
        // SAFETY: as mul_add_avx2's.
        unsafe {
            let matrix = _mm256_set1_epi64x(affine_matrix(coef) as i64);
            let mut i = 0;
            while i + 32 <= out.len() {
                let product = _mm256_gf2p8affine_epi64_epi8::<0>(_mm256_loadu_si256(src.as_ptr().add(i).cast()), matrix);
                let o = out.as_mut_ptr().add(i).cast();
                _mm256_storeu_si256(o, _mm256_xor_si256(_mm256_loadu_si256(o), product));
                i += 32;
            }
            mul_add_scalar(&mut out[i..], &src[i..], coef);
        }
    }

    /// GFNI at 512 bits.
    #[target_feature(enable = "gfni,avx512f,avx512bw")]
    pub unsafe fn mul_add_gfni512(out: &mut [u8], src: &[u8], coef: u8) {
        // SAFETY: as mul_add_avx512's.
        unsafe {
            let matrix = _mm512_set1_epi64(affine_matrix(coef) as i64);
            let mut i = 0;
            while i + 64 <= out.len() {
                let product = _mm512_gf2p8affine_epi64_epi8::<0>(_mm512_loadu_si512(src.as_ptr().add(i).cast()), matrix);
                let o = out.as_mut_ptr().add(i).cast();
                _mm512_storeu_si512(o, _mm512_xor_si512(_mm512_loadu_si512(o), product));
                i += 64;
            }
            mul_add_scalar(&mut out[i..], &src[i..], coef);
        }
    }
}

#[cfg(any(target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon")))]
mod neon {
    use super::{mul_add_scalar, nibble_tables};
    use crate::simd::arch::*;

    /// TBL looks each nibble up, as PSHUFB does. The two slices have one
    /// length.
    #[cfg_attr(target_arch = "arm", target_feature(enable = "neon,v7"))]
    pub unsafe fn mul_add(out: &mut [u8], src: &[u8], coef: u8) {
        let (lo, hi) = nibble_tables(coef);
        // SAFETY: each load and store covers 16 bytes the loop guard checks
        // are in bounds of both slices.
        unsafe {
            let (lo, hi) = (vld1q_u8(lo.as_ptr()), vld1q_u8(hi.as_ptr()));
            let mask = vdupq_n_u8(0x0F);
            let mut i = 0;
            while i + 16 <= out.len() {
                let s = vld1q_u8(src.as_ptr().add(i));
                let product = veorq_u8(vqtbl1q_u8(lo, vandq_u8(s, mask)), vqtbl1q_u8(hi, vshrq_n_u8::<4>(s)));
                let o = out.as_mut_ptr().add(i);
                vst1q_u8(o, veorq_u8(vld1q_u8(o), product));
                i += 16;
            }
            mul_add_scalar(&mut out[i..], &src[i..], coef);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Body = Box<dyn Fn(&mut [u8], &[u8], u8)>;

    /// The bodies this CPU runs, by tier, whatever `PWRS_CPU_MAX` says;
    /// one it lacks is printed as skipped.
    fn bodies() -> Vec<(&'static str, Body)> {
        let mut out: Vec<(&'static str, Body)> = Vec::new();
        // SAFETY in each closure: listed only when this CPU offers the
        // instructions, and given slices of one length.
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            let mut add = |name: &'static str, ok: bool, body: Body| if ok { out.push((name, body)) } else { eprintln!("GF256 SKIPPED {name}: this CPU lacks it") };
            add("ssse3", detected(Isa::Ssse3), Box::new(|o, s, c| unsafe { x86::mul_add_ssse3(o, s, c) }));
            add("avx2", detected(Isa::Avx2), Box::new(|o, s, c| unsafe { x86::mul_add_avx2(o, s, c) }));
            add("avx512bw", detected(Isa::Avx512f) && detected(Isa::Avx512bw), Box::new(|o, s, c| unsafe { x86::mul_add_avx512(o, s, c) }));
            add("gfni256", detected(Isa::Gfni) && detected(Isa::Avx2), Box::new(|o, s, c| unsafe { x86::mul_add_gfni256(o, s, c) }));
            add("gfni512", detected(Isa::Gfni) && detected(Isa::Avx512f) && detected(Isa::Avx512bw), Box::new(|o, s, c| unsafe { x86::mul_add_gfni512(o, s, c) }));
        }
        #[cfg(any(target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon")))]
        if crate::simd::arch::neon_detected() {
            out.push(("neon", Box::new(|o, s, c| unsafe { neon::mul_add(o, s, c) })));
        } else {
            eprintln!("GF256 SKIPPED neon: this CPU lacks it");
        }
        out
    }

    #[test]
    fn the_field_has_its_inverses_and_the_affine_matrix_multiplies() {
        // Every nonzero element has an inverse, and 2 generates the field.
        for a in 1..=255u8 {
            assert_eq!((1..=255u8).filter(|&b| mul(a, b) == 1).count(), 1, "{a}");
        }
        assert_eq!(mul(2, 0x80), 0x1D);
        // The matrix GFNI applies multiplies as the tables do, emulated
        // here bit for bit: output bit i is the parity of matrix byte
        // 7 - i ANDed with the input.
        for coef in [0u8, 1, 2, 0x1D, 0x53, 0xFF] {
            let m = affine_matrix(coef);
            for x in 0..=255u8 {
                let mut y = 0u8;
                for i in 0..8 {
                    if ((((m >> (8 * (7 - i))) & 0xFF) as u8) & x).count_ones() & 1 == 1 {
                        y |= 1 << i;
                    }
                }
                assert_eq!(y, mul(coef, x), "coef {coef}, x {x}");
            }
        }
    }

    #[test]
    fn every_tier_adds_the_products_the_scalar_form_adds() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let src: Vec<u8> = (0..4099).map(|_| next() as u8).collect();
        let base: Vec<u8> = (0..4099).map(|_| next() as u8).collect();
        let bodies = bodies();
        assert!(!bodies.is_empty() || !cfg!(any(target_arch = "x86_64", target_arch = "aarch64")), "no body ran");
        for (tier, body) in &bodies {
            eprintln!("GF256 RUNNING mul_add at {tier}");
            for len in (0..70).chain([127, 128, 129, 1000, 4099]) {
                for coef in [1u8, 2, 3, 0x1D, 0x80, 0xFF] {
                    let mut want = base[..len].to_vec();
                    mul_add_scalar(&mut want, &src[..len], coef);
                    let mut got = base[..len].to_vec();
                    body(&mut got, &src[..len], coef);
                    assert_eq!(got, want, "{tier}: {len} bytes times {coef:#x}");
                }
            }
        }
        let mut out = base[..100].to_vec();
        mul_add(&mut out, &src[..100], 0x53);
        let mut want = base[..100].to_vec();
        mul_add_scalar(&mut want, &src[..100], 0x53);
        assert_eq!(out, want);
    }
}
