//! The NEON intrinsics the kernels' bodies use, for the two architectures
//! that share them: AArch64's own, and on 32-bit ARM `core::arch::arm`'s
//! with the ones ARMv7 lacks written from what it has, each giving the
//! result AArch64's does.

#[cfg(target_arch = "aarch64")]
pub use core::arch::aarch64::*;

#[cfg(target_arch = "arm")]
pub use self::armv7::*;

#[cfg(all(target_arch = "arm", target_feature = "soft-float"))]
compile_error!("the arm-neon feature needs floating-point registers: build for a target ending in hf, or with -C target-feature=-soft-float,+vfp3,+d32,+neon");

/// Whether the NEON bodies run here: every AArch64 CPU has NEON.
#[cfg(target_arch = "aarch64")]
#[inline]
pub fn has_neon() -> bool {
    true
}

/// Whether this CPU has NEON, whatever `PWRS_CPU_MAX` says.
#[cfg(all(test, target_arch = "aarch64"))]
pub fn neon_detected() -> bool {
    true
}

/// Whether the NEON bodies run here: the CPU has NEON and `PWRS_CPU_MAX`
/// allows it.
#[cfg(target_arch = "arm")]
#[inline]
pub fn has_neon() -> bool {
    crate::cpu::arm32::has(crate::cpu::arm32::Feature::Neon)
}

/// Whether this CPU has NEON, whatever `PWRS_CPU_MAX` says.
#[cfg(all(test, target_arch = "arm"))]
pub fn neon_detected() -> bool {
    crate::cpu::arm32::detected(crate::cpu::arm32::Feature::Neon)
}

#[cfg(target_arch = "arm")]
mod armv7 {
    pub use core::arch::arm::*;

    /// The sum of the two lanes, wrapping.
    #[inline]
    #[target_feature(enable = "neon,v7")]
    pub fn vaddvq_u64(v: uint64x2_t) -> u64 {
        vgetq_lane_u64::<0>(v).wrapping_add(vgetq_lane_u64::<1>(v))
    }

    /// The sum of the sixteen lanes, which fits in 16 bits.
    #[inline]
    #[target_feature(enable = "neon,v7")]
    pub fn vaddlvq_u8(v: uint8x16_t) -> u16 {
        let wide = vpaddlq_u32(vpaddlq_u16(vpaddlq_u8(v)));
        (vgetq_lane_u64::<0>(wide) + vgetq_lane_u64::<1>(wide)) as u16
    }

    /// The largest of the sixteen lanes.
    #[inline]
    #[target_feature(enable = "neon,v7")]
    pub fn vmaxvq_u8(v: uint8x16_t) -> u8 {
        let mut m = vpmax_u8(vget_low_u8(v), vget_high_u8(v));
        m = vpmax_u8(m, m);
        m = vpmax_u8(m, m);
        m = vpmax_u8(m, m);
        vget_lane_u8::<0>(m)
    }

    /// Each lane of `index` looked up in the 16-byte `table`, 0 for an
    /// index past it: VTBL over the two halves of the table, for each half
    /// of the index.
    #[inline]
    #[target_feature(enable = "neon,v7")]
    pub fn vqtbl1q_u8(table: uint8x16_t, index: uint8x16_t) -> uint8x16_t {
        let halves = uint8x8x2_t(vget_low_u8(table), vget_high_u8(table));
        vcombine_u8(vtbl2_u8(halves, vget_low_u8(index)), vtbl2_u8(halves, vget_high_u8(index)))
    }

    /// Neighbouring lanes of `a` added, then those of `b`.
    #[inline]
    #[target_feature(enable = "neon,v7")]
    pub fn vpaddq_u8(a: uint8x16_t, b: uint8x16_t) -> uint8x16_t {
        vcombine_u8(vpadd_u8(vget_low_u8(a), vget_high_u8(a)), vpadd_u8(vget_low_u8(b), vget_high_u8(b)))
    }
}
