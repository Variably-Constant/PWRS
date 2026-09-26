//! `cargo pwrs build` lays out a module folder:
//!
//! ```text
//! <Name>/
//!   <Name>.psd1                   manifest
//!   <Name>.psm1                   bootstrap: pick the TFM folder, load through Pwrs.Bootstrap
//!   <Name>.Format.ps1xml          default views for output classes
//!   net10.0/Pwrs.Bootstrap.dll    frozen identity; per-module AssemblyLoadContext
//!   net10.0/Pwrs.Runtime.dll      support assembly for pwsh 7.6
//!   net10.0/<Name>.Shell.<stamp>.dll          generated cmdlet classes
//!   net10.0/en-US/<Name>.Shell.<stamp>.dll-Help.xml
//!   netstandard2.0/...            the same for Windows PowerShell 5.1
//!   runtimes/<rid>/native/        the Rust cdylib per runtime identifier
//! ```
//!
//! The C# is compiled by a csc fetched from NuGet and run on the
//! runtime pwsh ships, inside a pwsh process.
//!
//! `<stamp>` is a hash of the managed source the shell is built from,
//! so a changed surface gets an assembly identity of its own. That is
//! what lets a rebuilt module take its cmdlets over in a session that
//! has already run the previous build.

mod build;
mod cpu;
mod descriptor;
mod formats;
mod generate;
mod help;
mod merge;
mod notices;
mod scaffold;
mod surface;
mod test;

use std::path::PathBuf;

pub use pwrs_build::{pwsh, toolchain, Error};

const PUBLISH_PS1: &str = include_str!("../scripts/publish.ps1");

fn usage() -> ! {
    eprintln!(
        "usage: cargo pwrs <build|test|publish|new|toolchain> [--release] [--target <triple>] [--features <a,b>] [--all-features] [--debug-symbols] [--package <name>] [--manifest-dir <dir>] [--dry-run] [--cpu-tiers <x86-64,x86-64-v3,...>] [--pwrs <dependency spec>] [<new dir>]\n       cargo pwrs merge <into module dir> <from module dir>..."
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
    let mut positional = Vec::new();
    // The registry name differs from the library name, so a scaffolded
    // manifest has to name the package or it resolves to an unrelated
    // crate that holds `pwrs` on crates.io. The version is this tool's
    // own, which is the library's too: the workspace publishes them
    // together, and reading it here is what keeps a scaffold from
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
        match args[i].as_str() {
            "--release" => release = true,
            "--dry-run" => dry_run = true,
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
    let result = match sub.as_str() {
        "build" => build::build(&opts).map(|_| ()),
        "test" => test::run(&opts, cpu_tiers.as_deref()),
        "publish" => publish(&opts, dry_run),
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
        "toolchain" => toolchain::Toolchain::ensure().map(|tc| {
            eprintln!("pwrs: toolchain at {}", tc.root.display());
            eprintln!("pwrs: csc at {}", tc.csc_dir.display());
            eprintln!("pwrs: pwsh at {}", tc.pshome.display());
        }),
        _unknown => usage(),
    };
    if let Err(e) = result {
        eprintln!("cargo-pwrs: {e}");
        std::process::exit(1);
    }
}

fn publish(opts: &build::BuildOptions, dry_run: bool) -> Result<(), Error> {
    let built = build::build(opts)?;
    if !built.missing_metadata.is_empty() {
        return Err(Error::msg(format!(
            "{} cannot be published: its Cargo.toml declares no {}. A module on a gallery carries its own author and its own description, and pwrs will not publish one under a placeholder. Set them under [package] and build again.",
            built.module.name,
            built.missing_metadata.join(" and "),
        )));
    }
    undeclared_cpu_refusal(&built.module.name, &built.undeclared_cpu_features)?;
    license_text_refusal(&built.module.name, &built.crates_without_license_text)?;
    for bundled in &built.bundled {
        bundled_cpu_refusal(&built.module.name, &bundled.name, &bundled.undeclared_cpu_features)?;
        bundled_license_text_refusal(&built.module.name, &bundled.name, &bundled.crates_without_license_text)?;
    }
    for bundled in &built.bundled {
        eprintln!("pwrs: the package carries the bundled module {}", bundled.name);
    }
    let pwrs_dir = match built.module_dir.parent() {
        Some(p) => p.to_path_buf(),
        None => return Err(Error::msg("module dir has no parent")),
    };
    let script = pwsh::materialize_script(&pwrs_dir.join("work"), "publish.ps1", PUBLISH_PS1)?;
    let out_dir = pwrs_dir.join("publish").join(&built.module.name);
    let mut args = vec![
        "-ModulePath".to_string(),
        built.module_dir.display().to_string(),
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

/// Refuses a module whose library requires extensions beyond its target's
/// baseline that its manifest does not list under `cpu-features`.
fn undeclared_cpu_refusal(module: &str, undeclared: &[String]) -> Result<(), Error> {
    if undeclared.is_empty() {
        return Ok(());
    }
    Err(Error::msg(format!(
        "{module} cannot be published: its native library is compiled to require {}, and a CPU without them refuses to import it. Build with RUSTFLAGS='-C target-cpu=x86-64', or list them under [package.metadata.pwrs] cpu-features if every CPU the module is meant for has them.",
        undeclared.join(", "),
    )))
}

/// Refuses a module whose library links crates whose source carries no
/// license file, since its notices could name their licenses but not
/// quote them.
fn license_text_refusal(module: &str, without_text: &[String]) -> Result<(), Error> {
    if without_text.is_empty() {
        return Ok(());
    }
    Err(Error::msg(format!(
        "{module} cannot be published: its native library links {}, whose source carries no license file, so the notices beside the library name their licenses but cannot quote them. Depend on a release of each that ships its license text.",
        without_text.join(", "),
    )))
}

/// Refuses a module whose bundled module's library requires extensions
/// beyond its target's baseline that neither the bundled crate's
/// `cpu-features` nor its `bundled-modules` entry lists.
fn bundled_cpu_refusal(module: &str, bundled: &str, undeclared: &[String]) -> Result<(), Error> {
    if undeclared.is_empty() {
        return Ok(());
    }
    Err(Error::msg(format!(
        "{module} cannot be published: the native library of its bundled module {bundled} is compiled to require {}, and a CPU without them refuses to import it. Build with RUSTFLAGS='-C target-cpu=x86-64', or list them under the cpu-features of {bundled}'s bundled-modules entry if every CPU the module is meant for has them.",
        undeclared.join(", "),
    )))
}

/// Refuses a module whose bundled module's library links crates whose
/// source carries no license file and for which neither the bundled
/// crate nor its `bundled-modules` entry supplies one.
fn bundled_license_text_refusal(module: &str, bundled: &str, without_text: &[String]) -> Result<(), Error> {
    if without_text.is_empty() {
        return Ok(());
    }
    Err(Error::msg(format!(
        "{module} cannot be published: the native library of its bundled module {bundled} links {}, whose source carries no license file, so the notices beside that library name their licenses but cannot quote them. Supply each file under the license-files of {bundled}'s bundled-modules entry, or build it without the crates that need them.",
        without_text.join(", "),
    )))
}

#[cfg(test)]
mod tests {
    use super::{bundled_cpu_refusal, bundled_license_text_refusal, license_text_refusal, undeclared_cpu_refusal};

    #[test]
    fn a_bundled_librarys_gaps_are_refused_naming_it() {
        assert!(bundled_license_text_refusal("Hello", "Calc", &[]).is_ok());
        assert!(bundled_cpu_refusal("Hello", "Calc", &[]).is_ok());
        let texts = match bundled_license_text_refusal("Hello", "Calc", &["wasmparser 0.239.0".to_string()]) {
            Ok(()) => panic!("a bundled library linking a crate without a license text was accepted"),
            Err(e) => e.to_string(),
        };
        assert!(texts.starts_with("Hello cannot be published: the native library of its bundled module Calc links wasmparser 0.239.0"), "{texts}");
        assert!(texts.contains("license-files of Calc's bundled-modules entry"), "{texts}");
        let cpu = match bundled_cpu_refusal("Hello", "Calc", &["avx2".to_string()]) {
            Ok(()) => panic!("a bundled library requiring an undeclared extension was accepted"),
            Err(e) => e.to_string(),
        };
        assert!(cpu.starts_with("Hello cannot be published: the native library of its bundled module Calc is compiled to require avx2,"), "{cpu}");
        assert!(cpu.contains("cpu-features of Calc's bundled-modules entry"), "{cpu}");
    }

    #[test]
    fn a_crate_without_a_license_text_is_refused() {
        assert!(license_text_refusal("Hello", &[]).is_ok());
        let refused = match license_text_refusal("Hello", &["PoWerRuSt 0.2.1".to_string()]) {
            Ok(()) => panic!("a library linking a crate without a license text was accepted"),
            Err(e) => e.to_string(),
        };
        assert!(refused.starts_with("Hello cannot be published"), "{refused}");
        assert!(refused.contains("links PoWerRuSt 0.2.1, whose source carries no license file"), "{refused}");
    }

    #[test]
    fn only_undeclared_extensions_are_refused() {
        assert!(undeclared_cpu_refusal("Hello", &[]).is_ok());
        let refused = match undeclared_cpu_refusal("Hello", &["avx512f".to_string(), "gfni".to_string()]) {
            Ok(()) => panic!("a library requiring undeclared extensions was accepted"),
            Err(e) => e.to_string(),
        };
        assert!(refused.starts_with("Hello cannot be published"), "{refused}");
        assert!(refused.contains("compiled to require avx512f, gfni,"), "{refused}");
    }
}
