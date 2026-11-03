//! What a wrapper's shells declare of the command it wraps, from the
//! command's readings: each PowerShell 7 release's build for each
//! platform, or Windows PowerShell's one. A parameter every reading that
//! has the command declares alike, and that the shell's compile set can
//! name, is a property of the wrapper; any other is declared at bind time
//! from the reading of the running PowerShell. An attribute whose type
//! C# cannot name is left off with a warning, and one the compiler
//! writes for itself is left off without one.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::commands::{Attribute, Declared, Named, Parameter, TypeRef, Value};
use crate::descriptor::Module;
use crate::Error;
use pwrs_build::toolchain::Toolchain;

/// What a module's wrapping cmdlets need of the build: each one's plans,
/// by cmdlet id, and what each shell compile references beside its own
/// set to name the copied parameters' types.
#[derive(Default)]
pub struct Wrapped {
    pub shells: BTreeMap<u32, Shells>,
    /// PowerShell 7.4.0's cmdlet assemblies and the CIM client library.
    pub core_references: Vec<PathBuf>,
    /// Windows PowerShell's own cmdlet assemblies, its engine aside, which
    /// the shell's PowerShell Standard stands in for.
    pub desktop_references: Vec<PathBuf>,
}

/// Whether a module whose manifest lists `editions` targets Windows
/// PowerShell: it does when it lists none, as the manifest then says both.
fn targets_desktop(editions: &[String]) -> bool {
    editions.is_empty() || editions.iter().any(|e| e.eq_ignore_ascii_case("Desktop"))
}

/// Reads the command each of `module`'s wrapping cmdlets wraps, from
/// PowerShell 7's own packages at each release for each platform and, for
/// a module that targets Windows PowerShell, from Windows PowerShell's own
/// assemblies, and plans both shells' declarations. Only Windows has
/// Windows PowerShell's, so a build elsewhere warns, and its Windows
/// PowerShell shell's wrappers refuse to run. A module that wraps nothing
/// fetches and reads nothing.
pub fn read(module: &Module, editions: &[String], tc: &Toolchain, work: &Path, manifest_dir: &Path) -> Result<Wrapped, Error> {
    let wrapping: Vec<&crate::descriptor::Cmdlet> = module.cmdlets.iter().filter(|c| c.wraps.is_some()).collect();
    if wrapping.is_empty() {
        return Ok(Wrapped::default());
    }
    let targets_desktop = targets_desktop(editions);
    let windir = match std::env::var_os("windir").or_else(|| std::env::var_os("SystemRoot")) {
        Some(dir) if cfg!(windows) => Some(PathBuf::from(dir)),
        _not_windows => None,
    };
    // Only Windows has Windows PowerShell's own declarations. A build
    // elsewhere still makes the module, for its own platform and for a
    // merge into the Windows build's folder, whose shells the merge leaves as
    // they are.
    let desktop = targets_desktop && windir.is_some();

    let releases = tc.pwsh_releases()?;
    let core_references = tc.pwsh_compile_references()?;
    let mut core_compile = tc.net10_refs.clone();
    core_compile.extend(core_references.iter().cloned());
    // A reading's name, the release as major and minor, its version text,
    // its platform and the assemblies read there.
    type ReleaseSource<'a> = (String, (u32, u32), &'static str, Platform, &'a [PathBuf]);
    let mut sources: Vec<ReleaseSource<'_>> = Vec::new();
    for r in &releases {
        let release = release_of(r.version)?;
        sources.push((format!("{}/unix", r.version), release, r.version, Platform::Unix, &r.unix));
        sources.push((format!("{}/win", r.version), release, r.version, Platform::Windows, &r.windows));
    }
    let mut specs: Vec<crate::commands::Source<'_>> = sources
        .iter()
        .map(|(name, _, _, _, assemblies)| crate::commands::Source { name: name.clone(), compile: "net10.0", assemblies, context: &tc.net10_refs })
        .collect();

    let (gac, framework, desktop_compile, desktop_references) = match (&windir, desktop) {
        (Some(dir), true) => {
            let gac = pwrs_build::toolchain::windows_powershell_assemblies(dir)?;
            let references: Vec<PathBuf> =
                gac.iter().filter(|p| p.file_stem().is_none_or(|s| !s.eq_ignore_ascii_case("System.Management.Automation"))).cloned().collect();
            let mut compile = tc.netstandard_refs.clone();
            compile.extend(references.iter().cloned());
            (gac, tc.framework_exe_references()?, compile, references)
        }
        _core_only => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
    };
    if desktop {
        specs.push(crate::commands::Source { name: "5.1".to_string(), compile: "netstandard2.0", assemblies: &gac, context: &framework });
    }
    let mut commands: Vec<String> = Vec::new();
    for c in &wrapping {
        if let Some(wraps) = &c.wraps
            && !commands.iter().any(|n| n.eq_ignore_ascii_case(bare_name(wraps)))
        {
            commands.push(bare_name(wraps).to_string());
        }
    }
    let compile = [("net10.0", core_compile.as_slice()), ("netstandard2.0", desktop_compile.as_slice())];
    let read = crate::commands::read(work, &compile, &specs, &[], &commands)?;

    let lock_path = manifest_dir.join(LOCK_FILE);
    let mut lock = Lock::load(&lock_path)?;
    // Each wrapping cmdlet with its readings among PowerShell's own, and the
    // commands of other modules, those the lock records and those it does not.
    let mut routes = Vec::new();
    let mut to_check: Vec<String> = Vec::new();
    let mut to_read: Vec<(String, String)> = Vec::new();
    for c in wrapping {
        let Some(command) = &c.wraps else { continue };
        let bare = bare_name(command);
        let mut readings = Vec::new();
        for (name, release, version, platform, _) in &sources {
            if let Some(Some(declared)) = read.sources.get(name).and_then(|s| s.get(bare)) {
                readings.push(Reading { release: *release, version, platform: *platform, declared });
            }
        }
        let powershells_own = !readings.is_empty() && qualifier(command).is_none_or(|m| POWERSHELL_MODULES.iter().any(|own| own.eq_ignore_ascii_case(m)));
        if !powershells_own {
            match lock.key_of(command) {
                Some(key) if !to_check.iter().any(|k| k == key) => to_check.push(key.clone()),
                Some(_checked) => {}
                None if !to_read.iter().any(|(_, r)| r.eq_ignore_ascii_case(command)) => to_read.push((c.name.clone(), command.clone())),
                None => {}
            }
        }
        routes.push((c, command, readings, powershells_own));
    }
    let mut warnings = BTreeSet::new();
    if !to_check.is_empty() {
        warn_on_drift(&to_check, &lock, work, &mut warnings)?;
    }
    if !to_read.is_empty() {
        let hosts = Hosts { work, core_compile: &core_compile, desktop_compile: &desktop_compile, framework: &framework, gac: &gac, desktop };
        for (command, kept) in read_installed(&to_read, &hosts)? {
            eprintln!(
                "pwrs: read {command} from {} {} where it is installed; commit {LOCK_FILE}, which every build of this module compiles it from",
                kept.module, kept.module_version
            );
            lock.commands.insert(command, kept);
        }
        lock.save(&lock_path)?;
    }

    let mut refused_here: Vec<String> = Vec::new();
    let mut shells = BTreeMap::new();
    for (c, command, readings, powershells_own) in routes {
        let bare = bare_name(command);
        let (core, desktop_plan) = if powershells_own {
            let core = plan(bare, &readings, &mut warnings)?;
            let desktop_plan = if !targets_desktop {
                Desktop::Refused(format!("{} wraps {command} in PowerShell 7 only: its module declares compatible-ps-editions = [\"Core\"]", c.name))
            } else if !desktop {
                refused_here.push(c.name.clone());
                Desktop::Refused(format!(
                    "{} wraps {command} in Windows PowerShell only when its module is built on Windows, and this one was built elsewhere",
                    c.name
                ))
            } else {
                match read.sources.get("5.1").and_then(|s| s.get(bare)) {
                    Some(Some(declared)) => Desktop::Plan(plan_single(bare, declared, &mut warnings)?),
                    _absent => Desktop::Refused(format!("{} wraps {command}, which Windows PowerShell 5.1 does not have", c.name)),
                }
            };
            (core, desktop_plan)
        } else {
            let Some(kept) = lock.key_of(command).and_then(|key| lock.commands.get(key)) else {
                return Err(Error::msg(format!("{} wraps {command}, which {LOCK_FILE} does not have even after reading it where it is installed", c.name)));
            };
            let core = plan_single(bare, &kept.core, &mut warnings)?;
            let desktop_plan = match (&kept.desktop, targets_desktop) {
                (_, false) => Desktop::Refused(format!("{} wraps {command} in PowerShell 7 only: its module declares compatible-ps-editions = [\"Core\"]", c.name)),
                (Some(declared), true) => Desktop::Plan(plan_single(bare, declared, &mut warnings)?),
                (None, true) => Desktop::Refused(format!("{} wraps {command}, of which {LOCK_FILE} has no Windows PowerShell copy", c.name)),
            };
            (core, desktop_plan)
        };
        let shells_of = Shells { command: command.clone(), core, desktop: desktop_plan };
        refuse_clashes(c, &shells_of)?;
        shells.insert(c.id, shells_of);
    }
    if !refused_here.is_empty() {
        eprintln!(
            "pwrs: {} wrap{} a command whose Windows PowerShell 5.1 parameters only Windows has, so in Windows PowerShell this build's {} refuse{} to run; build on Windows for a module whose wrappers run there, or set compatible-ps-editions = [\"Core\"] under [package.metadata.pwrs] for one that targets PowerShell 7 only",
            refused_here.join(", "),
            if refused_here.len() == 1 { "s" } else { "" },
            if refused_here.len() == 1 { "wrapper" } else { "wrappers" },
            if refused_here.len() == 1 { "s" } else { "" }
        );
    }
    for w in &warnings {
        eprintln!("{w}");
    }
    Ok(Wrapped { shells, core_references, desktop_references })
}

/// The file beside a module's Cargo.toml that records each command it wraps
/// from outside PowerShell's own, as read where its module is installed.
pub const LOCK_FILE: &str = "wrapped.lock.json";

/// The modules of PowerShell's own cmdlets, which a qualified `wraps` may
/// name and still be read from PowerShell's own packages.
const POWERSHELL_MODULES: &[&str] = &[
    "Microsoft.PowerShell.Core",
    "Microsoft.PowerShell.Utility",
    "Microsoft.PowerShell.Management",
    "Microsoft.PowerShell.Security",
    "Microsoft.PowerShell.Diagnostics",
    "Microsoft.PowerShell.Host",
    "CimCmdlets",
    "Microsoft.WSMan.Management",
];

/// The module `wraps` names before a backslash, if it names one.
fn qualifier(wraps: &str) -> Option<&str> {
    wraps.split_once('\\').map(|(module, _)| module)
}

/// The command name `wraps` gives, without its module.
fn bare_name(wraps: &str) -> &str {
    match wraps.split_once('\\') {
        Some((_, command)) => command,
        None => wraps,
    }
}

/// `wrapped.lock.json`: each command read where its module is installed,
/// by the name `wraps` gives it.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Lock {
    commands: BTreeMap<String, Kept>,
}

/// One recorded command: the module and version it was read from, and each
/// edition's declarations; no Windows PowerShell copy where it was read
/// off Windows, or where Windows PowerShell has no such command.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Kept {
    module: String,
    module_version: String,
    core: Declared,
    desktop: Option<Declared>,
}

impl Lock {
    fn load(path: &Path) -> Result<Lock, Error> {
        if !path.is_file() {
            return Ok(Lock::default());
        }
        let text = std::fs::read_to_string(path).map_err(|e| Error::msg(format!("cannot read {}: {e}", path.display())))?;
        serde_json::from_str(&text).map_err(|e| Error::msg(format!("{} is not a {LOCK_FILE} cargo pwrs reads: {e}", path.display())))
    }

    fn save(&self, path: &Path) -> Result<(), Error> {
        let mut text = serde_json::to_string_pretty(self).map_err(|e| Error::msg(format!("cannot write {LOCK_FILE}: {e}")))?;
        text.push('\n');
        std::fs::write(path, text).map_err(|e| Error::msg(format!("cannot write {}: {e}", path.display())))
    }

    /// The name the lock records `command` under, matched as PowerShell
    /// matches command names, without regard to case.
    fn key_of(&self, command: &str) -> Option<&String> {
        self.commands.keys().find(|k| k.eq_ignore_ascii_case(command))
    }
}

/// What reading a command where it is installed needs: the shells'
/// compile sets, what resolves Windows PowerShell's types, and whether
/// this build reads Windows PowerShell's copy.
struct Hosts<'a> {
    work: &'a Path,
    core_compile: &'a [PathBuf],
    desktop_compile: &'a [PathBuf],
    framework: &'a [PathBuf],
    gac: &'a [PathBuf],
    desktop: bool,
}

/// What `installed.ps1` writes.
#[derive(serde::Deserialize)]
struct Installed {
    found: bool,
    #[serde(default)]
    module: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    assembly: Option<String>,
}

const INSTALLED_PS1: &str = include_str!("../scripts/installed.ps1");

/// What `installed.ps1` wrote for each command, by name: as parsed and as
/// raw JSON.
type InstalledCommands = BTreeMap<String, (Installed, serde_json::Value)>;

/// Runs `installed.ps1` once for every one of `commands`, in pwsh, or in
/// Windows PowerShell when `desktop`, and returns what it wrote for each,
/// as parsed and as raw JSON.
fn installed(commands: &[String], work: &Path, desktop: bool, version_only: bool) -> Result<InstalledCommands, Error> {
    let dir = work.join("installed");
    let script = crate::pwsh::materialize_script(&dir, "installed.ps1", INSTALLED_PS1)?;
    let stem = format!("{}.{}", if desktop { "desktop" } else { "core" }, std::process::id());
    let request = dir.join(format!("{stem}.request.json"));
    let output = dir.join(format!("{stem}.json"));
    let asked = serde_json::json!({ "names": commands, "version_only": version_only }).to_string();
    std::fs::write(&request, asked).map_err(|e| Error::msg(format!("cannot write {}: {e}", request.display())))?;
    let args = ["-Request".to_string(), request.display().to_string(), "-Output".to_string(), output.display().to_string()];
    let ran = if desktop { crate::pwsh::run_winps_script(&script, &args) } else { crate::pwsh::run_pwsh_tool(&script, &args) };
    let request_removed = std::fs::remove_file(&request);
    ran.map_err(|e| Error::msg(format!("reading {} where installed:\n{e}", commands.join(", "))))?;
    request_removed.map_err(|e| Error::msg(format!("cannot remove {}: {e}", request.display())))?;
    let text = std::fs::read_to_string(&output).map_err(|e| Error::msg(format!("cannot read {}: {e}", output.display())))?;
    let removed = std::fs::remove_file(&output);
    let mut records: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| Error::msg(format!("installed.ps1 wrote no JSON object for {}: {e}", commands.join(", "))))?;
    removed.map_err(|e| Error::msg(format!("cannot remove {}: {e}", output.display())))?;
    let mut read = BTreeMap::new();
    for command in commands {
        let Some(raw) = records.remove(command) else {
            return Err(Error::msg(format!("installed.ps1 wrote no record for {command}")));
        };
        let parsed: Installed = serde_json::from_value(raw.clone()).map_err(|e| Error::msg(format!("installed.ps1 wrote no record cargo pwrs reads for {command}: {e}")))?;
        read.insert(command.clone(), (parsed, raw));
    }
    Ok(read)
}

/// Reads each of `commands`, a cmdlet's name with the command it wraps,
/// where the command's module is installed: in pwsh, and in Windows
/// PowerShell when this build reads its copy, one run in each for them
/// all. A cmdlet is searched for in its assembly's file alone, the files
/// beside it resolving the types it names; a function is read from its
/// param block. Returns each command's copy by the name `wraps` gives it.
fn read_installed(commands: &[(String, String)], hosts: &Hosts<'_>) -> Result<BTreeMap<String, Kept>, Error> {
    let names: Vec<String> = commands.iter().map(|(_, command)| command.clone()).collect();
    let core = installed(&names, hosts.work, false, false)?;
    let missing: Vec<String> = commands
        .iter()
        .filter(|(_, command)| !core.get(command).is_some_and(|(found, _)| found.found))
        .map(|(cmdlet, command)| format!(
            "{cmdlet} wraps {command}, which is none of PowerShell's own cmdlets, is not installed for pwsh here, and has no entry in {LOCK_FILE}; install its module where you build, so it can be read and recorded there"
        ))
        .collect();
    if !missing.is_empty() {
        return Err(Error::msg(missing.join("\n")));
    }
    let desktop = if hosts.desktop { Some(installed(&names, hosts.work, true, false)?) } else { None };
    let mut desktop_context: Vec<PathBuf> = hosts.framework.to_vec();
    desktop_context.extend(hosts.gac.iter().cloned());
    let mut each: Vec<(&str, &str, &[PathBuf], &InstalledCommands)> = vec![("core", "net10.0", hosts.core_compile, &core)];
    if let Some(found) = &desktop {
        each.push(("desktop", "netstandard2.0", &desktop_context, found));
    }

    // Per host, a source for each assembly a cmdlet comes from, and one of
    // every function, keyed by the name `wraps` gives it so two modules'
    // functions of one name stay apart. Each command's declaration is then
    // found under its source by its key.
    let mut assemblies: Vec<(String, &str, Vec<PathBuf>, Vec<PathBuf>)> = Vec::new();
    let mut functions: Vec<(String, &str, serde_json::Value, &[PathBuf])> = Vec::new();
    let mut found_at: BTreeMap<(&str, &str), (String, String)> = BTreeMap::new();
    let mut bare_names: Vec<String> = Vec::new();
    for (host, compile, context, records) in &each {
        let mut raw = serde_json::Map::new();
        for (command, (found, record)) in records.iter() {
            if !found.found {
                continue;
            }
            match (found.kind.as_str(), &found.assembly) {
                ("cmdlet", Some(assembly)) => {
                    let file = PathBuf::from(assembly);
                    let source = format!("kept/{host}/{}", file.display());
                    if !assemblies.iter().any(|(name, ..)| *name == source) {
                        let mut resolving = beside(&file)?;
                        resolving.extend(context.iter().cloned());
                        assemblies.push((source.clone(), compile, vec![file], resolving));
                    }
                    let bare = bare_name(command).to_string();
                    if !bare_names.iter().any(|b| b.eq_ignore_ascii_case(&bare)) {
                        bare_names.push(bare.clone());
                    }
                    found_at.insert((host, command.as_str()), (source, bare));
                }
                _function => {
                    raw.insert(command.clone(), record.clone());
                    found_at.insert((host, command.as_str()), (format!("kept/{host}"), command.clone()));
                }
            }
        }
        if !raw.is_empty() {
            functions.push((format!("kept/{host}"), compile, serde_json::Value::Object(raw), context));
        }
    }
    let specs: Vec<crate::commands::Source<'_>> =
        assemblies.iter().map(|(name, compile, files, context)| crate::commands::Source { name: name.clone(), compile, assemblies: files, context }).collect();
    let raws: Vec<crate::commands::Raw<'_>> =
        functions.iter().map(|(name, compile, commands, context)| crate::commands::Raw { name: name.clone(), compile, context, commands: commands.clone() }).collect();
    let compile = [("net10.0", hosts.core_compile), ("netstandard2.0", hosts.desktop_compile)];
    let read = crate::commands::read(hosts.work, &compile, &specs, &raws, &bare_names)?;
    let declared = |host: &str, command: &str| -> Option<Declared> {
        let (source, key) = found_at.get(&(host, command))?;
        read.sources.get(source)?.get(key)?.clone()
    };

    let mut kept = BTreeMap::new();
    for (cmdlet, command) in commands {
        let (Some((found, _)), Some(core_declared)) = (core.get(command), declared("core", command)) else {
            return Err(Error::msg(format!("{cmdlet} wraps {command}, and reading it where it is installed found no declaration")));
        };
        kept.insert(
            command.clone(),
            Kept { module: found.module.clone(), module_version: found.version.clone(), core: core_declared, desktop: declared("desktop", command) },
        );
    }
    Ok(kept)
}

/// The assemblies beside `file` in its folder, which resolve the types
/// the assembly names from them.
fn beside(file: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut files = Vec::new();
    let Some(dir) = file.parent() else { return Ok(files) };
    let entries = std::fs::read_dir(dir).map_err(|e| Error::msg(format!("cannot read {}: {e}", dir.display())))?;
    for entry in entries {
        let p = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", dir.display())))?.path();
        let same = p.file_name().zip(file.file_name()).is_some_and(|(a, b)| a.eq_ignore_ascii_case(b));
        if !same && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dll")) {
            files.push(p);
        }
    }
    Ok(files)
}

/// Warns for each of `commands`, recorded in `lock`, whose module is installed
/// here at another version than the one it was read from: the recorded copy
/// is still what the build compiles. One pwsh run reads them all.
fn warn_on_drift(commands: &[String], lock: &Lock, work: &Path, warnings: &mut BTreeSet<String>) -> Result<(), Error> {
    let here = installed(commands, work, false, true)?;
    for command in commands {
        let (Some(kept), Some((found, _))) = (lock.commands.get(command), here.get(command)) else {
            return Err(Error::msg(format!("{LOCK_FILE} or installed.ps1 has no record of {command}")));
        };
        if found.found && found.version != kept.module_version {
            warnings.insert(format!(
                "pwrs: {LOCK_FILE} has {command} as {} {} declared it, and {} is installed here; delete its entry to read it again",
                kept.module, kept.module_version, found.version
            ));
        }
    }
    Ok(())
}

/// A release's major and minor version, from its full version.
fn release_of(version: &str) -> Result<(u32, u32), Error> {
    let mut parts = version.split('.').map(|p| p.parse::<u32>());
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => Ok((major, minor)),
        _malformed => Err(Error::msg(format!("the PowerShell release {version} has no major and minor version"))),
    }
}

/// Refuses a parameter of the cmdlet's own whose name, or one of whose
/// aliases, a copied parameter has, since one name would bind both. A
/// shell whose cmdlet refuses to run copies no parameter to clash with.
fn refuse_clashes(c: &crate::descriptor::Cmdlet, shells: &Shells) -> Result<(), Error> {
    let mut copied = shells.core.copied_names();
    if let Desktop::Plan(plan) = &shells.desktop {
        copied.extend(plan.copied_names());
    }
    for p in &c.params {
        let mut names = vec![p.name.to_ascii_lowercase()];
        names.extend(p.aliases.iter().map(|a| a.to_ascii_lowercase()));
        if let Some(clash) = names.iter().find(|n| copied.contains(*n)) {
            return Err(Error::msg(format!(
                "{}'s parameter {} is named {clash}, which {} has too; a wrapping cmdlet's own parameters need names its command does not use",
                c.name, p.name, shells.command
            )));
        }
    }
    Ok(())
}

/// The platform a reading's build runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Platform {
    Unix,
    Windows,
}

/// One reading of a command: one release's build for one platform.
pub struct Reading<'a> {
    /// The release's major and minor version, such as `(7, 4)`.
    pub release: (u32, u32),
    /// Its full version, as the warnings name it.
    pub version: &'a str,
    pub platform: Platform,
    pub declared: &'a Declared,
}

/// What one shell declares of the wrapped command.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The named arguments of the wrapped command's `[Cmdlet]`, which the
    /// wrapper's own `[Cmdlet]` takes beside its verb and noun.
    pub binding: Vec<Named>,
    /// Its `[OutputType]` attributes.
    pub output: Vec<Attribute>,
    /// The wrapped command adds parameters of its own at bind time, which
    /// the wrapper takes from it and forwards.
    pub forwards_dynamic: bool,
    /// The parameters declared alike in every reading, as properties, each
    /// with the attributes the shell copies.
    pub properties: Vec<Parameter>,
    /// The parameters any reading declares otherwise, by reading.
    pub variants: Vec<Variant>,
}

/// The parameters one reading declares at bind time.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// The release, for a PowerShell 7 reading; `None` for Windows
    /// PowerShell's one.
    pub release: Option<(u32, u32)>,
    pub platform: Option<Platform>,
    pub parameters: Vec<Parameter>,
}

impl Plan {
    /// Every parameter name the wrapper forwards whatever the running
    /// PowerShell: the properties and every variant's.
    pub fn copied_names(&self) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = self.properties.iter().map(|p| p.name.to_ascii_lowercase()).collect();
        for v in &self.variants {
            names.extend(v.parameters.iter().map(|p| p.name.to_ascii_lowercase()));
        }
        names
    }
}

/// Whether `ty` is one the compiler writes for its own use, such as the
/// nullability attributes, which C# refuses in source and binding never
/// reads.
fn compiler_written(ty: &TypeRef) -> bool {
    ty.name.starts_with("System.Runtime.CompilerServices.") || ty.name.starts_with("System.Diagnostics.CodeAnalysis.")
}

/// Whether a value can be written in C# as an attribute argument: every
/// type it names is one the compile set declares public.
fn static_value(v: &Value) -> bool {
    match v {
        Value::Null { ty } | Value::Enum { ty, .. } | Value::Type { ty } => ty.compile,
        Value::Array { element, items } => element.compile && items.iter().all(static_value),
        Value::Bool { .. } | Value::String { .. } | Value::Char { .. } | Value::Number { .. } => true,
    }
}

/// Whether a value can be built at bind time: as in C#, except that a
/// public type the compile set lacks is looked up by name.
fn runtime_value(v: &Value) -> bool {
    match v {
        Value::Type { ty } => ty.compile || (ty.visible && ty.runtime.is_some() && !ty.unsupported),
        Value::Array { element, items } => element.compile && items.iter().all(runtime_value),
        other => static_value(other),
    }
}

/// Whether `a` can be written as a C# attribute.
pub fn static_attribute(a: &Attribute) -> bool {
    a.error.is_none() && a.ty.compile && a.args.iter().all(static_value) && a.named.iter().all(|n| static_value(&n.value))
}

/// Whether `a` can be built at bind time.
pub fn runtime_attribute(a: &Attribute) -> bool {
    a.error.is_none() && a.ty.compile && a.args.iter().all(runtime_value) && a.named.iter().all(|n| runtime_value(&n.value))
}

/// Whether a parameter's type can be named in C# for a property.
fn static_type(ty: &TypeRef) -> bool {
    ty.compile && !ty.unsupported
}

/// Whether a parameter's type can be given at bind time.
fn runtime_type(ty: &TypeRef) -> bool {
    static_type(ty) || (ty.visible && ty.runtime.is_some() && !ty.unsupported)
}

/// The text a value compares by: its type by CLR name, so the assembly
/// version in a serialized type name does not count.
fn value_key(v: &Value) -> String {
    match v {
        Value::Null { ty } => format!("null:{}", ty.name),
        Value::Bool { value } => format!("bool:{value}"),
        Value::String { value } => format!("string:{value:?}"),
        Value::Char { value } => format!("char:{value:?}"),
        Value::Number { ty, text } => format!("{ty}:{text}"),
        Value::Enum { ty, text } => format!("{}:{text}", ty.name),
        Value::Type { ty } => format!("typeof:{}", ty.name),
        Value::Array { element, items } => {
            let each: Vec<String> = items.iter().map(value_key).collect();
            format!("{}[]{{{}}}", element.name, each.join(","))
        }
    }
}

fn attribute_key(a: &Attribute) -> String {
    let args: Vec<String> = a.args.iter().map(value_key).collect();
    let mut named: Vec<String> = a.named.iter().map(|n| format!("{}={}", n.name, value_key(&n.value))).collect();
    named.sort();
    format!("{}({};{})", a.ty.name, args.join(","), named.join(","))
}

/// The attributes of `p` the wrapper copies: every one but those the
/// compiler writes and those it cannot build, each of the latter adding a
/// warning naming the command, the parameter, the attribute and why.
fn kept(command: &str, p: &Parameter, version: &str, warnings: &mut BTreeSet<String>) -> Vec<Attribute> {
    let mut out = Vec::new();
    for a in &p.attributes {
        if compiler_written(&a.ty) {
            continue;
        }
        if runtime_attribute(a) {
            out.push(a.clone());
            continue;
        }
        let why = if let Some(e) = &a.error {
            format!("its arguments cannot be read ({e})")
        } else if !a.ty.visible {
            "its type is not public".to_string()
        } else {
            "it names a type that is not public".to_string()
        };
        warnings.insert(format!(
            "pwrs: {command}'s -{} declares [{}] in {version}, and {why}, so the wrapper leaves it off",
            p.name,
            short_name(&a.ty.name)
        ));
    }
    out
}

/// A type's name without its namespace, as a warning names it.
fn short_name(name: &str) -> &str {
    match name.rfind(['.', '+']) {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// A parameter as the wrapper declares it: with only the attributes it copies.
fn with_kept(p: &Parameter, attributes: Vec<Attribute>) -> Parameter {
    Parameter { name: p.name.clone(), ty: p.ty.clone(), attributes }
}

/// The comparison key of a parameter as the wrapper declares it: its type
/// and its copied attributes, in any order.
fn parameter_key(p: &Parameter) -> String {
    let mut attrs: Vec<String> = p.attributes.iter().map(attribute_key).collect();
    attrs.sort();
    format!("{}|{}", p.ty.name, attrs.join("|"))
}

/// The plan for PowerShell 7's shell from the command's readings, newest
/// release last. A reading of a release that lacks the command adds
/// nothing; a command no reading declares is an error.
pub fn plan(command: &str, readings: &[Reading<'_>], warnings: &mut BTreeSet<String>) -> Result<Plan, Error> {
    let Some(newest) = readings.iter().rev().find(|r| r.platform == Platform::Windows).or_else(|| readings.last()) else {
        return Err(Error::msg(format!("{command} is declared by none of the PowerShell releases read")));
    };
    let mut per_reading: Vec<(&Reading<'_>, Vec<Parameter>)> = Vec::new();
    for r in readings {
        let params: Vec<Parameter> = r.declared.parameters.iter().map(|p| with_kept(p, kept(command, p, r.version, warnings))).collect();
        per_reading.push((r, params));
    }
    let mut names: Vec<String> = Vec::new();
    for (_, params) in &per_reading {
        for p in params {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(&p.name)) {
                names.push(p.name.clone());
            }
        }
    }
    let mut properties = Vec::new();
    let mut versioned: Vec<String> = Vec::new();
    for name in &names {
        let decls: Vec<&Parameter> = per_reading.iter().filter_map(|(_, ps)| ps.iter().find(|p| p.name.eq_ignore_ascii_case(name))).collect();
        let everywhere = decls.len() == per_reading.len();
        let alike = decls.windows(2).all(|w| parameter_key(w[0]) == parameter_key(w[1]));
        let first = decls[0];
        if everywhere && alike && static_type(&first.ty) && first.attributes.iter().all(static_attribute) {
            properties.push(first.clone());
        } else {
            versioned.push(name.to_ascii_lowercase());
        }
    }
    let mut variants = Vec::new();
    for (r, params) in &per_reading {
        let mut parameters = Vec::new();
        for p in params.iter().filter(|p| versioned.contains(&p.name.to_ascii_lowercase())) {
            if runtime_type(&p.ty) {
                parameters.push(p.clone());
            } else {
                warnings.insert(format!(
                    "pwrs: {command}'s -{} takes {} in {}, which the wrapper cannot name, so it does not declare the parameter there",
                    p.name, p.ty.name, r.version
                ));
            }
        }
        variants.push(Variant { release: Some(r.release), platform: Some(r.platform), parameters });
    }
    Ok(Plan {
        binding: binding_of(command, newest.declared)?,
        output: newest.declared.output.iter().filter(|a| static_attribute(a)).cloned().collect(),
        forwards_dynamic: readings.iter().any(|r| r.declared.dynamic),
        properties,
        variants,
    })
}

/// The plan for Windows PowerShell's shell from its one reading: every
/// parameter the shell can name is a property, and the rest are declared
/// at bind time.
pub fn plan_single(command: &str, declared: &Declared, warnings: &mut BTreeSet<String>) -> Result<Plan, Error> {
    let mut properties = Vec::new();
    let mut parameters = Vec::new();
    for p in &declared.parameters {
        let p = with_kept(p, kept(command, p, "Windows PowerShell 5.1", warnings));
        if static_type(&p.ty) && p.attributes.iter().all(static_attribute) {
            properties.push(p);
        } else if runtime_type(&p.ty) {
            parameters.push(p);
        } else {
            warnings.insert(format!(
                "pwrs: {command}'s -{} takes {} in Windows PowerShell 5.1, which the wrapper cannot name, so it does not declare the parameter",
                p.name, p.ty.name
            ));
        }
    }
    let variants = if parameters.is_empty() { Vec::new() } else { vec![Variant { release: None, platform: None, parameters }] };
    Ok(Plan {
        binding: binding_of(command, declared)?,
        output: declared.output.iter().filter(|a| static_attribute(a)).cloned().collect(),
        forwards_dynamic: declared.dynamic,
        properties,
        variants,
    })
}

/// What both shells declare of one wrapping cmdlet's command.
#[derive(Debug, Clone, PartialEq)]
pub struct Shells {
    /// The command's name, as `wraps` gives it.
    pub command: String,
    /// PowerShell 7's shell's.
    pub core: Plan,
    pub desktop: Desktop,
}

/// What Windows PowerShell's shell declares of a wrapping cmdlet's
/// command.
#[derive(Debug, Clone, PartialEq)]
pub enum Desktop {
    /// The plan of its copy of the command.
    Plan(Plan),
    /// Why the cmdlet does not run the command there: its module targets
    /// PowerShell 7 only, or Windows PowerShell has no such command. The
    /// cmdlet declares no copied parameter there and refuses when called.
    Refused(String),
}

/// The base members a copied property's name may hide: the public and
/// protected ones of `PSCmdlet`, `Cmdlet`, `RustCmdlet` and
/// `WrappedCmdlet`, which a property of that name hides with `new`.
const BASE_MEMBERS: &[&str] = &[
    "CommandOrigin",
    "CommandRuntime",
    "CurrentPSTransaction",
    "Events",
    "Host",
    "InvokeCommand",
    "InvokeProvider",
    "JobManager",
    "JobRepository",
    "MyInvocation",
    "PagingParameters",
    "ParameterSetName",
    "SessionState",
    "Stopping",
    "PwrsCmdletId",
    "PwrsNative",
    "SelfHandle",
    "WrappedName",
    "ForwardsDynamic",
    "Refusal",
];

/// A value as a C# attribute argument.
fn cs_value(v: &Value) -> String {
    match v {
        Value::Null { .. } => "null".to_string(),
        Value::Bool { value } => value.to_string(),
        Value::String { value } => crate::generate::cs_str(value),
        Value::Char { value } => {
            let units: Vec<String> = value.encode_utf16().map(|u| format!("'\\u{u:04x}'")).collect();
            units.first().cloned().unwrap_or_else(|| "'\\0'".to_string())
        }
        Value::Number { ty, text } => cs_number(ty, text),
        Value::Enum { ty, text } => format!("({})({text})", ty.csharp),
        Value::Type { ty } => format!("typeof({})", ty.csharp),
        Value::Array { element, items } => {
            let each: Vec<String> = items.iter().map(cs_value).collect();
            format!("new {}[] {{ {} }}", element.csharp, each.join(", "))
        }
    }
}

/// A value as a C# expression built at bind time: as an attribute
/// argument, but a public type outside the compile set is looked up by
/// its runtime name, and an attribute naming one the engine lacks is left
/// off.
fn cs_runtime_value(v: &Value) -> String {
    match v {
        Value::Type { ty } if !ty.compile => match &ty.runtime {
            Some(name) => format!("Need({})", crate::generate::cs_str(name)),
            None => "null".to_string(),
        },
        Value::Array { element, items } => {
            let each: Vec<String> = items.iter().map(cs_runtime_value).collect();
            format!("new {}[] {{ {} }}", element.csharp, each.join(", "))
        }
        other => cs_value(other),
    }
}

/// A number of the CLR type `ty`, from its invariant text.
fn cs_number(ty: &str, text: &str) -> String {
    let special = match text {
        "NaN" => Some("NaN"),
        "Infinity" | "∞" => Some("PositiveInfinity"),
        "-Infinity" | "-∞" => Some("NegativeInfinity"),
        _finite => None,
    };
    if let Some(member) = special {
        return format!("global::{ty}.{member}");
    }
    if ty == "System.Int32" {
        return if text.starts_with('-') { format!("({text})") } else { text.to_string() };
    }
    format!("(global::{ty})({text})")
}

/// An attribute as C# writes it on a declaration.
fn cs_attribute(a: &Attribute) -> String {
    let mut args: Vec<String> = a.args.iter().map(cs_value).collect();
    args.extend(a.named.iter().map(|n| format!("{} = {}", n.name, cs_value(&n.value))));
    format!("[{}({})]", a.ty.csharp, args.join(", "))
}

/// An attribute as a C# expression that builds it at bind time.
fn cs_new_attribute(a: &Attribute) -> String {
    let args: Vec<String> = a.args.iter().map(cs_runtime_value).collect();
    let named: Vec<String> = a.named.iter().map(|n| format!("{} = {}", n.name, cs_runtime_value(&n.value))).collect();
    let built = if named.is_empty() {
        format!("new {}({})", a.ty.csharp, args.join(", "))
    } else {
        format!("new {}({}) {{ {} }}", a.ty.csharp, args.join(", "), named.join(", "))
    };
    let looks_up = |v: &Value| matches!(v, Value::Type { ty } if !ty.compile);
    if a.args.iter().any(looks_up) || a.named.iter().any(|n| looks_up(&n.value)) {
        format!("Make(() => {built})")
    } else {
        built
    }
}

/// The `[Cmdlet(...)]` and `[OutputType(...)]` lines of one shell's
/// wrapper, with the wrapper's own verb and noun.
fn cs_class_attributes(plan: &Plan, verb: &str, noun: &str) -> String {
    let mut args = vec![crate::generate::cs_str(verb), crate::generate::cs_str(noun)];
    args.extend(plan.binding.iter().map(|n| format!("{} = {}", n.name, cs_value(&n.value))));
    let mut s = format!("    [Cmdlet({})]\n", args.join(", "));
    for a in &plan.output {
        s.push_str(&format!("    {}\n", cs_attribute(a)));
    }
    s
}

/// One shell's copied properties, each with its attributes.
fn cs_properties(plan: &Plan) -> String {
    let mut s = String::new();
    for p in &plan.properties {
        for a in &p.attributes {
            s.push_str(&format!("        {}\n", cs_attribute(a)));
        }
        let hides = if BASE_MEMBERS.iter().any(|m| m.eq_ignore_ascii_case(&p.name)) { "new " } else { "" };
        // A reference type is marked as one the binder may leave null; a
        // value type is declared as its own type, which the parameter's is.
        let nullable = if p.ty.value_type { "" } else { "?" };
        s.push_str(&format!("        public {hides}{}{nullable} {} {{ get; set; }}\n\n", p.ty.csharp, p.name));
    }
    s
}

/// One shell's `IsCopiedProperty` body.
fn cs_is_copied(plan: &Plan) -> String {
    if plan.properties.is_empty() {
        return "            return false;\n".to_string();
    }
    let mut s = String::from("            switch (name)\n            {\n");
    for p in &plan.properties {
        s.push_str(&format!("                case {}:\n", crate::generate::cs_str(&p.name.to_ascii_lowercase())));
    }
    s.push_str("                    return true;\n                default:\n                    return false;\n            }\n");
    s
}

/// The `Add` call declaring one parameter at bind time.
fn cs_add(p: &Parameter) -> String {
    let ty = if p.ty.compile {
        format!("typeof({})", p.ty.csharp)
    } else {
        match &p.ty.runtime {
            Some(name) => format!("TypeNamed({})", crate::generate::cs_str(name)),
            None => "null".to_string(),
        }
    };
    let mut args = vec![crate::generate::cs_str(&p.name), ty];
    args.extend(p.attributes.iter().map(cs_new_attribute));
    format!("Add({});\n", args.join(", "))
}

/// One shell's `Declare` body: for PowerShell 7, each platform's readings
/// newest first, the first at or below the running engine declaring its
/// parameters and the oldest taking any older engine; for Windows
/// PowerShell, its one reading's.
fn cs_declare(plan: &Plan) -> String {
    let mut s = String::new();
    let single: Vec<&Variant> = plan.variants.iter().filter(|v| v.release.is_none()).collect();
    for v in single {
        for p in &v.parameters {
            s.push_str(&format!("            {}", cs_add(p)));
        }
    }
    for (platform, test) in [(Platform::Windows, "windows"), (Platform::Unix, "!windows")] {
        let mut readings: Vec<&Variant> = plan.variants.iter().filter(|v| v.platform == Some(platform)).collect();
        if readings.is_empty() {
            continue;
        }
        readings.sort_by_key(|v| std::cmp::Reverse(v.release));
        s.push_str(&format!("            if ({test})\n            {{\n"));
        let oldest = readings.len() - 1;
        for (i, v) in readings.iter().enumerate() {
            let Some((major, minor)) = v.release else { continue };
            if i == oldest {
                s.push_str("                {\n");
            } else {
                s.push_str(&format!("                if (AtLeast(major, minor, {major}, {minor}))\n                {{\n"));
            }
            for p in &v.parameters {
                s.push_str(&format!("                    {}", cs_add(p)));
            }
            s.push_str("                    return;\n                }\n");
        }
        s.push_str("            }\n");
    }
    s
}

/// The members a wrapping cmdlet's class adds: its `[Cmdlet]` and
/// `[OutputType]` attributes come first, then the body. Each part a shell
/// declares its own way is under `#if NET` for PowerShell 7's and `#else`
/// for Windows PowerShell's.
pub struct Emitted {
    pub class_attributes: String,
    pub members: String,
}

/// The C# of a wrapping cmdlet's class: verb and noun its own, the rest
/// from `shells`.
pub fn emit(shells: &Shells, verb: &str, noun: &str) -> Emitted {
    let core_attributes = cs_class_attributes(&shells.core, verb, noun);
    // A refusing cmdlet copies nothing, its [Cmdlet] settings included.
    let desktop_attributes = match &shells.desktop {
        Desktop::Plan(plan) => cs_class_attributes(plan, verb, noun),
        Desktop::Refused(..) => format!("    [Cmdlet({}, {})]\n", crate::generate::cs_str(verb), crate::generate::cs_str(noun)),
    };
    let class_attributes = format!("#if NET\n{core_attributes}#else\n{desktop_attributes}#endif\n");

    let mut m = String::new();
    m.push_str(&format!("        protected override string WrappedName => {};\n\n", crate::generate::cs_str(&shells.command)));
    m.push_str("#if NET\n");
    m.push_str(&cs_properties(&shells.core));
    m.push_str(&format!("        protected override bool ForwardsDynamic => {};\n\n", shells.core.forwards_dynamic));
    m.push_str("        protected override bool IsCopiedProperty(string name)\n        {\n");
    m.push_str(&cs_is_copied(&shells.core));
    m.push_str("        }\n\n");
    m.push_str("        protected override void Declare(int major, int minor, bool windows)\n        {\n");
    m.push_str(&cs_declare(&shells.core));
    m.push_str("        }\n");
    m.push_str("#else\n");
    match &shells.desktop {
        Desktop::Plan(plan) => {
            m.push_str(&cs_properties(plan));
            m.push_str(&format!("        protected override bool ForwardsDynamic => {};\n\n", plan.forwards_dynamic));
            m.push_str("        protected override bool IsCopiedProperty(string name)\n        {\n");
            m.push_str(&cs_is_copied(plan));
            m.push_str("        }\n\n");
            m.push_str("        protected override void Declare(int major, int minor, bool windows)\n        {\n");
            m.push_str(&cs_declare(plan));
            m.push_str("        }\n");
        }
        Desktop::Refused(reason) => {
            m.push_str("        protected override bool ForwardsDynamic => false;\n\n");
            m.push_str("        protected override bool IsCopiedProperty(string name) => false;\n\n");
            m.push_str("        protected override void Declare(int major, int minor, bool windows) { }\n\n");
            m.push_str(&format!("        protected override string? Refusal => {};\n", crate::generate::cs_str(reason)));
        }
    }
    m.push_str("#endif\n\n");
    Emitted { class_attributes, members: m }
}

/// The named arguments of a declaration's `[Cmdlet]` the wrapper can
/// write. `commands.ps1` finds a class by that attribute, so a
/// declaration without one is not what it writes.
fn binding_of(command: &str, declared: &Declared) -> Result<Vec<Named>, Error> {
    match declared.cmdlet.first() {
        Some(a) => Ok(a.named.iter().filter(|n| static_value(&n.value)).cloned().collect()),
        None => Err(Error::msg(format!("the declaration read for {command} has no [Cmdlet] attribute"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A type as `commands.ps1` writes one.
    fn ty(name: &str, csharp: &str, compile: bool, visible: bool) -> serde_json::Value {
        let assembly = if name.starts_with("System.Management.Automation") { "System.Management.Automation" } else { "System.Runtime" };
        serde_json::json!({ "name": name, "csharp": csharp, "assembly": assembly, "runtime": format!("{name}, {assembly}"),
                            "known": true, "visible": visible, "compile": compile })
    }

    fn attribute(name: &str, visible: bool) -> serde_json::Value {
        serde_json::json!({ "type": ty(name, &format!("global::{name}"), visible, visible), "args": [], "named": [] })
    }

    /// `Join-Path` as one reading declares it: -Path alike everywhere,
    /// -ChildPath of the type given, and -Extra only when `extra` is true.
    fn join_path(child: (&str, &str), extra: bool) -> Declared {
        let mut parameters = vec![
            serde_json::json!({ "name": "Path", "type": ty("System.String[]", "string[]", true, true),
                                "attributes": [attribute("System.Management.Automation.ParameterAttribute", true),
                                               attribute("System.Runtime.CompilerServices.NullableAttribute", false)] }),
            serde_json::json!({ "name": "ChildPath", "type": ty(child.0, child.1, true, true),
                                "attributes": [attribute("System.Management.Automation.ParameterAttribute", true),
                                               attribute("System.Management.Automation.InternalCompleterAttribute", false)] }),
        ];
        if extra {
            parameters.push(serde_json::json!({ "name": "Extra", "type": ty("System.Management.Automation.SwitchParameter", "global::System.Management.Automation.SwitchParameter", true, true),
                                                "attributes": [attribute("System.Management.Automation.ParameterAttribute", true)] }));
        }
        let cmdlet = serde_json::json!({ "type": ty("System.Management.Automation.CmdletAttribute", "global::System.Management.Automation.CmdletAttribute", true, true),
                                         "args": [{ "kind": "string", "value": "Join" }, { "kind": "string", "value": "Path" }],
                                         "named": [{ "name": "HelpUri", "value": { "kind": "string", "value": "https://example.invalid/join" } }] });
        serde_json::from_value(serde_json::json!({ "class": "JoinPathCommand", "assembly": "Microsoft.PowerShell.Commands.Management",
                                                   "cmdlet": [cmdlet], "output": [], "dynamic": true, "parameters": parameters }))
        .expect("a declaration")
    }

    #[test]
    fn a_parameter_declared_alike_everywhere_is_a_property_and_any_other_is_declared_per_reading() {
        let old = join_path(("System.String", "string"), false);
        let new = join_path(("System.String[]", "string[]"), true);
        let readings = [
            Reading { release: (7, 4), version: "7.4.22", platform: Platform::Windows, declared: &old },
            Reading { release: (7, 6), version: "7.6.9", platform: Platform::Windows, declared: &new },
        ];
        let mut warnings = BTreeSet::new();
        let plan = plan("Join-Path", &readings, &mut warnings).expect("a plan");
        assert_eq!(plan.properties.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Path"]);
        let names = |v: &Variant| v.parameters.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        assert_eq!(plan.variants.iter().map(names).collect::<Vec<_>>(), [vec!["ChildPath"], vec!["ChildPath", "Extra"]]);
        assert!(plan.forwards_dynamic);
        // The compiler's attribute goes silently; the internal one with a warning per reading.
        assert!(plan.properties[0].attributes.iter().all(|a| !a.ty.name.contains("Nullable")));
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings.iter().all(|w| w.contains("Join-Path's -ChildPath declares [InternalCompleterAttribute]") && w.contains("not public")), "{warnings:?}");
    }

    #[test]
    fn a_qualified_name_gives_its_module_and_its_bare_command() {
        assert_eq!(qualifier("Microsoft.PowerShell.Archive\\Compress-Archive"), Some("Microsoft.PowerShell.Archive"));
        assert_eq!(bare_name("Microsoft.PowerShell.Archive\\Compress-Archive"), "Compress-Archive");
        assert_eq!(qualifier("Get-ChildItem"), None);
        assert_eq!(bare_name("Get-ChildItem"), "Get-ChildItem");
    }

    #[test]
    fn the_files_beside_an_assembly_are_the_other_assemblies_in_its_folder() {
        let dir = std::env::temp_dir().join(format!("pwrs-beside-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch folder");
        for name in ["Module.dll", "Helper.DLL", "notes.txt"] {
            std::fs::write(dir.join(name), b"").expect("a scratch file");
        }
        let found = beside(&dir.join("module.dll"));
        let removed = std::fs::remove_dir_all(&dir);
        let names: Vec<String> = found.expect("the listing").iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Helper.DLL"]);
        removed.expect("the scratch folder is removed");
    }

    #[test]
    fn the_lock_finds_a_command_whatever_its_case() {
        let mut lock = Lock::default();
        let kept = Kept { module: "Some.Module".to_string(), module_version: "1.0.0".to_string(), core: join_path(("System.String", "string"), false), desktop: None };
        lock.commands.insert("Some.Module\\Get-Thing".to_string(), kept);
        assert_eq!(lock.key_of("some.module\\get-thing").map(String::as_str), Some("Some.Module\\Get-Thing"));
        assert!(lock.key_of("Other.Module\\Get-Thing").is_none());
    }

    /// A first build's read of commands where they are installed, in one
    /// run of pwsh: a compiled cmdlet from its assembly's file, and a
    /// function, here pwsh's own `TabExpansion2` of no module, from its
    /// param block, its types completed against the shell's compile set.
    #[test]
    fn an_installed_cmdlet_is_read_from_its_file_and_a_function_from_its_param_block() {
        let tc = pwrs_build::Toolchain::ensure().expect("the toolchain");
        let mut core_compile = tc.net10_refs.clone();
        core_compile.extend(tc.pwsh_compile_references().expect("the compile references"));
        let work = std::env::temp_dir().join(format!("pwrs-installed-{}", std::process::id()));
        let hosts = Hosts { work: &work, core_compile: &core_compile, desktop_compile: &[], framework: &[], gac: &[], desktop: false };
        let commands = [
            ("Get-RustDate".to_string(), "Microsoft.PowerShell.Utility\\Get-Date".to_string()),
            ("Expand-RustTab".to_string(), "TabExpansion2".to_string()),
        ];
        let kept = read_installed(&commands, &hosts);
        let removed = std::fs::remove_dir_all(&work);
        let kept = kept.expect("both are read");

        let date = &kept["Microsoft.PowerShell.Utility\\Get-Date"];
        assert_eq!(
            (date.module.as_str(), date.core.class.as_str(), date.core.assembly.as_str()),
            ("Microsoft.PowerShell.Utility", "Microsoft.PowerShell.Commands.GetDateCommand", "Microsoft.PowerShell.Commands.Utility")
        );
        let date_type = date.core.parameters.iter().find(|p| p.name == "Date").map(|p| p.ty.name.as_str());
        assert_eq!(date_type, Some("System.DateTime"));
        assert!(date.desktop.is_none());

        let tab = &kept["TabExpansion2"];
        assert_eq!((tab.module.as_str(), tab.core.class.as_str()), ("", ""));
        let names: Vec<&str> = tab.core.parameters.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["inputScript", "cursorColumn", "ast", "tokens", "positionOfCursor", "options"]);
        let ast = tab.core.parameters.iter().find(|p| p.name == "ast").expect("-ast");
        assert_eq!(ast.ty.name, "System.Management.Automation.Language.Ast");
        assert!(ast.ty.compile, "the shell's compile set names {:?}", ast.ty);
        let binding = tab.core.cmdlet.first().expect("[CmdletBinding]");
        assert_eq!(binding.named, [Named { name: "DefaultParameterSetName".to_string(), value: Value::String { value: "ScriptInputSet".to_string() } }]);
        removed.expect("the work folder is removed");
    }

    #[test]
    fn a_kept_copy_reads_back_as_it_was_written() {
        let dir = std::env::temp_dir().join(format!("pwrs-wrapped-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch folder");
        let path = dir.join(LOCK_FILE);
        assert!(Lock::load(&path).expect("no file reads as empty").commands.is_empty());
        let mut lock = Lock::default();
        let kept = Kept {
            module: "Some.Module".to_string(),
            module_version: "1.2.3".to_string(),
            core: join_path(("System.String", "string"), false),
            desktop: None,
        };
        lock.commands.insert("Some.Module\\Join-Path".to_string(), kept);
        lock.save(&path).expect("the save");
        let back = Lock::load(&path).expect("the load");
        let removed = std::fs::remove_dir_all(&dir);
        let entry = back.commands.get("Some.Module\\Join-Path").expect("the entry");
        assert_eq!((entry.module.as_str(), entry.module_version.as_str()), ("Some.Module", "1.2.3"));
        assert_eq!(entry.core, join_path(("System.String", "string"), false));
        assert!(entry.desktop.is_none());
        removed.expect("the scratch folder is removed");
    }

    #[test]
    fn the_emitted_class_picks_the_reading_for_the_running_engine_and_a_refused_shell_says_why() {
        let old = join_path(("System.String", "string"), false);
        let new = join_path(("System.String[]", "string[]"), true);
        let readings = [
            Reading { release: (7, 4), version: "7.4.22", platform: Platform::Windows, declared: &old },
            Reading { release: (7, 6), version: "7.6.9", platform: Platform::Windows, declared: &new },
        ];
        let core = plan("Join-Path", &readings, &mut BTreeSet::new()).expect("a plan");
        let shells = Shells { command: "Join-Path".to_string(), core, desktop: Desktop::Refused("no copy here".to_string()) };
        let e = emit(&shells, "Join", "RustPath");
        assert!(
            e.class_attributes.starts_with("#if NET\n    [Cmdlet(@\"Join\", @\"RustPath\", HelpUri = @\"https://example.invalid/join\")]\n#else\n    [Cmdlet(@\"Join\", @\"RustPath\")]\n#endif\n"),
            "{}",
            e.class_attributes
        );
        for piece in [
            "protected override string WrappedName => @\"Join-Path\";",
            "public string[]? Path { get; set; }",
            "case @\"path\":",
            "if (AtLeast(major, minor, 7, 6))",
            "Add(@\"ChildPath\", typeof(string[]), new global::System.Management.Automation.ParameterAttribute());",
            "Add(@\"Extra\", typeof(global::System.Management.Automation.SwitchParameter)",
            "Add(@\"ChildPath\", typeof(string), new global::System.Management.Automation.ParameterAttribute());",
            "protected override string? Refusal => @\"no copy here\";",
        ] {
            assert!(e.members.contains(piece), "no `{piece}` in\n{}", e.members);
        }
        // The newest reading is tried first and the oldest takes any older engine.
        let newest = e.members.find("AtLeast(major, minor, 7, 6)").expect("the 7.6 test");
        let oldest = e.members.find("Add(@\"ChildPath\", typeof(string),").expect("the 7.4 block");
        assert!(newest < oldest);
    }
}
