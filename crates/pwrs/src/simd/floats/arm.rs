//! The `f32` kernels on 32-bit ARM. NEON there treats a denormal as zero,
//! as an operand and as a result, so a block of values goes to NEON only
//! when its values prove no operation can meet one, and otherwise to the
//! scalar form, which keeps every kernel's bits the scalar form's for every
//! input.
//!
//! The proof: every `f32` that is 0 or at least 2^-103 in magnitude is a
//! multiple of 2^-126. A sum of two multiples of 2^-126 is one, and rounding
//! keeps it one, since a value below 2^-102 is exact and a larger one
//! rounds to a multiple of its last place, at least 2^-125. A nonzero
//! multiple of 2^-126 is at least 2^-126, a normal number. So no add or
//! subtract among such values has a denormal operand or result, however
//! closely they cancel. A product of two factors each 0 or at least 2^-51
//! is 0 or at least 2^-102, so it is such a value too. An infinity or a NaN
//! passes: neither is flushed, and a NaN's payload is outside the contract.
//!
//! The scalar steps stay out of line, in code compiled without NEON: inside
//! a function that enables it the compiler may turn scalar arithmetic into
//! NEON's, which would flush what those steps must keep.

use super::prefix::block_f32;
use super::{axpy_f32_scalar, combine_f32, tail_dot_f32, tail_f32, F32_LANES};
use crate::simd::arch::*;

/// `check` of the smallest addend that passes: 2^-103, exponent field 24.
const ADD_LIMIT: u32 = (24 << 24) - 1;
/// `check` of the smallest factor that passes: 2^-51, exponent field 76.
const MUL_LIMIT: u32 = (76 << 24) - 1;

/// A block of [`F32_LANES`] values in the four registers it fills.
type Block = [float32x4_t; 4];

#[inline(never)]
fn tail(lanes: &mut [f32; F32_LANES], rest: &[f32]) {
    tail_f32(lanes, rest);
}

#[inline(never)]
fn tail_dot(lanes: &mut [f32; F32_LANES], a: &[f32], b: &[f32]) {
    tail_dot_f32(lanes, a, b);
}

#[inline(never)]
fn combine(lanes: [f32; F32_LANES]) -> f32 {
    combine_f32(lanes)
}

#[inline(never)]
fn axpy_scalar(y: &mut [f32], a: f32, x: &[f32]) {
    axpy_f32_scalar(y, a, x);
}

#[inline(never)]
fn prefix_scalar(block: &mut [f32], carry: f32) -> f32 {
    block_f32(block, carry)
}

/// What a value is compared by: doubling drops the sign, and subtracting 1
/// wraps 0 to the top, so a nonzero value below a floor is what falls under
/// that floor's limit. It reads the bits: a NEON compare of the value
/// itself would flush it first.
#[inline]
fn check(value: f32) -> u32 {
    (value.to_bits() << 1).wrapping_sub(1)
}

/// # Safety
///
/// `p` reads a block.
#[inline]
#[target_feature(enable = "neon,v7")]
unsafe fn load(p: *const f32) -> Block {
    // SAFETY: the caller has checked that the block is in bounds.
    unsafe { [vld1q_f32(p), vld1q_f32(p.add(4)), vld1q_f32(p.add(8)), vld1q_f32(p.add(12))] }
}

/// # Safety
///
/// `p` writes a block.
#[inline]
#[target_feature(enable = "neon,v7")]
unsafe fn store(p: *mut f32, block: &Block) {
    // SAFETY: the caller has checked that the block is in bounds.
    unsafe {
        vst1q_f32(p, block[0]);
        vst1q_f32(p.add(4), block[1]);
        vst1q_f32(p.add(8), block[2]);
        vst1q_f32(p.add(12), block[3]);
    }
}

/// The least [`check`] of each register's lane across a block's registers.
#[inline]
#[target_feature(enable = "neon,v7")]
fn lowest(block: &Block) -> uint32x4_t {
    let one = vdupq_n_u32(1);
    let value = |v: float32x4_t| vsubq_u32(vshlq_n_u32::<1>(vreinterpretq_u32_f32(v)), one);
    vminq_u32(vminq_u32(value(block[0]), value(block[1])), vminq_u32(value(block[2]), value(block[3])))
}

/// The least of four lanes.
#[inline]
#[target_feature(enable = "neon,v7")]
fn fold(m: uint32x4_t) -> u32 {
    let d = vpmin_u32(vget_low_u32(m), vget_high_u32(m));
    vget_lane_u32::<0>(vpmin_u32(d, d))
}

/// The running sums over `blocks` blocks: `next` gives a block's addends,
/// or `None` for one that does not pass, which goes to `slow`. Sums only
/// NEON has touched need no check, as the start's zeros and every sum of
/// values that passed are multiples of 2^-126. Sums the scalar form has
/// touched are checked before NEON takes them back.
#[inline]
#[target_feature(enable = "neon,v7")]
fn accumulate(blocks: usize, mut next: impl FnMut(usize) -> Option<Block>, mut slow: impl FnMut(&mut [f32; F32_LANES], usize)) -> [f32; F32_LANES] {
    let mut lanes = [0.0; F32_LANES];
    let mut k = 0;
    while k < blocks {
        // SAFETY: the array holds a block.
        let mut sums = unsafe { load(lanes.as_ptr()) };
        if fold(lowest(&sums)) >= ADD_LIMIT {
            while k < blocks {
                let Some(addends) = next(k) else { break };
                for (s, a) in sums.iter_mut().zip(addends) {
                    *s = vaddq_f32(*s, a);
                }
                k += 1;
            }
            // SAFETY: the array holds a block.
            unsafe { store(lanes.as_mut_ptr(), &sums) };
        }
        if k < blocks {
            slow(&mut lanes, k);
            k += 1;
        }
    }
    lanes
}

/// # Safety
///
/// The CPU has NEON.
#[target_feature(enable = "neon,v7")]
pub unsafe fn sum_f32(v: &[f32]) -> f32 {
    let blocks = v.len() / F32_LANES;
    let mut lanes = accumulate(
        blocks,
        |k| {
            // SAFETY: `k` is below `blocks`.
            let block = unsafe { load(v.as_ptr().add(k * F32_LANES)) };
            (fold(lowest(&block)) >= ADD_LIMIT).then_some(block)
        },
        |lanes, k| tail(lanes, &v[k * F32_LANES..(k + 1) * F32_LANES]),
    );
    tail(&mut lanes, &v[blocks * F32_LANES..]);
    combine(lanes)
}

/// A product is rounded on its own before it is added.
///
/// # Safety
///
/// The CPU has NEON, and the slices have one length.
#[target_feature(enable = "neon,v7")]
pub unsafe fn dot_f32(a: &[f32], b: &[f32]) -> f32 {
    let blocks = a.len() / F32_LANES;
    let mut lanes = accumulate(
        blocks,
        |k| {
            // SAFETY: `k` is below `blocks`, in both slices.
            let (x, y) = unsafe { (load(a.as_ptr().add(k * F32_LANES)), load(b.as_ptr().add(k * F32_LANES))) };
            (fold(vminq_u32(lowest(&x), lowest(&y))) >= MUL_LIMIT).then(|| [vmulq_f32(x[0], y[0]), vmulq_f32(x[1], y[1]), vmulq_f32(x[2], y[2]), vmulq_f32(x[3], y[3])])
        },
        |lanes, k| tail_dot(lanes, &a[k * F32_LANES..(k + 1) * F32_LANES], &b[k * F32_LANES..(k + 1) * F32_LANES]),
    );
    tail_dot(&mut lanes, &a[blocks * F32_LANES..], &b[blocks * F32_LANES..]);
    combine(lanes)
}

/// `y[i] += a * x[i]`, each element alone, so nothing carries from one
/// block to the next.
///
/// # Safety
///
/// The CPU has NEON, and the slices have one length.
#[target_feature(enable = "neon,v7")]
pub unsafe fn axpy_f32(y: &mut [f32], a: f32, x: &[f32]) {
    if check(a) < MUL_LIMIT {
        return axpy_scalar(y, a, x);
    }
    let blocks = y.len() / F32_LANES;
    let scale = vdupq_n_f32(a);
    for k in 0..blocks {
        let (at, end) = (k * F32_LANES, (k + 1) * F32_LANES);
        // SAFETY: `k` is below `blocks`, in both slices.
        let (mut sums, factors) = unsafe { (load(y.as_ptr().add(at)), load(x.as_ptr().add(at))) };
        if fold(lowest(&sums)) >= ADD_LIMIT && fold(lowest(&factors)) >= MUL_LIMIT {
            for (s, f) in sums.iter_mut().zip(factors) {
                *s = vaddq_f32(*s, vmulq_f32(scale, f));
            }
            // SAFETY: as above.
            unsafe { store(y.as_mut_ptr().add(at), &sums) };
        } else {
            axpy_scalar(&mut y[at..end], a, &x[at..end]);
        }
    }
    axpy_scalar(&mut y[blocks * F32_LANES..], a, &x[blocks * F32_LANES..]);
}

/// One whole block's steps, in the order of the scalar form's, and the
/// block before's last sum added.
#[inline]
#[target_feature(enable = "neon,v7")]
fn prefix_block(mut r: Block, carry: f32) -> Block {
    let nz = vdupq_n_f32(-0.0);
    let carry = vdupq_n_f32(carry);
    let before = r;
    for k in 0..4 {
        r[k] = vaddq_f32(before[k], vextq_f32::<3>(if k == 0 { nz } else { before[k - 1] }, before[k]));
    }
    let before = r;
    for k in 0..4 {
        r[k] = vaddq_f32(before[k], vextq_f32::<2>(if k == 0 { nz } else { before[k - 1] }, before[k]));
    }
    let before = r;
    for k in 0..4 {
        r[k] = vaddq_f32(before[k], if k == 0 { nz } else { before[k - 1] });
    }
    let before = r;
    for k in 0..4 {
        r[k] = vaddq_f32(before[k], if k < 2 { nz } else { before[k - 2] });
    }
    for v in r.iter_mut() {
        *v = vaddq_f32(*v, carry);
    }
    r
}

/// # Safety
///
/// The CPU has NEON.
#[target_feature(enable = "neon,v7")]
pub unsafe fn prefix_f32(values: &mut [f32]) {
    let blocks = values.len() / F32_LANES;
    let mut carry = -0.0f32;
    for k in 0..blocks {
        let (at, end) = (k * F32_LANES, (k + 1) * F32_LANES);
        // SAFETY: `k` is below `blocks`.
        let block = unsafe { load(values.as_ptr().add(at)) };
        if check(carry) >= ADD_LIMIT && fold(lowest(&block)) >= ADD_LIMIT {
            let summed = prefix_block(block, carry);
            // SAFETY: as above.
            unsafe { store(values.as_mut_ptr().add(at), &summed) };
            carry = vgetq_lane_f32::<3>(summed[3]);
        } else {
            carry = prefix_scalar(&mut values[at..end], carry);
        }
    }
    let rest = &mut values[blocks * F32_LANES..];
    if !rest.is_empty() {
        prefix_scalar(rest, carry);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{axpy_f32_scalar, dot_f32_scalar, prefix_sum_f32_scalar, sum_f32_scalar};
    use super::*;
    use crate::cpu::arm32::{detected, Feature};
    use core::hint::black_box;

    /// The bits, or a NaN on both sides, whose payload Rust leaves
    /// unspecified.
    fn same(a: f32, b: f32) -> bool {
        a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 32) as u32
        }

        fn below(&mut self, n: u32) -> u32 {
            (self.next() >> 1) % n
        }

        /// A random sign and mantissa with an exponent field from `low`,
        /// among `span` of them.
        fn value(&mut self, low: u32, span: u32) -> f32 {
            let field = low + self.below(span);
            f32::from_bits((self.next() & 0x807f_ffff) | (field << 23))
        }

        fn values(&mut self, low: u32, span: u32, n: usize) -> Vec<f32> {
            (0..n).map(|_| self.value(low, span)).collect()
        }

        /// A value of the size real data has, 1e-6 to 1e6.
        fn ordinary(&mut self) -> f32 {
            let sign = if self.next() & 1 == 0 { 1.0f32 } else { -1.0 };
            let mantissa = (self.below(1 << 24) as f32 + 1.0) / 16_777_216.0;
            sign * mantissa * 10f32.powi(self.below(13) as i32 - 6)
        }

        fn ordinaries(&mut self, n: usize) -> Vec<f32> {
            (0..n).map(|_| self.ordinary()).collect()
        }

        fn subnormal(&mut self) -> f32 {
            f32::from_bits(self.next() & 0x807f_ffff)
        }

        /// `n` values that each mostly cancel the value a lane cycle
        /// earlier, in the same running sum, to within 7 units in its last
        /// place, so each sum lands as near zero as such values can.
        fn cancelling(&mut self, low: u32, span: u32, n: usize) -> Vec<f32> {
            let mut out: Vec<f32> = Vec::with_capacity(n);
            for i in 0..n {
                let v = if i >= F32_LANES && self.next() & 3 != 0 {
                    f32::from_bits((-out[i - F32_LANES]).to_bits().wrapping_add(self.below(8)))
                } else {
                    self.value(low, span)
                };
                out.push(v);
            }
            out
        }

        /// `n` values, 0 but in one running sum, where pairs of exponent
        /// field `field` cancel to within 3 units in the last place, so the
        /// sum's last bits are not rounded away by the other lanes.
        fn one_lane(&mut self, field: u32, n: usize) -> Vec<f32> {
            let mut out = vec![0.0f32; n];
            let mut i = self.below(F32_LANES as u32) as usize;
            while i + F32_LANES < n {
                let x = self.value(field, 1);
                out[i] = x;
                out[i + F32_LANES] = f32::from_bits((-x).to_bits().wrapping_add(1 + self.below(3)));
                i += 2 * F32_LANES;
            }
            out
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Shape {
        RawBits,
        AllSubnormal,
        AroundAddFloor,
        AroundMulFloor,
        LowFields,
        NearCancellation,
        ZerosAmongTiny,
        Ordinary,
        OrdinaryRareSubnormal,
        ProductsDenormal,
        TinyTimesHuge,
        AddEdge,
        BelowAddEdge,
        MulEdge,
        BelowMulEdge,
        MixedBlocks,
        OneLaneAtFloor,
        OneLaneBelowFloor,
    }

    const SHAPES: [Shape; 18] = [
        Shape::RawBits,
        Shape::AllSubnormal,
        Shape::AroundAddFloor,
        Shape::AroundMulFloor,
        Shape::LowFields,
        Shape::NearCancellation,
        Shape::ZerosAmongTiny,
        Shape::Ordinary,
        Shape::OrdinaryRareSubnormal,
        Shape::ProductsDenormal,
        Shape::TinyTimesHuge,
        Shape::AddEdge,
        Shape::BelowAddEdge,
        Shape::MulEdge,
        Shape::BelowMulEdge,
        Shape::MixedBlocks,
        Shape::OneLaneAtFloor,
        Shape::OneLaneBelowFloor,
    ];

    /// `n` values of a shape that sets each slice on its own.
    fn fill(r: &mut Rng, shape: Shape, n: usize) -> Vec<f32> {
        match shape {
            Shape::RawBits => (0..n).map(|_| f32::from_bits(r.next())).collect(),
            Shape::AllSubnormal => (0..n).map(|_| r.subnormal()).collect(),
            Shape::AroundAddFloor => r.values(20, 9, n),
            Shape::AroundMulFloor => r.values(72, 9, n),
            Shape::LowFields => r.values(0, 41, n),
            Shape::NearCancellation => r.cancelling(1, 40, n),
            Shape::ZerosAmongTiny => (0..n)
                .map(|_| match r.below(4) {
                    0 => 0.0,
                    1 => -0.0,
                    _tiny => r.value(0, 30),
                })
                .collect(),
            Shape::OrdinaryRareSubnormal => (0..n).map(|_| if r.below(64) == 0 { r.subnormal() } else { r.ordinary() }).collect(),
            _ordinary => r.ordinaries(n),
        }
    }

    /// One case's two slices, of `n` values each.
    fn pair(r: &mut Rng, shape: Shape, n: usize) -> (Vec<f32>, Vec<f32>) {
        match shape {
            Shape::ProductsDenormal => (r.values(45, 30, n), r.values(45, 30, n)),
            Shape::TinyTimesHuge => (r.values(0, 41, n), r.values(150, 105, n)),
            Shape::AddEdge => (r.cancelling(24, 7, n), r.ordinaries(n)),
            Shape::BelowAddEdge => (r.cancelling(23, 8, n), r.ordinaries(n)),
            Shape::MulEdge => (r.cancelling(76, 3, n), vec![f32::from_bits(76 << 23); n]),
            Shape::BelowMulEdge => (r.cancelling(75, 4, n), vec![f32::from_bits(75 << 23); n]),
            Shape::OneLaneAtFloor => (r.one_lane(24, n), r.ordinaries(n)),
            Shape::OneLaneBelowFloor => (r.one_lane(23, n), r.ordinaries(n)),
            Shape::MixedBlocks => {
                let kinds = [Shape::AllSubnormal, Shape::AroundAddFloor, Shape::Ordinary, Shape::Ordinary, Shape::ZerosAmongTiny, Shape::NearCancellation];
                let (mut c, mut d) = (Vec::with_capacity(n + F32_LANES), Vec::with_capacity(n + F32_LANES));
                while c.len() < n {
                    let kind = kinds[r.below(6) as usize];
                    c.extend(fill(r, kind, F32_LANES));
                    d.extend(fill(r, kind, F32_LANES));
                }
                c.truncate(n);
                d.truncate(n);
                (c, d)
            }
            _each_on_its_own => (fill(r, shape, n), fill(r, shape, n)),
        }
    }

    /// A value through a whole register of NEON's add or multiply, which a
    /// constant or a single lane would let the compiler do in scalar form.
    #[target_feature(enable = "neon,v7")]
    fn through_neon(x: f32, y: f32, multiply: bool) -> f32 {
        let (xs, ys) = (black_box([x; 4]), black_box([y; 4]));
        let mut out = [0.0f32; 4];
        // SAFETY: each array holds a register's four values.
        unsafe {
            let (p, q) = (black_box(vld1q_f32(xs.as_ptr())), black_box(vld1q_f32(ys.as_ptr())));
            vst1q_f32(out.as_mut_ptr(), if multiply { vmulq_f32(p, q) } else { vaddq_f32(p, q) });
        }
        black_box(out)[2]
    }

    #[test]
    fn neon_flushes_what_the_floors_keep_it_from_meeting() {
        if !detected(Feature::Neon) {
            eprintln!("FLOATS SKIPPED neon: this CPU lacks it");
            return;
        }
        let half = black_box(f32::from_bits(0x0040_0000));
        // SAFETY: the CPU has NEON.
        unsafe {
            assert_eq!(through_neon(half, half, false).to_bits(), 0, "a denormal operand is flushed");
            assert_eq!((half + half).to_bits(), 0x0080_0000, "the scalar form keeps it");
            assert_eq!(through_neon(half, 2.0f32.powi(127), true).to_bits(), 0, "a denormal factor is flushed");
            // One unit in the last place of a value of each exponent field:
            // field 23's is denormal, and field 24's, the floor, is not.
            let below = black_box(f32::from_bits(23 << 23));
            let next = f32::from_bits(below.to_bits() + 1);
            assert_eq!(through_neon(below, -next, false).to_bits() & 0x7fff_ffff, 0, "two normals with a denormal difference flush");
            assert_ne!((below - next).to_bits() & 0x7fff_ffff, 0);
            let floor = black_box(f32::from_bits(24 << 23));
            let next = f32::from_bits(floor.to_bits() + 1);
            assert_eq!(through_neon(floor, -next, false).to_bits(), (floor - next).to_bits(), "at the floor the difference is normal");
        }
    }

    #[test]
    fn every_kernel_gives_the_scalar_bits_on_every_shape() {
        if !detected(Feature::Neon) {
            eprintln!("FLOATS SKIPPED neon: this CPU lacks it");
            return;
        }
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        for shape in SHAPES {
            for n in [0, 1, 15, 16, 17, 31, 32, 33, 64, 129, 300, 1000] {
                for _round in 0..40 {
                    let (c, d) = pair(&mut r, shape, n);
                    // SAFETY: the CPU has NEON, and the pair has one length.
                    unsafe {
                        assert!(same(sum_f32(&c), sum_f32_scalar(&c)), "sum, {shape:?}, {n} values");
                        assert!(same(dot_f32(&c, &d), dot_f32_scalar(&c, &d)), "dot, {shape:?}, {n} values");
                        let scale = if d.is_empty() { 1.5 } else { d[n / 2] };
                        let (mut got, mut want) = (c.clone(), c.clone());
                        axpy_f32(&mut got, scale, &d);
                        axpy_f32_scalar(&mut want, scale, &d);
                        assert!(got.iter().zip(&want).all(|(x, y)| same(*x, *y)), "axpy, {shape:?}, {n} values");
                        let (mut got, mut want) = (c.clone(), c);
                        prefix_f32(&mut got);
                        prefix_sum_f32_scalar(&mut want);
                        assert!(got.iter().zip(&want).all(|(x, y)| same(*x, *y)), "prefix, {shape:?}, {n} values");
                    }
                }
            }
        }
    }

    #[test]
    fn a_denormal_the_scalar_form_leaves_in_a_running_sum_stays_through_passing_blocks() {
        if !detected(Feature::Neon) {
            eprintln!("FLOATS SKIPPED neon: this CPU lacks it");
            return;
        }
        let tiny = f32::from_bits(0x0000_0001);
        let mut scattered = vec![1.5f32; 40 * F32_LANES];
        for at in [3, 100, 101, 400, 639] {
            scattered[at] = tiny;
        }
        let mut zeros = vec![0.0f32; 40 * F32_LANES];
        zeros[20] = tiny;
        for v in [&scattered, &zeros] {
            // SAFETY: the CPU has NEON.
            assert_eq!(unsafe { sum_f32(v) }.to_bits(), sum_f32_scalar(v).to_bits());
        }
        assert_eq!(sum_f32_scalar(&zeros).to_bits(), tiny.to_bits());
    }
}
