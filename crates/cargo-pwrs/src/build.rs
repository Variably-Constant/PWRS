//! `cargo pwrs build`: compile the crate, read its descriptor, generate
//! and compile the managed side, and lay out the module folder.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::descriptor;
use crate::generate;
use crate::Error;
use pwrs_build::{DebugSymbols, Toolchain};

// The C# sits inside this crate so `cargo package` carries it: a
// file outside the crate directory is not packaged, and a cargo-pwrs
// installed from the registry could not compile a module without it.
const RUNTIME_SOURCES: &[(&str, &str)] = &[
    ("Native.cs", include_str!("../dotnet/Pwrs.Runtime/Native.cs")),
    ("RustCmdlet.cs", include_str!("../dotnet/Pwrs.Runtime/RustCmdlet.cs")),
    ("NativeModule.cs", include_str!("../dotnet/Pwrs.Runtime/NativeModule.cs")),
    ("HostVTable.cs", include_str!("../dotnet/Pwrs.Runtime/HostVTable.cs")),
    ("Factories.cs", include_str!("../dotnet/Pwrs.Runtime/Factories.cs")),
    ("ProxyBase.cs", include_str!("../dotnet/Pwrs.Runtime/ProxyBase.cs")),
    ("StaticCall.cs", include_str!("../dotnet/Pwrs.Runtime/StaticCall.cs")),
    ("CompleterBase.cs", include_str!("../dotnet/Pwrs.Runtime/CompleterBase.cs")),
    ("TransformBase.cs", include_str!("../dotnet/Pwrs.Runtime/TransformBase.cs")),
    ("DynamicParametersBase.cs", include_str!("../dotnet/Pwrs.Runtime/DynamicParametersBase.cs")),
    ("ProviderBase.cs", include_str!("../dotnet/Pwrs.Runtime/ProviderBase.cs")),
    ("RustMemoryManager.cs", include_str!("../dotnet/Pwrs.Runtime/RustMemoryManager.cs")),
    ("ReadOnlyTable.cs", include_str!("../dotnet/Pwrs.Runtime/ReadOnlyTable.cs")),
    ("CpuCheck.cs", include_str!("../dotnet/Pwrs.Runtime/CpuCheck.cs")),
];

const BOOTSTRAP_SOURCES: &[(&str, &str)] = &[("Loader.cs", include_str!("../dotnet/Pwrs.Bootstrap/Loader.cs"))];

pub struct BuildOptions {
    pub release: bool,
    pub package: Option<String>,
    pub manifest_dir: PathBuf,
    /// A Rust target triple for `cargo build --target`. The native
    /// library lands under the triple's runtime identifier; when the
    /// triple is not the building machine's, the crate is also built
    /// for the host, whose library is the one loaded to read the
    /// descriptor.
    pub target: Option<String>,
    /// Cargo features passed through as `--features`, `--all-features`
    /// and `--no-default-features`; all three reach `cargo build` and
    /// `cargo test`.
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    /// License files a bundling module supplies for crates this library
    /// links, from its `bundled-modules` entry, taken beside the crate's
    /// own `license-files`.
    pub supplied_license_files: Vec<crate::notices::Supplied>,
    /// Instruction-set extensions a bundling module requires of this
    /// library on purpose, from its `bundled-modules` entry, taken beside
    /// the crate's own `cpu-features`.
    pub supplied_cpu_features: Vec<String>,
    /// Writes a `.pdb` beside each managed assembly. A debug build
    /// does so anyway; this asks a release build for them too.
    pub debug_symbols: bool,
    /// Where cargo writes the build, in place of the manifest's own
    /// target directory. A bundled module's crate is only read: its
    /// build goes under the bundling module's target directory.
    pub target_dir: Option<PathBuf>,
    /// `--locked` on the cargo build, so a lock file that would change
    /// stops the build with the file named.
    pub locked: bool,
}

impl BuildOptions {
    /// Whether the managed compiles emit `.pdb` files: always for a
    /// debug build, for a release build only when asked.
    fn symbols(&self) -> DebugSymbols {
        if self.debug_symbols || !self.release { DebugSymbols::Emit } else { DebugSymbols::Omit }
    }
}

impl BuildOptions {
    /// The feature arguments for a cargo command.
    pub fn feature_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.all_features {
            args.push("--all-features".to_string());
        }
        if self.no_default_features {
            args.push("--no-default-features".to_string());
        }
        if !self.features.is_empty() {
            args.push("--features".to_string());
            args.push(self.features.join(","));
        }
        args
    }
}

/// What a failed import of a bundled module does to the import of the
/// module bundling it, from its entry's `on-import-failure`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportFailure {
    /// `"stop"`, the default: the bundling module's import fails with the
    /// bundled module's error.
    Stop,
    /// `"warn"`: a warning names the bundled module and its error, and
    /// the bundling module's import goes on without it.
    Warn,
}

/// One `bundled-modules` entry: a crate directory, written as a string or
/// as the `path` of a table that also says what a failed import does and
/// what the bundled crate is built and published with.
#[derive(Debug)]
struct BundledEntry {
    /// The crate directory, resolved.
    dir: PathBuf,
    on_import_failure: ImportFailure,
    /// `features` and `default-features`, for the bundled crate's build.
    features: Vec<String>,
    default_features: bool,
    /// `license-files`, supplied for crates the bundled library links.
    license_files: Vec<crate::notices::Supplied>,
    /// `cpu-features`, required of the bundled library on purpose.
    cpu_features: Vec<String>,
}

/// The keys a `bundled-modules` table takes.
const BUNDLED_KEYS: [&str; 6] = ["path", "on-import-failure", "features", "default-features", "license-files", "cpu-features"];

/// A module laid inside this one, and what its own build found that
/// `publish` refuses.
pub struct BundledModule {
    pub name: String,
    pub on_import_failure: ImportFailure,
    /// As [`Built::undeclared_cpu_features`], for the bundled library.
    pub undeclared_cpu_features: Vec<String>,
    /// As [`Built::crates_without_license_text`], for the bundled library.
    pub crates_without_license_text: Vec<String>,
}

pub struct Built {
    pub module_dir: PathBuf,
    /// What the built library declared, as the descriptor carries it.
    pub module: descriptor::Module,
    /// The modules laid inside this one, in `bundled-modules` order.
    pub bundled: Vec<BundledModule>,
    /// `[package]` keys the crate did not declare, whose manifest
    /// fields fell back to a placeholder.
    pub missing_metadata: Vec<&'static str>,
    /// Instruction-set extensions the native library was compiled to
    /// require beyond its target's baseline and not declared in
    /// `cpu-features`; the library refuses to import on a processor
    /// without them.
    pub undeclared_cpu_features: Vec<String>,
    /// `test-cpu-tiers` from the manifest.
    pub test_cpu_tiers: Vec<String>,
    /// Crates the library links whose source carries no license file,
    /// as `name version`: the notices name their license but cannot
    /// quote it, and `publish` refuses the module.
    pub crates_without_license_text: Vec<String>,
}

struct CrateInfo {
    package: String,
    lib_name: String,
    version: String,
    authors: Vec<String>,
    /// `description`, `repository` and `keywords` from the package,
    /// which the manifest shows in a gallery listing.
    description: Option<String>,
    repository: Option<String>,
    keywords: Vec<String>,
    /// Everything a module manifest carries that a cargo manifest has
    /// no field for, taken from `[package.metadata.pwrs]` under the
    /// manifest key's own name in kebab case. A license is an SPDX name
    /// in cargo and a link in a gallery, and the rest have nowhere to
    /// live at all, so each is named outright rather than guessed at.
    license_uri: Option<String>,
    release_notes: Option<String>,
    icon_uri: Option<String>,
    company: Option<String>,
    copyright: Option<String>,
    prerelease: Option<String>,
    require_license_acceptance: bool,
    external_module_dependencies: Vec<String>,
    powershell_version: Option<String>,
    compatible_ps_editions: Vec<String>,
    powershell_host_name: Option<String>,
    powershell_host_version: Option<String>,
    dotnet_framework_version: Option<String>,
    clr_version: Option<String>,
    processor_architecture: Option<String>,
    help_info_uri: Option<String>,
    default_command_prefix: Option<String>,
    required_modules: Vec<String>,
    required_assemblies: Vec<String>,
    scripts_to_process: Vec<String>,
    types_to_process: Vec<String>,
    nested_modules: Vec<String>,
    dsc_resources_to_export: Vec<String>,
    module_list: Vec<String>,
    file_list: Vec<String>,
    /// Instruction-set extensions the module requires on purpose, from
    /// `cpu-features`: a build compiled for these beyond its target's
    /// baseline is neither warned about nor refused by `publish`.
    cpu_features: Vec<String>,
    /// The `PWRS_CPU_MAX` levels `cargo pwrs test` runs the suites under
    /// again, from `test-cpu-tiers`, when `--cpu-tiers` names none.
    test_cpu_tiers: Vec<String>,
    /// Licenses the library may link crates under beyond the permissive
    /// defaults, from `allowed-licenses`.
    allowed_licenses: Vec<String>,
    /// The package's [[bin]] targets named in `helpers`, each shipped
    /// beside the native library.
    helpers: Vec<String>,
    /// The `bundled-modules` entries, their directories resolved: each
    /// one's module is built by this tool and laid inside this one.
    bundled_modules: Vec<BundledEntry>,
    /// License files the module supplies for linked crates whose packages
    /// ship none, from `[package.metadata.pwrs.license-files]`.
    license_files: Vec<crate::notices::Supplied>,
    /// The package's own `Cargo.toml`, the root of the crates the library
    /// links.
    manifest_path: PathBuf,
    target_dir: PathBuf,
}

fn crate_info(opts: &BuildOptions) -> Result<CrateInfo, Error> {
    let mut metadata = Command::new("cargo");
    metadata.args(["metadata", "--format-version", "1", "--no-deps"]).current_dir(&opts.manifest_dir);
    // cargo reports the target directory it will build into, so the
    // override reaches it the way it reaches the build.
    if let Some(dir) = &opts.target_dir {
        metadata.env("CARGO_TARGET_DIR", dir);
    }
    let out = metadata.output().map_err(|e| Error::msg(format!("cannot run cargo metadata: {e}")))?;
    if !out.status.success() {
        return Err(Error::msg(format!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr))));
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| Error::msg(format!("cargo metadata is not JSON: {e}")))?;
    let target_dir = meta["target_directory"].as_str().ok_or_else(|| Error::msg("cargo metadata has no target_directory"))?;
    let packages = meta["packages"].as_array().ok_or_else(|| Error::msg("cargo metadata has no packages"))?;
    let manifest_path = opts.manifest_dir.join("Cargo.toml");
    // Canonicalize so a relative --manifest-dir matches the absolute
    // paths cargo metadata reports; fall back to the given path when it
    // cannot be resolved. Both sides are canonicalized before the
    // comparison because Windows canonical form carries the `\\?\`
    // verbatim prefix that cargo's own paths do not.
    let manifest_canon = match std::fs::canonicalize(&manifest_path) {
        Ok(p) => p,
        Err(_not_resolvable) => manifest_path.clone(),
    };
    let manifest_str = manifest_path.display().to_string();
    let mut chosen = None;
    for p in packages {
        let name = p["name"].as_str().ok_or_else(|| Error::msg("package without a name"))?;
        let matches = match &opts.package {
            Some(want) => want == name,
            None => p["manifest_path"].as_str().is_some_and(|m| {
                let mp = Path::new(m);
                mp == Path::new(&manifest_str)
                    || mp == manifest_canon
                    || std::fs::canonicalize(mp).is_ok_and(|c| c == manifest_canon)
            }),
        };
        if !matches {
            continue;
        }
        let targets = p["targets"].as_array().ok_or_else(|| Error::msg("package without targets"))?;
        let mut lib_name = None;
        let mut bins: Vec<String> = Vec::new();
        for t in targets {
            let kinds = t["kind"].as_array().ok_or_else(|| Error::msg("target without kind"))?;
            if kinds.iter().any(|k| k.as_str() == Some("cdylib")) {
                lib_name = t["name"].as_str().map(|s| s.replace('-', "_"));
            }
            if kinds.iter().any(|k| k.as_str() == Some("bin")) {
                bins.push(t["name"].as_str().ok_or_else(|| Error::msg(format!("package {name} has a bin target without a name")))?.to_string());
            }
        }
        let lib_name = lib_name.ok_or_else(|| Error::msg(format!("package {name} has no cdylib target; add `crate-type = [\"cdylib\"]`")))?;
        let strings = |key: &str| -> Vec<String> {
            p[key].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_else(Vec::new)
        };
        let text = |key: &str| -> Option<String> {
            match p[key].as_str() {
                Some(s) if !s.trim().is_empty() => Some(s.to_string()),
                _absent => None,
            }
        };
        let pwrs_meta = |key: &str| -> Option<String> {
            match p["metadata"]["pwrs"][key].as_str() {
                Some(s) if !s.trim().is_empty() => Some(s.to_string()),
                _absent => None,
            }
        };
        let pwrs_meta_list = |key: &str| -> Vec<String> {
            match p["metadata"]["pwrs"][key].as_array() {
                Some(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                _absent => Vec::new(),
            }
        };
        let pwrs_meta_bool = |key: &str| -> bool { p["metadata"]["pwrs"][key].as_bool() == Some(true) };
        let helpers = pwrs_meta_list("helpers");
        check_helpers(name, &helpers, &bins)?;
        let manifest_path = PathBuf::from(p["manifest_path"].as_str().ok_or_else(|| Error::msg(format!("package {name} has no manifest path")))?);
        let package_dir = manifest_path.parent().ok_or_else(|| Error::msg(format!("{} has no folder", manifest_path.display())))?;
        let license_files = crate::notices::supplied_from_metadata(&p["metadata"]["pwrs"]["license-files"], package_dir, "[package.metadata.pwrs] license-files", None)?;
        let bundled_modules = bundled_entries(name, package_dir, &p["metadata"]["pwrs"]["bundled-modules"])?;
        chosen = Some(CrateInfo {
            package: name.to_string(),
            lib_name,
            version: p["version"].as_str().ok_or_else(|| Error::msg("package without version"))?.to_string(),
            authors: strings("authors"),
            description: text("description"),
            repository: text("repository"),
            keywords: strings("keywords"),
            license_uri: pwrs_meta("license-uri"),
            release_notes: pwrs_meta("release-notes"),
            icon_uri: pwrs_meta("icon-uri"),
            company: pwrs_meta("company"),
            copyright: pwrs_meta("copyright"),
            prerelease: pwrs_meta("prerelease"),
            require_license_acceptance: pwrs_meta_bool("require-license-acceptance"),
            external_module_dependencies: pwrs_meta_list("external-module-dependencies"),
            powershell_version: pwrs_meta("powershell-version"),
            compatible_ps_editions: pwrs_meta_list("compatible-ps-editions"),
            powershell_host_name: pwrs_meta("powershell-host-name"),
            powershell_host_version: pwrs_meta("powershell-host-version"),
            dotnet_framework_version: pwrs_meta("dotnet-framework-version"),
            clr_version: pwrs_meta("clr-version"),
            processor_architecture: pwrs_meta("processor-architecture"),
            help_info_uri: pwrs_meta("help-info-uri"),
            default_command_prefix: pwrs_meta("default-command-prefix"),
            required_modules: pwrs_meta_list("required-modules"),
            required_assemblies: pwrs_meta_list("required-assemblies"),
            scripts_to_process: pwrs_meta_list("scripts-to-process"),
            types_to_process: pwrs_meta_list("types-to-process"),
            nested_modules: pwrs_meta_list("nested-modules"),
            dsc_resources_to_export: pwrs_meta_list("dsc-resources-to-export"),
            module_list: pwrs_meta_list("module-list"),
            file_list: pwrs_meta_list("file-list"),
            cpu_features: pwrs_meta_list("cpu-features"),
            test_cpu_tiers: pwrs_meta_list("test-cpu-tiers"),
            allowed_licenses: pwrs_meta_list("allowed-licenses"),
            helpers,
            bundled_modules,
            license_files,
            manifest_path,
            target_dir: PathBuf::from(target_dir),
        });
        break;
    }
    chosen.ok_or_else(|| Error::msg(format!("no package found for {}; pass --package", manifest_str)))
}

/// Refuses a `helpers` entry that names no [[bin]] target of `package`:
/// a helper is one of the package's own binaries, shipped beside the
/// native library.
fn check_helpers(package: &str, helpers: &[String], bins: &[String]) -> Result<(), Error> {
    match helpers.iter().find(|h| !bins.contains(h)) {
        Some(stray) => Err(Error::msg(format!("[package.metadata.pwrs] helpers names {stray}, which is not a [[bin]] target of {package}"))),
        None => Ok(()),
    }
}

/// The `bundled-modules` entries of `package`, each a crate directory
/// joined to the package's own directory and resolved; a path may leave
/// the repository. An entry is a string, or a table with `path` and any
/// of `on-import-failure` ("stop" or "warn"), `features`,
/// `default-features`, `license-files` and `cpu-features`. An entry of
/// another shape, a key or value the table does not take, or a directory
/// with no `Cargo.toml` stops the build, named. An absent list is none.
fn bundled_entries(package: &str, package_dir: &Path, list: &serde_json::Value) -> Result<Vec<BundledEntry>, Error> {
    if list.is_null() {
        return Ok(Vec::new());
    }
    let items = list
        .as_array()
        .ok_or_else(|| Error::msg("[package.metadata.pwrs] bundled-modules is a list of crate directories, each a string or a table with `path`"))?;
    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let entry = match item {
            serde_json::Value::String(written) => BundledEntry {
                dir: bundled_dir(package_dir, written)?,
                on_import_failure: ImportFailure::Stop,
                features: Vec::new(),
                default_features: true,
                license_files: Vec::new(),
                cpu_features: Vec::new(),
            },
            serde_json::Value::Object(table) => bundled_table(package, package_dir, table)?,
            other => {
                return Err(Error::msg(format!(
                    "[package.metadata.pwrs] bundled-modules holds {other}, which is neither a crate directory nor a table with `path`"
                )))
            }
        };
        entries.push(entry);
    }
    Ok(entries)
}

/// A `bundled-modules` table entry; see [`bundled_entries`].
fn bundled_table(package: &str, package_dir: &Path, table: &serde_json::Map<String, serde_json::Value>) -> Result<BundledEntry, Error> {
    let written = match table.get("path") {
        Some(serde_json::Value::String(p)) => p.clone(),
        Some(other) => return Err(Error::msg(format!("[package.metadata.pwrs] a bundled-modules table's `path` is {other}; it is the crate directory, a string"))),
        None => return Err(Error::msg("[package.metadata.pwrs] a bundled-modules table has no `path`, the crate directory it bundles")),
    };
    let at = format!("[package.metadata.pwrs] the bundled-modules entry {written}");
    if let Some(key) = table.keys().find(|k| !BUNDLED_KEYS.contains(&k.as_str())) {
        return Err(Error::msg(format!("{at} sets `{key}`, which is not one of {}", BUNDLED_KEYS.join(", "))));
    }
    let on_import_failure = match table.get("on-import-failure") {
        None => ImportFailure::Stop,
        Some(serde_json::Value::String(s)) if s == "stop" => ImportFailure::Stop,
        Some(serde_json::Value::String(s)) if s == "warn" => ImportFailure::Warn,
        Some(other) => return Err(Error::msg(format!("{at} sets on-import-failure to {other}; it takes \"stop\" or \"warn\""))),
    };
    let strings = |key: &str| -> Result<Vec<String>, Error> {
        match table.get(key) {
            None => Ok(Vec::new()),
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .map(|v| v.as_str().map(String::from).ok_or_else(|| Error::msg(format!("{at} lists {v} in `{key}`, which takes strings"))))
                .collect(),
            Some(other) => Err(Error::msg(format!("{at} sets `{key}` to {other}; it takes a list of strings"))),
        }
    };
    let default_features = match table.get("default-features") {
        None => true,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(other) => return Err(Error::msg(format!("{at} sets default-features to {other}; it takes true or false"))),
    };
    let license_files = match table.get("license-files") {
        None => Vec::new(),
        Some(files) => crate::notices::supplied_from_metadata(files, package_dir, &format!("the bundled-modules entry {written} of {package}, license-files"), None)?,
    };
    Ok(BundledEntry {
        dir: bundled_dir(package_dir, &written)?,
        on_import_failure,
        features: strings("features")?,
        default_features,
        license_files,
        cpu_features: strings("cpu-features")?,
    })
}

/// The crate directory a `bundled-modules` entry names, joined to the
/// package's own directory and resolved. One with no `Cargo.toml` stops
/// the build, named.
fn bundled_dir(package_dir: &Path, written: &str) -> Result<PathBuf, Error> {
    let dir = package_dir.join(written);
    if !dir.join("Cargo.toml").is_file() {
        return Err(Error::msg(format!("[package.metadata.pwrs] bundled-modules names {written}, and {} holds no Cargo.toml", dir.display())));
    }
    std::fs::canonicalize(&dir).map_err(|e| Error::msg(format!("cannot resolve {}: {e}", dir.display())))
}

/// The native library's file name on an operating system: `win` takes
/// `<lib>.dll`, `osx` `lib<lib>.dylib`, the rest `lib<lib>.so`.
fn native_file_name(lib: &str, os: &str) -> String {
    match os {
        "win" => format!("{lib}.dll"),
        "osx" => format!("lib{lib}.dylib"),
        _unix => format!("lib{lib}.so"),
    }
}

/// The operating system and architecture halves of a runtime
/// identifier: the building machine's, or a Rust target triple's
/// (`x86_64-unknown-linux-gnu` is `linux` and `x64`).
pub fn target_os_arch(triple: Option<&str>) -> Result<(String, String), Error> {
    let triple = match triple {
        Some(t) => t,
        None => {
            let os = if cfg!(windows) {
                "win"
            } else if cfg!(target_os = "macos") {
                "osx"
            } else if cfg!(target_os = "freebsd") {
                "freebsd"
            } else {
                "linux"
            };
            let arch = match std::env::consts::ARCH {
                "x86_64" => "x64",
                "aarch64" => "arm64",
                "x86" => "x86",
                other => other,
            };
            return Ok((os.to_string(), arch.to_string()));
        }
    };
    let arch = match triple.split('-').next() {
        Some("x86_64") => "x64",
        Some("aarch64") => "arm64",
        Some("i686") => "x86",
        Some(other) => return Err(Error::msg(format!("target {triple}: no runtime identifier for the architecture {other}"))),
        None => return Err(Error::msg(format!("target {triple} names no architecture"))),
    };
    let os = if triple.contains("windows") {
        "win"
    } else if triple.contains("linux") {
        "linux"
    } else if triple.contains("apple") {
        "osx"
    } else if triple.contains("freebsd") {
        "freebsd"
    } else {
        return Err(Error::msg(format!("target {triple}: no runtime identifier for its operating system")));
    };
    Ok((os.to_string(), arch.to_string()))
}

/// A `--target` triple split into the triple rustc knows and, when a
/// GNU/Linux triple carries one after a dot as cargo-zigbuild spells it
/// (`x86_64-unknown-linux-gnu.2.35`), the glibc version to link against.
pub fn split_glibc(triple: &str) -> Result<(&str, Option<&str>), Error> {
    let Some((base, version)) = triple.split_once('.') else {
        return Ok((triple, None));
    };
    let numeric = version.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if base.contains("-linux-gnu") && numeric {
        return Ok((base, Some(version)));
    }
    Err(Error::msg(format!(
        "target {triple}: only a GNU/Linux triple takes a glibc version after a dot, as in x86_64-unknown-linux-gnu.2.35"
    )))
}

/// `cargo build` for `target`, or `cargo zigbuild` when the target names
/// a glibc version, which links the library through zig against that
/// glibc's symbol versions.
fn cargo_build(opts: &BuildOptions, package: &str, target: Option<&str>) -> Result<(), Error> {
    let zig = match target {
        Some(triple) => split_glibc(triple)?.1.is_some(),
        None => false,
    };
    let subcommand = if zig { "zigbuild" } else { "build" };
    let mut cargo = Command::new("cargo");
    cargo.arg(subcommand).arg("-p").arg(package).current_dir(&opts.manifest_dir);
    if let Some(dir) = &opts.target_dir {
        cargo.env("CARGO_TARGET_DIR", dir);
    }
    if opts.locked {
        cargo.arg("--locked");
    }
    if opts.release {
        cargo.arg("--release");
    }
    if let Some(triple) = target {
        cargo.arg("--target").arg(triple);
    }
    cargo.args(opts.feature_args());
    let status = cargo.status().map_err(|e| Error::msg(format!("cannot run cargo {subcommand}: {e}")))?;
    if !status.success() {
        let needs = if zig { "; a target naming a glibc version needs cargo-zigbuild and zig on PATH" } else { "" };
        let lock = if opts.locked {
            format!("; the build ran --locked, so {} must already agree with Cargo.toml", opts.manifest_dir.join("Cargo.lock").display())
        } else {
            String::new()
        };
        return Err(Error::msg(format!("cargo {subcommand} failed with {status}{needs}{lock}")));
    }
    Ok(())
}

/// The runtime assemblies two modules must share to load into one
/// Windows PowerShell session, which keeps one `Pwrs.Runtime` for all.
const SHARED_RUNTIME_FILES: [(&str, &str); 4] = [
    ("net10.0", "Pwrs.Bootstrap.dll"),
    ("net10.0", "Pwrs.Runtime.dll"),
    ("netstandard2.0", "Pwrs.Bootstrap.dll"),
    ("netstandard2.0", "Pwrs.Runtime.dll"),
];

/// Builds the module of `entry`'s crate with this tool, for the same
/// target and profile, `--locked`, with the entry's features and the
/// license files and instruction-set extensions it supplies, into
/// `<target>/pwrs-bundled/<crate dir name>`, refuses it unless its
/// runtime assemblies match `module_dir`'s byte for byte, and copies its
/// folder to `<module_dir>/<Name>/`. `outer` is the bundling module's
/// name, which the bundled library's notices give for a file its entry
/// supplies.
fn bundle_module(opts: &BuildOptions, info: &CrateInfo, entry: &BundledEntry, outer: &str, module_dir: &Path) -> Result<BundledModule, Error> {
    let crate_dir = &entry.dir;
    let dir_name = match crate_dir.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => return Err(Error::msg(format!("{} has no directory name", crate_dir.display()))),
    };
    let supplied_license_files = entry
        .license_files
        .iter()
        .map(|s| crate::notices::Supplied { supplier: Some(outer.to_string()), ..s.clone() })
        .collect();
    let inner = BuildOptions {
        release: opts.release,
        package: None,
        manifest_dir: crate_dir.to_path_buf(),
        target: opts.target.clone(),
        features: entry.features.clone(),
        all_features: false,
        no_default_features: !entry.default_features,
        supplied_license_files,
        supplied_cpu_features: entry.cpu_features.clone(),
        debug_symbols: opts.debug_symbols,
        target_dir: Some(info.target_dir.join("pwrs-bundled").join(&dir_name)),
        locked: true,
    };
    eprintln!("pwrs: building the module of {} to bundle", crate_dir.display());
    let built = build(&inner)?;
    for (tfm, file) in SHARED_RUNTIME_FILES {
        let ours = module_dir.join(tfm).join(file);
        let theirs = built.module_dir.join(tfm).join(file);
        let a = std::fs::read(&ours).map_err(|e| Error::msg(format!("cannot read {}: {e}", ours.display())))?;
        let b = std::fs::read(&theirs).map_err(|e| Error::msg(format!("cannot read {}: {e}", theirs.display())))?;
        if a != b {
            return Err(Error::msg(format!(
                "{} cannot bundle {}: {tfm}/{file} differs between the two builds, and one Windows PowerShell session keeps one runtime for both; build both with one cargo-pwrs and one toolchain",
                info.package, built.module.name
            )));
        }
    }
    let dest = module_dir.join(&built.module.name);
    if dest.exists() {
        return Err(Error::msg(format!("{} already holds {}, so a second module of that name cannot be bundled", module_dir.display(), built.module.name)));
    }
    crate::merge::copy_tree(&built.module_dir, &dest)?;
    eprintln!("pwrs: bundled {} at {}", built.module.name, dest.display());
    Ok(BundledModule {
        name: built.module.name,
        on_import_failure: entry.on_import_failure,
        undeclared_cpu_features: built.undeclared_cpu_features,
        crates_without_license_text: built.crates_without_license_text,
    })
}

fn write_sources(dir: &Path, sources: &[(&str, &str)]) -> Result<Vec<PathBuf>, Error> {
    std::fs::create_dir_all(dir).map_err(|e| Error::msg(format!("cannot create {}: {e}", dir.display())))?;
    let mut out = Vec::new();
    for (name, body) in sources {
        let p = dir.join(name);
        std::fs::write(&p, body).map_err(|e| Error::msg(format!("cannot write {}: {e}", p.display())))?;
        out.push(p);
    }
    Ok(out)
}

fn copy(from: &Path, to: &Path) -> Result<(), Error> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::msg(format!("cannot create {}: {e}", parent.display())))?;
    }
    std::fs::copy(from, to).map_err(|e| Error::msg(format!("cannot copy {} to {}: {e}", from.display(), to.display())))?;
    Ok(())
}

pub fn build(opts: &BuildOptions) -> Result<Built, Error> {
    let mut info = crate_info(opts)?;
    info.license_files.extend(opts.supplied_license_files.iter().cloned());
    info.cpu_features.extend(opts.supplied_cpu_features.iter().cloned());
    let profile_dir = if opts.release { "release" } else { "debug" };

    // The triple rustc knows: cargo writes the library under it, and
    // rustc and cargo metadata take it; a glibc version after it only
    // chooses the build command.
    let rust_target = match opts.target.as_deref() {
        Some(triple) => Some(split_glibc(triple)?.0),
        None => None,
    };
    let (host_os, host_arch) = target_os_arch(None)?;
    let (os, arch) = target_os_arch(rust_target)?;
    cargo_build(opts, &info.package, opts.target.as_deref())?;
    let mut built_dir = info.target_dir.clone();
    if let Some(triple) = rust_target {
        built_dir.push(triple);
    }
    let cdylib = built_dir.join(profile_dir).join(native_file_name(&info.lib_name, &os));
    if !cdylib.is_file() {
        return Err(Error::msg(format!("expected {} after cargo build", cdylib.display())));
    }

    // The descriptor is read by loading a library, which only the
    // building machine's own can be.
    let descriptor_lib = if (os.as_str(), arch.as_str()) == (host_os.as_str(), host_arch.as_str()) {
        cdylib.clone()
    } else {
        cargo_build(opts, &info.package, None)?;
        let host_lib = info.target_dir.join(profile_dir).join(native_file_name(&info.lib_name, &host_os));
        if !host_lib.is_file() {
            return Err(Error::msg(format!("expected {} after the host build", host_lib.display())));
        }
        host_lib
    };
    let module = descriptor::read(&descriptor_lib)?;
    generate::validate(&module)?;
    let tc = Toolchain::ensure()?;

    let work = info.target_dir.join("pwrs").join("work").join(&module.name);
    let module_dir = info.target_dir.join("pwrs").join(&module.name);
    if module_dir.exists() {
        std::fs::remove_dir_all(&module_dir).map_err(|e| Error::msg(format!("cannot clear {}: {e}", module_dir.display())))?;
    }
    std::fs::create_dir_all(&module_dir).map_err(|e| Error::msg(format!("cannot create {}: {e}", module_dir.display())))?;

    let runtime_src = write_sources(&work.join("runtime"), RUNTIME_SOURCES)?;
    let bootstrap_src = write_sources(&work.join("bootstrap"), BOOTSTRAP_SOURCES)?;
    let shell_src_path = work.join("shell").join(format!("{}.Shell.cs", module.name));
    std::fs::create_dir_all(work.join("shell")).map_err(|e| Error::msg(format!("cannot create shell dir: {e}")))?;
    let shell_text = generate::shell_source(&module, &info.lib_name);
    std::fs::write(&shell_src_path, &shell_text).map_err(|e| Error::msg(format!("cannot write {}: {e}", shell_src_path.display())))?;
    let mut shell_sources = vec![shell_src_path];
    // Everything the shell's own load context comes to hold: the
    // generated surface, the hand-written C# beside it, and the
    // runtime they are compiled against. The name the assembly takes
    // is a stamp of all of it.
    let mut stamped: Vec<String> = vec![shell_text];
    stamped.extend(RUNTIME_SOURCES.iter().map(|(_, text)| (*text).to_string()));
    let mut hybrid_cmdlets: Vec<generate::HybridCmdlet> = Vec::new();
    let hybrid = opts.manifest_dir.join("src").join("csharp");
    if hybrid.is_dir() {
        let rd = std::fs::read_dir(&hybrid).map_err(|e| Error::msg(format!("cannot read {}: {e}", hybrid.display())))?;
        let mut hybrid_files = Vec::new();
        for entry in rd {
            let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", hybrid.display())))?;
            let p = entry.path();
            if p.extension().is_some_and(|x| x == "cs") {
                hybrid_files.push(p);
            }
        }
        // By name, because a directory listing comes back in whatever
        // order the filesystem keeps. The order reaches the manifest
        // through CmdletsToExport and AliasesToExport, and `cargo pwrs
        // merge` joins two platforms' folders only when their manifests
        // are the same text.
        hybrid_files.sort();
        for p in hybrid_files {
            let source = std::fs::read_to_string(&p).map_err(|e| Error::msg(format!("cannot read {}: {e}", p.display())))?;
            let scan = generate::hybrid_cmdlets(&source);
            if scan.unclaimed > 0 {
                eprintln!(
                    "pwrs: {} carries {} [Cmdlet] attribute(s) that sit on no class declaration; those cmdlets are compiled but not exported",
                    p.display(),
                    scan.unclaimed
                );
            }
            hybrid_cmdlets.extend(scan.cmdlets);
            stamped.push(source);
            shell_sources.push(p);
        }
    }
    let shell_stamp = generate::shell_stamp(&stamped.iter().map(String::as_str).collect::<Vec<_>>());

    let targets: [(&str, &[PathBuf], &[&str]); 2] = [
        ("net10.0", &tc.net10_refs, pwrs_build::toolchain::CORE_DEFINES),
        ("netstandard2.0", &tc.netstandard_refs, pwrs_build::toolchain::NETSTANDARD_DEFINES),
    ];
    let help_xml = crate::help::maml(&module);
    for (tfm, refs, defines) in targets {
        let tfm_dir = module_dir.join(tfm);
        let tfm_work = work.join(tfm);
        let env = pwrs_build::CompileEnv { refs, defines, work: &tfm_work, debug: opts.symbols() };
        let bootstrap_dll = tfm_dir.join("Pwrs.Bootstrap.dll");
        tc.compile(&env, &bootstrap_src, &[], &bootstrap_dll)?;
        let runtime_dll = tfm_dir.join("Pwrs.Runtime.dll");
        tc.compile(&env, &runtime_src, &[], &runtime_dll)?;
        // Named for the source it was built from, so that a changed
        // surface is a new assembly; see `generate::shell_stamp`.
        let shell_stem = format!("{}.Shell.{}", module.name, shell_stamp);
        let shell_dll = tfm_dir.join(format!("{shell_stem}.dll"));
        // The shell asks the bootstrap where its module folder is,
        // because it runs from a staged copy and cannot answer from
        // its own location.
        tc.compile(&env, &shell_sources, &[runtime_dll.clone(), bootstrap_dll], &shell_dll)?;
        let help_dir = tfm_dir.join("en-US");
        std::fs::create_dir_all(&help_dir).map_err(|e| Error::msg(format!("cannot create {}: {e}", help_dir.display())))?;
        std::fs::write(help_dir.join(format!("{shell_stem}.dll-Help.xml")), &help_xml)
            .map_err(|e| Error::msg(format!("cannot write help: {e}")))?;
    }

    // Hand-written C# may use System.Numerics.Vector<T> and spans, which
    // .NET Framework does not carry, so a module with any ships the
    // builds its netstandard2.0 shell was compiled against, and the
    // module's script puts them beside the staged shell, where .NET
    // Framework looks for a shell's dependencies.
    let mut desktop_files: Vec<String> = Vec::new();
    if shell_sources.len() > 1 {
        for dep in &tc.desktop_dependencies {
            let name = match dep.file_name() {
                Some(n) => n.to_string_lossy().into_owned(),
                None => return Err(Error::msg(format!("{} has no file name", dep.display()))),
            };
            copy(dep, &module_dir.join("netstandard2.0").join(&name))?;
            desktop_files.push(name);
        }
    }
    let desktop_dependencies: Vec<&str> = desktop_files.iter().map(String::as_str).collect();

    // Each bundled module is built once this module's runtime assemblies
    // exist to compare against, and laid in before the script that
    // imports it is written.
    let mut bundled: Vec<BundledModule> = Vec::new();
    for entry in &info.bundled_modules {
        bundled.push(bundle_module(opts, &info, entry, &module.name, &module_dir)?);
    }

    let native_dir = module_dir.join("runtimes").join(format!("{os}-{arch}")).join("native");
    let native_dest = native_dir.join(native_file_name(&info.lib_name, &os));
    copy(&cdylib, &native_dest)?;
    // Each helper was built with the library, for the same target and
    // profile, and ships in the same folder.
    for helper in &info.helpers {
        let file = if os == "win" { format!("{helper}.exe") } else { helper.clone() };
        let built = built_dir.join(profile_dir).join(&file);
        if !built.is_file() {
            return Err(Error::msg(format!("expected the helper {} after cargo build", built.display())));
        }
        copy(&built, &native_dir.join(&file))?;
    }

    // What the library was compiled to require, read from the file so a
    // cross-built one answers for its own target; the target's own
    // baseline is no one's choice and is left out.
    let library = std::fs::read(&cdylib).map_err(|e| Error::msg(format!("cannot read {}: {e}", cdylib.display())))?;
    let undeclared_cpu_features = match crate::cpu::required(&library)? {
        Some(required) if !required.is_empty() => {
            let baseline = crate::cpu::target_baseline(rust_target)?;
            crate::cpu::undeclared(&required, &baseline, &info.cpu_features)
        }
        Some(_nothing_required) => Vec::new(),
        None => Vec::new(),
    };
    if !undeclared_cpu_features.is_empty() {
        eprintln!(
            "pwrs: the {os}-{arch} library is compiled to require {}, beyond what its target assumes, and refuses to import on a CPU without them. \
             -C target-cpu=native in RUSTFLAGS or a cargo config does this. Build with RUSTFLAGS='-C target-cpu=x86-64' for a module that loads on any x86-64 CPU, \
             or list them under [package.metadata.pwrs] cpu-features to require them on purpose; cargo pwrs publish refuses them otherwise",
            undeclared_cpu_features.join(", ")
        );
    }

    // Every crate the library links is under a license the module
    // allows, and the notices shipping them owes sit beside the library;
    // the folder's own notices cover what cargo-pwrs puts in it.
    let triple = match rust_target {
        Some(t) => t.to_string(),
        None => crate::cpu::host_triple()?,
    };
    let linked = crate::notices::linked_crates(&info.manifest_path, &triple, &opts.feature_args())?;
    let allowed = crate::notices::allowed(&info.allowed_licenses);
    let refused: Vec<String> = linked
        .iter()
        .filter_map(|c| crate::notices::refusal(c, &allowed).map(|why| format!("{} {}: {why}", c.name, c.version)))
        .collect();
    if !refused.is_empty() {
        return Err(Error::msg(format!(
            "{} links crates it cannot ship under the licenses it allows:\n  {}\nList a license under [package.metadata.pwrs] allowed-licenses to allow it; the defaults are {}",
            info.package,
            refused.join("\n  "),
            crate::notices::DEFAULT_ALLOWED.join(", ")
        )));
    }
    let library = native_file_name(&info.lib_name, &os);
    let supplied = crate::notices::supplied_texts(&info.license_files, &linked)?;
    let (native_text, crates_without_license_text) = crate::notices::native_notices(&linked, &supplied, &module.name, &library)?;
    crate::notices::write(&module_dir.join("runtimes").join(format!("{os}-{arch}")), &native_text)?;
    if !crates_without_license_text.is_empty() {
        eprintln!(
            "pwrs: the library links crates whose source carries no license file: {}. The notices name their licenses but cannot quote them, and cargo pwrs publish refuses the module until each ships one",
            crates_without_license_text.join(", ")
        );
    }
    let shipped_desktop: &[PathBuf] = if shell_sources.len() > 1 { &tc.desktop_dependencies } else { &[] };
    crate::notices::write(&module_dir, &crate::notices::root_notices(&module.name, shipped_desktop)?)?;

    let formats_file = match crate::formats::format_ps1xml(&module) {
        Some(xml) => {
            let name = format!("{}.Format.ps1xml", module.name);
            std::fs::write(module_dir.join(&name), xml).map_err(|e| Error::msg(format!("cannot write format file: {e}")))?;
            Some(name)
        }
        None => None,
    };

    // A published module names its own author and describes itself.
    // The build carries a placeholder so a module under development
    // still imports, and reports what it stood in for; `publish`
    // refuses the placeholder.
    let mut missing_metadata = Vec::new();
    let author = match info.authors.first() {
        Some(a) => a.clone(),
        None => {
            missing_metadata.push("authors");
            eprintln!("pwrs: {} declares no `authors`; the manifest says Author = 'pwrs', which cargo pwrs publish refuses", info.package);
            "pwrs".to_string()
        }
    };
    if info.description.is_none() {
        missing_metadata.push("description");
        eprintln!("pwrs: {} declares no `description`; the manifest describes it by name alone, which cargo pwrs publish refuses", info.package);
    }
    let pkg = generate::Package {
        version: &info.version,
        author: &author,
        description: info.description.as_deref(),
        repository: info.repository.as_deref(),
        keywords: &info.keywords,
        license_uri: info.license_uri.as_deref(),
        release_notes: info.release_notes.as_deref(),
        icon_uri: info.icon_uri.as_deref(),
        company: info.company.as_deref(),
        copyright: info.copyright.as_deref(),
        prerelease: info.prerelease.as_deref(),
        require_license_acceptance: info.require_license_acceptance,
        external_module_dependencies: &info.external_module_dependencies,
        powershell_version: info.powershell_version.as_deref(),
        compatible_ps_editions: &info.compatible_ps_editions,
        powershell_host_name: info.powershell_host_name.as_deref(),
        powershell_host_version: info.powershell_host_version.as_deref(),
        dotnet_framework_version: info.dotnet_framework_version.as_deref(),
        clr_version: info.clr_version.as_deref(),
        processor_architecture: info.processor_architecture.as_deref(),
        help_info_uri: info.help_info_uri.as_deref(),
        default_command_prefix: info.default_command_prefix.as_deref(),
        required_modules: &info.required_modules,
        required_assemblies: &info.required_assemblies,
        scripts_to_process: &info.scripts_to_process,
        types_to_process: &info.types_to_process,
        nested_modules: &info.nested_modules,
        dsc_resources_to_export: &info.dsc_resources_to_export,
        module_list: &info.module_list,
        file_list: &info.file_list,
    };
    std::fs::write(module_dir.join(format!("{}.psd1", module.name)), generate::manifest(&module, &pkg, formats_file.as_deref(), &hybrid_cmdlets))
        .map_err(|e| Error::msg(format!("cannot write manifest: {e}")))?;
    let imports: Vec<generate::Bundled> =
        bundled.iter().map(|b| generate::Bundled { name: &b.name, warn_on_failure: b.on_import_failure == ImportFailure::Warn }).collect();
    std::fs::write(module_dir.join(format!("{}.psm1", module.name)), generate::bootstrap_psm1(&module, &hybrid_cmdlets, &desktop_dependencies, &imports))
        .map_err(|e| Error::msg(format!("cannot write psm1: {e}")))?;

    eprintln!("pwrs: module folder {}", module_dir.display());
    Ok(Built { module_dir, module, bundled, missing_metadata, undeclared_cpu_features, test_cpu_tiers: info.test_cpu_tiers, crates_without_license_text })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `bundled-modules` entry is a crate directory as a string, which
    /// stops on a failed import and builds the defaults, or a table with
    /// `path` and the keys it takes; any other shape, key or value is
    /// refused, named.
    #[test]
    fn bundled_entries_are_strings_or_tables_of_the_keys_they_take() {
        let root = std::env::temp_dir().join(format!("pwrs-bundled-entries-{}", std::process::id()));
        let outer = root.join("outer");
        for dir in [&outer, &root.join("calc"), &root.join("flynnel")] {
            std::fs::create_dir_all(dir).expect("create a crate folder");
            std::fs::write(dir.join("Cargo.toml"), "[package]\n").expect("write a Cargo.toml");
        }
        let parse = |json: &str| bundled_entries("outer", &outer, &serde_json::from_str(json).expect("json"));

        let plain = parse(r#"["../calc"]"#).expect("a string entry");
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0].on_import_failure, ImportFailure::Stop);
        assert!(plain[0].default_features && plain[0].features.is_empty() && plain[0].cpu_features.is_empty() && plain[0].license_files.is_empty());
        assert!(plain[0].dir.ends_with("calc"), "{:?}", plain[0].dir);

        let table = parse(
            r#"["../calc", { "path": "../flynnel", "on-import-failure": "warn", "features": ["cpu"], "default-features": false,
                "license-files": { "wasmparser@0.239.0": "licenses/wasmparser.txt" }, "cpu-features": ["avx2"] }]"#,
        )
        .expect("a table entry");
        assert_eq!(table.len(), 2);
        let flynnel = &table[1];
        assert!(flynnel.dir.ends_with("flynnel"), "{:?}", flynnel.dir);
        assert_eq!(flynnel.on_import_failure, ImportFailure::Warn);
        assert_eq!(flynnel.features, ["cpu"]);
        assert!(!flynnel.default_features);
        assert_eq!(flynnel.cpu_features, ["avx2"]);
        assert_eq!(flynnel.license_files.len(), 1);
        assert_eq!(flynnel.license_files[0].name, "wasmparser");
        assert_eq!(flynnel.license_files[0].path, outer.join("licenses/wasmparser.txt"));
        assert_eq!(flynnel.license_files[0].source, "the bundled-modules entry ../flynnel of outer, license-files");
        assert!(parse(r#"[{ "path": "../calc", "on-import-failure": "stop" }]"#).expect("stop by name")[0].on_import_failure == ImportFailure::Stop);
        assert!(parse("null").expect("no list").is_empty());

        let refused = |json: &str, why: &str| match parse(json) {
            Ok(entries) => panic!("{json} was accepted: {entries:?}"),
            Err(e) => assert!(e.to_string().contains(why), "{json}: {e}"),
        };
        refused(r#"[{ "path": "../calc", "optional": true }]"#, "the bundled-modules entry ../calc sets `optional`, which is not one of path, on-import-failure");
        refused(r#"[{ "path": "../calc", "on-import-failure": "ignore" }]"#, "sets on-import-failure to \"ignore\"; it takes \"stop\" or \"warn\"");
        refused(r#"[{ "path": "../calc", "features": "cpu" }]"#, "sets `features` to \"cpu\"; it takes a list of strings");
        refused(r#"[{ "path": "../calc", "cpu-features": [2] }]"#, "lists 2 in `cpu-features`, which takes strings");
        refused(r#"[{ "path": "../calc", "default-features": "no" }]"#, "sets default-features to \"no\"; it takes true or false");
        refused(r#"[{ "path": "../calc", "license-files": ["x"] }]"#, "the bundled-modules entry ../calc of outer, license-files is a table");
        refused(r#"[{ "on-import-failure": "warn" }]"#, "a bundled-modules table has no `path`");
        refused(r#"[{ "path": 3 }]"#, "a bundled-modules table's `path` is 3");
        refused(r#"[7]"#, "bundled-modules holds 7, which is neither a crate directory nor a table with `path`");
        refused(r#""../calc""#, "bundled-modules is a list of crate directories");
        refused(r#"["../missing"]"#, "bundled-modules names ../missing, and");
        std::fs::remove_dir_all(&root).expect("remove the scratch folder");
    }

    #[test]
    fn a_helper_must_name_one_of_the_packages_bins() {
        let names = |xs: &[&str]| -> Vec<String> { xs.iter().map(|s| s.to_string()).collect() };
        assert!(check_helpers("demo", &[], &[]).is_ok());
        assert!(check_helpers("demo", &names(&["guest-host"]), &names(&["guest-host", "tooling"])).is_ok());
        match check_helpers("demo", &names(&["guest-host", "missing"]), &names(&["guest-host"])) {
            Ok(()) => panic!("a helper naming no bin target was accepted"),
            Err(e) => assert!(e.to_string().contains("helpers names missing, which is not a [[bin]] target of demo"), "{e}"),
        }
    }

    #[test]
    fn a_glibc_version_after_a_linux_gnu_triple_is_split_off() {
        fn parts(t: &str) -> (&str, Option<&str>) {
            match split_glibc(t) {
                Ok(parts) => parts,
                Err(e) => panic!("{t} was refused: {e}"),
            }
        }
        assert_eq!(parts("x86_64-unknown-linux-gnu.2.35"), ("x86_64-unknown-linux-gnu", Some("2.35")));
        assert_eq!(parts("aarch64-unknown-linux-gnu.2.17"), ("aarch64-unknown-linux-gnu", Some("2.17")));
        assert_eq!(parts("x86_64-unknown-linux-gnu"), ("x86_64-unknown-linux-gnu", None));
        assert_eq!(parts("x86_64-pc-windows-msvc"), ("x86_64-pc-windows-msvc", None));
        for bad in ["x86_64-pc-windows-msvc.2.35", "x86_64-unknown-linux-musl.1.2", "x86_64-unknown-linux-gnu.2.", "x86_64-unknown-linux-gnu.two"] {
            match split_glibc(bad) {
                Ok(parts) => panic!("{bad} was accepted as {parts:?}"),
                Err(e) => assert!(e.to_string().contains("only a GNU/Linux triple takes a glibc version"), "{bad}: {e}"),
            }
        }
    }

    #[test]
    fn triples_map_to_runtime_identifiers() {
        let rid = |t: &str| target_os_arch(Some(t)).map(|(os, arch)| format!("{os}-{arch}"));
        assert_eq!(rid("x86_64-pc-windows-msvc").expect("windows"), "win-x64");
        assert_eq!(rid("aarch64-pc-windows-msvc").expect("windows arm"), "win-arm64");
        assert_eq!(rid("i686-pc-windows-msvc").expect("windows x86"), "win-x86");
        assert_eq!(rid("x86_64-unknown-linux-gnu").expect("linux"), "linux-x64");
        assert_eq!(rid("aarch64-unknown-linux-musl").expect("linux arm"), "linux-arm64");
        assert_eq!(rid("aarch64-apple-darwin").expect("mac"), "osx-arm64");
        assert_eq!(rid("x86_64-unknown-freebsd").expect("freebsd"), "freebsd-x64");
        rid("riscv64gc-unknown-linux-gnu").expect_err("no rid for riscv");
        rid("x86_64-unknown-illumos").expect_err("no rid for illumos");
    }

    #[test]
    fn library_names_follow_the_operating_system() {
        assert_eq!(native_file_name("hello", "win"), "hello.dll");
        assert_eq!(native_file_name("hello", "osx"), "libhello.dylib");
        assert_eq!(native_file_name("hello", "linux"), "libhello.so");
        assert_eq!(native_file_name("hello", "freebsd"), "libhello.so");
    }
}
