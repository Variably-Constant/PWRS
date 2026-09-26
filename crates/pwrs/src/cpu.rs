//! Which x86-64 instruction-set extensions the CPU and the operating
//! system offer, and which ones this library was compiled to require.
//!
//! A module compiled with `-C target-cpu=native`, or with any flag that
//! enables an extension for the whole crate, carries those instructions
//! throughout, and on a CPU without them the first one to run ends the
//! host process. [`REQUIREMENTS`] lists the extensions this crate was
//! compiled with. `export_module!` publishes it as the data export
//! `pwrs_cpu_requirements`, beside `pwrs_cpuid` and `pwrs_xgetbv`, whose
//! bodies are only the register instructions they name, so the runtime
//! can read the list and ask the CPU before any compiled module code
//! runs. cargo-pwrs reads the same bytes out of the built file.
//!
//! [`has`] answers at run time, for a module choosing between kernels.
//! `PWRS_CPU_MAX` caps what it reports at one x86-64 psABI level
//! (`x86-64`, `x86-64-v2`, `x86-64-v3` or `x86-64-v4`; `native` or unset
//! is no cap), so one machine can run every tier of a module, and the
//! runtime's import check applies the same cap.

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
    /// string are no cap, and anything else unrecognised is `None`.
    pub fn from_name(name: &str) -> Option<Level> {
        match name.trim() {
            "x86-64" => Some(Level::X86_64),
            "x86-64-v2" => Some(Level::V2),
            "x86-64-v3" => Some(Level::V3),
            "x86-64-v4" => Some(Level::V4),
            "native" | "" => Some(Level::Beyond),
            _unrecognised => None,
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

/// `level`, except that the extensions a target itself enables sit at
/// the bottom: x86_64-pc-windows-msvc compiles everything for SSE3 and
/// CMPXCHG16B, so a cap cannot take those away from a Windows module.
const fn floor(name: &str, level: Level) -> Level {
    if cfg!(windows) && (same(name, "sse3") || same(name, "cmpxchg16b")) { Level::X86_64 } else { level }
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
        /// An x86-64 instruction-set extension this module can ask the
        /// CPU about.
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

const _: () = assert!(FEATURES.len() <= 64, "detection keeps one bit per extension in a u64");

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

/// Bytes of [`REQUIREMENTS`]: room for every extension at its longest.
pub const REQUIREMENTS_SIZE: usize = 4096;

/// The extensions this crate was compiled to require, in the form the
/// runtime and cargo-pwrs read: `PWRS-CPU/1`, then for each extension a
/// space and `name,level,leaf,subleaf,register,bit,xcr0` in decimal,
/// then NUL.
pub const REQUIREMENTS: [u8; REQUIREMENTS_SIZE] = requirements();

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
    // The NUL after the last byte written, which the zeroed array holds
    // as long as the list leaves room for it.
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
        let v2: Vec<&str> = if cfg!(windows) { vec!["popcnt", "sse4.1", "sse4.2", "ssse3"] } else { vec!["cmpxchg16b", "popcnt", "sse3", "sse4.1", "sse4.2", "ssse3"] };
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
        let body = text.strip_prefix("PWRS-CPU/1").expect("the marker leads");
        let listed: Vec<Vec<u64>> = body
            .split(' ')
            .filter(|e| !e.is_empty())
            .map(|e| {
                let fields: Vec<&str> = e.split(',').collect();
                assert_eq!(fields.len(), 7, "{e}");
                let f = Isa::from_name(fields[0]).expect("a known name").feature();
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

    #[test]
    fn what_the_crate_was_compiled_for_is_what_this_cpu_has() {
        // A test binary runs on the machine that built it, so every
        // extension compiled in is one the CPU reports.
        for isa in super::compiled() {
            assert!(super::detected(isa), "compiled for {} but this CPU does not report it", isa.name());
        }
    }
}
