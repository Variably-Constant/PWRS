//! 32-bit ARM: the features the CPU and Linux offer, as `AT_HWCAP` and
//! `AT_HWCAP2` report them, and the tiers `PWRS_CPU_MAX` names there.
//!
//! NEON is optional on 32-bit ARM, so the tiers start below it: `scalar`,
//! then `neon`. An ARMv8 core running 32-bit code reports what it has
//! beyond NEON, such as `crc32`, which no tier includes and a cap withholds,
//! as an x86-64 extension no psABI level includes is.

use std::sync::OnceLock;

/// A tier: what `PWRS_CPU_MAX` names on 32-bit ARM. [`Tier::Beyond`] is the
/// tier of the features no tier includes, and as a cap, no cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Scalar = 0,
    Neon = 1,
    Beyond = 2,
}

impl Tier {
    /// The tier a `PWRS_CPU_MAX` value names: `native` and the empty
    /// string are no cap, and anything else unrecognized is `None`.
    pub fn from_name(name: &str) -> Option<Tier> {
        match name.trim() {
            "scalar" => Some(Tier::Scalar),
            "neon" => Some(Tier::Neon),
            "native" | "" => Some(Tier::Beyond),
            _unrecognized => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tier::Scalar => "scalar",
            Tier::Neon => "neon",
            Tier::Beyond => "native",
        }
    }
}

/// A feature the kernels dispatch on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feature {
    Neon = 0,
    Crc32 = 1,
}

impl Feature {
    /// The tier the feature belongs to.
    pub fn tier(self) -> Tier {
        match self {
            Feature::Neon => Tier::Neon,
            Feature::Crc32 => Tier::Beyond,
        }
    }
}

/// Linux's `HWCAP_NEON`, a bit of `AT_HWCAP`.
const HWCAP_NEON: u64 = 1 << 12;
/// Linux's `HWCAP2_CRC32`, a bit of `AT_HWCAP2`.
const HWCAP2_CRC32: u64 = 1 << 4;

/// The features the two words report, one bit each by [`Feature`].
fn features_of(hwcap: u64, hwcap2: u64) -> u64 {
    let mut out = 0;
    if hwcap & HWCAP_NEON != 0 {
        out |= 1 << Feature::Neon as usize;
    }
    if hwcap2 & HWCAP2_CRC32 != 0 {
        out |= 1 << Feature::Crc32 as usize;
    }
    out
}

#[cfg(all(target_arch = "arm", target_os = "linux"))]
fn detect_all() -> u64 {
    unsafe extern "C" {
        fn getauxval(kind: core::ffi::c_ulong) -> core::ffi::c_ulong;
    }
    const AT_HWCAP: core::ffi::c_ulong = 16;
    const AT_HWCAP2: core::ffi::c_ulong = 26;
    // SAFETY: getauxval reads the auxiliary vector the kernel gave the
    // process, and returns 0 for a kind it does not hold.
    let (hwcap, hwcap2) = unsafe { (getauxval(AT_HWCAP), getauxval(AT_HWCAP2)) };
    features_of(u64::from(hwcap), u64::from(hwcap2))
}

#[cfg(not(all(target_arch = "arm", target_os = "linux")))]
fn detect_all() -> u64 {
    features_of(0, 0)
}

/// Whether Linux reports `feature` for this CPU, whatever `PWRS_CPU_MAX`
/// says; never on another architecture or system.
pub fn detected(feature: Feature) -> bool {
    static DETECTED: OnceLock<u64> = OnceLock::new();
    (*DETECTED.get_or_init(detect_all) >> (feature as usize)) & 1 == 1
}

/// The tier `PWRS_CPU_MAX` caps [`has`] at, read once per process;
/// [`Tier::Beyond`] when it is unset, `native`, or names no tier.
pub fn cap() -> Tier {
    static CAP: OnceLock<Tier> = OnceLock::new();
    *CAP.get_or_init(|| match std::env::var("PWRS_CPU_MAX") {
        Ok(v) => Tier::from_name(&v).unwrap_or(Tier::Beyond),
        Err(_unset_or_not_unicode) => Tier::Beyond,
    })
}

/// Whether a module may use `feature` here: detected, and at or below the
/// `PWRS_CPU_MAX` cap. This is the predicate to dispatch on.
pub fn has(feature: Feature) -> bool {
    feature.tier() <= cap() && detected(feature)
}

#[cfg(test)]
mod tests {
    use super::{features_of, Feature, Tier, HWCAP2_CRC32, HWCAP_NEON};

    #[test]
    fn each_word_reports_its_own_feature() {
        assert_eq!(features_of(0, 0), 0);
        assert_eq!(features_of(HWCAP_NEON, 0), 1 << Feature::Neon as usize);
        assert_eq!(features_of(0, HWCAP2_CRC32), 1 << Feature::Crc32 as usize);
        assert_eq!(features_of(HWCAP2_CRC32, HWCAP_NEON), 0, "each bit counts only in its own word");
    }

    #[test]
    fn a_tier_is_named_and_read_back() {
        for tier in [Tier::Scalar, Tier::Neon, Tier::Beyond] {
            assert_eq!(Tier::from_name(tier.name()), Some(tier));
        }
        assert_eq!(Tier::from_name(""), Some(Tier::Beyond));
        assert_eq!(Tier::from_name("sve"), None);
        assert!(Feature::Neon.tier() < Feature::Crc32.tier(), "a cap at neon withholds what no tier includes");
    }
}
