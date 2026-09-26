//! The compiler toolchain: csc from NuGet, run on pwsh's runtime, with
//! reference assemblies for both target frameworks, also from NuGet.
//!
//! Layout under `~/.pwrs/toolchain/<toolset version>/`, which tools of
//! different versions share: one folder per package version, named
//! `<name>-<version>`, for `csc` (Microsoft.Net.Compilers.Toolset),
//! `netcoreref` (Microsoft.NETCore.App.Ref), `sma`
//! (System.Management.Automation), `netstandard` (NETStandard.Library),
//! `psstandard` (PowerShellStandard.Library), and `simd`, `memory`,
//! `buffers` and `unsafe` (System.Numerics.Vectors, System.Memory,
//! System.Buffers, System.Runtime.CompilerServices.Unsafe);
//! `scripts-<stamp>/`, named for the scripts it holds; and
//! `toolchain.lock` (package id, version, sha512 per fetched package).

use std::path::{Path, PathBuf};

use crate::pwsh;
use crate::Error;

/// Whether a compile emits a portable `.pdb` beside the assembly.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DebugSymbols {
    Emit,
    Omit,
}

pub const TOOLSET_VERSION: &str = "5.9.0";

/// The compiler toolset to fetch, which `PWRS_TOOLSET` overrides.
///
/// The compiler runs inside the pwsh process, so it needs a runtime
/// that host already has: 5.9.0 is built for net10.0 and 5.3.0 is the
/// newest built for net9.0. A host whose pwsh predates .NET 10, which
/// is every FreeBSD one at the time of writing, sets this to 5.3.0.
/// Only the compiler's own runtime differs. The assemblies it emits
/// are compiled against the reference packs below, not against the
/// running pwsh, so they are the same either way, and nothing in the
/// generated shell or the runtime uses a language feature newer than
/// C# 9.
pub fn toolset_version() -> String {
    match std::env::var("PWRS_TOOLSET") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => TOOLSET_VERSION.to_string(),
    }
}

/// The Core compile's framework reference pack. Every module references
/// .NET 8's assemblies whichever pwsh builds it, so one build loads on
/// PowerShell 7.4 and every later release: a host binds an assembly
/// only when its own framework is at least the version referenced.
pub const CORE_REF_VERSION: &str = "8.0.31";
/// The Core compile's System.Management.Automation reference, held at
/// 7.4.0 rather than the newest 7.4 package: a host binds the shell only
/// when its own System.Management.Automation is at least the version
/// referenced, and 7.4.0's reference is 7.4.0.0 where 7.4.20's is
/// 7.4.6.500.
pub const CORE_SMA_VERSION: &str = "7.4.0";
pub const NETSTANDARD_VERSION: &str = "2.0.3";
pub const PSSTANDARD_VERSION: &str = "5.1.1";
/// `System.Numerics.Vector<T>` is the only hardware-accelerated vector
/// type Windows PowerShell can reach, since `System.Runtime.Intrinsics`
/// is .NET-only, and neither NETStandard.Library's reference set nor
/// .NET Framework carries it. Spans come from System.Memory, which needs
/// System.Buffers and System.Runtime.CompilerServices.Unsafe. The
/// netstandard2.0 compile references the four packages' .NET Framework
/// builds, because each package's netstandard2.0 build carries a lower
/// assembly version than its .NET Framework build and .NET Framework
/// binds only the exact version referenced; a module with hand-written
/// C# ships those same builds.
pub const SIMD_VERSION: &str = "4.6.1";
pub const MEMORY_VERSION: &str = "4.6.3";
pub const BUFFERS_VERSION: &str = "4.6.1";
pub const UNSAFE_VERSION: &str = "6.1.2";

/// The preprocessor symbols the .NET SDK defines for net8.0, the
/// framework of the Core compile's reference pack.
pub const CORE_DEFINES: &[&str] = &[
    "NET",
    "NETCOREAPP",
    "NET8_0",
    "NET5_0_OR_GREATER",
    "NET6_0_OR_GREATER",
    "NET7_0_OR_GREATER",
    "NET8_0_OR_GREATER",
    "NETCOREAPP1_0_OR_GREATER",
    "NETCOREAPP1_1_OR_GREATER",
    "NETCOREAPP2_0_OR_GREATER",
    "NETCOREAPP2_1_OR_GREATER",
    "NETCOREAPP2_2_OR_GREATER",
    "NETCOREAPP3_0_OR_GREATER",
    "NETCOREAPP3_1_OR_GREATER",
];

/// The preprocessor symbols the .NET SDK defines for netstandard2.0.
pub const NETSTANDARD_DEFINES: &[&str] = &[
    "NETSTANDARD",
    "NETSTANDARD2_0",
    "NETSTANDARD1_0_OR_GREATER",
    "NETSTANDARD1_1_OR_GREATER",
    "NETSTANDARD1_2_OR_GREATER",
    "NETSTANDARD1_3_OR_GREATER",
    "NETSTANDARD1_4_OR_GREATER",
    "NETSTANDARD1_5_OR_GREATER",
    "NETSTANDARD1_6_OR_GREATER",
    "NETSTANDARD2_0_OR_GREATER",
];

const FETCH_PS1: &str = include_str!("../scripts/fetch.ps1");
const CSC_PS1: &str = include_str!("../scripts/csc.ps1");

pub struct Toolchain {
    pub root: PathBuf,
    pub csc_dir: PathBuf,
    pub pshome: PathBuf,
    /// Reference assemblies for the compile whose output goes in the
    /// module's `net10.0` folder: .NET 8's reference pack and the
    /// System.Management.Automation 7.4 reference.
    pub net10_refs: Vec<PathBuf>,
    /// Reference assemblies for the netstandard2.0 compile.
    pub netstandard_refs: Vec<PathBuf>,
    /// The .NET Framework builds the netstandard2.0 compile references
    /// beyond .NET Standard and PowerShell Standard, for a module to ship
    /// beside its Windows PowerShell shell.
    pub desktop_dependencies: Vec<PathBuf>,
    scripts: PathBuf,
}

fn env_dir(key: &str) -> Option<PathBuf> {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => Some(PathBuf::from(v)),
        Ok(_empty) => None,
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(raw)) => Some(PathBuf::from(raw)),
    }
}

fn home_dir() -> Result<PathBuf, Error> {
    if let Some(p) = env_dir("PWRS_HOME") {
        return Ok(p);
    }
    for key in ["USERPROFILE", "HOME"] {
        if let Some(p) = env_dir(key) {
            return Ok(p.join(".pwrs"));
        }
    }
    Err(Error::msg("neither PWRS_HOME, USERPROFILE nor HOME is set; cannot place the toolchain"))
}

fn dlls_in(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::new();
    let rd = std::fs::read_dir(dir).map_err(|e| Error::msg(format!("cannot read {}: {e}", dir.display())))?;
    for entry in rd {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", dir.display())))?;
        let p = entry.path();
        if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dll")) {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Where one version of one package lives under a toolset root. No two
/// versions share a folder, so a tool never writes one that a tool of
/// another version sharing the root reads; cargo-pwrs 0.2.0 reads the
/// unversioned `csc/`, `netstandard/`, `psstandard/` and `simd/`, which
/// this never names.
fn package_dir(root: &Path, dir: &str, version: &str) -> PathBuf {
    root.join(format!("{dir}-{version}"))
}

/// FNV-1a over the texts, with a separator between them so two lists
/// that differ only in where one text ends do not stamp alike.
fn content_stamp(parts: &[&str]) -> u64 {
    const PRIME: u64 = 0x0100_0000_01b3;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for b in part.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(PRIME);
        }
        h ^= 0xff;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// Finds `bincore/csc.dll` anywhere under the extracted toolset.
fn find_csc(root: &Path) -> Result<PathBuf, Error> {
    fn walk(dir: &Path, out: &mut Option<PathBuf>) -> Result<(), Error> {
        let rd = std::fs::read_dir(dir).map_err(|e| Error::msg(format!("cannot read {}: {e}", dir.display())))?;
        for entry in rd {
            let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", dir.display())))?;
            let p = entry.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == "bincore") && p.join("csc.dll").is_file() {
                    *out = Some(p);
                    return Ok(());
                }
                walk(&p, out)?;
                if out.is_some() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
    let mut found = None;
    walk(root, &mut found)?;
    found.ok_or_else(|| Error::msg(format!("no bincore/csc.dll under {}", root.display())))
}

impl Toolchain {
    /// Fetches anything missing, verifies hashes against the lock, and
    /// returns the resolved paths.
    pub fn ensure() -> Result<Toolchain, Error> {
        let toolset = toolset_version();
        // Each toolset gets its own tree, so switching between them
        // neither refetches nor mixes two compilers under one lock.
        let root = home_dir()?.join("toolchain").join(&toolset);
        std::fs::create_dir_all(&root).map_err(|e| Error::msg(format!("cannot create {}: {e}", root.display())))?;
        // Every run writes the scripts, so their folder is named for them
        // and a tool carrying other scripts writes a folder of its own.
        let scripts = root.join(format!("scripts-{:016x}", content_stamp(&[FETCH_PS1, CSC_PS1])));
        let fetch = pwsh::materialize_script(&scripts, "fetch.ps1", FETCH_PS1)?;
        pwsh::materialize_script(&scripts, "csc.ps1", CSC_PS1)?;

        let mut lock = Lock::load(&root.join("toolchain.lock"))?;
        let packages = [
            ("Microsoft.Net.Compilers.Toolset", toolset.as_str(), "csc"),
            ("Microsoft.NETCore.App.Ref", CORE_REF_VERSION, "netcoreref"),
            ("System.Management.Automation", CORE_SMA_VERSION, "sma"),
            ("NETStandard.Library", NETSTANDARD_VERSION, "netstandard"),
            ("PowerShellStandard.Library", PSSTANDARD_VERSION, "psstandard"),
            ("System.Numerics.Vectors", SIMD_VERSION, "simd"),
            ("System.Memory", MEMORY_VERSION, "memory"),
            ("System.Buffers", BUFFERS_VERSION, "buffers"),
            ("System.Runtime.CompilerServices.Unsafe", UNSAFE_VERSION, "unsafe"),
        ];
        for (id, version, dir) in packages {
            let dest = package_dir(&root, dir, version);
            let marker = dest.join(".complete");
            if marker.is_file() {
                continue;
            }
            eprintln!("pwrs: fetching {id} {version}");
            let out = pwsh::run_pwsh_script(&fetch, &[id.to_string(), version.to_string(), dest.display().to_string()])?;
            let hash = out.trim().to_string();
            if hash.len() != 128 {
                return Err(Error::msg(format!("fetch of {id} printed no SHA-512 hash: {out}")));
            }
            lock.verify_or_record(id, version, &hash)?;
            std::fs::write(&marker, &hash).map_err(|e| Error::msg(format!("cannot mark {id} complete: {e}")))?;
        }
        lock.save()?;

        let file = |path: PathBuf| -> Result<PathBuf, Error> {
            if path.is_file() {
                Ok(path)
            } else {
                Err(Error::msg(format!("{} not found", path.display())))
            }
        };

        let pkg = |dir: &str, version: &str| package_dir(&root, dir, version);
        let pshome = pwsh::pshome()?;
        let mut net10_refs = dlls_in(&pkg("netcoreref", CORE_REF_VERSION).join("ref").join("net8.0"))?;
        net10_refs.push(file(pkg("sma", CORE_SMA_VERSION).join("ref").join("net8.0").join("System.Management.Automation.dll"))?);

        let mut netstandard_refs = dlls_in(&pkg("netstandard", NETSTANDARD_VERSION).join("build").join("netstandard2.0").join("ref"))?;
        netstandard_refs.push(file(pkg("psstandard", PSSTANDARD_VERSION).join("lib").join("netstandard2.0").join("System.Management.Automation.dll"))?);
        let desktop_dependencies = vec![
            file(pkg("simd", SIMD_VERSION).join("lib").join("net462").join("System.Numerics.Vectors.dll"))?,
            file(pkg("memory", MEMORY_VERSION).join("lib").join("net462").join("System.Memory.dll"))?,
            file(pkg("buffers", BUFFERS_VERSION).join("lib").join("net462").join("System.Buffers.dll"))?,
            file(pkg("unsafe", UNSAFE_VERSION).join("lib").join("net462").join("System.Runtime.CompilerServices.Unsafe.dll"))?,
        ];
        netstandard_refs.extend(desktop_dependencies.iter().cloned());

        let csc_dir = find_csc(&pkg("csc", toolset.as_str()))?;
        Ok(Toolchain { root, csc_dir, pshome, net10_refs, netstandard_refs, desktop_dependencies, scripts })
    }

    /// Compiles C# sources into a library. `extra_refs` are
    /// implementation assemblies this assembly references on top of
    /// the environment's reference set.
    pub fn compile(&self, env: &CompileEnv<'_>, sources: &[PathBuf], extra_refs: &[PathBuf], out: &Path) -> Result<(), Error> {
        let work = env.work;
        let debug = env.debug;
        let refs = env.refs;
        let defines = env.defines;
        std::fs::create_dir_all(work).map_err(|e| Error::msg(format!("cannot create {}: {e}", work.display())))?;
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::msg(format!("cannot create {}: {e}", parent.display())))?;
        }
        let mut rsp = String::new();
        rsp.push_str("/nologo\n/nostdlib+\n/target:library\n/unsafe+\n/nullable:enable\n/langversion:latest\n");
        // `/warnaserror-` keeps a csc warning from failing a module
        // author's build over generated code they did not write. The
        // warning itself is printed below.
        rsp.push_str("/deterministic+\n/optimize+\n/warnaserror-\n");
        rsp.push_str(match debug {
            DebugSymbols::Emit => "/debug:portable\n",
            DebugSymbols::Omit => "/debug-\n",
        });
        rsp.push_str(&format!("/out:\"{}\"\n", out.display()));
        if !defines.is_empty() {
            rsp.push_str(&format!("/define:{}\n", defines.join(";")));
        }
        for r in refs.iter().chain(extra_refs) {
            rsp.push_str(&format!("/reference:\"{}\"\n", r.display()));
        }
        for s in sources {
            rsp.push_str(&format!("\"{}\"\n", s.display()));
        }
        let name = match out.file_stem() {
            Some(stem) => stem.to_string_lossy().into_owned(),
            None => return Err(Error::msg(format!("{} has no file name", out.display()))),
        };
        let rsp_path = work.join(format!("{name}.rsp"));
        std::fs::write(&rsp_path, rsp).map_err(|e| Error::msg(format!("cannot write {}: {e}", rsp_path.display())))?;
        let csc = self.scripts.join("csc.ps1");
        let stdout = pwsh::run_pwsh_script(&csc, &[self.csc_dir.display().to_string(), format!("/noconfig @{}", rsp_path.display())])
            .map_err(|e| Error::msg(format!("compiling {}:\n{e}", out.display())))?;
        let warnings = stdout.trim();
        if !warnings.is_empty() {
            eprintln!("{warnings}");
        }
        if !out.is_file() {
            return Err(Error::msg(format!("csc reported success but {} does not exist", out.display())));
        }
        Ok(())
    }
}

/// What every assembly of one target framework compiles against: the
/// reference set, the preprocessor symbols, the scratch directory the
/// response file is written to, and whether a `.pdb` is written beside
/// the output.
pub struct CompileEnv<'a> {
    pub refs: &'a [PathBuf],
    pub defines: &'a [&'a str],
    pub work: &'a Path,
    pub debug: DebugSymbols,
}

/// Recorded hashes; a package fetched again must match.
struct Lock {
    path: PathBuf,
    entries: Vec<(String, String, String)>,
}

impl Lock {
    fn load(path: &Path) -> Result<Lock, Error> {
        let mut entries = Vec::new();
        if path.is_file() {
            let text = std::fs::read_to_string(path).map_err(|e| Error::msg(format!("cannot read {}: {e}", path.display())))?;
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() == 3 {
                    entries.push((parts[0].to_string(), parts[1].to_string(), parts[2].to_string()));
                } else if !line.trim().is_empty() {
                    return Err(Error::msg(format!("malformed line in {}: {line}", path.display())));
                }
            }
        }
        Ok(Lock { path: path.to_path_buf(), entries })
    }

    fn verify_or_record(&mut self, id: &str, version: &str, hash: &str) -> Result<(), Error> {
        for (i, v, h) in &self.entries {
            if i == id && v == version {
                if h == hash {
                    return Ok(());
                }
                return Err(Error::msg(format!(
                    "{id} {version} downloaded with sha512 {hash} but {} records {h}; refusing to use it",
                    self.path.display()
                )));
            }
        }
        self.entries.push((id.to_string(), version.to_string(), hash.to_string()));
        Ok(())
    }

    fn save(&self) -> Result<(), Error> {
        let mut s = String::new();
        for (i, v, h) in &self.entries {
            s.push_str(&format!("{i} {v} {h}\n"));
        }
        std::fs::write(&self.path, s).map_err(|e| Error::msg(format!("cannot write {}: {e}", self.path.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two versions of one package never share a folder, and none is the
    /// unversioned folder cargo-pwrs 0.2.0 reads from the same root.
    #[test]
    fn every_package_version_has_a_folder_of_its_own() {
        let root = Path::new("toolchain-root");
        let older = package_dir(root, "simd", "4.5.0");
        let newer = package_dir(root, "simd", SIMD_VERSION);
        assert_ne!(older, newer);
        for dir in ["csc", "netstandard", "psstandard", "simd"] {
            assert_ne!(package_dir(root, dir, "1.0.0"), root.join(dir), "{dir}");
        }
        assert_eq!(package_dir(root, "memory", "4.6.3"), root.join("memory-4.6.3"));
    }

    /// The scripts' folder follows their text, so a tool whose scripts
    /// differ writes a folder of its own.
    #[test]
    fn the_scripts_folder_changes_with_the_scripts() {
        let current = content_stamp(&[FETCH_PS1, CSC_PS1]);
        assert_eq!(current, content_stamp(&[FETCH_PS1, CSC_PS1]));
        assert_ne!(current, content_stamp(&[FETCH_PS1, "# another csc.ps1"]));
        assert_ne!(content_stamp(&["ab", "c"]), content_stamp(&["a", "bc"]));
    }
}
