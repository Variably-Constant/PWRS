//! Which x86-64 instruction-set extensions the CPU and the operating
//! system offer, and which ones this library was compiled to require.
//!
//! A module compiled with `-C target-cpu=native`, or with any flag that
//! enables an extension for the whole crate, contains those instructions
//! throughout, and on a CPU without them the first one to run ends the
//! host process. [`REQUIREMENTS`] lists the extensions this crate was
//! compiled with. `export_module!` publishes it as the data export
//! `pwrs_cpu_requirements`, beside `pwrs_cpuid` and `pwrs_xgetbv`, whose
//! bodies are only the register instructions they name, so the runtime
//! can read the list and query the CPU before any compiled module code
//! runs. cargo-pwrs reads the same bytes out of the built file.
//!
//! [`has`] checks at run time, for a module choosing between kernels.
//! `PWRS_CPU_MAX` caps what it reports at one x86-64 psABI level
//! (`x86-64`, `x86-64-v2`, `x86-64-v3` or `x86-64-v4`; `native` or unset
//! is no cap), so one machine can run every tier of a module, and the
//! runtime's import check applies the same cap. [`level`] is the highest
//! level whose every extension [`has`] reports, the value a kernel ladder
//! dispatches on.
//!
//! [`arm`] does the same on AArch64, where the operating system reports
//! the features and `PWRS_CPU_MAX` names a feature tier (`neon`,
//! `dotprod`, `i8mm`, `sve` or `sve2`). There [`REQUIREMENTS`] names each
//! feature compiled in with how Linux, Windows or macOS reports it, and
//! the runtime reads the features from the operating system before any
//! compiled module code runs.

use std::sync::OnceLock;

/// The CPUID output register an extension is reported in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Register {
    Eax = 0,
    Ebx = 1,
    Ecx = 2,
    Edx = 3,
}

/// The x86-64 psABI microarchitecture levels, and `Beyond` for the
/// extensions no level includes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    X86_64 = 1,
    V2 = 2,
    V3 = 3,
    V4 = 4,
    Beyond = 5,
}

impl Level {
    /// The level a `PWRS_CPU_MAX` value names: `native` and the empty
    /// string are no cap, and anything else unrecognized is `None`.
    pub fn from_name(name: &str) -> Option<Level> {
        match name.trim() {
            "x86-64" => Some(Level::X86_64),
            "x86-64-v2" => Some(Level::V2),
            "x86-64-v3" => Some(Level::V3),
            "x86-64-v4" => Some(Level::V4),
            "native" | "" => Some(Level::Beyond),
            _unrecognized => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Level::X86_64 => "x86-64",
            Level::V2 => "x86-64-v2",
            Level::V3 => "x86-64-v3",
            Level::V4 => "x86-64-v4",
            Level::Beyond => "native",
        }
    }
}

/// The XCR0 state components the AVX family needs enabled by the
/// operating system: SSE and AVX, bits 1 and 2.
const AVX_STATE: u64 = 0x6;
/// The AVX-512 family also needs the opmask registers and both halves
/// of the upper ZMM state, bits 5 to 7.
const AVX512_STATE: u64 = 0xe6;

/// One instruction-set extension: where CPUID reports it, which state
/// the operating system must enable for it, and whether this crate was
/// compiled with it.
#[derive(Clone, Copy, Debug)]
pub struct Feature {
    pub isa: Isa,
    /// The name `#[target_feature(enable = ...)]` and `cfg(target_feature)` use.
    pub name: &'static str,
    pub level: Level,
    pub leaf: u32,
    pub subleaf: u32,
    pub register: Register,
    pub bit: u8,
    /// XCR0 bits that must all be set; zero for none.
    pub xcr0: u64,
    /// Whether this crate was compiled with the extension enabled.
    pub compiled: bool,
}

/// The extensions above the baseline that this target compiles every
/// crate for, as `rustc --print cfg` lists them with no flags: SSE3 and
/// CMPXCHG16B on every x86-64 Windows target, and SSSE3 and SSE4.1
/// besides on x86_64-apple-darwin.
const TARGET_FLOOR: &[&str] = if cfg!(all(target_arch = "x86_64", windows)) {
    &["sse3", "cmpxchg16b"]
} else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
    &["sse3", "ssse3", "sse4.1", "cmpxchg16b"]
} else {
    &[]
};

/// `level`, except that the extensions the target itself enables sit at
/// the bottom, so a cap cannot take them away from a module: every build
/// of it has them.
const fn floor(name: &str, level: Level) -> Level {
    let mut i = 0;
    while i < TARGET_FLOOR.len() {
        if same(TARGET_FLOOR[i], name) {
            return Level::X86_64;
        }
        i += 1;
    }
    level
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

macro_rules! extensions {
    ($($isa:ident = $name:tt, $level:ident, $leaf:expr, $sub:expr, $reg:ident, $bit:expr, $xcr0:expr;)*) => {
        /// An x86-64 instruction-set extension this module can check the
        /// CPU for.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum Isa {
            $($isa,)*
        }

        /// Every extension, indexed by `Isa as usize`.
        pub const FEATURES: &[Feature] = &[$(
            Feature {
                isa: Isa::$isa,
                name: $name,
                level: floor($name, Level::$level),
                leaf: $leaf,
                subleaf: $sub,
                register: Register::$reg,
                bit: $bit,
                xcr0: $xcr0,
                compiled: cfg!(all(target_arch = "x86_64", target_feature = $name)),
            },
        )*];
    };
}

// The rows are Intel's CPUID tables. The psABI levels: v2 adds SSE3,
// SSSE3, SSE4.1, SSE4.2, POPCNT and CMPXCHG16B; v3 adds AVX, AVX2, BMI1,
// BMI2, F16C, FMA, LZCNT, MOVBE and XSAVE; v4 adds AVX-512 F, BW, CD,
// DQ and VL. The baseline (SSE, SSE2, FXSR) is on every x86-64 CPU and
// has no row.
extensions! {
    Sse3 = "sse3", V2, 1, 0, Ecx, 0, 0;
    Ssse3 = "ssse3", V2, 1, 0, Ecx, 9, 0;
    Sse41 = "sse4.1", V2, 1, 0, Ecx, 19, 0;
    Sse42 = "sse4.2", V2, 1, 0, Ecx, 20, 0;
    Popcnt = "popcnt", V2, 1, 0, Ecx, 23, 0;
    Cmpxchg16b = "cmpxchg16b", V2, 1, 0, Ecx, 13, 0;
    Avx = "avx", V3, 1, 0, Ecx, 28, AVX_STATE;
    Avx2 = "avx2", V3, 7, 0, Ebx, 5, AVX_STATE;
    Bmi1 = "bmi1", V3, 7, 0, Ebx, 3, 0;
    Bmi2 = "bmi2", V3, 7, 0, Ebx, 8, 0;
    F16c = "f16c", V3, 1, 0, Ecx, 29, AVX_STATE;
    Fma = "fma", V3, 1, 0, Ecx, 12, AVX_STATE;
    Lzcnt = "lzcnt", V3, 0x8000_0001, 0, Ecx, 5, 0;
    Movbe = "movbe", V3, 1, 0, Ecx, 22, 0;
    Xsave = "xsave", V3, 1, 0, Ecx, 26, 0;
    Avx512f = "avx512f", V4, 7, 0, Ebx, 16, AVX512_STATE;
    Avx512bw = "avx512bw", V4, 7, 0, Ebx, 30, AVX512_STATE;
    Avx512cd = "avx512cd", V4, 7, 0, Ebx, 28, AVX512_STATE;
    Avx512dq = "avx512dq", V4, 7, 0, Ebx, 17, AVX512_STATE;
    Avx512vl = "avx512vl", V4, 7, 0, Ebx, 31, AVX512_STATE;
    Avx512ifma = "avx512ifma", Beyond, 7, 0, Ebx, 21, AVX512_STATE;
    Avx512vbmi = "avx512vbmi", Beyond, 7, 0, Ecx, 1, AVX512_STATE;
    Avx512vbmi2 = "avx512vbmi2", Beyond, 7, 0, Ecx, 6, AVX512_STATE;
    Avx512vnni = "avx512vnni", Beyond, 7, 0, Ecx, 11, AVX512_STATE;
    Avx512bitalg = "avx512bitalg", Beyond, 7, 0, Ecx, 12, AVX512_STATE;
    Avx512vpopcntdq = "avx512vpopcntdq", Beyond, 7, 0, Ecx, 14, AVX512_STATE;
    Avx512vp2intersect = "avx512vp2intersect", Beyond, 7, 0, Edx, 8, AVX512_STATE;
    Avx512fp16 = "avx512fp16", Beyond, 7, 0, Edx, 23, AVX512_STATE;
    Avx512bf16 = "avx512bf16", Beyond, 7, 1, Eax, 5, AVX512_STATE;
    Avxvnni = "avxvnni", Beyond, 7, 1, Eax, 4, AVX_STATE;
    Avxifma = "avxifma", Beyond, 7, 1, Eax, 23, AVX_STATE;
    Avxvnniint8 = "avxvnniint8", Beyond, 7, 1, Edx, 4, AVX_STATE;
    Avxvnniint16 = "avxvnniint16", Beyond, 7, 1, Edx, 10, AVX_STATE;
    Avxneconvert = "avxneconvert", Beyond, 7, 1, Edx, 5, AVX_STATE;
    Sha512 = "sha512", Beyond, 7, 1, Eax, 0, AVX_STATE;
    Sm3 = "sm3", Beyond, 7, 1, Eax, 1, AVX_STATE;
    Sm4 = "sm4", Beyond, 7, 1, Eax, 2, AVX_STATE;
    Gfni = "gfni", Beyond, 7, 0, Ecx, 8, 0;
    Vaes = "vaes", Beyond, 7, 0, Ecx, 9, AVX_STATE;
    Vpclmulqdq = "vpclmulqdq", Beyond, 7, 0, Ecx, 10, AVX_STATE;
    Aes = "aes", Beyond, 1, 0, Ecx, 25, 0;
    Pclmulqdq = "pclmulqdq", Beyond, 1, 0, Ecx, 1, 0;
    Sha = "sha", Beyond, 7, 0, Ebx, 29, 0;
    Adx = "adx", Beyond, 7, 0, Ebx, 19, 0;
    Rdrand = "rdrand", Beyond, 1, 0, Ecx, 30, 0;
    Rdseed = "rdseed", Beyond, 7, 0, Ebx, 18, 0;
    Sse4a = "sse4a", Beyond, 0x8000_0001, 0, Ecx, 6, 0;
    Tbm = "tbm", Beyond, 0x8000_0001, 0, Ecx, 21, 0;
    Xsaveopt = "xsaveopt", Beyond, 0xd, 1, Eax, 0, 0;
    Xsavec = "xsavec", Beyond, 0xd, 1, Eax, 1, 0;
    Xsaves = "xsaves", Beyond, 0xd, 1, Eax, 3, 0;
}

const _: () = assert!(FEATURES.len() <= 64, "detection stores one bit per extension in a u64");

impl Isa {
    pub fn feature(self) -> &'static Feature {
        &FEATURES[self as usize]
    }

    pub fn name(self) -> &'static str {
        self.feature().name
    }

    /// The extension a `target_feature` name spells.
    pub fn from_name(name: &str) -> Option<Isa> {
        FEATURES.iter().find(|f| f.name == name).map(|f| f.isa)
    }
}

/// Bytes of [`REQUIREMENTS`]: room for every extension or feature at its
/// longest.
pub const REQUIREMENTS_SIZE: usize = 4096;

/// What this crate was compiled to require, in the form the runtime and
/// cargo-pwrs read, ending in NUL.
///
/// On every architecture but AArch64: `PWRS-CPU/1`, then for each x86-64
/// extension compiled in a space and
/// `name,level,leaf,subleaf,register,bit,xcr0` in decimal, which the
/// runtime checks with CPUID.
///
/// On AArch64 the operating system reports the features, so the runtime
/// reads them from it, and the list says how: `PWRS-ARM/1`, a space and
/// the system the target runs
/// (`linux`, `windows`, `macos`, or `other`, which lists nothing), then
/// for each feature compiled in a space and `name,tier,report`. `tier` is
/// the [`arm::Tier`] number above which `PWRS_CPU_MAX` withholds the
/// feature, `neon`'s for one the target compiles every crate with.
/// `report` is how that system reports the feature, as Rust's own
/// detection reads it there: on Linux `hwcap:hwcap2:half`, the `AT_HWCAP`
/// and `AT_HWCAP2` bits that must all be set in decimal, and 1 when
/// half-precision floats (`AT_HWCAP` bit 9) must come with their Advanced
/// SIMD form (bit 10); on Windows the `IsProcessorFeaturePresent`
/// constants that must all be present, joined by `+`; on macOS the terms
/// that must all be true, joined by `+`, each the `sysctlbyname` names any
/// one of which reading nonzero meets it, joined by `|`. An empty report
/// is a feature that system cannot report.
pub const REQUIREMENTS: [u8; REQUIREMENTS_SIZE] =
    if cfg!(target_arch = "aarch64") { arm::requirements(arm::Os::TARGET, false) } else { requirements() };

// Evaluates the list for every target this crate is checked for, so one
// that outgrew its array fails here rather than in the first module that
// exports it.
const _: () = assert!(REQUIREMENTS[0] == b'P', "the requirement list leads with its marker");

const fn requirements() -> [u8; REQUIREMENTS_SIZE] {
    let mut out = [0u8; REQUIREMENTS_SIZE];
    // Two pieces, so the marker's bytes exist only in the finished list
    // and a scan of a built file cannot find a stray literal instead.
    let mut at = put(&mut out, 0, b"PWRS-");
    at = put(&mut out, at, b"CPU/1");
    let mut i = 0;
    while i < FEATURES.len() {
        let f = &FEATURES[i];
        if f.compiled {
            at = put(&mut out, at, b" ");
            at = put(&mut out, at, f.name.as_bytes());
            at = put(&mut out, at, b",");
            at = put_decimal(&mut out, at, f.level as u64);
            at = put(&mut out, at, b",");
            at = put_decimal(&mut out, at, f.leaf as u64);
            at = put(&mut out, at, b",");
            at = put_decimal(&mut out, at, f.subleaf as u64);
            at = put(&mut out, at, b",");
            at = put_decimal(&mut out, at, f.register as u64);
            at = put(&mut out, at, b",");
            at = put_decimal(&mut out, at, f.bit as u64);
            at = put(&mut out, at, b",");
            at = put_decimal(&mut out, at, f.xcr0);
        }
        i += 1;
    }
    // The NUL after the last byte written, which the zeroed array already
    // contains as long as the list leaves room for it.
    assert!(at < REQUIREMENTS_SIZE, "the requirement list outgrew REQUIREMENTS_SIZE");
    out
}

const fn put(out: &mut [u8; REQUIREMENTS_SIZE], at: usize, bytes: &[u8]) -> usize {
    let mut i = 0;
    while i < bytes.len() {
        out[at + i] = bytes[i];
        i += 1;
    }
    at + bytes.len()
}

const fn put_decimal(out: &mut [u8; REQUIREMENTS_SIZE], at: usize, value: u64) -> usize {
    let mut digits = [0u8; 20];
    let mut n = 0;
    let mut v = value;
    loop {
        digits[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let mut i = 0;
    while i < n {
        out[at + i] = digits[n - 1 - i];
        i += 1;
    }
    at + n
}

/// Every extension this crate was compiled to require.
pub fn compiled() -> impl Iterator<Item = Isa> {
    FEATURES.iter().filter(|f| f.compiled).map(|f| f.isa)
}

/// Whether the CPU reports `isa` and the operating system enables the
/// state it needs, whatever `PWRS_CPU_MAX` says.
pub fn detected(isa: Isa) -> bool {
    static DETECTED: OnceLock<u64> = OnceLock::new();
    (*DETECTED.get_or_init(detect_all) >> (isa as usize)) & 1 == 1
}

/// The level `PWRS_CPU_MAX` caps [`has`] at, read once per process;
/// [`Level::Beyond`] when it is unset, `native`, or names no level (the
/// runtime refuses the import in that last case before any cmdlet runs).
pub fn cap() -> Level {
    static CAP: OnceLock<Level> = OnceLock::new();
    *CAP.get_or_init(|| match std::env::var("PWRS_CPU_MAX") {
        Ok(v) => Level::from_name(&v).unwrap_or(Level::Beyond),
        Err(_unset_or_not_unicode) => Level::Beyond,
    })
}

/// Whether a module may use `isa` here: detected, and at or below the
/// `PWRS_CPU_MAX` cap. This is the predicate to dispatch on.
pub fn has(isa: Isa) -> bool {
    isa.feature().level <= cap() && detected(isa)
}

/// The highest psABI level every one of whose extensions, and every lower
/// level's, [`has`] reports, read once per process: the level a kernel
/// ladder dispatches on, which `PWRS_CPU_MAX` lowers. Never
/// [`Level::Beyond`], and [`Level::X86_64`] on every architecture but
/// x86-64, where it names nothing to run.
pub fn level() -> Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    *LEVEL.get_or_init(|| {
        let mut reached = Level::X86_64;
        for next in [Level::V2, Level::V3, Level::V4] {
            if !FEATURES.iter().filter(|f| f.level == next).all(|f| has(f.isa)) {
                break;
            }
            reached = next;
        }
        reached
    })
}

/// Returns, for one of [`crate::sys::cpu_query`]'s numbers, this module's
/// own value, for the runtime's `Pwrs.Cpu`, so hand-written C# takes the
/// path the Rust side takes. For a name, it writes the name to `out` and
/// returns its length; for an extension, it looks `name` up among this
/// architecture's extensions and returns 1 or 0.
pub fn query(question: u32, name: &str, out: &mut [u8]) -> i32 {
    use pwrs_sys::cpu_query as q;
    let write = |text: &str, out: &mut [u8]| -> i32 {
        let bytes = text.as_bytes();
        match out.get_mut(..bytes.len()) {
            Some(slot) => {
                slot.copy_from_slice(bytes);
                bytes.len() as i32
            }
            None => q::TOO_LONG,
        }
    };
    let x86 = cfg!(target_arch = "x86_64");
    let aarch64 = cfg!(target_arch = "aarch64");
    match question {
        q::LEVEL if x86 => write(level().name(), out),
        q::ARM_TIER if aarch64 => write(arm::tier().name(), out),
        q::CAP if x86 => write(cap().name(), out),
        q::CAP if aarch64 => write(arm::cap().name(), out),
        q::LEVEL | q::ARM_TIER | q::CAP => q::NONE,
        q::HAS | q::DETECTED => {
            let capped = question == q::HAS;
            if x86 {
                match Isa::from_name(name) {
                    Some(isa) => i32::from(if capped { has(isa) } else { detected(isa) }),
                    None => q::NONE,
                }
            } else if aarch64 {
                match arm::Feature::from_name(name) {
                    Some(feature) => i32::from(if capped { arm::has(feature) } else { arm::detected(feature) }),
                    None => q::NONE,
                }
            } else {
                q::NONE
            }
        }
        _unknown => q::UNKNOWN_QUESTION,
    }
}

/// [`query`] over the raw parts the `pwrs_cpu_query` export receives.
///
/// # Safety
///
/// `name` points to `name_len` readable bytes, or is null, and `out` to
/// `out_len` writable bytes, or is null.
#[doc(hidden)]
pub unsafe fn query_raw(question: u32, name: *const u8, name_len: usize, out: *mut u8, out_len: usize) -> i32 {
    let name: &[u8] = if name.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(name, name_len) } };
    let out: &mut [u8] = if out.is_null() { &mut [] } else { unsafe { std::slice::from_raw_parts_mut(out, out_len) } };
    match std::str::from_utf8(name) {
        Ok(name) => query(question, name, out),
        Err(_not_utf8) => pwrs_sys::cpu_query::NONE,
    }
}

/// AArch64: the features the CPU and the operating system offer, as the
/// operating system reports them, and the feature tiers `PWRS_CPU_MAX`
/// names there.
///
/// The tiers are `neon`, which every AArch64 CPU has, then `dotprod`,
/// `i8mm`, `sve` and `sve2`, each including the ones before it. A feature
/// no tier includes, such as `crc` or `aes`, is
/// [`Tier::Beyond`](crate::cpu::arm::Tier::Beyond), which a cap withholds,
/// as an x86-64 extension no psABI level includes is.
pub mod arm {
    use std::sync::OnceLock;

    /// A feature tier: what `PWRS_CPU_MAX` names on AArch64, and what a
    /// kernel ladder dispatches on. [`Tier::Beyond`] is the tier of the
    /// features no tier includes, and as a cap, no cap.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum Tier {
        Neon = 1,
        Dotprod = 2,
        I8mm = 3,
        Sve = 4,
        Sve2 = 5,
        Beyond = 6,
    }

    impl Tier {
        /// The tier a `PWRS_CPU_MAX` value names: `native` and the empty
        /// string are no cap, and anything else unrecognized is `None`.
        pub fn from_name(name: &str) -> Option<Tier> {
            match name.trim() {
                "neon" => Some(Tier::Neon),
                "dotprod" => Some(Tier::Dotprod),
                "i8mm" => Some(Tier::I8mm),
                "sve" => Some(Tier::Sve),
                "sve2" => Some(Tier::Sve2),
                "native" | "" => Some(Tier::Beyond),
                _unrecognized => None,
            }
        }

        pub fn name(self) -> &'static str {
            match self {
                Tier::Neon => "neon",
                Tier::Dotprod => "dotprod",
                Tier::I8mm => "i8mm",
                Tier::Sve => "sve",
                Tier::Sve2 => "sve2",
                Tier::Beyond => "native",
            }
        }
    }

    /// One AArch64 feature: the name `#[target_feature]` and
    /// `is_aarch64_feature_detected!` use, its tier, whether this crate
    /// was compiled with it, and whether the target compiles every crate
    /// with it.
    #[derive(Clone, Copy, Debug)]
    pub struct FeatureInfo {
        pub feature: Feature,
        pub name: &'static str,
        pub tier: Tier,
        pub compiled: bool,
        /// Whether this AArch64 target compiles every crate with the
        /// feature, as `rustc --print cfg` lists it with no flags. The
        /// runtime's import check never withholds such a feature under a
        /// cap, since no build of the module is without it; [`has`] still
        /// applies the cap to it, so a kernel ladder takes the body for
        /// the tier the cap names.
        pub baseline: bool,
    }

    /// How each operating system reports a feature, as Rust's own
    /// detection, `is_aarch64_feature_detected!`, reads it there.
    struct Report {
        /// Linux: the `AT_HWCAP` bits that must all be set.
        hwcap: u64,
        /// Linux: the `AT_HWCAP2` bits that must all be set.
        hwcap2: u64,
        /// Linux: half-precision floats (`AT_HWCAP` bit 9), where
        /// reported, must come with their Advanced SIMD form (bit 10).
        half: bool,
        /// Windows: the `IsProcessorFeaturePresent` constants that must
        /// all be present; none when Windows cannot report the feature.
        windows: &'static [u32],
        /// macOS: terms that must all be true, each met by any one of its
        /// `sysctlbyname` names reading nonzero; none when macOS cannot
        /// report the feature.
        macos: &'static [&'static [&'static str]],
    }

    macro_rules! features {
        ($($feature:ident = $name:tt, $tier:ident,
            linux [$($hw:literal),*] [$($hw2:literal),*] $half:literal,
            windows [$($pf:literal),*],
            macos [$([$($ctl:literal),+]),*];)*) => {
            /// An AArch64 feature this module can check the CPU for.
            #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
            #[non_exhaustive]
            pub enum Feature {
                $($feature,)*
            }

            /// Every feature, indexed by `Feature as usize`.
            pub const FEATURES: &[FeatureInfo] = &[$(
                FeatureInfo {
                    feature: Feature::$feature,
                    name: $name,
                    tier: Tier::$tier,
                    compiled: cfg!(all(target_arch = "aarch64", target_feature = $name)),
                    baseline: cfg!(target_arch = "aarch64") && in_baseline(Os::TARGET, $name),
                },
            )*];

            /// Every feature's reports, indexed by `Feature as usize`.
            const REPORTS: &[Report] = &[$(
                Report {
                    hwcap: 0 $(| 1 << $hw)*,
                    hwcap2: 0 $(| 1 << $hw2)*,
                    half: $half,
                    windows: &[$($pf),*],
                    macos: &[$(&[$($ctl),+]),*],
                },
            )*];

            #[cfg(target_arch = "aarch64")]
            fn detect_all() -> u64 {
                let mut mask = 0u64;
                $(
                    if std::arch::is_aarch64_feature_detected!($name) {
                        mask |= 1 << (Feature::$feature as usize);
                    }
                )*
                mask
            }
        };
    }

    // The tiers' own features first, then the rest the operating systems
    // report. `aes` covers PMULL as well, as the target feature does.
    // Each row's reports are std's own detection: the `AT_HWCAP` and
    // `AT_HWCAP2` bits it requires on Linux, a feature resting on Advanced
    // SIMD requiring that one's three as well, the `PF_ARM_*` constants it
    // checks on Windows, and the `sysctlbyname` names it reads on macOS.
    features! {
        Neon = "neon", Neon, linux [0, 1] [] true, windows [19], macos [["hw.optional.AdvSIMD"]];
        Dotprod = "dotprod", Dotprod, linux [20] [] false, windows [43], macos [["hw.optional.arm.FEAT_DotProd"]];
        I8mm = "i8mm", I8mm, linux [] [13] false, windows [66], macos [["hw.optional.arm.FEAT_I8MM"]];
        Sve = "sve", Sve, linux [22, 0, 1] [] true, windows [46], macos [];
        Sve2 = "sve2", Sve2, linux [22, 0, 1] [1] true, windows [47], macos [];
        Aes = "aes", Beyond, linux [3, 0, 1] [] true, windows [30], macos [["hw.optional.arm.FEAT_AES"], ["hw.optional.arm.FEAT_PMULL"]];
        Sha2 = "sha2", Beyond, linux [5, 6, 0, 1] [] true, windows [30],
            macos [["hw.optional.arm.FEAT_SHA1"], ["hw.optional.arm.FEAT_SHA256"], ["hw.optional.AdvSIMD"]];
        Sha3 = "sha3", Beyond, linux [21, 17, 5, 6, 0, 1] [] true, windows [64, 65],
            macos [["hw.optional.arm.FEAT_SHA512"], ["hw.optional.arm.FEAT_SHA3"], ["hw.optional.AdvSIMD"]];
        Sm4 = "sm4", Beyond, linux [18, 19, 0, 1] [] true, windows [], macos [];
        Crc = "crc", Beyond, linux [7] [] false, windows [31], macos [["hw.optional.armv8_crc32", "hw.optional.arm.FEAT_CRC32"]];
        Rdm = "rdm", Beyond, linux [12] [] false, windows [43], macos [["hw.optional.arm.FEAT_RDM"]];
        Fp16 = "fp16", Beyond, linux [0, 9] [] false, windows [67], macos [["hw.optional.arm.FEAT_FP16"]];
        Fhm = "fhm", Beyond, linux [9, 23] [] false, windows [], macos [["hw.optional.arm.FEAT_FHM"]];
        Bf16 = "bf16", Beyond, linux [] [14] false, windows [68], macos [["hw.optional.arm.FEAT_BF16"]];
        F32mm = "f32mm", Beyond, linux [22, 0, 1] [10] true, windows [58], macos [];
        F64mm = "f64mm", Beyond, linux [22, 0, 1] [11] true, windows [59], macos [];
        Sve2Aes = "sve2-aes", Beyond, linux [22, 0, 1, 3] [1, 2, 3] true, windows [49, 50], macos [];
        Sve2Bitperm = "sve2-bitperm", Beyond, linux [] [4, 1] false, windows [51], macos [];
        Sve2Sha3 = "sve2-sha3", Beyond, linux [22, 0, 1, 21, 17, 5, 6] [1, 5] true, windows [55], macos [];
        Sve2Sm4 = "sve2-sm4", Beyond, linux [22, 0, 1, 18, 19] [1, 6] true, windows [56], macos [];
        Lse = "lse", Beyond, linux [8] [] false, windows [34], macos [["hw.optional.arm.FEAT_LSE"]];
        Rcpc = "rcpc", Beyond, linux [15] [] false, windows [45], macos [["hw.optional.arm.FEAT_LRCPC"]];
        Fcma = "fcma", Beyond, linux [14, 0, 1] [] true, windows [], macos [["hw.optional.arm.FEAT_FCMA"]];
        Jsconv = "jsconv", Beyond, linux [13, 0] [] false, windows [44], macos [["hw.optional.arm.FEAT_JSCVT"]];
        Frintts = "frintts", Beyond, linux [] [8] false, windows [], macos [["hw.optional.arm.FEAT_FRINTTS"]];
        Dit = "dit", Beyond, linux [24] [] false, windows [], macos [["hw.optional.arm.FEAT_DIT"]];
        Rand = "rand", Beyond, linux [] [16] false, windows [], macos [];
    }

    const _: () = assert!(FEATURES.len() <= 64, "detection stores one bit per feature in a u64");

    /// The operating system an AArch64 requirement list records the
    /// reports of, named as the list names it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Os {
        Linux,
        Windows,
        Macos,
        Other,
    }

    impl Os {
        /// The system this crate's target runs.
        pub(super) const TARGET: Os = if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Other
        };

        pub(super) const fn name(self) -> &'static str {
            match self {
                Os::Linux => "linux",
                Os::Windows => "windows",
                Os::Macos => "macos",
                Os::Other => "other",
            }
        }
    }

    /// The tracked features `aarch64-apple-darwin` compiles every crate
    /// with, as `rustc --print cfg --target aarch64-apple-darwin` lists
    /// them; every other AArch64 target enables `neon` alone.
    const MACOS_BASELINE: &[&str] =
        &["aes", "crc", "dit", "dotprod", "fcma", "fhm", "fp16", "frintts", "jsconv", "lse", "neon", "rcpc", "rdm", "sha2", "sha3"];

    /// Whether an AArch64 target for `os` compiles every crate with the
    /// feature `name`.
    const fn in_baseline(os: Os, name: &str) -> bool {
        if super::same(name, "neon") {
            return true;
        }
        if !matches!(os, Os::Macos) {
            return false;
        }
        let mut i = 0;
        while i < MACOS_BASELINE.len() {
            if super::same(MACOS_BASELINE[i], name) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// The AArch64 requirement list for `os` in [`REQUIREMENTS`]'s form:
    /// the features compiled in, or every feature when `every` is set.
    ///
    /// [`REQUIREMENTS`]: super::REQUIREMENTS
    pub(super) const fn requirements(os: Os, every: bool) -> [u8; super::REQUIREMENTS_SIZE] {
        let mut out = [0u8; super::REQUIREMENTS_SIZE];
        // Two pieces, as for the x86-64 list, so the marker's bytes exist
        // only in the finished list.
        let mut at = super::put(&mut out, 0, b"PWRS-");
        at = super::put(&mut out, at, b"ARM/1 ");
        at = super::put(&mut out, at, os.name().as_bytes());
        at = entries(os, every, &mut out, at);
        // The NUL after the last byte written, which the zeroed array
        // already has as long as the list leaves room for it.
        assert!(at < super::REQUIREMENTS_SIZE, "the AArch64 requirement list outgrew REQUIREMENTS_SIZE");
        out
    }

    /// Writes the list's entries for `os` from `at`, each after a space:
    /// the features compiled in, or every feature when `every` is set.
    /// Returns where the writing stopped.
    const fn entries(os: Os, every: bool, out: &mut [u8; super::REQUIREMENTS_SIZE], mut at: usize) -> usize {
        if matches!(os, Os::Other) {
            return at;
        }
        let mut i = 0;
        while i < FEATURES.len() {
            let f = &FEATURES[i];
            if every || f.compiled {
                let r = &REPORTS[i];
                at = super::put(out, at, b" ");
                at = super::put(out, at, f.name.as_bytes());
                at = super::put(out, at, b",");
                let tier = if in_baseline(os, f.name) { Tier::Neon } else { f.tier };
                at = super::put_decimal(out, at, tier as u64);
                at = super::put(out, at, b",");
                match os {
                    Os::Linux => {
                        at = super::put_decimal(out, at, r.hwcap);
                        at = super::put(out, at, b":");
                        at = super::put_decimal(out, at, r.hwcap2);
                        at = super::put(out, at, if r.half { b":1" } else { b":0" });
                    }
                    Os::Windows => {
                        let mut k = 0;
                        while k < r.windows.len() {
                            if k > 0 {
                                at = super::put(out, at, b"+");
                            }
                            at = super::put_decimal(out, at, r.windows[k] as u64);
                            k += 1;
                        }
                    }
                    Os::Macos => {
                        let mut t = 0;
                        while t < r.macos.len() {
                            if t > 0 {
                                at = super::put(out, at, b"+");
                            }
                            let term = r.macos[t];
                            let mut k = 0;
                            while k < term.len() {
                                if k > 0 {
                                    at = super::put(out, at, b"|");
                                }
                                at = super::put(out, at, term[k].as_bytes());
                                k += 1;
                            }
                            t += 1;
                        }
                    }
                    Os::Other => {}
                }
            }
            i += 1;
        }
        at
    }

    /// The entries an AArch64 requirement list for `os` (`linux`,
    /// `windows` or `macos`) gives every feature, joined by spaces: what
    /// follows `PWRS-ARM/1` and the system's name in the list of a library
    /// compiled with them all, which the runtime's import check reads.
    /// `None` for any other name. The marker is left out, so text a
    /// compiler folds from this into a module library cannot pass for that
    /// library's own list when cargo-pwrs reads the file.
    #[doc(hidden)]
    pub fn requirement_entries(os: &str) -> Option<String> {
        let os = match os {
            "linux" => Os::Linux,
            "windows" => Os::Windows,
            "macos" => Os::Macos,
            _other => return None,
        };
        let mut out = [0u8; super::REQUIREMENTS_SIZE];
        let end = entries(os, true, &mut out, 0);
        Some(out[..end].iter().skip(1).map(|&b| char::from(b)).collect())
    }

    #[cfg(not(target_arch = "aarch64"))]
    fn detect_all() -> u64 {
        0
    }

    impl Feature {
        pub fn info(self) -> &'static FeatureInfo {
            &FEATURES[self as usize]
        }

        pub fn name(self) -> &'static str {
            self.info().name
        }

        /// The feature a `target_feature` name spells.
        pub fn from_name(name: &str) -> Option<Feature> {
            FEATURES.iter().find(|f| f.name == name).map(|f| f.feature)
        }
    }

    /// Every feature this crate was compiled to require.
    pub fn compiled() -> impl Iterator<Item = Feature> {
        FEATURES.iter().filter(|f| f.compiled).map(|f| f.feature)
    }

    /// Whether the operating system reports `feature` for this CPU,
    /// whatever `PWRS_CPU_MAX` says; never on another architecture.
    pub fn detected(feature: Feature) -> bool {
        static DETECTED: OnceLock<u64> = OnceLock::new();
        (*DETECTED.get_or_init(detect_all) >> (feature as usize)) & 1 == 1
    }

    /// The tier `PWRS_CPU_MAX` caps [`has`] at, read once per process;
    /// [`Tier::Beyond`] when it is unset, `native`, or names no tier (the
    /// runtime refuses the import in that last case before any cmdlet
    /// runs).
    pub fn cap() -> Tier {
        static CAP: OnceLock<Tier> = OnceLock::new();
        *CAP.get_or_init(|| match std::env::var("PWRS_CPU_MAX") {
            Ok(v) => Tier::from_name(&v).unwrap_or(Tier::Beyond),
            Err(_unset_or_not_unicode) => Tier::Beyond,
        })
    }

    /// Whether a module may use `feature` here: detected, and at or below
    /// the `PWRS_CPU_MAX` cap. This is the predicate to dispatch on.
    pub fn has(feature: Feature) -> bool {
        feature.info().tier <= cap() && detected(feature)
    }

    /// The highest tier every one of whose features, and every lower
    /// tier's, [`has`] reports, read once per process: the tier a kernel
    /// ladder dispatches on, which `PWRS_CPU_MAX` lowers. Never
    /// [`Tier::Beyond`], and [`Tier::Neon`] on every architecture but
    /// AArch64, where it names nothing to run.
    pub fn tier() -> Tier {
        static TIER: OnceLock<Tier> = OnceLock::new();
        *TIER.get_or_init(|| {
            let mut reached = Tier::Neon;
            for next in [Tier::Dotprod, Tier::I8mm, Tier::Sve, Tier::Sve2] {
                if !FEATURES.iter().filter(|f| f.tier == next).all(|f| has(f.feature)) {
                    break;
                }
                reached = next;
            }
            reached
        })
    }

    #[cfg(test)]
    mod tests {
        use super::{in_baseline, requirement_entries, requirements, Os, Tier, FEATURES, REPORTS};

        /// The list naming every feature for `os`, as the import check
        /// reads it from a library compiled with them all.
        fn list(os: Os) -> String {
            requirements(os, true).iter().take_while(|&&b| b != 0).map(|&b| char::from(b)).collect()
        }

        /// The entries hello passes to its tests are the list's, after the
        /// marker and the system's name.
        #[test]
        fn the_entries_are_the_list_without_its_marker() {
            for os in [Os::Linux, Os::Windows, Os::Macos] {
                let full = list(os);
                let tail = full.strip_prefix(&format!("PWRS-ARM/1 {} ", os.name())).expect("the marker and the system lead");
                assert_eq!(requirement_entries(os.name()).as_deref(), Some(tail));
            }
            assert_eq!(requirement_entries("freebsd"), None);
        }

        #[test]
        fn every_feature_has_its_reports() {
            assert_eq!(REPORTS.len(), FEATURES.len());
            for r in REPORTS {
                for name in r.macos.iter().flat_map(|term| term.iter()) {
                    assert!(!name.contains([' ', ',', '+', '|']), "{name} would break the list's form");
                }
            }
        }

        /// The list's form for each system, against rows worked out by
        /// hand: neon is AT_HWCAP bits 0 and 1 with the half-precision
        /// rule, sve2-aes bits 22, 0, 1 and 3 with AT_HWCAP2 bits 1 to 3,
        /// and on macOS a baseline feature takes neon's tier.
        #[test]
        fn the_list_spells_each_systems_reports() {
            let linux = list(Os::Linux);
            assert!(linux.starts_with("PWRS-ARM/1 linux neon,1,3:0:1 dotprod,2,1048576:0:0 "), "{linux}");
            assert!(linux.contains(" sve2-aes,6,4194315:14:1 "), "{linux}");
            assert!(linux.contains(" sve2-bitperm,6,0:18:0 "), "{linux}");
            assert!(linux.ends_with(" rand,6,0:65536:0"), "{linux}");
            let windows = list(Os::Windows);
            assert!(windows.starts_with("PWRS-ARM/1 windows neon,1,19 dotprod,2,43 "), "{windows}");
            assert!(windows.contains(" sha3,6,64+65 "), "{windows}");
            assert!(windows.contains(" fhm,6, "), "{windows}");
            let macos = list(Os::Macos);
            assert!(macos.starts_with("PWRS-ARM/1 macos neon,1,hw.optional.AdvSIMD dotprod,1,hw.optional.arm.FEAT_DotProd i8mm,3,"), "{macos}");
            assert!(macos.contains(" aes,1,hw.optional.arm.FEAT_AES+hw.optional.arm.FEAT_PMULL "), "{macos}");
            assert!(macos.contains(" crc,1,hw.optional.armv8_crc32|hw.optional.arm.FEAT_CRC32 "), "{macos}");
            assert!(macos.contains(" sve,4, "), "{macos}");
        }

        /// Every entry of each system's list reads back as its row.
        #[test]
        fn each_entry_reads_back_as_its_row() {
            for os in [Os::Linux, Os::Windows, Os::Macos] {
                let text = list(os);
                let mut entries = text.strip_prefix("PWRS-ARM/1 ").expect("the marker").split(' ');
                assert_eq!(entries.next(), Some(os.name()));
                let entries: Vec<&str> = entries.collect();
                assert_eq!(entries.len(), FEATURES.len());
                for ((entry, f), r) in entries.iter().zip(FEATURES).zip(REPORTS) {
                    let fields: Vec<&str> = entry.split(',').collect();
                    assert_eq!(fields.len(), 3, "{entry}");
                    assert_eq!(fields[0], f.name);
                    let tier: u64 = fields[1].parse().expect("a tier number");
                    let want = if in_baseline(os, f.name) { Tier::Neon } else { f.tier };
                    assert_eq!(tier, want as u64, "{entry}");
                    match os {
                        Os::Linux => {
                            let parts: Vec<u64> = fields[2].split(':').map(|n| n.parse().expect("decimal")).collect();
                            assert_eq!(parts, vec![r.hwcap, r.hwcap2, u64::from(r.half)], "{entry}");
                        }
                        Os::Windows => {
                            let pf: Vec<u32> = fields[2].split('+').filter(|p| !p.is_empty()).map(|p| p.parse().expect("decimal")).collect();
                            assert_eq!(pf, r.windows, "{entry}");
                        }
                        Os::Macos => {
                            let terms: Vec<Vec<&str>> = fields[2].split('+').filter(|t| !t.is_empty()).map(|t| t.split('|').collect()).collect();
                            let want: Vec<Vec<&str>> = r.macos.iter().map(|t| t.to_vec()).collect();
                            assert_eq!(terms, want, "{entry}");
                        }
                        Os::Other => unreachable!("the loop names three systems"),
                    }
                }
            }
        }

        /// Linux reports each feature, read from this process's auxiliary
        /// vector the way the runtime's import check reads it, exactly
        /// when Rust's own detection says the CPU has it. Under
        /// qemu-aarch64 the CPU is the one `QEMU_CPU` names.
        #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
        #[test]
        fn each_linux_report_reads_the_auxiliary_vector_as_std_does() {
            unsafe extern "C" {
                fn getauxval(kind: core::ffi::c_ulong) -> core::ffi::c_ulong;
            }
            // SAFETY: getauxval reads the process's auxiliary vector and
            // takes no pointers; 16 is AT_HWCAP and 26 AT_HWCAP2.
            let (hwcap, hwcap2) = unsafe { (getauxval(16), getauxval(26)) };
            let mut offered = 0;
            for (f, r) in FEATURES.iter().zip(REPORTS) {
                let here = hwcap & r.hwcap == r.hwcap
                    && hwcap2 & r.hwcap2 == r.hwcap2
                    && (!r.half || hwcap & (1 << 9) == 0 || hwcap & (1 << 10) != 0);
                assert_eq!(here, super::detected(f.feature), "{}: hwcap {hwcap:#x}, hwcap2 {hwcap2:#x}", f.name);
                offered += usize::from(here);
            }
            println!("ARM REPORTS RUNNING linux over {} features, {offered} offered, hwcap {hwcap:#x}, hwcap2 {hwcap2:#x}", FEATURES.len());
        }

        /// macOS reports each feature beyond the target's baseline, read
        /// with sysctlbyname the way the runtime's import check reads it,
        /// exactly when Rust's own detection says the CPU has it. A
        /// baseline feature, which the check does not read, is printed with
        /// how its names read.
        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        #[test]
        fn each_macos_report_reads_sysctl_as_std_does() {
            unsafe extern "C" {
                fn sysctlbyname(
                    name: *const core::ffi::c_char,
                    oldp: *mut core::ffi::c_void,
                    oldlenp: *mut usize,
                    newp: *mut core::ffi::c_void,
                    newlen: usize,
                ) -> core::ffi::c_int;
            }
            let reads = |name: &str| -> bool {
                let name = std::ffi::CString::new(name).expect("a sysctl name has no NUL");
                let mut value: i32 = 0;
                let mut length = core::mem::size_of::<i32>();
                // SAFETY: the name is NUL-terminated, and value and length
                // describe a 4-byte buffer that outlives the call.
                let status = unsafe { sysctlbyname(name.as_ptr(), (&raw mut value).cast(), &mut length, core::ptr::null_mut(), 0) };
                status == 0 && value != 0
            };
            let (mut beyond, mut offered) = (0, 0);
            let mut baseline = Vec::new();
            for (f, r) in FEATURES.iter().zip(REPORTS) {
                if r.macos.is_empty() {
                    continue;
                }
                let here = r.macos.iter().all(|term| term.iter().any(|&name| reads(name)));
                if in_baseline(Os::Macos, f.name) {
                    baseline.push(format!("{}={}", f.name, u8::from(here)));
                    continue;
                }
                assert_eq!(here, super::detected(f.feature), "{}: {:?}", f.name, r.macos);
                beyond += 1;
                offered += usize::from(here);
            }
            println!("ARM REPORTS RUNNING macos over {beyond} features beyond the baseline, {offered} offered; the baseline reads {}", baseline.join(" "));
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn detect_all() -> u64 {
    use core::arch::x86_64::{CpuidResult, __cpuid_count, _xgetbv};
    #[allow(unused_unsafe)]
    let cpuid = |leaf: u32, subleaf: u32| -> CpuidResult { unsafe { __cpuid_count(leaf, subleaf) } };
    let max_basic = cpuid(0, 0).eax;
    let max_extended = cpuid(0x8000_0000, 0).eax;
    let osxsave = max_basic >= 1 && (cpuid(1, 0).ecx >> 27) & 1 == 1;
    // XGETBV exists only when CPUID reports OSXSAVE.
    let xcr0 = if osxsave { unsafe { _xgetbv(0) } } else { 0 };
    let mut mask = 0u64;
    for (i, f) in FEATURES.iter().enumerate() {
        let reported = if f.leaf >= 0x8000_0000 { f.leaf <= max_extended } else { f.leaf <= max_basic };
        if !reported {
            continue;
        }
        let r = cpuid(f.leaf, f.subleaf);
        let word = match f.register {
            Register::Eax => r.eax,
            Register::Ebx => r.ebx,
            Register::Ecx => r.ecx,
            Register::Edx => r.edx,
        };
        if (word >> f.bit) & 1 == 1 && xcr0 & f.xcr0 == f.xcr0 {
            mask |= 1 << i;
        }
    }
    mask
}

#[cfg(not(target_arch = "x86_64"))]
fn detect_all() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::{Isa, Level, FEATURES, REQUIREMENTS};

    /// What `Pwrs.Cpu` reads through the export is what this module's own
    /// calls return.
    #[test]
    fn a_query_returns_the_modules_own_values() {
        use crate::sys::cpu_query as q;
        let mut out = [0u8; 32];
        let named = |question: u32, out: &mut [u8]| -> String {
            let n = super::query(question, "", out);
            assert!(n >= 0, "question {question} returned {n}");
            String::from_utf8(out[..n as usize].to_vec()).expect("a name is UTF-8")
        };
        if cfg!(target_arch = "x86_64") {
            assert_eq!(named(q::LEVEL, &mut out), super::level().name());
            assert_eq!(named(q::CAP, &mut out), super::cap().name());
            assert_eq!(super::query(q::ARM_TIER, "", &mut out), q::NONE);
            for f in FEATURES {
                assert_eq!(super::query(q::HAS, f.name, &mut out), i32::from(super::has(f.isa)), "{}", f.name);
                assert_eq!(super::query(q::DETECTED, f.name, &mut out), i32::from(super::detected(f.isa)), "{}", f.name);
            }
            // The x86-64 baseline is not an extension the module tracks.
            assert_eq!(super::query(q::DETECTED, "sse2", &mut out), q::NONE);
            assert_eq!(super::query(q::HAS, "sve", &mut out), q::NONE);
            assert_eq!(super::query(q::LEVEL, "", &mut [0u8; 2]), q::TOO_LONG);
        }
        if cfg!(target_arch = "aarch64") {
            assert_eq!(named(q::ARM_TIER, &mut out), super::arm::tier().name());
            assert_eq!(named(q::CAP, &mut out), super::arm::cap().name());
            assert_eq!(super::query(q::LEVEL, "", &mut out), q::NONE);
            for f in super::arm::FEATURES {
                assert_eq!(super::query(q::HAS, f.name, &mut out), i32::from(super::arm::has(f.feature)), "{}", f.name);
                assert_eq!(super::query(q::DETECTED, f.name, &mut out), i32::from(super::arm::detected(f.feature)), "{}", f.name);
            }
            assert_eq!(super::query(q::DETECTED, "neon", &mut out), 1);
            assert_eq!(super::query(q::HAS, "avx2", &mut out), q::NONE);
            assert_eq!(super::query(q::ARM_TIER, "", &mut [0u8; 2]), q::TOO_LONG);
        }
        assert_eq!(super::query(99, "", &mut out), q::UNKNOWN_QUESTION);
        assert_eq!(super::query(q::HAS, "not-an-extension", &mut out), q::NONE);
        // SAFETY: both pointers are null, which query_raw reads as empty.
        assert_eq!(unsafe { super::query_raw(q::HAS, std::ptr::null(), 0, std::ptr::null_mut(), 0) }, q::NONE);
    }

    #[test]
    fn every_row_sits_at_its_index_and_names_one_extension() {
        for (i, f) in FEATURES.iter().enumerate() {
            assert_eq!(f.isa as usize, i, "{} is out of place", f.name);
            assert_eq!(Isa::from_name(f.name), Some(f.isa));
            assert!(f.bit < 32, "{} names bit {}", f.name, f.bit);
            if f.name.starts_with("avx512") {
                assert_eq!(f.xcr0, 0xe6, "{} needs the AVX-512 state", f.name);
            }
        }
        let mut names: Vec<&str> = FEATURES.iter().map(|f| f.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), FEATURES.len(), "a name appears twice");
    }

    #[test]
    fn the_levels_are_the_psabi_levels() {
        let at = |level: Level| -> Vec<&str> {
            let mut v: Vec<&str> = FEATURES.iter().filter(|f| f.level == level).map(|f| f.name).collect();
            v.sort_unstable();
            v
        };
        // The target's own extensions sit at the baseline instead.
        let v2: Vec<&str> =
            ["cmpxchg16b", "popcnt", "sse3", "sse4.1", "sse4.2", "ssse3"].into_iter().filter(|n| !super::TARGET_FLOOR.contains(n)).collect();
        assert_eq!(at(Level::V2), v2);
        assert_eq!(at(Level::V3), vec!["avx", "avx2", "bmi1", "bmi2", "f16c", "fma", "lzcnt", "movbe", "xsave"]);
        assert_eq!(at(Level::V4), vec!["avx512bw", "avx512cd", "avx512dq", "avx512f", "avx512vl"]);
        assert_eq!(Level::from_name("x86-64-v3"), Some(Level::V3));
        assert_eq!(Level::from_name("native"), Some(Level::Beyond));
        assert_eq!(Level::from_name("x86-64-v5"), None);
    }

    #[test]
    fn the_requirement_list_is_the_compiled_rows_in_the_documented_form() {
        let end = REQUIREMENTS.iter().position(|&b| b == 0).expect("NUL-terminated");
        let text = std::str::from_utf8(&REQUIREMENTS[..end]).expect("ASCII");
        if cfg!(target_arch = "aarch64") {
            let body = text.strip_prefix("PWRS-ARM/1 ").expect("the AArch64 marker leads");
            let mut entries = body.split(' ');
            assert_eq!(entries.next(), Some(super::arm::Os::TARGET.name()), "the system the target runs follows");
            let listed: Vec<&str> = entries.map(|e| e.split(',').next().expect("a name leads each entry")).collect();
            let compiled: Vec<&str> =
                if super::arm::Os::TARGET == super::arm::Os::Other { Vec::new() } else { super::arm::compiled().map(|f| f.name()).collect() };
            assert_eq!(listed, compiled);
            return;
        }
        let body = text.strip_prefix("PWRS-CPU/1").expect("the marker leads");
        let listed: Vec<Vec<u64>> = body
            .split(' ')
            .filter(|e| !e.is_empty())
            .map(|e| {
                let fields: Vec<&str> = e.split(',').collect();
                assert_eq!(fields.len(), 7, "{e}");
                let f = Isa::from_name(fields[0]).expect("a name in the extension list").feature();
                let numbers: Vec<u64> = fields[1..].iter().map(|n| n.parse().expect("decimal")).collect();
                assert_eq!(numbers, vec![f.level as u64, f.leaf as u64, f.subleaf as u64, f.register as u64, f.bit as u64, f.xcr0], "{e}");
                numbers
            })
            .collect();
        assert_eq!(listed.len(), super::compiled().count());
        for f in FEATURES {
            assert_eq!(body.contains(&format!(" {},", f.name)), f.compiled, "{}", f.name);
        }
    }

    /// The extensions and features a target compiles every crate with are
    /// the ones this module puts at the bottom: the x86-64 table's
    /// baseline level and the AArch64 table's baseline mark agree with
    /// what `rustc --print cfg` enables for the target with no flags.
    /// Under emulation on another architecture's host, that rustc
    /// describes another target, so the test says it skipped.
    #[test]
    fn the_targets_own_features_sit_at_the_bottom() {
        let run = |args: &[&str]| -> String {
            let out = std::process::Command::new("rustc").args(args).output().expect("rustc runs where cargo runs the tests");
            assert!(out.status.success(), "rustc {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).expect("rustc prints UTF-8")
        };
        let version = run(&["-vV"]);
        let host = version.lines().find_map(|l| l.strip_prefix("host: ")).expect("rustc names its host");
        if !host.starts_with(std::env::consts::ARCH) {
            println!("TARGET FLOOR SKIPPED: this {} test runs on a {host} host", std::env::consts::ARCH);
            return;
        }
        let cfg = run(&["--print", "cfg"]);
        let enabled: Vec<&str> = cfg.lines().filter_map(|l| l.strip_prefix("target_feature=")).map(|v| v.trim_matches('"')).collect();
        let mut floored = Vec::new();
        for f in FEATURES {
            let own = cfg!(target_arch = "x86_64") && enabled.contains(&f.name);
            assert_eq!(f.level == Level::X86_64, own, "{}", f.name);
            if own {
                floored.push(f.name);
            }
        }
        for f in super::arm::FEATURES {
            let own = cfg!(target_arch = "aarch64") && enabled.contains(&f.name);
            assert_eq!(f.baseline, own, "{}", f.name);
            if own {
                floored.push(f.name);
            }
        }
        println!("TARGET FLOOR RUNNING {host}: {}", floored.join(" "));
    }

    #[test]
    fn what_the_crate_was_compiled_for_is_what_this_cpu_has() {
        // A test binary runs on the machine that built it, so every
        // extension compiled in is one the CPU reports.
        for isa in super::compiled() {
            assert!(super::detected(isa), "compiled for {} but this CPU does not report it", isa.name());
        }
        for feature in super::arm::compiled() {
            assert!(super::arm::detected(feature), "compiled for {} but this CPU does not report it", feature.name());
        }
    }

    #[test]
    fn the_level_is_the_highest_whose_extensions_are_all_had() {
        let all_had = |level: Level| FEATURES.iter().filter(|f| f.level == level).all(|f| super::has(f.isa));
        let direct = [Level::V2, Level::V3, Level::V4].into_iter().take_while(|l| all_had(*l)).last().unwrap_or(Level::X86_64);
        assert_eq!(super::level(), direct);
        if cfg!(target_arch = "x86_64") {
            eprintln!("cpu level here: {}", super::level().name());
        }
    }

    /// `#[tiered]` calls a tier's copy only when `level` or `arm::tier`
    /// returns it, which is true only when the CPU supports every feature
    /// at or below it, so the copy may be compiled with exactly those.
    #[test]
    fn the_tiered_attribute_enables_exactly_what_each_tier_requires() {
        use super::arm::{Tier, FEATURES as ARM};
        let (x86, arm) = pwrs_macros::__tier_features!();
        for (variant, features) in x86 {
            let level = match variant {
                "V2" => Level::V2,
                "V3" => Level::V3,
                "V4" => Level::V4,
                other => panic!("no psABI level {other}"),
            };
            let mut want: Vec<&str> = FEATURES.iter().filter(|f| f.level > Level::X86_64 && f.level <= level).map(|f| f.name).collect();
            // The table puts the target's own extensions at the baseline,
            // and every one of them is a v2 extension, so the copies may
            // name them as well.
            want.extend(super::TARGET_FLOOR.iter().copied());
            let mut have: Vec<&str> = features.split(',').collect();
            want.sort_unstable();
            have.sort_unstable();
            assert_eq!(have, want, "{variant}");
        }
        for (variant, features) in arm {
            let tier = Tier::from_name(&variant.to_lowercase()).expect("a tier name");
            let mut want: Vec<&str> = ARM.iter().filter(|f| f.tier > Tier::Neon && f.tier <= tier).map(|f| f.name).collect();
            let mut have: Vec<&str> = features.split(',').collect();
            want.sort_unstable();
            have.sort_unstable();
            assert_eq!(have, want, "{variant}");
        }
    }

    #[test]
    fn every_arm_row_sits_at_its_index_and_names_one_feature() {
        use super::arm::{Feature, FEATURES as ARM};
        for (i, f) in ARM.iter().enumerate() {
            assert_eq!(f.feature as usize, i, "{} is out of place", f.name);
            assert_eq!(Feature::from_name(f.name), Some(f.feature));
        }
        let mut names: Vec<&str> = ARM.iter().map(|f| f.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ARM.len(), "a name appears twice");
    }

    #[test]
    fn each_arm_tier_is_its_own_feature_and_names_itself() {
        use super::arm::{Tier, FEATURES as ARM};
        for tier in [Tier::Neon, Tier::Dotprod, Tier::I8mm, Tier::Sve, Tier::Sve2] {
            let own: Vec<&str> = ARM.iter().filter(|f| f.tier == tier).map(|f| f.name).collect();
            assert_eq!(own, vec![tier.name()]);
            assert_eq!(Tier::from_name(tier.name()), Some(tier));
        }
        assert_eq!(Tier::from_name("native"), Some(Tier::Beyond));
        assert_eq!(Tier::from_name("x86-64-v3"), None);
        assert_eq!(Level::from_name("sve2"), None);
        assert!(Tier::Neon < Tier::Dotprod && Tier::Dotprod < Tier::I8mm && Tier::I8mm < Tier::Sve && Tier::Sve < Tier::Sve2 && Tier::Sve2 < Tier::Beyond);
    }

    #[test]
    fn the_arm_tier_is_the_highest_whose_features_are_all_had() {
        use super::arm::{has, tier, Tier, FEATURES as ARM};
        let all_had = |t: Tier| ARM.iter().filter(|f| f.tier == t).all(|f| has(f.feature));
        let direct = [Tier::Dotprod, Tier::I8mm, Tier::Sve, Tier::Sve2].into_iter().take_while(|t| all_had(*t)).last().unwrap_or(Tier::Neon);
        assert_eq!(tier(), direct);
        if cfg!(target_arch = "aarch64") {
            eprintln!("arm tier here: {}", tier().name());
        }
    }
}
