//! The compiler toolchain: csc from NuGet, run on pwsh's runtime, with
//! reference assemblies for both target frameworks, also from NuGet. A
//! compile that writes a Windows `.pdb` runs the same package's .NET
//! Framework `csc.exe` instead, on Windows.
//!
//! Layout under `~/.pwrs/toolchain/<toolset version>/v2/`: one folder
//! per package version, named `<name>-<version>`, for `csc`
//! (Microsoft.Net.Compilers.Toolset),
//! `netcoreref` (Microsoft.NETCore.App.Ref), `sma`
//! (System.Management.Automation), `netstandard` (NETStandard.Library),
//! `psstandard` (PowerShellStandard.Library), `simd`, `memory`,
//! `buffers` and `unsafe` (System.Numerics.Vectors, System.Memory,
//! System.Buffers, System.Runtime.CompilerServices.Unsafe), and
//! `accesscontrol`, `principalwindows`, `filesystemaccesscontrol` and
//! `registry` (System.Security.AccessControl,
//! System.Security.Principal.Windows, System.IO.FileSystem.AccessControl,
//! Microsoft.Win32.Registry); `desktopref`
//! (Microsoft.WindowsDesktop.App.Ref) and `netfxref472`
//! (Microsoft.NETFramework.ReferenceAssemblies.net472), fetched only for a
//! module that names `references`; `netfxref472` and `ps51ref`
//! (Microsoft.PowerShell.5.1.ReferenceAssemblies), fetched only for an
//! executable for Windows PowerShell; `pester` (Pester), fetched only to
//! run a module's Pester suites; `pkcs` (System.Security.Cryptography.Pkcs)
//! and `openauthenticode` (OpenAuthenticode, from the PowerShell Gallery),
//! fetched only to sign a module; `scripts-<stamp>/`, named for the
//! scripts in it; and `toolchain.lock` (package id, version, sha512 per
//! fetched package).
//!
//! Tools fetch side by side and take no lock. A missing package is
//! fetched into a folder of the fetching tool's own, marked complete and
//! renamed into place, so a package folder that exists with its marker is
//! whole, and of two tools fetching one package the second to rename
//! takes the first's folder. Scripts are written under a temporary name
//! and renamed into place, and a hash is recorded by appending one line
//! to `toolchain.lock` in one write, whose first line for a package is
//! its record.
//!
//! cargo-pwrs versions before this layout store their packages directly
//! under `<toolset version>/`, fetch into a package's own folder under a
//! lock on `<toolset version>/fetch.guard`, and rewrite
//! `<toolset version>/toolchain.lock` whole; this layout reads nothing
//! under `v2/` from them and writes nothing outside it, and checks a
//! fetched package against their `toolchain.lock` as well as its own.

use std::path::{Path, PathBuf};

use crate::pwsh;
use crate::Error;

/// Whether a compile writes a `.pdb` beside the assembly, and in which
/// format.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DebugSymbols {
    /// A portable `.pdb`, which .NET reads on every system.
    Emit,
    /// A Windows `.pdb`, which .NET Framework reads for a stack trace's
    /// file and line in an app built for a .NET Framework before 4.7.2,
    /// as Windows PowerShell is; it ignores a portable one there. Only
    /// Windows writes one, through the toolset's .NET Framework compiler.
    EmitWindows,
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
/// The Core compile's System.Management.Automation reference, pinned at
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
/// .NET Framework includes it. Spans come from System.Memory, which needs
/// System.Buffers and System.Runtime.CompilerServices.Unsafe. The
/// netstandard2.0 compile references the four packages' .NET Framework
/// builds, because each package's netstandard2.0 build has a lower
/// assembly version than its .NET Framework build and .NET Framework
/// binds only the exact version referenced; a module with hand-written
/// C# ships those same builds.
pub const SIMD_VERSION: &str = "4.6.1";
pub const MEMORY_VERSION: &str = "4.6.3";
pub const BUFFERS_VERSION: &str = "4.6.1";
pub const UNSAFE_VERSION: &str = "6.1.2";

/// The access-control packages, by package id and folder: the types
/// Windows PowerShell's engine takes from mscorlib, which .NET Standard
/// 2.0 leaves out. They are `ObjectSecurity`, the file, directory and
/// registry descriptors, and the identities their rules name. The
/// netstandard2.0 compile references each package's netstandard2.0
/// build, and every module ships each one's .NET Framework build beside
/// its Windows PowerShell shell, which forwards those types to mscorlib.
/// Each assembly is named for its package.
pub const ACCESS_CONTROL_PACKAGES: [(&str, &str); 4] = [
    ("System.Security.AccessControl", "accesscontrol"),
    ("System.Security.Principal.Windows", "principalwindows"),
    ("System.IO.FileSystem.AccessControl", "filesystemaccesscontrol"),
    ("Microsoft.Win32.Registry", "registry"),
];
/// The version of every access-control package. Its assemblies have the
/// versions PowerShell Standard's System.Management.Automation names;
/// later versions have others.
pub const ACCESS_CONTROL_VERSION: &str = "4.5.0";

/// The reference pack a module's `references` resolve from for the Core
/// compile: Windows Forms, WPF and the rest of the Windows desktop
/// framework, at the Core reference pack's own version.
pub const DESKTOP_REF_VERSION: &str = CORE_REF_VERSION;
/// The .NET Framework 4.7.2 reference assemblies a module's `references`
/// resolve from, and the netstandard2.0 folder's shell then compiles
/// against, so such a module requires .NET Framework 4.7.2.
pub const NETFX_REF_VERSION: &str = "1.0.3";
/// Windows PowerShell 5.1's reference assemblies, whose full
/// System.Management.Automation and Microsoft.PowerShell.ConsoleHost an
/// executable for Windows PowerShell compiles against: both are the
/// 3.0.0.0 in the GAC, and the full one has what PowerShell
/// Standard's leaves out, such as RunspaceConfiguration, which starting
/// the console host takes.
pub const PS51_REF_VERSION: &str = "1.0.0";
/// The Pester a module's suites run under, in pwsh and in Windows
/// PowerShell alike. nuget.org has the same module as the PowerShell
/// Gallery, file for file, under the package's `tools/`.
pub const PESTER_VERSION: &str = "6.2.0";
/// The System.Security.Cryptography.Pkcs a module's signer compiles
/// against: the newest 8.0 release, whose assembly version, 8.0.0.0,
/// binds to the one every pwsh from 7.4 on ships.
pub const PKCS_VERSION: &str = "8.0.2";
/// The OpenAuthenticode that signs a module whose signing settings name
/// it. It is published on the PowerShell Gallery only.
pub const OPENAUTHENTICODE_VERSION: &str = "0.6.3";

/// The preprocessor symbols the .NET SDK defines for net472, the
/// framework an executable for Windows PowerShell compiles for.
pub const FRAMEWORK_DEFINES: &[&str] = &[
    "NETFRAMEWORK",
    "NET472",
    "NET20_OR_GREATER",
    "NET30_OR_GREATER",
    "NET35_OR_GREATER",
    "NET40_OR_GREATER",
    "NET45_OR_GREATER",
    "NET451_OR_GREATER",
    "NET452_OR_GREATER",
    "NET46_OR_GREATER",
    "NET461_OR_GREATER",
    "NET462_OR_GREATER",
    "NET47_OR_GREATER",
    "NET471_OR_GREATER",
    "NET472_OR_GREATER",
];

/// The assembly attribute the .NET SDK compiles into a net472 build, which
/// [`Toolchain::compile_exe`] compiles into every executable. .NET
/// Framework takes the defaults it runs a process with from the entry
/// assembly's attribute, and from 4.0's when there is none.
pub const FRAMEWORK_TARGET: &str =
    "[assembly: global::System.Runtime.Versioning.TargetFramework(\".NETFramework,Version=v4.7.2\", FrameworkDisplayName = \".NET Framework 4.7.2\")]\n";

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
    /// The .NET Framework builds of the [`ACCESS_CONTROL_PACKAGES`], for
    /// every module to ship beside its Windows PowerShell shell. The
    /// netstandard2.0 compile references the packages' netstandard2.0
    /// builds instead, which define the types these forward.
    pub access_control_forwarders: Vec<PathBuf>,
    scripts: PathBuf,
    /// The fetch script, for the packages fetched only on demand.
    fetch: PathBuf,
    /// The `toolchain.lock` of the cargo-pwrs versions before this
    /// layout, beside `root`, read for their records.
    earlier_lock: PathBuf,
}

/// The folder under a toolset's tree that this layout fetches into.
const STORE: &str = "v2";

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
    /// facades, PowerShell Standard, the .NET Framework dependencies and
    /// the access-control forwarders, in place of netstandard 2.0's set.
    /// Empty when none is given.
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
/// assemblies (`framework`), without regard to case. A name in either
/// set goes to the compile whose set it is; a name in neither is
/// refused.
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
                "[package.metadata.pwrs] references names {name}, which is in neither Microsoft.WindowsDesktop.App.Ref {DESKTOP_REF_VERSION} \
                 nor the .NET Framework 4.7.2 reference assemblies"
            )));
        }
        out.taken.push((name.clone(), in_core.is_some(), in_framework.is_some()));
        out.core.extend(in_core);
        out.framework.extend(in_framework);
    }
    Ok(out)
}

/// Fetches one package from nuget.org into its folder under `root`, as
/// [`fetch_package_from`] does.
fn fetch_package(root: &Path, earlier: &Path, fetch: &Path, id: &str, version: &str, dir: &str) -> Result<PathBuf, Error> {
    fetch_package_from(root, earlier, fetch, "nuget", id, version, dir)
}

/// Fetches one package from `source`, `nuget` or `gallery`, into its
/// folder under `root` unless it is there already, checking its hash
/// against `root`'s `toolchain.lock` and the `earlier` one: into a folder
/// of this fetch's own, marked complete and renamed into place.
fn fetch_package_from(root: &Path, earlier: &Path, fetch: &Path, source: &str, id: &str, version: &str, dir: &str) -> Result<PathBuf, Error> {
    let dest = package_dir(root, dir, version);
    if dest.join(".complete").is_file() {
        return Ok(dest);
    }
    eprintln!("pwrs: fetching {id} {version}");
    let staging = crate::temporary(&dest);
    let out = pwsh::run_pwsh_tool(fetch, &[id.to_string(), version.to_string(), staging.display().to_string(), "-Source".to_string(), source.to_string()])?;
    let hash = out.trim().to_string();
    if hash.len() != 128 {
        return Err(Error::msg(format!("fetch of {id} printed no SHA-512 hash: {out}")));
    }
    record_hash(&root.join("toolchain.lock"), earlier, id, version, &hash)?;
    std::fs::write(staging.join(".complete"), &hash).map_err(|e| Error::msg(format!("cannot mark {id} complete: {e}")))?;
    publish_package(&staging, &dest)?;
    Ok(dest)
}

/// Renames the complete package folder `staging` to `dest`. A `dest`
/// with its marker, renamed there by another fetch meanwhile, is left in
/// place and `staging` goes; a `dest` without one, which no fetch in this
/// layout leaves, goes first.
fn publish_package(staging: &Path, dest: &Path) -> Result<(), Error> {
    if dest.exists()
        && !dest.join(".complete").is_file()
        && let Err(e) = std::fs::remove_dir_all(dest)
    {
        remove_fetch(staging);
        return Err(Error::msg(format!("{} is there without its .complete marker and cannot be removed: {e}", dest.display())));
    }
    match std::fs::rename(staging, dest) {
        Ok(()) => Ok(()),
        Err(_another_fetch_renamed_first) if dest.join(".complete").is_file() => {
            remove_fetch(staging);
            Ok(())
        }
        Err(e) => {
            remove_fetch(staging);
            Err(Error::msg(format!("cannot move {} to {}: {e}", staging.display(), dest.display())))
        }
    }
}

/// Removes a fetch's own folder that was not renamed into place,
/// reporting on standard error when it cannot.
fn remove_fetch(staging: &Path) {
    if let Err(e) = std::fs::remove_dir_all(staging) {
        eprintln!("pwrs: cannot remove the fetch folder {}: {e}", staging.display());
    }
}

/// Checks `hash` against the one `lock` records for `id` `version`, or
/// records it there, after checking it against the one the `earlier`
/// file records, which is only read. A record is one line appended to
/// the file in one write, which appends whole whatever another tool
/// appends at the same moment, so no record replaces another; the first
/// line for a package is its record, and a tool whose hash is not that
/// line's is refused.
fn record_hash(lock: &Path, earlier: &Path, id: &str, version: &str, hash: &str) -> Result<(), Error> {
    Lock::load(earlier)?.verify(id, version, hash)?;
    if Lock::load(lock)?.verify(id, version, hash)? {
        return Ok(());
    }
    let line = format!("{id} {version} {hash}\n");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(lock)
        .map_err(|e| Error::msg(format!("cannot open {}: {e}", lock.display())))?;
    let written = std::io::Write::write(&mut file, line.as_bytes()).map_err(|e| Error::msg(format!("cannot append to {}: {e}", lock.display())))?;
    if written != line.len() {
        return Err(Error::msg(format!("appended {written} of the {} bytes of a record to {}", line.len(), lock.display())));
    }
    drop(file);
    if Lock::load(lock)?.verify(id, version, hash)? {
        Ok(())
    } else {
        Err(Error::msg(format!("{} has no record of {id} {version} after one was appended", lock.display())))
    }
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
/// `RedistList/FrameworkList.xml` lists. The folder also has native
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
        // neither refetches nor mixes two compilers under one lock file.
        let tree = home_dir()?.join("toolchain").join(&toolset);
        let root = tree.join(STORE);
        let earlier_lock = tree.join("toolchain.lock");
        std::fs::create_dir_all(&root).map_err(|e| Error::msg(format!("cannot create {}: {e}", root.display())))?;
        // Every run writes the scripts, so their folder is named for them
        // and a tool with other scripts writes a folder of its own.
        let scripts = root.join(format!("scripts-{:016x}", content_stamp(&[FETCH_PS1, CSC_PS1])));
        let fetch = pwsh::materialize_script(&scripts, "fetch.ps1", FETCH_PS1)?;
        pwsh::materialize_script(&scripts, "csc.ps1", CSC_PS1)?;

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
            fetch_package(&root, &earlier_lock, &fetch, id, version, dir)?;
        }
        for (id, dir) in ACCESS_CONTROL_PACKAGES {
            fetch_package(&root, &earlier_lock, &fetch, id, ACCESS_CONTROL_VERSION, dir)?;
        }

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
        let mut access_control_forwarders = Vec::new();
        for (id, dir) in ACCESS_CONTROL_PACKAGES {
            let package = pkg(dir, ACCESS_CONTROL_VERSION);
            let assembly = format!("{id}.dll");
            netstandard_refs.push(file(package.join("ref").join("netstandard2.0").join(&assembly))?);
            access_control_forwarders.push(file(package.join("lib").join("net461").join(&assembly))?);
        }

        let csc_dir = find_csc(&pkg("csc", toolset.as_str()))?;
        Ok(Toolchain { root, csc_dir, pshome, net10_refs, netstandard_refs, psstandard, desktop_dependencies, access_control_forwarders, scripts, fetch, earlier_lock })
    }

    /// The assemblies `names` add to a module's shell compiles: from the
    /// desktop reference pack for the Core compile, and for the
    /// netstandard2.0 folder's shell the whole .NET Framework 4.7.2 set,
    /// which includes every name the .NET Framework resolves; see
    /// [`resolve_references`] and [`DesktopReferences::framework_set`].
    /// Both packages are fetched the first time a module names any, and
    /// checked against the lock like the rest.
    pub fn desktop_references(&self, names: &[String]) -> Result<DesktopReferences, Error> {
        if names.is_empty() {
            return Ok(DesktopReferences::default());
        }
        let desktop = fetch_package(&self.root, &self.earlier_lock, &self.fetch, "Microsoft.WindowsDesktop.App.Ref", DESKTOP_REF_VERSION, "desktopref")?;
        let netfx = fetch_package(&self.root, &self.earlier_lock, &self.fetch, "Microsoft.NETFramework.ReferenceAssemblies.net472", NETFX_REF_VERSION, "netfxref472")?;
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
        // The forwarders rather than the netstandard2.0 builds: this set's
        // mscorlib defines the access-control types itself.
        out.framework_set.extend(self.access_control_forwarders.iter().cloned());
        Ok(out)
    }

    /// What an executable for Windows PowerShell 5.1 compiles against:
    /// every reference assembly .NET Framework 4.7.2 lists, its facades,
    /// and Windows PowerShell 5.1's full System.Management.Automation and
    /// Microsoft.PowerShell.ConsoleHost, both 3.0.0.0, so the executable
    /// binds the engine in the GAC and can start its console host. Both
    /// packages are fetched the first time, and checked against the lock
    /// like the rest. Compile with [`compile_exe`](Self::compile_exe) and
    /// [`FRAMEWORK_DEFINES`].
    pub fn framework_exe_references(&self) -> Result<Vec<PathBuf>, Error> {
        let netfx = fetch_package(&self.root, &self.earlier_lock, &self.fetch, "Microsoft.NETFramework.ReferenceAssemblies.net472", NETFX_REF_VERSION, "netfxref472")?;
        let ps51 = fetch_package(&self.root, &self.earlier_lock, &self.fetch, "Microsoft.PowerShell.5.1.ReferenceAssemblies", PS51_REF_VERSION, "ps51ref")?;
        let framework_dir = netfx.join("build").join(".NETFramework").join("v4.7.2");
        let mut refs = framework_assemblies(&framework_dir)?;
        refs.extend(dlls_in(&framework_dir.join("Facades"))?);
        let engine = ps51.join("lib").join("net461");
        for name in ["System.Management.Automation.dll", "Microsoft.PowerShell.ConsoleHost.dll"] {
            let path = engine.join(name);
            if !path.is_file() {
                return Err(Error::msg(format!("{} not found", path.display())));
            }
            refs.push(path);
        }
        Ok(refs)
    }

    /// The manifest of the Pester [`PESTER_VERSION`], which both hosts
    /// import to run a module's suites: fetched the first time, and
    /// checked against the lock like the rest.
    pub fn pester(&self) -> Result<PathBuf, Error> {
        let pester = fetch_package(&self.root, &self.earlier_lock, &self.fetch, "Pester", PESTER_VERSION, "pester")?;
        let manifest = pester.join("tools").join("Pester.psd1");
        if manifest.is_file() {
            Ok(manifest)
        } else {
            Err(Error::msg(format!("{} not found", manifest.display())))
        }
    }

    /// System.Security.Cryptography.Pkcs [`PKCS_VERSION`]'s .NET 8 build,
    /// which a module's signer compiles against beside the Core reference
    /// pack: fetched the first time, and checked against the lock like
    /// the rest.
    pub fn pkcs(&self) -> Result<PathBuf, Error> {
        let pkcs = fetch_package(&self.root, &self.earlier_lock, &self.fetch, "System.Security.Cryptography.Pkcs", PKCS_VERSION, "pkcs")?;
        let assembly = pkcs.join("lib").join("net8.0").join("System.Security.Cryptography.Pkcs.dll");
        if assembly.is_file() {
            Ok(assembly)
        } else {
            Err(Error::msg(format!("{} not found", assembly.display())))
        }
    }

    /// The manifest of the OpenAuthenticode [`OPENAUTHENTICODE_VERSION`]:
    /// fetched from the PowerShell Gallery the first time, and checked
    /// against the lock like the rest.
    pub fn openauthenticode(&self) -> Result<PathBuf, Error> {
        let module = fetch_package_from(&self.root, &self.earlier_lock, &self.fetch, "gallery", "OpenAuthenticode", OPENAUTHENTICODE_VERSION, "openauthenticode")?;
        let manifest = module.join("OpenAuthenticode.psd1");
        if manifest.is_file() {
            Ok(manifest)
        } else {
            Err(Error::msg(format!("{} not found", manifest.display())))
        }
    }

    /// Compiles C# sources into a library. `extra_refs` are
    /// implementation assemblies this assembly references on top of
    /// the environment's reference set.
    pub fn compile(&self, env: &CompileEnv<'_>, sources: &[PathBuf], extra_refs: &[PathBuf], out: &Path) -> Result<(), Error> {
        self.compile_as("library", env, sources, extra_refs, out)
    }

    /// Compiles C# sources into an executable for .NET Framework 4.7.2, as
    /// [`compile`](Self::compile) compiles a library, for any CPU; its
    /// entry point is the one `Main` the sources declare.
    /// [`FRAMEWORK_TARGET`] is written to `<work>/<name>.TargetFramework.cs`
    /// and compiled in with the sources, so .NET Framework runs it with 4.7.2's
    /// defaults, as it runs a .NET SDK build: under Windows PowerShell's
    /// console host, TLS versions are the system's, an escaped `/` in a
    /// URL's path stays escaped and long paths open, as in powershell.exe.
    /// Sources that declare a `TargetFramework` of their own fail to
    /// compile with CS0579.
    pub fn compile_exe(&self, env: &CompileEnv<'_>, sources: &[PathBuf], extra_refs: &[PathBuf], out: &Path) -> Result<(), Error> {
        let name = match out.file_stem() {
            Some(stem) => stem.to_string_lossy().into_owned(),
            None => return Err(Error::msg(format!("{} has no file name", out.display()))),
        };
        std::fs::create_dir_all(env.work).map_err(|e| Error::msg(format!("cannot create {}: {e}", env.work.display())))?;
        let target = env.work.join(format!("{name}.TargetFramework.cs"));
        std::fs::write(&target, FRAMEWORK_TARGET).map_err(|e| Error::msg(format!("cannot write {}: {e}", target.display())))?;
        let mut all = sources.to_vec();
        all.push(target);
        self.compile_as("exe", env, &all, extra_refs, out)
    }

    /// Compiles C# sources into an assembly of csc's `/target:<target>`.
    fn compile_as(&self, target: &str, env: &CompileEnv<'_>, sources: &[PathBuf], extra_refs: &[PathBuf], out: &Path) -> Result<(), Error> {
        let work = env.work;
        let debug = env.debug;
        let refs = env.refs;
        let defines = env.defines;
        std::fs::create_dir_all(work).map_err(|e| Error::msg(format!("cannot create {}: {e}", work.display())))?;
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::msg(format!("cannot create {}: {e}", parent.display())))?;
        }
        let mut rsp = String::new();
        rsp.push_str(&format!("/nologo\n/nostdlib+\n/target:{target}\n/unsafe+\n/nullable:enable\n/langversion:latest\n"));
        // `/warnaserror-` stops a csc warning from failing a module
        // author's build over generated code they did not write. The
        // warning itself is printed below.
        rsp.push_str("/deterministic+\n/optimize+\n/warnaserror-\n");
        rsp.push_str(match debug {
            DebugSymbols::Emit => "/debug:portable\n",
            DebugSymbols::EmitWindows => "/debug:full\n",
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
        let stdout = if debug == DebugSymbols::EmitWindows {
            self.framework_csc(&rsp_path)
        } else {
            let csc = self.scripts.join("csc.ps1");
            pwsh::run_pwsh_tool(&csc, &[self.csc_dir.display().to_string(), format!("/noconfig @{}", rsp_path.display())])
        }
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

    /// Runs the toolset's .NET Framework compiler, `csc.exe` in the
    /// `net472` folder beside the .NET one, over the response file `rsp`,
    /// and returns what it printed. It writes a Windows `.pdb`, which the
    /// .NET one cannot: the library that writes the format ships only
    /// beside this compiler. Only Windows runs it.
    fn framework_csc(&self, rsp: &Path) -> Result<String, Error> {
        if !cfg!(windows) {
            return Err(Error::msg("a Windows .pdb is written only on Windows, where the toolset's .NET Framework compiler runs"));
        }
        let exe = match self.csc_dir.parent().and_then(Path::parent) {
            Some(tasks) => tasks.join("net472").join("csc.exe"),
            None => return Err(Error::msg(format!("{} has no toolset folder above it", self.csc_dir.display()))),
        };
        if !exe.is_file() {
            return Err(Error::msg(format!("the toolset has no .NET Framework compiler at {}", exe.display())));
        }
        let mut cmd = std::process::Command::new(&exe);
        cmd.args(["/noconfig", "/utf8output"]).arg(format!("@{}", rsp.display()));
        let out = crate::trace::output(&mut cmd).map_err(|e| Error::msg(format!("cannot run {}: {e}", exe.display())))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            return Err(Error::msg(format!(
                "{} exited with {}\n--- stdout ---\n{stdout}\n--- stderr ---\n{}",
                exe.display(),
                out.status,
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        Ok(stdout)
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
    /// The records in the file, in the order appended. A last line with
    /// no line break is a record still being appended, and is not one yet.
    fn load(path: &Path) -> Result<Lock, Error> {
        let mut entries = Vec::new();
        if path.is_file() {
            let text = std::fs::read_to_string(path).map_err(|e| Error::msg(format!("cannot read {}: {e}", path.display())))?;
            let ended = match text.rfind('\n') {
                Some(last_break) => &text[..=last_break],
                None => "",
            };
            for line in ended.lines() {
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

    /// Whether the file's first record for `id` `version` is `hash`: true
    /// when it is, false when there is no record for them, and an error
    /// when the first record is another hash.
    fn verify(&self, id: &str, version: &str, hash: &str) -> Result<bool, Error> {
        for (i, v, h) in &self.entries {
            if i == id && v == version {
                if h == hash {
                    return Ok(true);
                }
                return Err(Error::msg(format!(
                    "{id} {version} downloaded with sha512 {hash} but {} records {h}; refusing to use it",
                    self.path.display()
                )));
            }
        }
        Ok(false)
    }

    /// The file's text: one `id version sha512` line per entry.
    #[cfg(test)]
    fn text(&self) -> String {
        let mut s = String::new();
        for (i, v, h) in &self.entries {
            s.push_str(&format!("{i} {v} {h}\n"));
        }
        s
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

    /// An empty folder of the test's own under the system's temporary
    /// folder.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pwrs-build-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("clear the scratch folder");
        }
        std::fs::create_dir_all(&dir).expect("make the scratch folder");
        dir
    }

    /// Eight threads recording eight packages at once each find their
    /// entry in the file afterwards, and another hash for a recorded
    /// package is refused.
    #[test]
    fn hashes_recorded_at_once_all_reach_the_lock_and_a_changed_one_is_refused() {
        let dir = scratch("record");
        let lock = dir.join("toolchain.lock");
        let earlier = dir.join("earlier.lock");
        let threads: Vec<_> = (0..8)
            .map(|n| {
                let (lock, earlier) = (lock.clone(), earlier.clone());
                std::thread::spawn(move || record_hash(&lock, &earlier, &format!("Package{n}"), "1.0.0", &format!("{n:0128}")))
            })
            .collect();
        for t in threads {
            t.join().expect("a recording thread panicked").expect("record a hash");
        }
        let held = Lock::load(&lock).expect("read the lock");
        assert_eq!(held.entries.len(), 8, "{}", held.text());
        for n in 0..8 {
            assert!(held.verify(&format!("Package{n}"), "1.0.0", &format!("{n:0128}")).expect("verify"), "Package{n} in {}", held.text());
        }
        let refused = record_hash(&lock, &earlier, "Package3", "1.0.0", &"f".repeat(128)).expect_err("another hash for a recorded package");
        assert!(refused.to_string().contains("refusing to use it"), "{refused}");
        std::fs::remove_dir_all(&dir).expect("remove the scratch folder");
    }

    /// A record only in the earlier versions' file is checked, and the
    /// file is only read: a matching hash is recorded in this layout's own
    /// file, and another hash is refused.
    #[test]
    fn a_record_of_the_earlier_layout_is_checked_and_never_written() {
        let dir = scratch("earlier");
        let lock = dir.join("toolchain.lock");
        let earlier = dir.join("earlier.lock");
        let (a, b) = ("a".repeat(128), "b".repeat(128));
        std::fs::write(&earlier, format!("Old 1.0.0 {a}\n")).expect("write the earlier lock");
        let refused = record_hash(&lock, &earlier, "Old", "1.0.0", &b).expect_err("another hash than the earlier record");
        assert!(refused.to_string().contains("refusing to use it"), "{refused}");
        record_hash(&lock, &earlier, "Old", "1.0.0", &a).expect("the earlier record's hash");
        assert!(Lock::load(&lock).expect("read the lock").verify("Old", "1.0.0", &a).expect("verify"));
        assert_eq!(std::fs::read_to_string(&earlier).expect("read the earlier lock"), format!("Old 1.0.0 {a}\n"));
        std::fs::remove_dir_all(&dir).expect("remove the scratch folder");
    }

    /// A last line with no line break is a record still being appended,
    /// and the first record of a package is the one that counts.
    #[test]
    fn a_record_still_being_appended_is_not_read_and_the_first_record_counts() {
        let dir = scratch("partial");
        let lock = dir.join("toolchain.lock");
        let (a, b) = ("a".repeat(128), "b".repeat(128));
        std::fs::write(&lock, format!("One 1.0.0 {a}\nOne 1.0.0 {b}\nTwo 2.0.0 {b}")).expect("write the lock");
        let held = Lock::load(&lock).expect("read the lock");
        assert_eq!(held.entries.len(), 2, "{}", held.text());
        assert!(held.verify("One", "1.0.0", &a).expect("the first record"));
        assert!(!held.verify("Two", "2.0.0", &b).expect("a record not yet whole"));
        std::fs::remove_dir_all(&dir).expect("remove the scratch folder");
    }

    /// An incomplete folder in a package's place goes, the first complete
    /// fetch renamed there stays, and a second one's folder goes.
    #[test]
    fn a_fetch_renamed_second_leaves_the_first_and_an_incomplete_folder_goes() {
        let dir = scratch("publish");
        let dest = dir.join("simd-4.6.1");
        let fetched = |mark: &str| -> PathBuf {
            let staging = crate::temporary(&dest);
            std::fs::create_dir_all(&staging).expect("make a fetch folder");
            std::fs::write(staging.join("payload"), mark).expect("write the payload");
            std::fs::write(staging.join(".complete"), mark).expect("mark it complete");
            staging
        };
        std::fs::create_dir_all(&dest).expect("make the stopped fetch's folder");
        std::fs::write(dest.join("payload"), "stopped").expect("write its payload");
        let first = fetched("first");
        publish_package(&first, &dest).expect("rename the first fetch");
        let second = fetched("second");
        publish_package(&second, &dest).expect("leave the first fetch in place");
        assert_eq!(std::fs::read_to_string(dest.join("payload")).expect("read the payload"), "first");
        assert!(!first.exists() && !second.exists());
        std::fs::remove_dir_all(&dir).expect("remove the scratch folder");
    }

    fn files(dir: &str, names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| Path::new(dir).join(format!("{n}.dll"))).collect()
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// Each name goes to the compiles whose sets include it, matched without
    /// regard to case, in the order named.
    #[test]
    fn a_reference_goes_to_each_compile_whose_set_includes_it() {
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

    /// A name in neither set, one that is a path or a file name, and
    /// one named twice each stop the build, naming the key and the name.
    #[test]
    fn a_reference_that_resolves_nowhere_is_refused() {
        let core = files("desk", &["System.Drawing", "System.Windows.Forms"]);
        let framework = files("fx", &["System.Drawing", "System.Windows.Forms"]);
        let refused = |list: &[&str], says: &str| match resolve_references(&names(list), &core, &framework) {
            Ok(r) => panic!("{list:?} resolved: {r:?}"),
            Err(e) => assert!(e.to_string().contains(says), "{list:?}: {e}"),
        };
        refused(&["System.Text.Json"], "references names System.Text.Json, which is in neither Microsoft.WindowsDesktop.App.Ref");
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

    /// Removes a folder whose executable has just run, trying again every
    /// 10 milliseconds for up to five seconds. Windows can leave the
    /// executable's file in use for a moment after its process exits, and
    /// `remove_dir_all` does not wait for that.
    ///
    /// The numbers come from 110 runs of the test that calls this, 100
    /// alone and 10 beside the whole workspace's tests. Every removal
    /// worked on the first try. The median removal took a millisecond, so
    /// a try every 10 milliseconds costs little. The longest took 40
    /// milliseconds, and five seconds is 125 times that. A longer wait
    /// costs time only when the folder cannot be removed at all.
    #[cfg(windows)]
    fn remove_after_run(dir: &Path) -> std::io::Result<()> {
        let give_up = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let removed = std::fs::remove_dir_all(dir);
            if removed.is_ok() || std::time::Instant::now() >= give_up {
                return removed;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// An executable compiled against Windows PowerShell's set starts the
    /// console host with a cmdlet from a library it references, runs the
    /// command its arguments give, and exits with the code the command
    /// exits with; in it, TLS versions, an escaped `/` in a URL's path and
    /// the long path switches read as they read in powershell.exe.
    #[cfg(windows)]
    #[test]
    fn an_executable_for_windows_powershell_runs_its_command_in_the_console_host() {
        const ANSWER: &str = r#"using System.Management.Automation;

[Cmdlet(VerbsCommon.Get, "Answer")]
public sealed class GetAnswer : Cmdlet
{
    protected override void ProcessRecord() => WriteObject(42);
}
"#;
        const SHELL: &str = r#"using System.Management.Automation.Runspaces;
using Microsoft.PowerShell;

public static class Shell
{
    public static int Main(string[] args)
    {
        var config = RunspaceConfiguration.Create();
        config.Cmdlets.Append(new CmdletConfigurationEntry("Get-Answer", typeof(GetAnswer), null));
        return ConsoleShell.Start(config, "", "", args);
    }
}
"#;
        const PROBE: &str = "[Net.ServicePointManager]::SecurityProtocol; ([Uri] 'http://h/a%2Fb/./c').AbsolutePath; \
             foreach ($s in 'Switch.System.IO.UseLegacyPathHandling', 'Switch.System.IO.BlockLongPaths') { $on = $false; $null = [AppContext]::TryGetSwitch($s, [ref] $on); $s + ' ' + $on }";
        let tc = Toolchain::ensure().expect("the toolchain");
        let refs = tc.framework_exe_references().expect("the references");
        let base = std::env::temp_dir().join(format!("pwrs-framework-exe-{}", std::process::id()));
        let (work, bin) = (base.join("work"), base.join("bin"));
        let ran = (|| -> Result<std::process::Output, Error> {
            std::fs::create_dir_all(&work).map_err(|e| Error::msg(format!("cannot create {}: {e}", work.display())))?;
            let source = |name: &str, text: &str| -> Result<PathBuf, Error> {
                let path = work.join(name);
                std::fs::write(&path, text).map_err(|e| Error::msg(format!("cannot write {}: {e}", path.display())))?;
                Ok(path)
            };
            let env = CompileEnv { refs: &refs, defines: FRAMEWORK_DEFINES, work: &work, debug: DebugSymbols::Omit, path_map: &[] };
            let library = bin.join("Answer.dll");
            tc.compile(&env, &[source("Answer.cs", ANSWER)?], &[], &library)?;
            let exe = bin.join("Shell.exe");
            tc.compile_exe(&env, &[source("Shell.cs", SHELL)?], &[library], &exe)?;
            std::process::Command::new(&exe)
                .args(["-NoProfile", "-NoLogo", "-NonInteractive", "-Command", &format!("'answer {{0}} on {{1}}' -f (Get-Answer), $PSVersionTable.PSVersion.ToString(2); {PROBE}; exit 3")])
                .output()
                .map_err(|e| Error::msg(format!("cannot run {}: {e}", exe.display())))
        })();
        let removed = remove_after_run(&base);
        let out = ran.expect("the executable compiles and runs");
        let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert_eq!(out.status.code(), Some(3), "standard output:\n{stdout}\nstandard error:\n{stderr}");
        let lines: Vec<&str> = stdout.lines().map(str::trim_end).collect();
        assert_eq!(lines.first(), Some(&"answer 42 on 5.1"), "standard output:\n{stdout}\nstandard error:\n{stderr}");
        let reference = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", PROBE]).output().expect("run powershell.exe");
        let expected = String::from_utf8_lossy(&reference.stdout);
        assert!(reference.status.success(), "powershell.exe:\n{expected}\n{}", String::from_utf8_lossy(&reference.stderr));
        let expected: Vec<&str> = expected.lines().map(str::trim_end).collect();
        assert_eq!(expected.len(), 4, "powershell.exe printed:\n{expected:?}");
        assert_eq!(&lines[1..], &expected[..], "the executable against powershell.exe");
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

    /// On Windows, a compile asked for a Windows .pdb writes one, which
    /// starts with the MSF signature; a portable one starts with the
    /// metadata signature, `BSJB`.
    #[cfg(windows)]
    #[test]
    fn a_windows_pdb_is_written_in_the_windows_format() {
        let tc = Toolchain::ensure().expect("the toolchain");
        let base = std::env::temp_dir().join(format!("pwrs-pdb-format-{}", std::process::id()));
        let compile = |debug: DebugSymbols, name: &str| -> Vec<u8> {
            let work = base.join(name);
            std::fs::create_dir_all(&work).expect("the work folder");
            let src = work.join("Probe.cs");
            std::fs::write(&src, "public static class Probe { public static int Answer() => 42; }\n").expect("the source");
            let env = CompileEnv { refs: &tc.netstandard_refs, defines: NETSTANDARD_DEFINES, work: &work.join("rsp"), debug, path_map: &[] };
            let out = work.join(format!("{name}.dll"));
            tc.compile(&env, &[src], &[], &out).expect("the compile");
            std::fs::read(out.with_extension("pdb")).expect("the .pdb")
        };
        let windows = compile(DebugSymbols::EmitWindows, "windows");
        let portable = compile(DebugSymbols::Emit, "portable");
        let removed = std::fs::remove_dir_all(&base);
        assert!(windows.starts_with(b"Microsoft C/C++ MSF 7.00"), "the Windows .pdb starts {:?}", &windows[..windows.len().min(24)]);
        assert!(portable.starts_with(b"BSJB"), "the portable .pdb starts {:?}", &portable[..portable.len().min(4)]);
        removed.expect("the test's folder is removed");
    }

    /// The pinned Pester arrives with its manifest under `tools/`, at the
    /// version pinned.
    #[test]
    fn the_pinned_pester_is_fetched_with_its_manifest() {
        let tc = Toolchain::ensure().expect("the toolchain");
        let manifest = tc.pester().expect("Pester");
        let text = std::fs::read_to_string(&manifest).expect("the manifest");
        assert!(text.contains(&format!("ModuleVersion     = '{PESTER_VERSION}'")), "{} names another version", manifest.display());
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
