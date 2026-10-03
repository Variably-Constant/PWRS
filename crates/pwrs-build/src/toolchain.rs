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
//! System.Buffers, System.Runtime.CompilerServices.Unsafe); `desktopref`
//! (Microsoft.WindowsDesktop.App.Ref) and `netfxref472`
//! (Microsoft.NETFramework.ReferenceAssemblies.net472), fetched only for a
//! module that names `references`; `scripts-<stamp>/`, named for the
//! scripts it holds; `toolchain.lock` (package id, version, sha512 per
//! fetched package); and `fetch.guard`, locked while a tool fetches.

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

/// The reference pack a module's `references` resolve from for the Core
/// compile: Windows Forms, WPF and the rest of the Windows desktop
/// framework, at the Core reference pack's own version.
pub const DESKTOP_REF_VERSION: &str = CORE_REF_VERSION;
/// The .NET Framework 4.7.2 reference assemblies a module's `references`
/// resolve from, and the netstandard2.0 folder's shell then compiles
/// against, so such a module requires .NET Framework 4.7.2.
pub const NETFX_REF_VERSION: &str = "1.0.3";

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
    /// PowerShell Standard's System.Management.Automation, which the
    /// netstandard2.0 folder's compiles reference whatever else they do.
    pub psstandard: PathBuf,
    /// The .NET Framework builds the netstandard2.0 compile references
    /// beyond .NET Standard and PowerShell Standard, for a module to ship
    /// beside its Windows PowerShell shell.
    pub desktop_dependencies: Vec<PathBuf>,
    scripts: PathBuf,
    /// The fetch script, kept for the packages fetched only on demand.
    fetch: PathBuf,
}

/// The assemblies a module's `references` add to its shell's compiles,
/// resolved per target.
#[derive(Default, Debug)]
pub struct DesktopReferences {
    /// For the Core compile, from the desktop reference pack. Each one
    /// replaces a Core reference of the same file name.
    pub core: Vec<PathBuf>,
    /// The names found in the .NET Framework 4.7.2 reference assemblies.
    pub framework: Vec<PathBuf>,
    /// The netstandard2.0 folder's shell compile set when any name is
    /// given: every reference assembly .NET Framework 4.7.2 lists, its
    /// facades, PowerShell Standard and the .NET Framework dependencies,
    /// in place of netstandard 2.0's set. Empty when none is given.
    pub framework_set: Vec<PathBuf>,
    /// Each name, in the order named, with whether the Core and the
    /// netstandard2.0 compile took it.
    pub taken: Vec<(String, bool, bool)>,
}

impl DesktopReferences {
    /// `base` without the references `added` replaces: those of the same
    /// file name, compared without regard to case.
    pub fn replacing(base: &[PathBuf], added: &[PathBuf]) -> Vec<PathBuf> {
        let names: Vec<String> = added.iter().filter_map(|p| file_key(p)).collect();
        base.iter().filter(|p| file_key(p).is_none_or(|k| !names.contains(&k))).cloned().collect()
    }
}

/// A reference file's name without `.dll`, lowercased.
fn file_key(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    Some(name.strip_suffix(".dll").unwrap_or(&name).to_string())
}

/// Resolves each name in `names` against the file names of the desktop
/// reference pack (`core`) and the .NET Framework 4.7.2 reference
/// assemblies (`framework`), without regard to case. A name either set
/// carries goes to the compile whose set it is; a name neither carries
/// is refused.
pub fn resolve_references(names: &[String], core: &[PathBuf], framework: &[PathBuf]) -> Result<DesktopReferences, Error> {
    let find = |set: &[PathBuf], key: &str| set.iter().find(|p| file_key(p).as_deref() == Some(key)).cloned();
    let mut out = DesktopReferences::default();
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        if name.is_empty() || name.contains(['/', '\\']) || name.to_ascii_lowercase().ends_with(".dll") {
            return Err(Error::msg(format!(
                "[package.metadata.pwrs] references names {name:?}; an entry is an assembly name such as System.Windows.Forms, with no folder and no .dll"
            )));
        }
        let key = name.to_ascii_lowercase();
        if seen.contains(&key) {
            return Err(Error::msg(format!("[package.metadata.pwrs] references names {name} twice")));
        }
        seen.push(key.clone());
        let in_core = find(core, &key);
        let in_framework = find(framework, &key);
        if in_core.is_none() && in_framework.is_none() {
            return Err(Error::msg(format!(
                "[package.metadata.pwrs] references names {name}, which neither Microsoft.WindowsDesktop.App.Ref {DESKTOP_REF_VERSION} \
                 nor the .NET Framework 4.7.2 reference assemblies carry"
            )));
        }
        out.taken.push((name.clone(), in_core.is_some(), in_framework.is_some()));
        out.core.extend(in_core);
        out.framework.extend(in_framework);
    }
    Ok(out)
}

/// `root`'s `fetch.guard`, open and locked: one tool at a time writes the
/// scripts, fetches and records hashes under `root`, across threads and
/// processes. The lock is released when the file is dropped or its
/// process ends.
fn fetch_guard(root: &Path) -> Result<std::fs::File, Error> {
    let path = root.join("fetch.guard");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| Error::msg(format!("cannot open {}: {e}", path.display())))?;
    file.lock().map_err(|e| Error::msg(format!("cannot lock {}: {e}", path.display())))?;
    Ok(file)
}

/// Fetches one package into its folder under `root` unless it is there
/// already, and checks its hash against the lock.
fn fetch_package(root: &Path, fetch: &Path, lock: &mut Lock, id: &str, version: &str, dir: &str) -> Result<PathBuf, Error> {
    let dest = package_dir(root, dir, version);
    let marker = dest.join(".complete");
    if marker.is_file() {
        return Ok(dest);
    }
    eprintln!("pwrs: fetching {id} {version}");
    let out = pwsh::run_pwsh_script(fetch, &[id.to_string(), version.to_string(), dest.display().to_string()])?;
    let hash = out.trim().to_string();
    if hash.len() != 128 {
        return Err(Error::msg(format!("fetch of {id} printed no SHA-512 hash: {out}")));
    }
    lock.verify_or_record(id, version, &hash)?;
    std::fs::write(&marker, &hash).map_err(|e| Error::msg(format!("cannot mark {id} complete: {e}")))?;
    Ok(dest)
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

/// The assemblies in a .NET Framework reference folder that its
/// `RedistList/FrameworkList.xml` lists. The folder also holds native
/// images, System.EnterpriseServices.Thunk.dll and .Wrapper.dll, which
/// csc refuses as references.
fn framework_assemblies(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let list = dir.join("RedistList").join("FrameworkList.xml");
    let text = std::fs::read_to_string(&list).map_err(|e| Error::msg(format!("cannot read {}: {e}", list.display())))?;
    let listed: Vec<String> = text.split("AssemblyName=\"").skip(1).filter_map(|rest| rest.split('"').next()).map(str::to_ascii_lowercase).collect();
    if listed.is_empty() {
        return Err(Error::msg(format!("{} lists no assembly", list.display())));
    }
    Ok(dlls_in(dir)?.into_iter().filter(|p| file_key(p).is_some_and(|k| listed.contains(&k))).collect())
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
        let guard = fetch_guard(&root)?;
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
            fetch_package(&root, &fetch, &mut lock, id, version, dir)?;
        }
        lock.save()?;
        drop(guard);

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

        let psstandard = file(pkg("psstandard", PSSTANDARD_VERSION).join("lib").join("netstandard2.0").join("System.Management.Automation.dll"))?;
        let mut netstandard_refs = dlls_in(&pkg("netstandard", NETSTANDARD_VERSION).join("build").join("netstandard2.0").join("ref"))?;
        netstandard_refs.push(psstandard.clone());
        let desktop_dependencies = vec![
            file(pkg("simd", SIMD_VERSION).join("lib").join("net462").join("System.Numerics.Vectors.dll"))?,
            file(pkg("memory", MEMORY_VERSION).join("lib").join("net462").join("System.Memory.dll"))?,
            file(pkg("buffers", BUFFERS_VERSION).join("lib").join("net462").join("System.Buffers.dll"))?,
            file(pkg("unsafe", UNSAFE_VERSION).join("lib").join("net462").join("System.Runtime.CompilerServices.Unsafe.dll"))?,
        ];
        netstandard_refs.extend(desktop_dependencies.iter().cloned());

        let csc_dir = find_csc(&pkg("csc", toolset.as_str()))?;
        Ok(Toolchain { root, csc_dir, pshome, net10_refs, netstandard_refs, psstandard, desktop_dependencies, scripts, fetch })
    }

    /// The assemblies `names` add to a module's shell compiles: from the
    /// desktop reference pack for the Core compile, and for the
    /// netstandard2.0 folder's shell the whole .NET Framework 4.7.2 set,
    /// which carries every name the .NET Framework resolves; see
    /// [`resolve_references`] and [`DesktopReferences::framework_set`].
    /// Both packages are fetched the first time a module names any, and
    /// checked against the lock like the rest.
    pub fn desktop_references(&self, names: &[String]) -> Result<DesktopReferences, Error> {
        if names.is_empty() {
            return Ok(DesktopReferences::default());
        }
        let guard = fetch_guard(&self.root)?;
        let mut lock = Lock::load(&self.root.join("toolchain.lock"))?;
        let desktop = fetch_package(&self.root, &self.fetch, &mut lock, "Microsoft.WindowsDesktop.App.Ref", DESKTOP_REF_VERSION, "desktopref")?;
        let netfx = fetch_package(&self.root, &self.fetch, &mut lock, "Microsoft.NETFramework.ReferenceAssemblies.net472", NETFX_REF_VERSION, "netfxref472")?;
        lock.save()?;
        drop(guard);
        let core = dlls_in(&desktop.join("ref").join("net8.0"))?;
        let framework_dir = netfx.join("build").join(".NETFramework").join("v4.7.2");
        let framework = framework_assemblies(&framework_dir)?;
        let mut out = resolve_references(names, &core, &framework)?;
        // The facades, netstandard.dll among them, let the shell consume
        // the netstandard2.0 runtime it is compiled beside.
        out.framework_set = framework;
        out.framework_set.extend(dlls_in(&framework_dir.join("Facades"))?);
        out.framework_set.push(self.psstandard.clone());
        out.framework_set.extend(self.desktop_dependencies.iter().cloned());
        Ok(out)
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
        if !env.path_map.is_empty() {
            let pairs: Vec<String> = env.path_map.iter().map(|(from, to)| format!("{}={to}", from.display())).collect();
            rsp.push_str(&format!("/pathmap:\"{}\"\n", pairs.join(",")));
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
/// response file is written to, whether a `.pdb` is written beside the
/// output, and the folders written under a fixed name instead.
pub struct CompileEnv<'a> {
    pub refs: &'a [PathBuf],
    pub defines: &'a [&'a str],
    pub work: &'a Path,
    pub debug: DebugSymbols,
    /// Each folder, and the name the `.pdb` and the assembly's pointer to
    /// it give paths under it, so the same sources compiled in two folders
    /// give the same bytes.
    pub path_map: &'a [(PathBuf, &'a str)],
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

    fn files(dir: &str, names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| Path::new(dir).join(format!("{n}.dll"))).collect()
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// Each name goes to the compiles whose sets carry it, matched without
    /// regard to case, in the order named.
    #[test]
    fn a_reference_goes_to_each_compile_whose_set_carries_it() {
        let core = files("desk", &["System.Windows.Forms", "System.Drawing.Common", "WindowsBase", "System.Drawing"]);
        let framework = files("fx", &["System.Windows.Forms", "WindowsBase", "PresentationCore", "System.Drawing"]);
        let r = resolve_references(&names(&["system.windows.forms", "System.Drawing.Common", "PresentationCore", "System.Drawing"]), &core, &framework).expect("resolved");
        assert_eq!(r.core, vec![core[0].clone(), core[1].clone(), core[3].clone()]);
        assert_eq!(r.framework, vec![framework[0].clone(), framework[2].clone(), framework[3].clone()]);
        assert_eq!(
            r.taken,
            vec![
                ("system.windows.forms".to_string(), true, true),
                ("System.Drawing.Common".to_string(), true, false),
                ("PresentationCore".to_string(), false, true),
                ("System.Drawing".to_string(), true, true)
            ]
        );
        assert!(r.framework_set.is_empty(), "resolving alone gathers no compile set");
        assert!(resolve_references(&[], &core, &framework).expect("nothing named").taken.is_empty());
    }

    /// A name neither set carries, one that is a path or a file name, and
    /// one named twice each stop the build, naming the key and the name.
    #[test]
    fn a_reference_that_resolves_nowhere_is_refused() {
        let core = files("desk", &["System.Drawing", "System.Windows.Forms"]);
        let framework = files("fx", &["System.Drawing", "System.Windows.Forms"]);
        let refused = |list: &[&str], says: &str| match resolve_references(&names(list), &core, &framework) {
            Ok(r) => panic!("{list:?} resolved: {r:?}"),
            Err(e) => assert!(e.to_string().contains(says), "{list:?}: {e}"),
        };
        refused(&["System.Text.Json"], "references names System.Text.Json, which neither Microsoft.WindowsDesktop.App.Ref");
        refused(&["System.Windows.Forms.dll"], "with no folder and no .dll");
        refused(&["ref/System.Windows.Forms"], "with no folder and no .dll");
        refused(&[""], "an entry is an assembly name");
        refused(&["System.Windows.Forms", "system.windows.forms"], "references names system.windows.forms twice");
    }

    /// Only the assemblies FrameworkList.xml lists are references; the
    /// native images and documentation beside them are not.
    #[test]
    fn a_framework_reference_is_one_its_list_names() {
        let dir = std::env::temp_dir().join(format!("pwrs-framework-list-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("RedistList")).expect("the list's folder");
        std::fs::write(
            dir.join("RedistList").join("FrameworkList.xml"),
            "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<FileList Name=\".NET Framework 4.7.2\">\n  \
             <File AssemblyName=\"mscorlib\" Version=\"4.0.0.0\" />\n  <File AssemblyName=\"System.Drawing\" Version=\"4.0.0.0\" />\n</FileList>\n",
        )
        .expect("the list");
        for name in ["mscorlib.dll", "System.Drawing.dll", "System.Drawing.xml", "System.EnterpriseServices.Thunk.dll"] {
            std::fs::write(dir.join(name), b"").expect("a file");
        }
        let found = framework_assemblies(&dir);
        let removed = std::fs::remove_dir_all(&dir);
        assert_eq!(found.expect("the listed assemblies"), vec![dir.join("System.Drawing.dll"), dir.join("mscorlib.dll")]);
        removed.expect("the test's folder is removed");
    }

    /// Against the .NET Framework set a named reference brings, the
    /// netstandard2.0 folder's shell uses System.Drawing beside a library
    /// compiled for netstandard 2.0, as it uses the module's runtime.
    #[test]
    fn the_framework_set_compiles_drawing_beside_a_netstandard_library() {
        let tc = Toolchain::ensure().expect("the toolchain");
        let desktop = tc.desktop_references(&names(&["System.Windows.Forms"])).expect("the references");
        let base = std::env::temp_dir().join(format!("pwrs-framework-set-{}", std::process::id()));
        let work = base.join("work");
        let compiled = (|| -> Result<(), Error> {
            std::fs::create_dir_all(&work).map_err(|e| Error::msg(format!("cannot create {}: {e}", work.display())))?;
            let library_src = work.join("Library.cs");
            std::fs::write(&library_src, "public static class Library { public static int Width() => 4; }\n")
                .map_err(|e| Error::msg(format!("cannot write {}: {e}", library_src.display())))?;
            let library = base.join("Library.dll");
            let netstandard = CompileEnv { refs: &tc.netstandard_refs, defines: NETSTANDARD_DEFINES, work: &work, debug: DebugSymbols::Omit, path_map: &[] };
            tc.compile(&netstandard, &[library_src], &[], &library)?;
            let shell_src = work.join("Shell.cs");
            std::fs::write(
                &shell_src,
                "public static class Shell { public static int Area() { using (var b = new System.Drawing.Bitmap(Library.Width(), 3)) { return b.Width * b.Height; } } }\n",
            )
            .map_err(|e| Error::msg(format!("cannot write {}: {e}", shell_src.display())))?;
            let framework = CompileEnv { refs: &desktop.framework_set, defines: NETSTANDARD_DEFINES, work: &work, debug: DebugSymbols::Omit, path_map: &[] };
            tc.compile(&framework, &[shell_src], &[library], &base.join("Shell.dll"))
        })();
        let removed = std::fs::remove_dir_all(&base);
        compiled.expect("the shell compiles against the .NET Framework set");
        removed.expect("the test's folder is removed");
    }

    /// A desktop reference replaces the Core reference of the same file
    /// name, as the desktop pack's WindowsBase replaces the Core pack's.
    #[test]
    fn a_desktop_reference_replaces_the_core_one_of_its_name() {
        let base = files("core", &["System.Runtime", "WindowsBase", "Microsoft.VisualBasic"]);
        let added = files("desk", &["windowsbase", "System.Windows.Forms"]);
        assert_eq!(DesktopReferences::replacing(&base, &added), vec![base[0].clone(), base[2].clone()]);
        assert_eq!(DesktopReferences::replacing(&base, &[]), base);
    }

    /// One source compiled with a .pdb in two folders of different length
    /// gives the same bytes when both folders are mapped to the same
    /// names, and different bytes when they are not.
    #[test]
    fn mapped_folders_compile_to_the_same_bytes() {
        let tc = Toolchain::ensure().expect("the toolchain");
        let base = std::env::temp_dir().join(format!("pwrs-path-map-{}", std::process::id()));
        let compile = |side: &str, mapped: bool| -> Vec<u8> {
            let work = base.join(side).join("work");
            let module = base.join(side).join("module");
            std::fs::create_dir_all(work.join("src")).expect("the source folder");
            let src = work.join("src").join("Probe.cs");
            std::fs::write(&src, "public static class Probe { public static int Answer() => 42; }\n").expect("the source");
            let map = [(work.clone(), "/pwrs/work"), (module.clone(), "/pwrs/module")];
            let path_map: &[(PathBuf, &str)] = if mapped { &map } else { &[] };
            let env = CompileEnv { refs: &tc.net10_refs, defines: CORE_DEFINES, work: &work.join("rsp"), debug: DebugSymbols::Emit, path_map };
            let out = module.join(if mapped { "Mapped.dll" } else { "Unmapped.dll" });
            tc.compile(&env, &[src], &[], &out).expect("the compile");
            std::fs::read(&out).expect("the assembly")
        };
        let mapped = (compile("a", true), compile("bb", true));
        let unmapped = (compile("a", false), compile("bb", false));
        let removed = std::fs::remove_dir_all(&base);
        assert_eq!(mapped.0, mapped.1, "the mapped compiles differ");
        assert_ne!(unmapped.0, unmapped.1, "the unmapped compiles agree, so this test could not see a difference");
        removed.expect("the test's folder is removed");
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
