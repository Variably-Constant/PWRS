//! Columnar filters: an `i32` column compared with a value ([`eq_i32`]), a
//! range ([`range_i32`]) or a set ([`in_i32`]), with a bitmap of the rows
//! that pass as the result. At every tier, for every input, each returns the bitmap
//! its scalar form returns.
//!
//! A bitmap stores row `i` as bit `i % 64` of word `i / 64`, the lowest bit
//! first, one word for each 64 rows or part of 64, and its bits past the
//! last row are zero. [`ints::and_u64`](super::ints::and_u64),
//! [`or_u64`](super::ints::or_u64) and
//! [`and_not_u64`](super::ints::and_not_u64) combine two filters' bitmaps
//! and [`ints::count_ones`](super::ints::count_ones) counts the rows that
//! passed; [`indices`] and [`indices_page`], which walk a bitmap one set
//! bit at a time and have no tier bodies, give their positions.
//!
//! The tiers compare 4 rows at a time on the x86-64 baseline and on NEON, 8
//! under x86-64-v3 and 16 under x86-64-v4, and gather 64 rows' results into
//! each word.

/// The bitmap of the rows of `column` equal to `value`.
pub fn eq_i32(column: &[i32], value: i32) -> Vec<u64> {
    tiered::eq(column, value)
}

/// The scalar form of [`eq_i32`]: the result every tier gives.
pub fn eq_i32_scalar(column: &[i32], value: i32) -> Vec<u64> {
    bitmap(column, |x| x == value)
}

/// The bitmap of the rows of `column` from `lo` to `hi`, both included;
/// no row passes where `lo` is above `hi`.
pub fn range_i32(column: &[i32], lo: i32, hi: i32) -> Vec<u64> {
    tiered::range(column, lo, hi)
}

/// The scalar form of [`range_i32`]: the result every tier gives.
pub fn range_i32_scalar(column: &[i32], lo: i32, hi: i32) -> Vec<u64> {
    bitmap(column, |x| lo <= x && x <= hi)
}

/// The bitmap of the rows of `column` equal to any of `values`; no row
/// passes where `values` is empty.
pub fn in_i32(column: &[i32], values: &[i32]) -> Vec<u64> {
    tiered::any_of(column, values)
}

/// The scalar form of [`in_i32`]: the result every tier gives.
pub fn in_i32_scalar(column: &[i32], values: &[i32]) -> Vec<u64> {
    bitmap(column, |x| values.contains(&x))
}

/// The positions of the set bits of `bitmap` below `n`, in order: the rows
/// a filter of an `n`-row column passed.
pub fn indices(bitmap: &[u64], n: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for (w, &word) in bitmap.iter().enumerate() {
        let mut bits = word;
        while bits != 0 {
            let i = (w << 6) + bits.trailing_zeros() as usize;
            if i >= n {
                return out;
            }
            out.push(i);
            bits &= bits - 1;
        }
    }
    out
}

/// [`indices`] after passing over the first `skip` of them, at most `take`
/// of them: one page of the rows a filter passed. A word whose set bits all
/// fall before the page is passed over by its count, without visiting its
/// bits.
pub fn indices_page(bitmap: &[u64], n: usize, skip: usize, take: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut skip = skip;
    for (w, &word) in bitmap.iter().enumerate() {
        if out.len() == take || w << 6 >= n {
            break;
        }
        // Only rows below `n` count, so the word with the last row is masked
        // to the bits of rows below `n`.
        let live = n - (w << 6);
        let mut bits = if live < 64 { word & ((1u64 << live) - 1) } else { word };
        let count = bits.count_ones() as usize;
        if skip >= count {
            skip -= count;
            continue;
        }
        while bits != 0 && out.len() < take {
            if skip > 0 {
                skip -= 1;
            } else {
                out.push((w << 6) + bits.trailing_zeros() as usize);
            }
            bits &= bits - 1;
        }
    }
    out
}

/// The bitmap of the rows of `column` that pass, one row at a time.
fn bitmap(column: &[i32], pass: impl Fn(i32) -> bool) -> Vec<u64> {
    let mut out = vec![0u64; column.len().div_ceil(64)];
    tail(&mut out, column, 0, pass);
    out
}

/// The rows from `from` on, one at a time, into a bitmap whose words below
/// them are written.
fn tail(out: &mut [u64], column: &[i32], from: usize, pass: impl Fn(i32) -> bool) {
    for (i, &x) in column.iter().enumerate().skip(from) {
        if pass(x) {
            out[i >> 6] |= 1 << (i & 63);
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod tiered {
    use super::x86;
    use crate::cpu::{level, Level};

    // A level is reached only when the CPU and the operating system offer
    // every extension of it; SSE2 is in every x86-64 CPU.
    pub fn eq(column: &[i32], value: i32) -> Vec<u64> {
        match level() {
            Level::V4 => unsafe { x86::eq_avx512(column, value) },
            Level::V3 => unsafe { x86::eq_avx2(column, value) },
            _baseline => unsafe { x86::eq_sse2(column, value) },
        }
    }

    pub fn range(column: &[i32], lo: i32, hi: i32) -> Vec<u64> {
        match level() {
            Level::V4 => unsafe { x86::range_avx512(column, lo, hi) },
            Level::V3 => unsafe { x86::range_avx2(column, lo, hi) },
            _baseline => unsafe { x86::range_sse2(column, lo, hi) },
        }
    }

    pub fn any_of(column: &[i32], values: &[i32]) -> Vec<u64> {
        match level() {
            Level::V4 => unsafe { x86::in_avx512(column, values) },
            Level::V3 => unsafe { x86::in_avx2(column, values) },
            _baseline => unsafe { x86::in_sse2(column, values) },
        }
    }
}

// NEON is in every AArch64 CPU, and optional on 32-bit ARM.
#[cfg(any(target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon")))]
mod tiered {
    use super::neon;
    use crate::simd::arch::has_neon;

    pub fn eq(column: &[i32], value: i32) -> Vec<u64> {
        if has_neon() { unsafe { neon::eq(column, value) } } else { super::eq_i32_scalar(column, value) }
    }

    pub fn range(column: &[i32], lo: i32, hi: i32) -> Vec<u64> {
        if has_neon() { unsafe { neon::range(column, lo, hi) } } else { super::range_i32_scalar(column, lo, hi) }
    }

    pub fn any_of(column: &[i32], values: &[i32]) -> Vec<u64> {
        if has_neon() { unsafe { neon::any_of(column, values) } } else { super::in_i32_scalar(column, values) }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon"))))]
mod tiered {
    pub fn eq(column: &[i32], value: i32) -> Vec<u64> {
        super::eq_i32_scalar(column, value)
    }

    pub fn range(column: &[i32], lo: i32, hi: i32) -> Vec<u64> {
        super::range_i32_scalar(column, lo, hi)
    }

    pub fn any_of(column: &[i32], values: &[i32]) -> Vec<u64> {
        super::in_i32_scalar(column, values)
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::tail;
    use core::arch::x86_64::*;

    // Each scan reads a whole word's 64 rows from an offset its loop guard
    // checks is 64 or more short of the end, and leaves the rest to `tail`.
    // `hit` gives a compare's lanes, all ones where a row passes. A range
    // takes the rows neither below `lo` nor above `hi`, which needs no
    // bound moved by one and so works at both ends of `i32`.

    #[inline(always)]
    unsafe fn scan_sse2(column: &[i32], hit: impl Fn(__m128i) -> __m128i, pass: impl Fn(i32) -> bool) -> Vec<u64> {
        // SAFETY: as above.
        unsafe {
            let mut out = vec![0u64; column.len().div_ceil(64)];
            let words = column.len() >> 6;
            for (w, word) in out.iter_mut().enumerate().take(words) {
                let p = column.as_ptr().add(w << 6);
                for k in 0..16 {
                    let lanes = _mm_movemask_ps(_mm_castsi128_ps(hit(_mm_loadu_si128(p.add(4 * k).cast())))) as u64;
                    *word |= lanes << (4 * k);
                }
            }
            tail(&mut out, column, words << 6, pass);
            out
        }
    }

    #[inline(always)]
    unsafe fn scan_avx2(column: &[i32], hit: impl Fn(__m256i) -> __m256i, pass: impl Fn(i32) -> bool) -> Vec<u64> {
        // SAFETY: as above.
        unsafe {
            let mut out = vec![0u64; column.len().div_ceil(64)];
            let words = column.len() >> 6;
            for (w, word) in out.iter_mut().enumerate().take(words) {
                let p = column.as_ptr().add(w << 6);
                for k in 0..8 {
                    let lanes = _mm256_movemask_ps(_mm256_castsi256_ps(hit(_mm256_loadu_si256(p.add(8 * k).cast())))) as u64;
                    *word |= lanes << (8 * k);
                }
            }
            tail(&mut out, column, words << 6, pass);
            out
        }
    }

    /// `hit` gives the compare's mask itself.
    #[inline(always)]
    unsafe fn scan_avx512(column: &[i32], hit: impl Fn(__m512i) -> u16, pass: impl Fn(i32) -> bool) -> Vec<u64> {
        // SAFETY: as above.
        unsafe {
            let mut out = vec![0u64; column.len().div_ceil(64)];
            let words = column.len() >> 6;
            for (w, word) in out.iter_mut().enumerate().take(words) {
                let p = column.as_ptr().add(w << 6);
                for k in 0..4 {
                    *word |= u64::from(hit(_mm512_loadu_si512(p.add(16 * k).cast()))) << (16 * k);
                }
            }
            tail(&mut out, column, words << 6, pass);
            out
        }
    }

    /// The x86-64 baseline.
    pub unsafe fn eq_sse2(column: &[i32], value: i32) -> Vec<u64> {
        unsafe {
            let wanted = _mm_set1_epi32(value);
            scan_sse2(column, |v| _mm_cmpeq_epi32(v, wanted), |x| x == value)
        }
    }

    pub unsafe fn range_sse2(column: &[i32], lo: i32, hi: i32) -> Vec<u64> {
        unsafe {
            let (low, high, ones) = (_mm_set1_epi32(lo), _mm_set1_epi32(hi), _mm_set1_epi32(-1));
            scan_sse2(column, |v| _mm_andnot_si128(_mm_or_si128(_mm_cmpgt_epi32(low, v), _mm_cmpgt_epi32(v, high)), ones), |x| lo <= x && x <= hi)
        }
    }

    pub unsafe fn in_sse2(column: &[i32], values: &[i32]) -> Vec<u64> {
        unsafe {
            let wanted: Vec<__m128i> = values.iter().map(|&x| _mm_set1_epi32(x)).collect();
            scan_sse2(column, |v| wanted.iter().fold(_mm_setzero_si128(), |acc, &w| _mm_or_si128(acc, _mm_cmpeq_epi32(v, w))), |x| values.contains(&x))
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn eq_avx2(column: &[i32], value: i32) -> Vec<u64> {
        unsafe {
            let wanted = _mm256_set1_epi32(value);
            scan_avx2(column, |v| _mm256_cmpeq_epi32(v, wanted), |x| x == value)
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn range_avx2(column: &[i32], lo: i32, hi: i32) -> Vec<u64> {
        unsafe {
            let (low, high, ones) = (_mm256_set1_epi32(lo), _mm256_set1_epi32(hi), _mm256_set1_epi32(-1));
            scan_avx2(column, |v| _mm256_andnot_si256(_mm256_or_si256(_mm256_cmpgt_epi32(low, v), _mm256_cmpgt_epi32(v, high)), ones), |x| lo <= x && x <= hi)
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn in_avx2(column: &[i32], values: &[i32]) -> Vec<u64> {
        unsafe {
            let wanted: Vec<__m256i> = values.iter().map(|&x| _mm256_set1_epi32(x)).collect();
            scan_avx2(column, |v| wanted.iter().fold(_mm256_setzero_si256(), |acc, &w| _mm256_or_si256(acc, _mm256_cmpeq_epi32(v, w))), |x| values.contains(&x))
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn eq_avx512(column: &[i32], value: i32) -> Vec<u64> {
        unsafe {
            let wanted = _mm512_set1_epi32(value);
            scan_avx512(column, |v| _mm512_cmpeq_epi32_mask(v, wanted), |x| x == value)
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn range_avx512(column: &[i32], lo: i32, hi: i32) -> Vec<u64> {
        unsafe {
            let (low, high) = (_mm512_set1_epi32(lo), _mm512_set1_epi32(hi));
            scan_avx512(column, |v| _mm512_cmpge_epi32_mask(v, low) & _mm512_cmple_epi32_mask(v, high), |x| lo <= x && x <= hi)
        }
    }

    #[target_feature(enable = "avx512f")]
    pub unsafe fn in_avx512(column: &[i32], values: &[i32]) -> Vec<u64> {
        unsafe {
            let wanted: Vec<__m512i> = values.iter().map(|&x| _mm512_set1_epi32(x)).collect();
            scan_avx512(column, |v| wanted.iter().fold(0, |acc, &w| acc | _mm512_cmpeq_epi32_mask(v, w)), |x| values.contains(&x))
        }
    }
}

#[cfg(any(target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon")))]
mod neon {
    use super::tail;
    use crate::simd::arch::*;

    /// Sixteen rows' compare lanes, each 0 or all ones, as sixteen bits:
    /// narrowed to a byte a row, each byte masked to its own bit of the eight
    /// in its half, and three pairwise adds summing each half into a byte.
    #[inline]
    #[cfg_attr(target_arch = "arm", target_feature(enable = "neon,v7"))]
    unsafe fn bits16(c: [uint32x4_t; 4]) -> u64 {
        // SAFETY: register operations, and a load of the 16-byte array.
        unsafe {
            const WEIGHTS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
            let low = vmovn_u16(vcombine_u16(vmovn_u32(c[0]), vmovn_u32(c[1])));
            let high = vmovn_u16(vcombine_u16(vmovn_u32(c[2]), vmovn_u32(c[3])));
            let m = vandq_u8(vcombine_u8(low, high), vld1q_u8(WEIGHTS.as_ptr()));
            let s = vpaddq_u8(m, m);
            let s = vpaddq_u8(s, s);
            let s = vpaddq_u8(s, s);
            u64::from(vgetq_lane_u16::<0>(vreinterpretq_u16_u8(s)))
        }
    }

    /// Reads a whole word's 64 rows from an offset the loop guard checks is
    /// 64 or more short of the end, and leaves the rest to `tail`.
    #[cfg_attr(target_arch = "aarch64", inline(always))]
    #[cfg_attr(target_arch = "arm", inline, target_feature(enable = "neon,v7"))]
    unsafe fn scan(column: &[i32], hit: impl Fn(int32x4_t) -> uint32x4_t, pass: impl Fn(i32) -> bool) -> Vec<u64> {
        // SAFETY: as above.
        unsafe {
            let mut out = vec![0u64; column.len().div_ceil(64)];
            let words = column.len() >> 6;
            for (w, word) in out.iter_mut().enumerate().take(words) {
                let p = column.as_ptr().add(w << 6);
                for k in 0..4 {
                    let q = p.add(16 * k);
                    let c = [hit(vld1q_s32(q)), hit(vld1q_s32(q.add(4))), hit(vld1q_s32(q.add(8))), hit(vld1q_s32(q.add(12)))];
                    *word |= bits16(c) << (16 * k);
                }
            }
            tail(&mut out, column, words << 6, pass);
            out
        }
    }

    #[cfg_attr(target_arch = "arm", target_feature(enable = "neon,v7"))]
    pub unsafe fn eq(column: &[i32], value: i32) -> Vec<u64> {
        unsafe {
            let wanted = vdupq_n_s32(value);
            scan(column, |v| vceqq_s32(v, wanted), |x| x == value)
        }
    }

    #[cfg_attr(target_arch = "arm", target_feature(enable = "neon,v7"))]
    pub unsafe fn range(column: &[i32], lo: i32, hi: i32) -> Vec<u64> {
        unsafe {
            let (low, high) = (vdupq_n_s32(lo), vdupq_n_s32(hi));
            scan(column, |v| vandq_u32(vcgeq_s32(v, low), vcleq_s32(v, high)), |x| lo <= x && x <= hi)
        }
    }

    #[cfg_attr(target_arch = "arm", target_feature(enable = "neon,v7"))]
    pub unsafe fn any_of(column: &[i32], values: &[i32]) -> Vec<u64> {
        unsafe {
            let wanted: Vec<int32x4_t> = values.iter().map(|&x| vdupq_n_s32(x)).collect();
            scan(column, |v| wanted.iter().fold(vdupq_n_u32(0), |acc, &w| vorrq_u32(acc, vceqq_s32(v, w))), |x| values.contains(&x))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Eq = unsafe fn(&[i32], i32) -> Vec<u64>;
    type Range = unsafe fn(&[i32], i32, i32) -> Vec<u64>;
    type In = unsafe fn(&[i32], &[i32]) -> Vec<u64>;

    /// One tier's bodies.
    struct TierBodies {
        tier: &'static str,
        eq: Eq,
        range: Range,
        any_of: In,
    }

    /// Every body this CPU runs, by tier, whatever `PWRS_CPU_MAX` says; a
    /// tier it lacks is printed as skipped.
    fn bodies() -> Vec<TierBodies> {
        let mut out = Vec::new();
        #[cfg(target_arch = "x86_64")]
        {
            use crate::cpu::{detected, Isa};
            out.push(TierBodies { tier: "x86-64", eq: x86::eq_sse2, range: x86::range_sse2, any_of: x86::in_sse2 });
            if detected(Isa::Avx2) {
                out.push(TierBodies { tier: "x86-64-v3", eq: x86::eq_avx2, range: x86::range_avx2, any_of: x86::in_avx2 });
            } else {
                eprintln!("FILTER SKIPPED x86-64-v3: this CPU lacks avx2");
            }
            if detected(Isa::Avx512f) {
                out.push(TierBodies { tier: "x86-64-v4", eq: x86::eq_avx512, range: x86::range_avx512, any_of: x86::in_avx512 });
            } else {
                eprintln!("FILTER SKIPPED x86-64-v4: this CPU lacks avx512f");
            }
        }
        #[cfg(any(target_arch = "aarch64", all(target_arch = "arm", feature = "arm-neon")))]
        if crate::simd::arch::neon_detected() {
            out.push(TierBodies { tier: "neon", eq: neon::eq, range: neon::range, any_of: neon::any_of });
        } else {
            eprintln!("FILTER SKIPPED neon: this CPU lacks it");
        }
        out
    }

    const EDGES: [i32; 7] = [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX];

    /// Columns of every length to 70 and a few longer, drawn from four
    /// values, so rows pass often, from the edges of `i32` and from all of
    /// it.
    fn columns() -> Vec<Vec<i32>> {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut out = Vec::new();
        for n in (0..70).chain([127, 128, 129, 1000, 4099]) {
            out.push((0..n).map(|_| (next() % 4) as i32 - 1).collect());
            out.push((0..n).map(|_| EDGES[(next() % EDGES.len() as u64) as usize]).collect());
            out.push((0..n).map(|_| next() as i32).collect());
        }
        out
    }

    #[test]
    fn every_tier_filters_what_the_scalar_filter_passes() {
        let columns = columns();
        let bodies = bodies();
        assert!(!bodies.is_empty() || !cfg!(any(target_arch = "x86_64", target_arch = "aarch64")), "no filter body ran");
        let sets: [&[i32]; 5] = [&[], &[0], &[i32::MIN, i32::MAX], &[-1, 1, 2, 3], &EDGES];
        for body in &bodies {
            let tier = body.tier;
            eprintln!("FILTER RUNNING eq, range and in at {tier} over {} columns", columns.len());
            for column in &columns {
                let n = column.len();
                // SAFETY in each call: bodies() lists only bodies this CPU
                // runs.
                for value in EDGES.iter().copied().chain([2, 12345]) {
                    let got = unsafe { (body.eq)(column, value) };
                    assert_eq!(got, eq_i32_scalar(column, value), "{tier}: {n} rows equal to {value}");
                }
                for lo in EDGES {
                    for hi in EDGES {
                        let got = unsafe { (body.range)(column, lo, hi) };
                        assert_eq!(got, range_i32_scalar(column, lo, hi), "{tier}: {n} rows from {lo} to {hi}");
                    }
                }
                for set in sets {
                    let got = unsafe { (body.any_of)(column, set) };
                    assert_eq!(got, in_i32_scalar(column, set), "{tier}: {n} rows in {set:?}");
                }
            }
        }
        for column in &columns {
            assert_eq!(eq_i32(column, 0), eq_i32_scalar(column, 0));
            assert_eq!(range_i32(column, i32::MIN, 0), range_i32_scalar(column, i32::MIN, 0));
            assert_eq!(in_i32(column, &[0, 1]), in_i32_scalar(column, &[0, 1]));
        }
    }

    #[test]
    fn a_bitmap_reads_back_into_its_rows() {
        let column: Vec<i32> = (0..300).map(|i| i % 7).collect();
        let bitmap = eq_i32(&column, 3);
        assert_eq!(bitmap.len(), 5);
        assert_eq!(bitmap[4] >> (300 - 256), 0, "the bits past the last row are zero");
        let rows: Vec<usize> = (0..300).filter(|&i| column[i] == 3).collect();
        assert_eq!(indices(&bitmap, 300), rows);
        assert_eq!(super::super::ints::count_ones(&bitmap) as usize, rows.len());
        // A bitmap with bits set past `n`, which only `n` excludes.
        let mut noisy = bitmap.clone();
        noisy[4] |= !0 << (300 - 256);
        assert_eq!(indices(&noisy, 300), rows);
        for skip in [0usize, 1, 5, 9, 10, 20, 42, 43, 44, 100] {
            for take in [0usize, 1, 3, 9, 10, 11, 43, 100] {
                let want: Vec<usize> = rows.iter().copied().skip(skip).take(take).collect();
                assert_eq!(indices_page(&bitmap, 300, skip, take), want, "skip {skip}, take {take}");
                assert_eq!(indices_page(&noisy, 300, skip, take), want, "skip {skip}, take {take}, bits past the end");
            }
        }
        assert_eq!(indices(&[], 0), Vec::<usize>::new());
        assert_eq!(indices_page(&[u64::MAX], 10, 2, 3), [2, 3, 4]);
        assert_eq!(range_i32(&[5, 6, 7], 7, 5), [0]);
    }
}
