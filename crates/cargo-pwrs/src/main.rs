//! `cargo pwrs build` lays out a module folder:
//!
//! ```text
//! <Name>/
//!   <Name>.psd1                   manifest
//!   <Name>.psm1                   bootstrap: pick the TFM folder, load through Pwrs.Bootstrap
//!   <Name>.Format.ps1xml          default views for output classes
//!   net10.0/Pwrs.Bootstrap.dll    frozen identity; per-module AssemblyLoadContext
//!   net10.0/Pwrs.Runtime.dll      support assembly for PowerShell 7
//!   net10.0/<Name>.Shell.<stamp>.dll          generated cmdlet classes
//!   net10.0/en-US/<Name>.Shell.<stamp>.dll-Help.xml
//!   netstandard2.0/...            the same for Windows PowerShell 5.1
//!   runtimes/<rid>/native/        the Rust cdylib per runtime identifier, its
//!                                 helpers and the files native-files ships
//!   runtimes/<rid>/pwrs-build.json  what that build found that publish refuses
//! ```
//!
//! The C# is compiled by a csc fetched from NuGet and run on the
//! runtime pwsh ships, inside a pwsh process.
//!
//! `<stamp>` is a hash of the managed source the shell is built from,
//! so a changed surface gets an assembly identity of its own. That is
//! what lets a rebuilt module take its cmdlets over in a session that
//! has already run the previous build.

mod bases;
mod build;
mod build_record;
mod cfg;
mod cpu;
mod descriptor;
mod formats;
mod generate;
mod help;
mod members;
mod merge;
mod native_files;
mod notices;
mod pdb;
mod records;
mod scaffold;
mod signing;
mod surface;
mod test;

use std::path::{Path, PathBuf};

pub use pwrs_build::{pwsh, toolchain, Error};

const PUBLISH_PS1: &str = include_str!("../scripts/publish.ps1");

fn usage() -> ! {
    eprintln!(
        "usage: cargo pwrs <build|test|publish|sign|new|toolchain> [--release] [--target <triple>] [--features <a,b>] [--all-features] [--debug-symbols] [--package <name>] [--manifest-dir <dir>] [--dry-run] [--cpu-tiers <x86-64,x86-64-v3,...>] [--pwrs <dependency spec>] [<new dir>]\n       cargo pwrs publish --folder <module dir> [--release] [--dry-run]\n       cargo pwrs sign [--folder <module dir>] [--pfx <file> | --pem <'[\"certs\", \"key\"]'> | --store <thumbprint> | --command <'[\"program\", \"args\"]'> | --pkcs11 <'{{ library = \"...\", token = \"...\", key = \"...\" }}'>] [--password-env <name>] [--certificate <file>] [--timestamp-url <url>] [--hash <sha256|sha384|sha512>] [--catalog] [--signer <pwrs|openauthenticode>]\n       cargo pwrs merge <into module dir> <from module dir>..."
    );
    std::process::exit(2);
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("pwrs") {
        args.remove(0);
    }
    let sub = match args.first() {
        Some(s) => s.clone(),
        None => usage(),
    };
    let mut release = false;
    let mut dry_run = false;
    let mut package = None;
    let mut target = None;
    let mut features: Vec<String> = Vec::new();
    let mut all_features = false;
    let mut debug_symbols = false;
    let mut cpu_tiers: Option<Vec<String>> = None;
    let mut folder: Option<PathBuf> = None;
    let mut given: Vec<String> = Vec::new();
    let mut signing_flags: Vec<(String, serde_json::Value)> = Vec::new();
    let mut positional = Vec::new();
    // The registry name differs from the library name, so a scaffolded
    // manifest has to name the package or it resolves to an unrelated
    // crate named `pwrs` on crates.io. The version is this tool's
    // own, which is the library's too: the workspace publishes them
    // together, and reading it here is what stops a scaffold from
    // naming a version the tool predates.
    let mut pwrs_dependency = format!("{{ package = \"PoWerRuSt\", version = \"{}\" }}", env!("CARGO_PKG_VERSION"));
    let mut manifest_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cargo-pwrs: cannot read the current directory: {e}");
            std::process::exit(2);
        }
    };
    let mut i = 1;
    while i < args.len() {
        if args[i].starts_with('-') {
            given.push(args[i].clone());
        }
        match args[i].as_str() {
            "--release" => release = true,
            "--dry-run" => dry_run = true,
            "--folder" => {
                i += 1;
                match args.get(i) {
                    Some(d) => folder = Some(PathBuf::from(d)),
                    None => usage(),
                }
            }
            "--package" | "-p" => {
                i += 1;
                package = args.get(i).cloned();
                if package.is_none() {
                    usage();
                }
            }
            "--manifest-dir" => {
                i += 1;
                match args.get(i) {
                    Some(d) => manifest_dir = PathBuf::from(d),
                    None => usage(),
                }
            }
            "--target" => {
                i += 1;
                target = args.get(i).cloned();
                if target.is_none() {
                    usage();
                }
            }
            "--features" | "-F" => {
                i += 1;
                match args.get(i) {
                    Some(list) => features.extend(list.split([',', ' ']).filter(|f| !f.is_empty()).map(String::from)),
                    None => usage(),
                }
            }
            "--all-features" => all_features = true,
            "--debug-symbols" => debug_symbols = true,
            "--cpu-tiers" => {
                i += 1;
                match args.get(i) {
                    Some(list) => cpu_tiers = Some(list.split([',', ' ']).filter(|t| !t.is_empty()).map(String::from).collect()),
                    None => usage(),
                }
            }
            "--pwrs" => {
                i += 1;
                match args.get(i) {
                    Some(d) => pwrs_dependency = d.clone(),
                    None => usage(),
                }
            }
            other if other.strip_prefix("--").is_some_and(|s| signing::SETTINGS.contains(&s)) => {
                let setting = &other[2..];
                let text = if setting == "catalog" {
                    None
                } else {
                    i += 1;
                    match args.get(i) {
                        Some(v) => Some(v.as_str()),
                        None => usage(),
                    }
                };
                match signing::flag_value(setting, text) {
                    Ok(value) => signing_flags.push((setting.to_string(), value)),
                    Err(e) => {
                        eprintln!("cargo-pwrs: {e}");
                        std::process::exit(2);
                    }
                }
            }
            other if other.starts_with('-') => {
                eprintln!("cargo-pwrs: unknown argument {other}");
                usage();
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    let opts = build::BuildOptions {
        release,
        package,
        manifest_dir,
        target,
        features,
        all_features,
        no_default_features: false,
        supplied_license_files: Vec::new(),
        supplied_cpu_features: Vec::new(),
        debug_symbols,
        target_dir: None,
        locked: false,
    };
    if folder.is_some() && sub != "publish" && sub != "sign" {
        eprintln!("cargo-pwrs: --folder names the module folder publish packages or sign signs; {sub} takes none");
        usage();
    }
    if !signing_flags.is_empty() && sub != "publish" && sub != "sign" {
        eprintln!("cargo-pwrs: --{} is a signing setting, which sign and publish take and {sub} does not", signing_flags[0].0);
        usage();
    }
    let unused = if sub == "publish" && folder.is_some() { Ok(()) } else { unused_flags(&sub, folder.is_some(), &given) };
    if let Err(e) = unused {
        eprintln!("cargo-pwrs: {e}");
        usage();
    }
    let result = match sub.as_str() {
        "build" => build::build(&opts).map(|_| ()),
        "test" => test::run(&opts, cpu_tiers.as_deref()),
        "publish" => match &folder {
            Some(dir) => folder_flags(&given).and_then(|()| publish_folder(dir, release, dry_run)),
            None => publish(&opts, dry_run, &signing_flags),
        },
        "sign" => sign(&opts, folder.as_deref(), &signing_flags),
        "new" => match positional.first() {
            Some(dir) => scaffold::new_module(&PathBuf::from(dir), &pwrs_dependency),
            None => usage(),
        },
        "merge" => match positional.split_first() {
            Some((into, from)) if !from.is_empty() => {
                let from: Vec<PathBuf> = from.iter().map(PathBuf::from).collect();
                merge::merge(&PathBuf::from(into), &from)
            }
            Some(_incomplete) => usage(),
            None => usage(),
        },
        "toolchain" => toolchain::Toolchain::ensure().and_then(|tc| {
            let pester = tc.pester()?;
            eprintln!("pwrs: toolchain at {}", tc.root.display());
            eprintln!("pwrs: csc at {}", tc.csc_dir.display());
            eprintln!("pwrs: Pester at {}", pester.display());
            eprintln!("pwrs: pwsh at {}", tc.pshome.display());
            Ok(())
        }),
        _unknown => usage(),
    };
    if let Err(e) = result {
        eprintln!("cargo-pwrs: {e}");
        std::process::exit(1);
    }
}

/// The signing settings of the crate `opts` names with `flags` over
/// them, read before anything builds so a setting out of shape stops the
/// command at once, and the work folder the signer is compiled in.
fn signing_settings(opts: &build::BuildOptions, flags: &[(String, serde_json::Value)]) -> Result<(Option<signing::Settings>, PathBuf), Error> {
    let crate_signing = build::signing_of(opts)?;
    let here = std::env::current_dir().map_err(|e| Error::msg(format!("cannot read the current directory: {e}")))?;
    Ok((signing::settings(&crate_signing.table, &crate_signing.crate_dir, flags, &here)?, crate_signing.work))
}

fn publish(opts: &build::BuildOptions, dry_run: bool, signing_flags: &[(String, serde_json::Value)]) -> Result<(), Error> {
    let (signing, work) = signing_settings(opts, signing_flags)?;
    let built = build::build(opts)?;
    let missing: Vec<String> = built.missing_metadata.iter().map(|m| m.to_string()).collect();
    metadata_refusal(&built.module.name, &missing)?;
    undeclared_cpu_refusal(&built.module.name, &library(None), &built.undeclared_cpu_features)?;
    license_text_refusal(&built.module.name, &library(None), &built.crates_without_license_text)?;
    for bundled in &built.bundled {
        bundled_cpu_refusal(&built.module.name, &bundled_library(&bundled.name, None), &bundled.name, &bundled.undeclared_cpu_features)?;
        bundled_license_text_refusal(&built.module.name, &bundled_library(&bundled.name, None), &bundled.name, &bundled.crates_without_license_text)?;
    }
    for bundled in &built.bundled {
        eprintln!("pwrs: the package includes the bundled module {}", bundled.name);
    }
    if let Some(settings) = &signing {
        signing::sign(&built.module_dir, settings, &work)?;
    }
    package(&built.module_dir, &built.module.name, dry_run)
}

/// Signs the module folder `folder`, or without one the folder a build
/// of the crate makes, with the crate's signing settings and `flags`
/// over them.
fn sign(opts: &build::BuildOptions, folder: Option<&Path>, flags: &[(String, serde_json::Value)]) -> Result<(), Error> {
    let (signing, work) = signing_settings(opts, flags)?;
    let Some(settings) = signing else {
        return Err(Error::msg(
            "there is nothing to sign with: the crate has no [package.metadata.pwrs.signing], and no key flag such as --pfx is given",
        ));
    };
    let dir = match folder {
        Some(dir) => dir.to_path_buf(),
        None => build::build(opts)?.module_dir,
    };
    signing::sign(&dir, &settings, &work)
}

/// Publishes the module folder `dir` as it stands, such as one `merge`
/// joined from several platforms' builds: no build runs, and what any
/// runtime's build recorded that `publish` refuses is refused, naming the
/// runtime, as is a runtime whose build left no record and, with
/// `release`, one that is not a release build.
fn publish_folder(dir: &Path, release: bool, dry_run: bool) -> Result<(), Error> {
    let (_, name) = merge::manifest_of(dir)?;
    folder_refusals(dir, &name, release)?;
    for bundled in merge::bundled_in(dir)? {
        eprintln!("pwrs: the package includes the bundled module {bundled}");
    }
    package(dir, &name, dry_run)
}

/// Validates and publishes the module folder `module_dir` of module
/// `name`, or with `dry_run` packages it in `publish/<name>` beside the
/// folder.
fn package(module_dir: &Path, name: &str, dry_run: bool) -> Result<(), Error> {
    let pwrs_dir = match module_dir.parent() {
        Some(p) => p.to_path_buf(),
        None => return Err(Error::msg("module dir has no parent")),
    };
    let script = pwsh::materialize_script(&pwrs_dir.join("work"), "publish.ps1", PUBLISH_PS1)?;
    let out_dir = pwrs_dir.join("publish").join(name);
    let mut args = vec![
        "-ModulePath".to_string(),
        module_dir.display().to_string(),
        "-OutDir".to_string(),
        out_dir.display().to_string(),
    ];
    if dry_run {
        args.push("-DryRun".to_string());
    }
    let out = pwsh::run_pwsh_script(&script, &args)?;
    print!("{out}");
    Ok(())
}

/// Refuses the flags `publish --folder` does not use: a folder is
/// packaged as its builds left it, so an option that changes a build has
/// no build to change, and a signing setting has nothing to sign, since
/// `publish --folder` signs nothing. `--release` requires every runtime
/// to be a release build.
fn folder_flags(given: &[String]) -> Result<(), Error> {
    let (settings, shaping): (Vec<&str>, Vec<&str>) = given
        .iter()
        .map(String::as_str)
        .filter(|f| !matches!(*f, "--folder" | "--dry-run" | "--release"))
        .partition(|f| f.strip_prefix("--").is_some_and(|s| signing::SETTINGS.contains(&s)));
    let mut refused = Vec::new();
    if !shaping.is_empty() {
        refused.push(format!(
            "publish --folder packages a module folder as its builds left it, so it takes no {}; give them to the build of each platform, then merge the folders",
            shaping.join(", ")
        ));
    }
    if !settings.is_empty() {
        refused.push(format!(
            "publish --folder signs nothing, so it takes no {}; sign the folder first with cargo pwrs sign --folder <module dir> and the same flags",
            settings.join(", ")
        ));
    }
    if refused.is_empty() { Ok(()) } else { Err(Error::msg(refused.join("\n"))) }
}

/// Refuses each flag in `given` that the command `sub` does not use, so
/// no flag is taken and then ignored. A command that builds takes the
/// build's options, `test` also `--cpu-tiers`, `publish` also
/// `--dry-run`, and `new` only `--pwrs`. `sign --folder` builds nothing,
/// so it takes only `--manifest-dir` and `--package`, which find the
/// crate whose signing settings it reads. The signing settings are
/// checked before this, and `publish --folder` by `folder_flags`.
fn unused_flags(sub: &str, folder: bool, given: &[String]) -> Result<(), Error> {
    const BUILD: [&str; 9] = ["--release", "--target", "--features", "-F", "--all-features", "--debug-symbols", "--package", "-p", "--manifest-dir"];
    let taken: Vec<&str> = match (sub, folder) {
        ("build", _) => BUILD.to_vec(),
        ("test", _) => BUILD.iter().copied().chain(["--cpu-tiers"]).collect(),
        ("publish", _) => BUILD.iter().copied().chain(["--dry-run"]).collect(),
        ("sign", false) => BUILD.to_vec(),
        ("sign", true) => vec!["--folder", "--manifest-dir", "--package", "-p"],
        ("new", _) => vec!["--pwrs"],
        ("merge" | "toolchain", _) => Vec::new(),
        _unknown => return Ok(()),
    };
    let signs = matches!(sub, "sign" | "publish");
    let unused: Vec<&str> = given
        .iter()
        .map(String::as_str)
        .filter(|f| !taken.contains(f) && !(signs && f.strip_prefix("--").is_some_and(|s| signing::SETTINGS.contains(&s))))
        .collect();
    if unused.is_empty() {
        return Ok(());
    }
    let label = if folder { format!("{sub} --folder") } else { sub.to_string() };
    let hint = if unused.contains(&"--dry-run") {
        "; only publish has a dry run"
    } else if sub == "sign" && folder {
        "; it signs the folder as it is and builds nothing"
    } else {
        ""
    };
    Err(Error::msg(format!("{label} takes no {}{hint}", unused.join(", "))))
}

/// Every refusal `publish --folder` makes of module folder `dir`, module
/// `name`, at once: a folder with no runtime; a runtime whose build left
/// no record; the metadata any build wrote a placeholder for; each
/// runtime's undeclared extensions and crates without license text, and
/// the same of each bundled module's runtimes; and with `release`, any
/// runtime that is not a release build.
fn folder_refusals(dir: &Path, name: &str, release: bool) -> Result<(), Error> {
    let own = build_record::runtimes(dir)?;
    if own.is_empty() {
        return Err(Error::msg(format!("{name} cannot be published: {} has no runtime folder; build it with cargo pwrs build", dir.display())));
    }
    let mut runtimes: Vec<(Option<String>, build_record::Runtime)> = own.into_iter().map(|r| (None, r)).collect();
    for bundled in merge::bundled_in(dir)? {
        runtimes.extend(build_record::runtimes(&dir.join(&bundled))?.into_iter().map(|r| (Some(bundled.clone()), r)));
    }
    let mut refused: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for (bundled, runtime) in &runtimes {
        let place = match bundled {
            Some(b) => format!("{b}/runtimes/{}", runtime.rid),
            None => format!("runtimes/{}", runtime.rid),
        };
        let Some(record) = &runtime.record else {
            refused.push(format!(
                "{name} cannot be published: {place} has no {}, so what its build found cannot be checked. Build that platform with cargo-pwrs {} or later and merge it again",
                build_record::FILE,
                env!("CARGO_PKG_VERSION")
            ));
            continue;
        };
        if release && !record.release {
            refused.push(format!("{name} cannot be published: {place} is a debug build, and --release requires release builds. Build that platform with --release and merge it again"));
        }
        let outcomes = match bundled {
            None => {
                for m in &record.missing_metadata {
                    if !missing.contains(m) {
                        missing.push(m.clone());
                    }
                }
                let library = library(Some(&runtime.rid));
                [undeclared_cpu_refusal(name, &library, &record.undeclared_cpu_features), license_text_refusal(name, &library, &record.crates_without_license_text)]
            }
            Some(b) => {
                let library = bundled_library(b, Some(&runtime.rid));
                [bundled_cpu_refusal(name, &library, b, &record.undeclared_cpu_features), bundled_license_text_refusal(name, &library, b, &record.crates_without_license_text)]
            }
        };
        for outcome in outcomes {
            if let Err(e) = outcome {
                refused.push(e.to_string());
            }
        }
    }
    if let Err(e) = metadata_refusal(name, &missing) {
        refused.insert(0, e.to_string());
    }
    if refused.is_empty() { Ok(()) } else { Err(Error::msg(refused.join("\n"))) }
}

/// The library a refusal names: the one this build made, or one runtime's.
fn library(rid: Option<&str>) -> String {
    match rid {
        Some(rid) => format!("its {rid} native library"),
        None => String::from("its native library"),
    }
}

/// A bundled module's library, as `library` names the module's own.
fn bundled_library(bundled: &str, rid: Option<&str>) -> String {
    match rid {
        Some(rid) => format!("the {rid} native library of its bundled module {bundled}"),
        None => format!("the native library of its bundled module {bundled}"),
    }
}

/// Refuses a module whose manifest has a placeholder for `missing`, the
/// `[package]` keys its Cargo.toml does not declare.
fn metadata_refusal(module: &str, missing: &[String]) -> Result<(), Error> {
    if missing.is_empty() {
        return Ok(());
    }
    let set = if missing.iter().any(|m| m == "description") {
        "Set them under [package], or the description under [package.metadata.pwrs], and build again."
    } else {
        "Set them under [package] and build again."
    };
    Err(Error::msg(format!(
        "{module} cannot be published: its Cargo.toml declares no {}. A module on a gallery names its own author and has its own description, so pwrs does not publish one with a placeholder. {set}",
        missing.join(" and "),
    )))
}

/// Refuses a module whose `library` requires extensions beyond its
/// target's baseline that its manifest does not list under `cpu-features`.
fn undeclared_cpu_refusal(module: &str, library: &str, undeclared: &[String]) -> Result<(), Error> {
    if undeclared.is_empty() {
        return Ok(());
    }
    Err(Error::msg(format!(
        "{module} cannot be published: {library} is compiled to require {}, and it fails to import on a CPU without them. Build with RUSTFLAGS='-C target-cpu=x86-64', or list them under [package.metadata.pwrs] cpu-features if every CPU the module is meant for has them.",
        undeclared.join(", "),
    )))
}

/// Refuses a module whose `library` links crates whose source has no
/// license file and for which the module supplies none, since its notices
/// could name their licenses but not quote them.
fn license_text_refusal(module: &str, library: &str, without_text: &[String]) -> Result<(), Error> {
    if without_text.is_empty() {
        return Ok(());
    }
    Err(Error::msg(format!(
        "{module} cannot be published: {library} links {}, whose source has no license file, so the notices beside the library name their licenses but cannot quote them. Supply each one's text under [package.metadata.pwrs] license-files, or depend on a release of each that ships it.",
        without_text.join(", "),
    )))
}

/// Refuses a module whose bundled module's `library` requires extensions
/// beyond its target's baseline that neither the bundled crate's
/// `cpu-features` nor its `bundled-modules` entry lists.
fn bundled_cpu_refusal(module: &str, library: &str, bundled: &str, undeclared: &[String]) -> Result<(), Error> {
    if undeclared.is_empty() {
        return Ok(());
    }
    Err(Error::msg(format!(
        "{module} cannot be published: {library} is compiled to require {}, and it fails to import on a CPU without them. Build with RUSTFLAGS='-C target-cpu=x86-64', or list them under the cpu-features of {bundled}'s bundled-modules entry if every CPU the module is meant for has them.",
        undeclared.join(", "),
    )))
}

/// Refuses a module whose bundled module's `library` links crates whose
/// source has no license file and for which neither the bundled crate
/// nor its `bundled-modules` entry supplies one.
fn bundled_license_text_refusal(module: &str, library: &str, bundled: &str, without_text: &[String]) -> Result<(), Error> {
    if without_text.is_empty() {
        return Ok(());
    }
    Err(Error::msg(format!(
        "{module} cannot be published: {library} links {}, whose source has no license file, so the notices beside that library name their licenses but cannot quote them. Supply each file under the license-files of {bundled}'s bundled-modules entry, or build it without the crates that need them.",
        without_text.join(", "),
    )))
}

#[cfg(test)]
mod tests {
    use super::{bundled_cpu_refusal, bundled_library, bundled_license_text_refusal, folder_flags, folder_refusals, library, license_text_refusal, metadata_refusal, undeclared_cpu_refusal, unused_flags};
    use crate::build_record::BuildRecord;
    use std::path::{Path, PathBuf};

    fn refusal(outcome: Result<(), crate::Error>) -> String {
        match outcome {
            Ok(()) => panic!("what should be refused was accepted"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn a_bundled_librarys_gaps_are_refused_naming_it() {
        let calc = bundled_library("Calc", None);
        assert!(bundled_license_text_refusal("Hello", &calc, "Calc", &[]).is_ok());
        assert!(bundled_cpu_refusal("Hello", &calc, "Calc", &[]).is_ok());
        let texts = refusal(bundled_license_text_refusal("Hello", &calc, "Calc", &["wasmparser 0.239.0".to_string()]));
        assert!(texts.starts_with("Hello cannot be published: the native library of its bundled module Calc links wasmparser 0.239.0"), "{texts}");
        assert!(texts.contains("license-files of Calc's bundled-modules entry"), "{texts}");
        let cpu = refusal(bundled_cpu_refusal("Hello", &calc, "Calc", &["avx2".to_string()]));
        assert!(cpu.starts_with("Hello cannot be published: the native library of its bundled module Calc is compiled to require avx2,"), "{cpu}");
        assert!(cpu.contains("cpu-features of Calc's bundled-modules entry"), "{cpu}");
        let runtime = refusal(bundled_cpu_refusal("Hello", &bundled_library("Calc", Some("osx-arm64")), "Calc", &["avx2".to_string()]));
        assert!(runtime.starts_with("Hello cannot be published: the osx-arm64 native library of its bundled module Calc is compiled to require avx2,"), "{runtime}");
    }

    #[test]
    fn a_crate_without_a_license_text_is_refused() {
        assert!(license_text_refusal("Hello", &library(None), &[]).is_ok());
        let refused = refusal(license_text_refusal("Hello", &library(None), &["PoWerRuSt 0.2.1".to_string()]));
        assert!(refused.starts_with("Hello cannot be published: its native library links PoWerRuSt 0.2.1, whose source has no license file"), "{refused}");
        assert!(refused.contains("[package.metadata.pwrs] license-files"), "{refused}");
    }

    #[test]
    fn only_undeclared_extensions_are_refused() {
        assert!(undeclared_cpu_refusal("Hello", &library(None), &[]).is_ok());
        let refused = refusal(undeclared_cpu_refusal("Hello", &library(None), &["avx512f".to_string(), "gfni".to_string()]));
        assert!(refused.starts_with("Hello cannot be published: its native library is compiled to require avx512f, gfni,"), "{refused}");
        assert!(metadata_refusal("Hello", &[]).is_ok());
        assert!(refusal(metadata_refusal("Hello", &["authors".to_string(), "description".to_string()])).contains("declares no authors and description"));
    }

    /// A module folder `Demo` in a fresh scratch folder named for `test`,
    /// with a runtime folder for each rid and its record when one is given.
    fn folder(test: &str, runtimes: &[(&str, Option<BuildRecord>)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cargo-pwrs-publish-folder-{test}-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("clear the scratch folder");
        }
        let dir = root.join("Demo");
        std::fs::create_dir_all(&dir).expect("make the module folder");
        std::fs::write(dir.join("Demo.psd1"), "@{ ModuleVersion = '0.1.0' }").expect("write the manifest");
        lay_runtimes(&dir, runtimes);
        dir
    }

    fn lay_runtimes(dir: &Path, runtimes: &[(&str, Option<BuildRecord>)]) {
        for (rid, record) in runtimes {
            let runtime = dir.join("runtimes").join(rid);
            std::fs::create_dir_all(runtime.join("native")).expect("make the runtime folder");
            if let Some(record) = record {
                record.write(&runtime).expect("write the record");
            }
        }
    }

    fn clean(release: bool) -> BuildRecord {
        BuildRecord { built_by: String::from("cargo-pwrs 0.7.0"), release, ..BuildRecord::default() }
    }

    #[test]
    fn a_merged_folder_is_refused_for_every_runtime_s_gaps_at_once() {
        let unquoted = BuildRecord { crates_without_license_text: vec![String::from("spirv 0.4.0+sdk-1.4.341.0")], ..clean(true) };
        let placeholder = BuildRecord { missing_metadata: vec![String::from("description")], ..clean(true) };
        let dir = folder("gaps", &[("win-x64", Some(placeholder)), ("osx-arm64", Some(unquoted)), ("linux-x64", None)]);
        let calc = dir.join("Calc");
        std::fs::create_dir_all(&calc).expect("make the bundled module");
        std::fs::write(calc.join("Calc.psd1"), "@{ ModuleVersion = '0.1.0' }").expect("write the bundled manifest");
        lay_runtimes(&calc, &[("win-x64", Some(BuildRecord { undeclared_cpu_features: vec![String::from("avx2")], ..clean(false) }))]);
        let refused = refusal(folder_refusals(&dir, "Demo", true));
        let lines: Vec<&str> = refused.lines().collect();
        assert_eq!(lines.len(), 5, "{refused}");
        assert!(lines[0].starts_with("Demo cannot be published: its Cargo.toml declares no description"), "the metadata comes first: {refused}");
        assert!(lines[1].starts_with("Demo cannot be published: runtimes/linux-x64 has no pwrs-build.json"), "{refused}");
        assert!(lines[2].starts_with("Demo cannot be published: its osx-arm64 native library links spirv 0.4.0+sdk-1.4.341.0"), "{refused}");
        assert!(lines[3].starts_with("Demo cannot be published: Calc/runtimes/win-x64 is a debug build, and --release requires release builds"), "{refused}");
        assert!(lines[4].starts_with("Demo cannot be published: the win-x64 native library of its bundled module Calc is compiled to require avx2"), "{refused}");
        assert_eq!(refusal(folder_refusals(&dir, "Demo", false)).lines().count(), 4, "without --release a debug build is taken");
        std::fs::remove_dir_all(dir.parent().expect("the scratch folder")).expect("remove the scratch folder");
    }

    #[test]
    fn a_merged_folder_whose_builds_found_nothing_is_taken_and_one_with_no_runtime_is_not() {
        let dir = folder("clean", &[("win-x64", Some(clean(true))), ("osx-arm64", Some(clean(true)))]);
        assert!(folder_refusals(&dir, "Demo", true).is_ok());
        let empty = folder("empty", &[]);
        std::fs::create_dir_all(empty.join("runtimes")).expect("make an empty runtimes folder");
        assert!(refusal(folder_refusals(&empty, "Demo", false)).contains("has no runtime folder"));
        for d in [dir, empty] {
            std::fs::remove_dir_all(d.parent().expect("the scratch folder")).expect("remove the scratch folder");
        }
    }

    #[test]
    fn publish_folder_takes_release_and_dry_run_and_names_where_other_flags_go() {
        let given = |flags: &[&str]| flags.iter().map(|f| f.to_string()).collect::<Vec<String>>();
        assert!(folder_flags(&given(&["--folder", "--release", "--dry-run"])).is_ok());
        let refused = refusal(folder_flags(&given(&["--folder", "--target", "--features"])));
        assert!(refused.starts_with("publish --folder packages a module folder as its builds left it, so it takes no --target, --features"), "{refused}");
        let signing = refusal(folder_flags(&given(&["--folder", "--pfx", "--catalog"])));
        assert!(
            signing.starts_with("publish --folder signs nothing, so it takes no --pfx, --catalog; sign the folder first with cargo pwrs sign --folder"),
            "{signing}"
        );
        let both = refusal(folder_flags(&given(&["--folder", "--target", "--pfx"])));
        assert_eq!(both.lines().count(), 2, "{both}");
        assert!(both.contains("takes no --target;") && both.contains("takes no --pfx;"), "{both}");
    }

    /// Each command takes the flags it uses and refuses the rest, named,
    /// so `sign --dry-run` cannot sign while its caller expects nothing to
    /// change.
    #[test]
    fn each_command_refuses_the_flags_it_does_not_use() {
        let given = |flags: &[&str]| flags.iter().map(|f| f.to_string()).collect::<Vec<String>>();
        let build = ["--release", "--target", "--features", "-F", "--all-features", "--debug-symbols", "--package", "-p", "--manifest-dir"];
        assert!(unused_flags("build", false, &given(&build)).is_ok());
        assert!(unused_flags("test", false, &given(&["--cpu-tiers", "--release", "-F"])).is_ok());
        assert!(unused_flags("publish", false, &given(&["--dry-run", "--release", "--pfx", "--catalog"])).is_ok());
        assert!(unused_flags("sign", false, &given(&["--release", "--target", "--pfx", "--catalog"])).is_ok());
        assert!(unused_flags("sign", true, &given(&["--folder", "--manifest-dir", "-p", "--pem"])).is_ok());
        assert!(unused_flags("new", false, &given(&["--pwrs"])).is_ok());
        assert!(unused_flags("merge", false, &[]).is_ok());
        assert_eq!(refusal(unused_flags("sign", false, &given(&["--dry-run"]))), "sign takes no --dry-run; only publish has a dry run");
        assert_eq!(
            refusal(unused_flags("sign", true, &given(&["--folder", "--release", "--target"]))),
            "sign --folder takes no --release, --target; it signs the folder as it is and builds nothing"
        );
        assert_eq!(refusal(unused_flags("merge", false, &given(&["--dry-run"]))), "merge takes no --dry-run; only publish has a dry run");
        assert_eq!(refusal(unused_flags("build", false, &given(&["--cpu-tiers", "--pwrs"]))), "build takes no --cpu-tiers, --pwrs");
        assert_eq!(refusal(unused_flags("toolchain", false, &given(&["--release"]))), "toolchain takes no --release");
        assert_eq!(refusal(unused_flags("new", false, &given(&["--manifest-dir"]))), "new takes no --manifest-dir");
        assert_eq!(refusal(unused_flags("test", false, &given(&["--pfx"]))), "test takes no --pfx");
    }
}
