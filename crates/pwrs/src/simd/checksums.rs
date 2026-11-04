//! CRC-32C, the Castagnoli CRC that iSCSI, ext4 and SCTP use, and CRC-32,
//! the one zip, gzip and PNG use: [`crc32c`] and [`crc32c_append`],
//! [`crc32`] and [`crc32_append`]. A CRC is defined by its polynomial, so
//! every tier returns the value the scalar form, [`crc32c_append_scalar`]
//! or [`crc32_append_scalar`], computes, for every input.
//!
//! CRC-32C's tiers are SSE4.2's CRC32 instruction from x86-64-v2, eight
//! bytes at a time, and AArch64's CRC32C instructions where the CPU has
//! the `crc` feature, which no feature tier includes, so a `PWRS_CPU_MAX`
//! cap withholds it. 32-bit ARM's `arm-neon` feature takes the same
//! instructions, four bytes at a time, on an ARMv8 CPU that reports them,
//! for CRC-32 as well as CRC-32C: `core::arch::arm` has no PMULL to fold
//! with.
//!
//! CRC-32 has no instruction of its own on x86-64, so its tiers fold the
//! data with carry-less multiplication, as Intel's paper on computing CRCs
//! with PCLMULQDQ describes: four 128-bit registers at a time with
//! PCLMULQDQ and SSE4.1 from x86-64-v2, four 256-bit ones with VPCLMULQDQ
//! from x86-64-v3, four 512-bit ones from x86-64-v4, and four 128-bit ones
//! with AArch64's PMULL where the CPU has `aes`. Each folds the data in
//! sixteen-byte blocks down to one 128-bit remainder, reduces that to 32
//! bits by Barrett reduction, and takes the bytes after the last whole
//! block through the table. PCLMULQDQ, VPCLMULQDQ and `aes` are in no
//! level or tier, so a cap withholds them. The folding constants are
//! powers of x modulo the polynomial, computed below from the polynomial
//! itself.

// Only the tiers that fold use the polynomial arithmetic.
#![cfg_attr(not(any(target_arch = "x86_64", target_arch = "aarch64")), allow(dead_code))]

/// The reflected Castagnoli polynomial.
const POLY: u32 = 0x82F6_3B78;

/// The reflected polynomial of CRC-32 as zip, gzip and PNG use it.
const POLY_IEEE: u32 = 0xEDB8_8320;

/// The CRC-32C of each byte value, for the scalar form.
const TABLE: [u32; 256] = table(POLY);

/// The CRC-32 of each byte value, for the scalar form.
const TABLE_IEEE: [u32; 256] = table(POLY_IEEE);

const fn table(poly: u32) -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ poly } else { crc >> 1 };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// The CRC-32C of `data`: `0xE3069283` for `b"123456789"`.
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c_append(0, data)
}

/// The CRC-32C of the bytes `crc` was computed over followed by `data`, so
/// a stream can be checksummed a piece at a time; `crc32c_append(0, data)`
/// is `crc32c(data)`.
pub fn crc32c_append(crc: u32, data: &[u8]) -> u32 {
    !tiered::update(!crc, data)
}

/// The scalar form of [`crc32c_append`], one table lookup per byte: the
/// result every tier gives.
pub fn crc32c_append_scalar(crc: u32, data: &[u8]) -> u32 {
    !update_scalar(!crc, data)
}

/// The CRC-32 of `data`, as zip, gzip and PNG compute it: `0xCBF43926`
/// for `b"123456789"`.
pub fn crc32(data: &[u8]) -> u32 {
    crc32_append(0, data)
}

/// The CRC-32 of the bytes `crc` was computed over followed by `data`, so
/// a stream can be checksummed a piece at a time; `crc32_append(0, data)`
/// is `crc32(data)`.
pub fn crc32_append(crc: u32, data: &[u8]) -> u32 {
    !ieee::update(!crc, data)
}

/// The scalar form of [`crc32_append`], one table lookup per byte: the
/// result every tier gives.
pub fn crc32_append_scalar(crc: u32, data: &[u8]) -> u32 {
    !update_scalar_ieee(!crc, data)
}

/// The register after `data`, from `state`, before the final inversion.
fn update_scalar(mut state: u32, data: &[u8]) -> u32 {
    for &b in data {
        state = (state >> 8) ^ TABLE[((state ^ u32::from(b)) & 0xFF) as usize];
    }
    state
}

/// [`update_scalar`] for CRC-32.
fn update_scalar_ieee(mut state: u32, data: &[u8]) -> u32 {
    for &b in data {
        state = (state >> 8) ^ TABLE_IEEE[((state ^ u32::from(b)) & 0xFF) as usize];
    }
    state
}

/// CRC-32's polynomial with its x^32 term: P(x) = x^32 + x^26 + ... + 1.
const P_IEEE: u64 = 0x1_04C1_1DB7;

/// x^n modulo P(x) over GF(2), a polynomial below degree 32.
const fn xpow_mod(n: u32) -> u32 {
    let mut r: u64 = 1;
    let mut i = 0;
    while i < n {
        r <<= 1;
        if r & (1 << 32) != 0 {
            r ^= P_IEEE;
        }
        i += 1;
    }
    r as u32
}

/// x^n mod P(x) in the bit-reflected form folding multiplies by, shifted
/// left one bit, as a carry-less product of reflected values needs.
const fn fold_k(n: u32) -> u64 {
    (xpow_mod(n).reverse_bits() as u64) << 1
}

/// The pair that folds a 128-bit remainder `distance` bits forward: its
/// low 64 bits multiply by x^(distance + 32) mod P(x), its high 64 bits by
/// x^(distance - 32) mod P(x).
const fn fold_pair(distance: u32) -> (u64, u64) {
    (fold_k(distance + 32), fold_k(distance - 32))
}

/// The 33 bits of `v` in reverse order.
const fn reflect33(v: u64) -> u64 {
    (v.reverse_bits() >> 31) & 0x1_FFFF_FFFF
}

/// floor(x^64 / P(x)), the Barrett reduction's multiplier.
const fn barrett_mu() -> u64 {
    let mut rem: u128 = 1u128 << 64;
    let mut q: u64 = 0;
    let mut bit: i32 = 32;
    while bit >= 0 {
        if rem & (1u128 << (32 + bit)) != 0 {
            rem ^= (P_IEEE as u128) << bit;
            q |= 1 << bit;
        }
        bit -= 1;
    }
    q
}

/// Folds four 128-bit registers 512 bits forward.
const FOLD_4X128: (u64, u64) = fold_pair(4 * 128);
/// Folds one 128-bit register 128 bits forward.
const FOLD_1X128: (u64, u64) = fold_pair(128);
/// x^64 mod P(x), which takes 96 bits to 64.
const K_64: u64 = fold_k(64);
/// The reflected polynomial, 33 bits.
const P_REFLECTED: u64 = reflect33(P_IEEE);
/// The reflected Barrett multiplier, 33 bits.
const MU_REFLECTED: u64 = reflect33(barrett_mu());

/// The CRC register a 128-bit remainder `x` leaves, given the carry-less
/// product of two 64-bit values: folded to 96 bits, then to 64, then
/// reduced to 32 by Barrett reduction, all in the reflected domain.
#[inline(always)]
fn reduce_128(x: u128, clmul: impl Fn(u64, u64) -> u128) -> u32 {
    const LOW32: u128 = 0x0000_0000_FFFF_FFFF_0000_0000_FFFF_FFFF;
    // The low 64 bits move 96 bits forward.
    let x = (x >> 64) ^ clmul(x as u64, FOLD_1X128.1);
    // The low 32 bits move 64 bits forward.
    let x = (x >> 32) ^ clmul((x & LOW32) as u64, K_64);
    // The quotient estimated from the low 32 bits, times the polynomial,
    // cancels them and leaves the remainder above them.
    let t = clmul((x & LOW32) as u64, MU_REFLECTED) & LOW32;
    let x = x ^ clmul(t as u64, P_REFLECTED);
    (x >> 32) as u32
}

#[cfg(target_arch = "x86_64")]
mod tiered {
    use crate::cpu::{level, Level};

    // x86-64-v2 includes SSE4.2, so the level reaches it only when the CPU
    // offers SSE4.2.
    pub fn update(state: u32, data: &[u8]) -> u32 {
        if level() >= Level::V2 { unsafe { super::x86::update_sse42(state, data) } } else { super::update_scalar(state, data) }
    }
}

#[cfg(target_arch = "aarch64")]
mod tiered {
    use crate::cpu::arm::{has, Feature};

    pub fn update(state: u32, data: &[u8]) -> u32 {
        if has(Feature::Crc) { unsafe { super::arm::update_crc(state, data) } } else { super::update_scalar(state, data) }
    }
}

#[cfg(all(target_arch = "arm", feature = "arm-neon"))]
mod tiered {
    use crate::cpu::arm32::{has, Feature};

    pub fn update(state: u32, data: &[u8]) -> u32 {
        if has(Feature::Crc32) { unsafe { super::arm::update_crc(state, data) } } else { super::update_scalar(state, data) }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon"))))]
mod tiered {
    pub fn update(state: u32, data: &[u8]) -> u32 {
        super::update_scalar(state, data)
    }
}

#[cfg(target_arch = "x86_64")]
mod ieee {
    use crate::cpu::{has, level, Isa, Level};

    // Each folding body needs carry-less multiplication besides its level:
    // PCLMULQDQ with SSE4.1 from x86-64-v2, VPCLMULQDQ with AVX2 from v3
    // and with AVX-512 from v4.
    pub fn update(state: u32, data: &[u8]) -> u32 {
        let level = level();
        if level >= Level::V4 && has(Isa::Vpclmulqdq) {
            unsafe { super::x86::update_vpclmul512(state, data) }
        } else if level >= Level::V3 && has(Isa::Vpclmulqdq) {
            unsafe { super::x86::update_vpclmul256(state, data) }
        } else if level >= Level::V2 && has(Isa::Pclmulqdq) {
            unsafe { super::x86::update_pclmul(state, data) }
        } else {
            super::update_scalar_ieee(state, data)
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod ieee {
    use crate::cpu::arm::{has, Feature};

    // PMULL comes with the `aes` feature.
    pub fn update(state: u32, data: &[u8]) -> u32 {
        if has(Feature::Aes) { unsafe { super::arm::update_pmull(state, data) } } else { super::update_scalar_ieee(state, data) }
    }
}

#[cfg(all(target_arch = "arm", feature = "arm-neon"))]
mod ieee {
    use crate::cpu::arm32::{has, Feature};

    pub fn update(state: u32, data: &[u8]) -> u32 {
        if has(Feature::Crc32) { unsafe { super::arm::update_crc_ieee(state, data) } } else { super::update_scalar_ieee(state, data) }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon"))))]
mod ieee {
    pub fn update(state: u32, data: &[u8]) -> u32 {
        super::update_scalar_ieee(state, data)
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use core::arch::x86_64::*;

    use super::{fold_pair, reduce_128, update_scalar_ieee, FOLD_1X128, FOLD_4X128};

    /// Folds four 256-bit registers 1024 bits forward.
    const FOLD_4X256: (u64, u64) = fold_pair(4 * 256);
    /// Folds one 256-bit register 256 bits forward, as it folds a 512-bit
    /// register's lane 1 into lane 3.
    const FOLD_1X256: (u64, u64) = fold_pair(256);
    /// Folds four 512-bit registers 2048 bits forward.
    const FOLD_4X512: (u64, u64) = fold_pair(4 * 512);
    /// Folds one 512-bit register 512 bits forward.
    const FOLD_1X512: (u64, u64) = fold_pair(512);
    /// Folds a 512-bit register's lane 0 into lane 3, 384 bits forward.
    const FOLD_3X128: (u64, u64) = fold_pair(3 * 128);

    #[target_feature(enable = "sse4.2")]
    pub unsafe fn update_sse42(state: u32, data: &[u8]) -> u32 {
        let mut crc = u64::from(state);
        let (words, rest) = data.as_chunks::<8>();
        for word in words {
            crc = _mm_crc32_u64(crc, u64::from_le_bytes(*word));
        }
        let mut crc = crc as u32;
        for &b in rest {
            crc = _mm_crc32_u8(crc, b);
        }
        crc
    }

    /// A fold pair as a register, the low constant in the low 64 bits.
    #[inline]
    #[target_feature(enable = "sse2")]
    fn pair(k: (u64, u64)) -> __m128i {
        _mm_set_epi64x(k.1 as i64, k.0 as i64)
    }

    /// `x` folded forward by the pair `k`, plus the next block `next`.
    #[inline]
    #[target_feature(enable = "pclmulqdq")]
    fn fold(x: __m128i, k: __m128i, next: __m128i) -> __m128i {
        _mm_xor_si128(_mm_xor_si128(_mm_clmulepi64_si128::<0x00>(x, k), _mm_clmulepi64_si128::<0x11>(x, k)), next)
    }

    /// The sixteen bytes of `data` from `at`.
    ///
    /// # Safety
    ///
    /// `at + 16` is at most `data.len()`.
    #[inline(always)]
    unsafe fn load(data: &[u8], at: usize) -> __m128i {
        // SAFETY: the caller has checked that the sixteen bytes are inside `data`.
        unsafe { _mm_loadu_si128(data.as_ptr().add(at).cast()) }
    }

    /// The carry-less product of `a` and `b`.
    #[inline]
    #[target_feature(enable = "pclmulqdq,sse4.1")]
    fn clmul(a: u64, b: u64) -> u128 {
        let p = _mm_clmulepi64_si128::<0x00>(_mm_set_epi64x(0, a as i64), _mm_set_epi64x(0, b as i64));
        u128::from(_mm_extract_epi64::<0>(p) as u64) | (u128::from(_mm_extract_epi64::<1>(p) as u64) << 64)
    }

    /// Folds the sixteen-byte blocks of `data` from `at` into `x`, reduces
    /// the remainder to the CRC register, and takes the bytes after the
    /// last whole block through the table.
    #[inline]
    #[target_feature(enable = "pclmulqdq,sse4.1")]
    fn finish(mut x: __m128i, data: &[u8], mut at: usize) -> u32 {
        let k = pair(FOLD_1X128);
        while data.len() - at >= 16 {
            // SAFETY: the loop's guard checks that the block is inside `data`.
            x = fold(x, k, unsafe { load(data, at) });
            at += 16;
        }
        let x = u128::from(_mm_extract_epi64::<0>(x) as u64) | (u128::from(_mm_extract_epi64::<1>(x) as u64) << 64);
        let state = reduce_128(x, |a, b| clmul(a, b));
        update_scalar_ieee(state, &data[at..])
    }

    /// Four 128-bit registers fold 64 bytes at a time, then into one; for
    /// `data` of 64 bytes or more.
    #[inline]
    #[target_feature(enable = "pclmulqdq,sse4.1")]
    fn fold_128(state: u32, data: &[u8]) -> u32 {
        // SAFETY: every load stays inside `data`, which has 64 bytes or
        // more, and the loop's guard checks that each block of 64 is inside it.
        unsafe {
            let mut x0 = _mm_xor_si128(load(data, 0), _mm_cvtsi32_si128(state as i32));
            let mut x1 = load(data, 16);
            let mut x2 = load(data, 32);
            let mut x3 = load(data, 48);
            let mut at = 64;
            let k4 = pair(FOLD_4X128);
            while data.len() - at >= 64 {
                x0 = fold(x0, k4, load(data, at));
                x1 = fold(x1, k4, load(data, at + 16));
                x2 = fold(x2, k4, load(data, at + 32));
                x3 = fold(x3, k4, load(data, at + 48));
                at += 64;
            }
            let k1 = pair(FOLD_1X128);
            finish(fold(fold(fold(x0, k1, x1), k1, x2), k1, x3), data, at)
        }
    }

    #[target_feature(enable = "pclmulqdq,sse4.1")]
    pub unsafe fn update_pclmul(state: u32, data: &[u8]) -> u32 {
        if data.len() < 64 { update_scalar_ieee(state, data) } else { fold_128(state, data) }
    }

    /// A fold pair in each 128-bit lane of a 256-bit register.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn pair256(k: (u64, u64)) -> __m256i {
        _mm256_broadcastsi128_si256(pair(k))
    }

    #[inline]
    #[target_feature(enable = "vpclmulqdq,avx2")]
    fn fold256(x: __m256i, k: __m256i, next: __m256i) -> __m256i {
        _mm256_xor_si256(_mm256_xor_si256(_mm256_clmulepi64_epi128::<0x00>(x, k), _mm256_clmulepi64_epi128::<0x11>(x, k)), next)
    }

    /// The 32 bytes of `data` from `at`.
    ///
    /// # Safety
    ///
    /// `at + 32` is at most `data.len()`, and the CPU has AVX.
    #[inline(always)]
    unsafe fn load256(data: &[u8], at: usize) -> __m256i {
        // SAFETY: the caller has checked that the 32 bytes are inside `data`,
        // and runs where AVX is.
        unsafe { _mm256_loadu_si256(data.as_ptr().add(at).cast()) }
    }

    #[target_feature(enable = "vpclmulqdq,pclmulqdq,avx2,sse4.1")]
    pub unsafe fn update_vpclmul256(state: u32, data: &[u8]) -> u32 {
        if data.len() < 128 {
            return if data.len() < 64 { update_scalar_ieee(state, data) } else { fold_128(state, data) };
        }
        // SAFETY: every load stays inside `data`, which has 128 bytes or
        // more, and each loop's guard checks that its blocks are inside it.
        unsafe {
            let mut y0 = _mm256_xor_si256(load256(data, 0), _mm256_setr_epi32(state as i32, 0, 0, 0, 0, 0, 0, 0));
            let mut y1 = load256(data, 32);
            let mut y2 = load256(data, 64);
            let mut y3 = load256(data, 96);
            let mut at = 128;
            let k4 = pair256(FOLD_4X256);
            while data.len() - at >= 128 {
                y0 = fold256(y0, k4, load256(data, at));
                y1 = fold256(y1, k4, load256(data, at + 32));
                y2 = fold256(y2, k4, load256(data, at + 64));
                y3 = fold256(y3, k4, load256(data, at + 96));
                at += 128;
            }
            let k1 = pair256(FOLD_1X256);
            let mut y = fold256(fold256(fold256(y0, k1, y1), k1, y2), k1, y3);
            while data.len() - at >= 32 {
                y = fold256(y, k1, load256(data, at));
                at += 32;
            }
            // The low lane has the earlier bytes and moves 128 bits
            // forward into the high one.
            let x = fold(_mm256_castsi256_si128(y), pair(FOLD_1X128), _mm256_extracti128_si256::<1>(y));
            finish(x, data, at)
        }
    }

    /// A fold pair in each 128-bit lane of a 512-bit register.
    #[inline]
    #[target_feature(enable = "avx512f")]
    fn pair512(k: (u64, u64)) -> __m512i {
        _mm512_broadcast_i32x4(pair(k))
    }

    #[inline]
    #[target_feature(enable = "vpclmulqdq,avx512f")]
    fn fold512(x: __m512i, k: __m512i, next: __m512i) -> __m512i {
        _mm512_xor_si512(_mm512_xor_si512(_mm512_clmulepi64_epi128::<0x00>(x, k), _mm512_clmulepi64_epi128::<0x11>(x, k)), next)
    }

    /// The 64 bytes of `data` from `at`.
    ///
    /// # Safety
    ///
    /// `at + 64` is at most `data.len()`, and the CPU has AVX-512F.
    #[inline(always)]
    unsafe fn load512(data: &[u8], at: usize) -> __m512i {
        // SAFETY: the caller has checked that the 64 bytes are inside `data`,
        // and runs where AVX-512F is.
        unsafe { _mm512_loadu_si512(data.as_ptr().add(at).cast()) }
    }

    #[target_feature(enable = "vpclmulqdq,pclmulqdq,avx512f,avx2,sse4.1")]
    pub unsafe fn update_vpclmul512(state: u32, data: &[u8]) -> u32 {
        if data.len() < 256 {
            return if data.len() < 64 { update_scalar_ieee(state, data) } else { fold_128(state, data) };
        }
        // SAFETY: every load stays inside `data`, which has 256 bytes or
        // more, and each loop's guard checks that its blocks are inside it.
        unsafe {
            let first = _mm512_setr_epi32(state as i32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            let mut z0 = _mm512_xor_si512(load512(data, 0), first);
            let mut z1 = load512(data, 64);
            let mut z2 = load512(data, 128);
            let mut z3 = load512(data, 192);
            let mut at = 256;
            let k4 = pair512(FOLD_4X512);
            while data.len() - at >= 256 {
                z0 = fold512(z0, k4, load512(data, at));
                z1 = fold512(z1, k4, load512(data, at + 64));
                z2 = fold512(z2, k4, load512(data, at + 128));
                z3 = fold512(z3, k4, load512(data, at + 192));
                at += 256;
            }
            let k1 = pair512(FOLD_1X512);
            let mut z = fold512(fold512(fold512(z0, k1, z1), k1, z2), k1, z3);
            while data.len() - at >= 64 {
                z = fold512(z, k1, load512(data, at));
                at += 64;
            }
            // Lane 0 has the earliest bytes; lane i moves (3 - i) * 128
            // bits forward into lane 3.
            let zero = _mm_setzero_si128();
            let x = _mm_xor_si128(
                _mm_xor_si128(
                    fold(_mm512_extracti32x4_epi32::<0>(z), pair(FOLD_3X128), zero),
                    fold(_mm512_extracti32x4_epi32::<1>(z), pair(FOLD_1X256), zero),
                ),
                fold(_mm512_extracti32x4_epi32::<2>(z), pair(FOLD_1X128), _mm512_extracti32x4_epi32::<3>(z)),
            );
            finish(x, data, at)
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod arm {
    use core::arch::aarch64::*;

    use super::{reduce_128, update_scalar_ieee, FOLD_1X128, FOLD_4X128};

    #[target_feature(enable = "crc")]
    pub unsafe fn update_crc(state: u32, data: &[u8]) -> u32 {
        let mut crc = state;
        let (words, rest) = data.as_chunks::<8>();
        for word in words {
            crc = __crc32cd(crc, u64::from_le_bytes(*word));
        }
        for &b in rest {
            crc = __crc32cb(crc, b);
        }
        crc
    }

    /// A fold pair as a register, the low constant in lane 0.
    #[inline]
    #[target_feature(enable = "neon,aes")]
    fn pair(k: (u64, u64)) -> poly64x2_t {
        vcombine_p64(vcreate_p64(k.0), vcreate_p64(k.1))
    }

    /// `x` folded forward by the pair `k`, plus the next block `next`:
    /// PMULL on the low halves, PMULL2 on the high ones.
    #[inline]
    #[target_feature(enable = "neon,aes")]
    fn fold(x: uint8x16_t, k: poly64x2_t, next: uint8x16_t) -> uint8x16_t {
        let x = vreinterpretq_p64_u8(x);
        let low = vreinterpretq_u8_p128(vmull_p64(vgetq_lane_p64::<0>(x), vgetq_lane_p64::<0>(k)));
        let high = vreinterpretq_u8_p128(vmull_high_p64(x, k));
        veorq_u8(veorq_u8(low, high), next)
    }

    /// The sixteen bytes of `data` from `at`.
    ///
    /// # Safety
    ///
    /// `at + 16` is at most `data.len()`.
    #[inline(always)]
    unsafe fn load(data: &[u8], at: usize) -> uint8x16_t {
        // SAFETY: the caller has checked that the sixteen bytes are inside `data`.
        unsafe { vld1q_u8(data.as_ptr().add(at)) }
    }

    #[target_feature(enable = "neon,aes")]
    pub unsafe fn update_pmull(state: u32, data: &[u8]) -> u32 {
        if data.len() < 64 {
            return update_scalar_ieee(state, data);
        }
        // SAFETY: every load stays inside `data`, which has 64 bytes or
        // more, and each loop's guard checks that its blocks are inside it.
        unsafe {
            let first = vreinterpretq_u8_u32(vsetq_lane_u32::<0>(state, vdupq_n_u32(0)));
            let mut x0 = veorq_u8(load(data, 0), first);
            let mut x1 = load(data, 16);
            let mut x2 = load(data, 32);
            let mut x3 = load(data, 48);
            let mut at = 64;
            let k4 = pair(FOLD_4X128);
            while data.len() - at >= 64 {
                x0 = fold(x0, k4, load(data, at));
                x1 = fold(x1, k4, load(data, at + 16));
                x2 = fold(x2, k4, load(data, at + 32));
                x3 = fold(x3, k4, load(data, at + 48));
                at += 64;
            }
            let k1 = pair(FOLD_1X128);
            let mut x = fold(fold(fold(x0, k1, x1), k1, x2), k1, x3);
            while data.len() - at >= 16 {
                x = fold(x, k1, load(data, at));
                at += 16;
            }
            let state = reduce_128(vreinterpretq_p128_u8(x), |a, b| vmull_p64(a, b));
            update_scalar_ieee(state, &data[at..])
        }
    }
}

#[cfg(all(target_arch = "arm", feature = "arm-neon"))]
mod arm {
    use core::arch::arm::*;

    #[target_feature(enable = "crc,v8")]
    pub unsafe fn update_crc(state: u32, data: &[u8]) -> u32 {
        let mut crc = state;
        let (words, rest) = data.as_chunks::<4>();
        for word in words {
            crc = __crc32cw(crc, u32::from_le_bytes(*word));
        }
        for &b in rest {
            crc = __crc32cb(crc, b);
        }
        crc
    }

    #[target_feature(enable = "crc,v8")]
    pub unsafe fn update_crc_ieee(state: u32, data: &[u8]) -> u32 {
        let mut crc = state;
        let (words, rest) = data.as_chunks::<4>();
        for word in words {
            crc = __crc32w(crc, u32::from_le_bytes(*word));
        }
        for &b in rest {
            crc = __crc32b(crc, b);
        }
        crc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A register update, with the name of its tier as the tests print it.
    type Body = (&'static str, Box<dyn Fn(u32, &[u8]) -> u32>);

    /// The register updates this CPU runs, by tier, whatever
    /// `PWRS_CPU_MAX` says; one it lacks is printed as skipped.
    fn bodies() -> Vec<Body> {
        let mut out: Vec<Body> = vec![("scalar", Box::new(update_scalar))];
        #[cfg(target_arch = "x86_64")]
        {
            if crate::cpu::detected(crate::cpu::Isa::Sse42) {
                // SAFETY: listed only when this CPU offers SSE4.2.
                out.push(("sse4.2", Box::new(|s, d| unsafe { x86::update_sse42(s, d) })));
            } else {
                eprintln!("CHECKSUMS SKIPPED sse4.2: this CPU lacks it");
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if crate::cpu::arm::detected(crate::cpu::arm::Feature::Crc) {
                // SAFETY: listed only when this CPU offers the CRC instructions.
                out.push(("crc", Box::new(|s, d| unsafe { arm::update_crc(s, d) })));
            } else {
                eprintln!("CHECKSUMS SKIPPED crc: this CPU lacks it");
            }
        }
        #[cfg(all(target_arch = "arm", feature = "arm-neon"))]
        {
            if crate::cpu::arm32::detected(crate::cpu::arm32::Feature::Crc32) {
                // SAFETY: listed only when this CPU offers the CRC instructions.
                out.push(("crc", Box::new(|s, d| unsafe { arm::update_crc(s, d) })));
            } else {
                eprintln!("CHECKSUMS SKIPPED crc: this CPU lacks it");
            }
        }
        out
    }

    /// CRC-32's register updates this CPU runs, by tier, whatever
    /// `PWRS_CPU_MAX` says; one it lacks is printed as skipped.
    fn ieee_bodies() -> Vec<Body> {
        let mut out: Vec<Body> = vec![("scalar", Box::new(update_scalar_ieee))];
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            let all = |isas: &[Isa]| isas.iter().all(|&i| detected(i));
            if all(&[Isa::Pclmulqdq, Isa::Sse41]) {
                // SAFETY: listed only when this CPU offers every extension the body enables.
                out.push(("pclmulqdq", Box::new(|s, d| unsafe { x86::update_pclmul(s, d) })));
            } else {
                eprintln!("CHECKSUMS SKIPPED crc32 pclmulqdq: this CPU lacks it");
            }
            if all(&[Isa::Vpclmulqdq, Isa::Pclmulqdq, Isa::Avx2, Isa::Sse41]) {
                // SAFETY: as above.
                out.push(("vpclmulqdq-256", Box::new(|s, d| unsafe { x86::update_vpclmul256(s, d) })));
            } else {
                eprintln!("CHECKSUMS SKIPPED crc32 vpclmulqdq-256: this CPU lacks it");
            }
            if all(&[Isa::Vpclmulqdq, Isa::Pclmulqdq, Isa::Avx512f, Isa::Avx2, Isa::Sse41]) {
                // SAFETY: as above.
                out.push(("vpclmulqdq-512", Box::new(|s, d| unsafe { x86::update_vpclmul512(s, d) })));
            } else {
                eprintln!("CHECKSUMS SKIPPED crc32 vpclmulqdq-512: this CPU lacks it");
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if crate::cpu::arm::detected(crate::cpu::arm::Feature::Aes) {
                // SAFETY: listed only when this CPU offers PMULL, which `aes` covers.
                out.push(("pmull", Box::new(|s, d| unsafe { arm::update_pmull(s, d) })));
            } else {
                eprintln!("CHECKSUMS SKIPPED crc32 pmull: this CPU lacks it");
            }
        }
        #[cfg(all(target_arch = "arm", feature = "arm-neon"))]
        {
            if crate::cpu::arm32::detected(crate::cpu::arm32::Feature::Crc32) {
                // SAFETY: listed only when this CPU offers the CRC instructions.
                out.push(("crc", Box::new(|s, d| unsafe { arm::update_crc_ieee(s, d) })));
            } else {
                eprintln!("CHECKSUMS SKIPPED crc32 crc: this CPU lacks it");
            }
        }
        out
    }

    fn sample(len: usize) -> Vec<u8> {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        (0..len).map(|_| next() as u8).collect()
    }

    #[test]
    fn every_tier_gives_the_scalar_crc() {
        let data = sample(5000);
        let bodies = bodies();
        assert!(bodies.len() > 1 || !cfg!(any(target_arch = "x86_64", target_arch = "aarch64")), "only the scalar form ran");
        for (tier, update) in &bodies {
            eprintln!("CHECKSUMS RUNNING crc32c at {tier}");
            for len in (0..80).chain([255, 256, 1023, 4099, 5000]) {
                for start in [0usize, 1, 3, 7] {
                    let piece = &data[start.min(data.len())..(start + len).min(data.len())];
                    for seed in [0u32, 0xFFFF_FFFF, 0x1234_5678] {
                        assert_eq!(update(seed, piece), update_scalar(seed, piece), "{tier}: {} bytes from {start}, seed {seed:#x}", piece.len());
                    }
                }
            }
        }
    }

    /// Every length up to 600 bytes crosses each body's thresholds, 64,
    /// 128 and 256 bytes, and leaves every remainder of 16, 32 and 64;
    /// the longer ones run each folding loop many times.
    #[test]
    fn every_crc32_tier_gives_the_scalar_crc() {
        let data = sample(9000);
        let bodies = ieee_bodies();
        assert!(bodies.len() > 1 || !cfg!(any(target_arch = "x86_64", target_arch = "aarch64")), "only the scalar form ran");
        for (tier, update) in &bodies {
            let mut cases = 0;
            for len in (0..600).chain([1023, 1024, 4095, 4099, 8191, 9000]) {
                for start in [0usize, 1, 3, 7] {
                    let piece = &data[start.min(data.len())..(start + len).min(data.len())];
                    for seed in [0u32, 0xFFFF_FFFF, 0x1234_5678] {
                        assert_eq!(update(seed, piece), update_scalar_ieee(seed, piece), "{tier}: {} bytes from {start}, seed {seed:#x}", piece.len());
                        cases += 1;
                    }
                }
            }
            eprintln!("CHECKSUMS RUNNING crc32 at {tier} over {cases} cases");
        }
    }

    #[test]
    fn the_check_value_holds_and_a_stream_appends() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c_append_scalar(0, b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(b""), 0);
        let whole = crc32c(b"The quick brown fox jumps over the lazy dog");
        let pieces = crc32c_append(crc32c(b"The quick brown "), b"fox jumps over the lazy dog");
        assert_eq!(pieces, whole);
        assert_eq!(whole, 0x2262_0404);
    }

    #[test]
    fn crc32_check_values_hold_and_a_stream_appends() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32_append_scalar(0, b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
        assert_eq!(crc32_append(crc32(b"The quick brown "), b"fox jumps over the lazy dog"), 0x414F_A339);
        // Long enough for every folding body's widest loop.
        let long = sample(100_000);
        assert_eq!(crc32(&long), crc32_append_scalar(0, &long));
        assert_eq!(crc32_append(crc32(&long[..33_333]), &long[33_333..]), crc32(&long));
    }

    /// The constants computed from the polynomial are the ones Intel's
    /// paper on computing CRCs with PCLMULQDQ publishes for this CRC.
    #[test]
    fn the_folding_constants_are_the_published_ones() {
        assert_eq!(FOLD_4X128, (0x01_5444_2bd4, 0x01_c6e4_1596));
        assert_eq!(FOLD_1X128, (0x01_7519_97d0, 0x00_ccaa_009e));
        assert_eq!(K_64, 0x01_63cd_6124);
        assert_eq!(P_REFLECTED, 0x01_db71_0641);
        assert_eq!(MU_REFLECTED, 0x01_f701_1641);
    }
}
