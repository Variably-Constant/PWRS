//! `cargo pwrs test`: cargo test, then the module's Pester suite in
//! pwsh and, on Windows, in Windows PowerShell.

use std::process::Command;

use crate::build::{build, BuildOptions};
use crate::Error;
use pwrs_build::pwsh;

const PESTER_PS1: &str = include_str!("../scripts/pester.ps1");

/// The `PWRS_CPU_MAX` values a tier run may name, the ones the runtime's
/// import check and `pwrs::cpu` read.
const CPU_TIERS: &[&str] = &["x86-64", "x86-64-v2", "x86-64-v3", "x86-64-v4", "native"];

/// `cargo test`, the build, and the Pester suites; then the suites once
/// more under each of `cpu_tiers` (the manifest's `test-cpu-tiers` when
/// `None`), with `PWRS_CPU_MAX` set to it in the hosts' environment, so
/// every kernel tier a module dispatches between runs on one machine.
pub fn run(opts: &BuildOptions, cpu_tiers: Option<&[String]>) -> Result<(), Error> {
    if let Some(tiers) = cpu_tiers {
        check_tiers(tiers)?;
    }
    // The Pester is found before anything builds, so a fetch that fails
    // stops the command at once.
    let tests = opts.manifest_dir.join("tests");
    let pester = if tests.is_dir() { Some(pester_path()?) } else { None };
    let status = pwrs_build::trace::status(Command::new("cargo").arg("test").args(opts.feature_args()).current_dir(&opts.manifest_dir))
        .map_err(|e| Error::msg(format!("cannot run cargo test: {e}")))?;
    if !status.success() {
        return Err(Error::msg(format!("cargo test failed with {status}")));
    }

    let built = build(opts)?;
    let Some(pester_path) = pester else {
        eprintln!("pwrs: no tests/ directory, skipping Pester");
        return Ok(());
    };
    let pwrs_dir = match built.module_dir.parent() {
        Some(p) => p.to_path_buf(),
        None => return Err(Error::msg("module dir has no parent")),
    };
    let script = pwsh::materialize_script(&pwrs_dir.join("work"), "pester.ps1", PESTER_PS1)?;
    let args = [built.module_dir.display().to_string(), tests.display().to_string(), pester_path];

    // Each host writes what it exercised to its own file in here,
    // named for its process. The variable reaches them because a
    // child inherits this process's environment, and the hosts are
    // spawned one after another from this thread.
    let surface_dir = pwrs_dir.join("work").join("surface");
    let _cleared = std::fs::remove_dir_all(&surface_dir);
    // SAFETY: no other thread is running: the one a streamed host run reads the host's errors on ends before that run returns.
    unsafe { std::env::set_var("PWRS_SURFACE_DIR", &surface_dir) };
    // A PsObject method called from a worker the module started is
    // refused under test, where a release build would let it through.
    // SAFETY: no other thread is running: the one a streamed host run reads the host's errors on ends before that run returns.
    unsafe { std::env::set_var("PWRS_THREAD_CHECK", "1") };

    // Each host's output reaches the console as Pester writes it, so a
    // test that never returns is on screen by name.
    eprintln!("pwrs: Pester in pwsh");
    pwsh::stream_pwsh_script(&script, &args)?;

    if cfg!(windows) {
        eprintln!("pwrs: Pester in Windows PowerShell");
        pwsh::stream_winps_script(&script, &args)?;
    }

    let tiers: &[String] = match cpu_tiers {
        Some(t) => t,
        None => {
            check_tiers(&built.test_cpu_tiers)?;
            &built.test_cpu_tiers
        }
    };
    for tier in tiers {
        // SAFETY: no other thread is running: the one a streamed host run reads the host's errors on ends before that run returns.
        unsafe { std::env::set_var("PWRS_CPU_MAX", tier) };
        eprintln!("pwrs: Pester in pwsh under PWRS_CPU_MAX={tier}");
        let pwsh_run = pwsh::stream_pwsh_script(&script, &args);
        let winps_run = if cfg!(windows) && pwsh_run.is_ok() {
            eprintln!("pwrs: Pester in Windows PowerShell under PWRS_CPU_MAX={tier}");
            pwsh::stream_winps_script(&script, &args)
        } else {
            Ok(())
        };
        // SAFETY: no other thread is running: the one a streamed host run reads the host's errors on ends before that run returns.
        unsafe { std::env::remove_var("PWRS_CPU_MAX") };
        pwsh_run?;
        winps_run?;
    }

    report_surface(&built.module, &surface_dir)
}

/// The Pester module the suites import in each host: the one
/// `PWRS_PESTER_PATH` names, else the pinned one the toolchain fetches.
fn pester_path() -> Result<String, Error> {
    match std::env::var("PWRS_PESTER_PATH") {
        Ok(path) if !path.is_empty() => Ok(path),
        Err(std::env::VarError::NotUnicode(raw)) => Ok(raw.to_string_lossy().into_owned()),
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok(pwrs_build::toolchain::Toolchain::ensure()?.pester()?.display().to_string()),
    }
}

/// Refuses a tier the runtime would refuse, before anything runs.
fn check_tiers(tiers: &[String]) -> Result<(), Error> {
    for t in tiers {
        if !CPU_TIERS.contains(&t.as_str()) {
            return Err(Error::msg(format!("cpu tier '{t}' is not one of {}", CPU_TIERS.join(", "))));
        }
    }
    Ok(())
}

/// Prints the module's declarations the suite did not exercise. A
/// finding does not fail the run: it names a missing test or a
/// declaration the body does not follow, and only the author can tell
/// which.
fn report_surface(module: &crate::descriptor::Module, dir: &std::path::Path) -> Result<(), Error> {
    let mut unread = Vec::new();
    let seen = crate::surface::read(dir, &mut unread)?;
    for note in &unread {
        eprintln!("{note}");
    }
    let findings = crate::surface::findings(module, &seen);
    if findings.is_empty() {
        eprintln!("pwrs: surface check: every cmdlet ran, every pipeline parameter took a record, every ShouldProcess cmdlet called should_process");
        return Ok(());
    }
    eprintln!("{}", surface_summary(findings.len()));
    for f in &findings {
        eprintln!("  {f}");
    }
    Ok(())
}

/// The line the surface check prints before its `count` findings.
fn surface_summary(count: usize) -> String {
    let verb = if count == 1 { "was" } else { "were" };
    format!("pwrs: surface check: {count} of the module's declarations {verb} not exercised")
}

#[cfg(test)]
mod tests {
    use super::PESTER_PS1;
    use pwrs_build::pwsh;

    #[test]
    fn a_tier_the_runtime_would_refuse_is_refused_before_anything_runs() {
        let tiers = |names: &[&str]| -> Vec<String> { names.iter().map(|s| s.to_string()).collect() };
        assert!(super::check_tiers(&tiers(&["x86-64", "x86-64-v3", "native"])).is_ok());
        let e = super::check_tiers(&tiers(&["x86-64-v3", "avx2"])).expect_err("avx2 names an extension, not a level");
        assert!(e.to_string().contains("'avx2'"), "{e}");
    }

    #[test]
    fn the_surface_summary_agrees_with_its_count() {
        assert_eq!(super::surface_summary(1), "pwrs: surface check: 1 of the module's declarations was not exercised");
        assert_eq!(super::surface_summary(2), "pwrs: surface check: 2 of the module's declarations were not exercised");
    }

    /// A Pester that will not load is reported as that, on one line of
    /// stderr naming what was tried, what it said and the variable that
    /// overrides it, rather than as the bare Import-Module error.
    #[test]
    fn a_pester_that_will_not_load_names_itself_and_the_override() {
        if let Err(e) = pwsh::pshome() {
            eprintln!("skipped: no pwsh to run pester.ps1 in ({e})");
            return;
        }
        let dir = std::env::temp_dir().join(format!("pwrs-pester-probe-{}", std::process::id()));
        let pester = dir.join("Pester");
        std::fs::create_dir_all(&pester).expect("create the fake Pester");
        let psm1 = pester.join("Pester.psm1");
        std::fs::write(&psm1, "throw 'this Pester is broken on purpose'\n").expect("write the fake Pester");
        let script = pwsh::materialize_script(&dir, "pester.ps1", PESTER_PS1).expect("write pester.ps1");
        let args = [dir.display().to_string(), dir.display().to_string(), psm1.display().to_string()];
        let err = match pwsh::run_pwsh_tool(&script, &args) {
            Ok(out) => panic!("a Pester that throws on import was reported as a run: {out}"),
            Err(e) => e.to_string(),
        };
        let line = match err.lines().find(|l| l.contains("Pester would not load")) {
            Some(l) => l,
            None => panic!("no report line in: {err}"),
        };
        assert!(line.contains(&psm1.display().to_string()), "{line}");
        assert!(line.contains("It said: this Pester is broken on purpose."), "{line}");
        assert!(line.contains("PWRS_PESTER_PATH"), "{line}");
    }
}
