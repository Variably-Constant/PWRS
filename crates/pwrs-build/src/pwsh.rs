//! Locating and running the PowerShell hosts.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::Error;

/// Reads an environment variable that may legitimately be absent.
fn env_opt(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => Some(v),
        Ok(_empty) => None,
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(raw)) => Some(raw.to_string_lossy().into_owned()),
    }
}

/// The pwsh executable: `PWRS_PWSH` if set, else `pwsh` on PATH.
pub fn pwsh_exe() -> String {
    match env_opt("PWRS_PWSH") {
        Some(p) => p,
        None => "pwsh".to_string(),
    }
}

/// `$PSHOME` of the pwsh on PATH, or `PWRS_PSHOME`.
pub fn pshome() -> Result<PathBuf, Error> {
    if let Some(p) = env_opt("PWRS_PSHOME") {
        return Ok(PathBuf::from(p));
    }
    let out = Command::new(pwsh_exe())
        .args(["-NoProfile", "-NonInteractive", "-c", "$PSHOME"])
        .output()
        .map_err(|e| Error::msg(format!("cannot run pwsh: {e}")))?;
    if !out.status.success() {
        return Err(Error::msg(format!("pwsh -c $PSHOME failed: {}", String::from_utf8_lossy(&out.stderr))));
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        return Err(Error::msg("pwsh printed an empty $PSHOME"));
    }
    Ok(PathBuf::from(s))
}

/// Runs a script file in pwsh and returns stdout; a non-zero exit is
/// an error carrying stdout and stderr.
pub fn run_pwsh_script(script: &Path, args: &[String]) -> Result<String, Error> {
    run_host(&pwsh_exe(), script, args)
}

/// Runs a script file in Windows PowerShell 5.1.
pub fn run_winps_script(script: &Path, args: &[String]) -> Result<String, Error> {
    run_host("powershell", script, args)
}

/// Runs a script file in pwsh with its stdout and stderr going to this
/// process's own as the script writes them, so a run that never ends
/// has already shown how far it got. Stdin is empty. A non-zero exit
/// is an error naming the script and the status.
pub fn stream_pwsh_script(script: &Path, args: &[String]) -> Result<(), Error> {
    stream_host(&pwsh_exe(), script, args)
}

/// Runs a script file in Windows PowerShell 5.1, its output streamed as
/// [`stream_pwsh_script`] streams it.
pub fn stream_winps_script(script: &Path, args: &[String]) -> Result<(), Error> {
    stream_host("powershell", script, args)
}

/// True for a `PSModulePath` entry belonging to PowerShell Core: the
/// `$PSHOME\Modules` of an MSI install or of an MSIX package, and the
/// two Core-only scopes.
fn is_core_module_dir(entry: &str) -> bool {
    let e = entry.to_ascii_lowercase().replace('/', "\\");
    e.contains("\\powershell\\7")
        || e.contains("microsoft.powershell_")
        || e.ends_with("\\documents\\powershell\\modules")
        || e.ends_with("\\program files\\powershell\\modules")
}

/// `PSModulePath` with PowerShell Core's directories removed.
///
/// Windows PowerShell started from a pwsh process inherits pwsh's
/// `PSModulePath`, finds Core's `Microsoft.PowerShell.Security` ahead
/// of its own, and fails to load it: Core's types file redefines
/// members 5.1 already has. The error is not confined to that module,
/// because it leaves module auto-loading broken for the session, so
/// `Import-PowerShellDataFile` and `ConvertTo-SecureString` go missing
/// as well.
fn winps_module_path() -> Option<String> {
    let current = env_opt("PSModulePath")?;
    let kept: Vec<&str> = current.split(';').filter(|e| !e.is_empty() && !is_core_module_dir(e)).collect();
    Some(kept.join(";"))
}

/// The command that runs `script` in host `exe` with `args`, with
/// Windows PowerShell given a `PSModulePath` free of Core's directories.
fn host_command(exe: &str, script: &Path, args: &[String]) -> Command {
    let mut cmd = Command::new(exe);
    if exe == "powershell" {
        if let Some(p) = winps_module_path() {
            cmd.env("PSModulePath", p);
        }
    }
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(script).args(args);
    cmd
}

/// Runs `script` in `exe` with stdout and stderr inherited and stdin
/// null, as `Command::output` gives it, so a script reading the console
/// gets end of input instead of waiting on the terminal.
fn stream_host(exe: &str, script: &Path, args: &[String]) -> Result<(), Error> {
    let status = host_command(exe, script, args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| Error::msg(format!("cannot run {exe}: {e}")))?;
    if !status.success() {
        return Err(Error::msg(format!("{exe} {} exited with {status}; its output is above", script.display())));
    }
    Ok(())
}

fn run_host(exe: &str, script: &Path, args: &[String]) -> Result<String, Error> {
    let out = host_command(exe, script, args).output().map_err(|e| Error::msg(format!("cannot run {exe}: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(Error::msg(format!(
            "{exe} {} exited with {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            script.display(),
            out.status,
            stdout,
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(stdout)
}

/// Writes an embedded script to the tool's scratch directory and
/// returns its path.
pub fn materialize_script(dir: &Path, name: &str, body: &str) -> Result<PathBuf, Error> {
    std::fs::create_dir_all(dir).map_err(|e| Error::msg(format!("cannot create {}: {e}", dir.display())))?;
    let path = dir.join(name);
    std::fs::write(&path, body).map_err(|e| Error::msg(format!("cannot write {}: {e}", path.display())))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::is_core_module_dir;

    #[test]
    fn core_module_dirs_are_recognized_in_every_install_shape() {
        for entry in [
            r"C:\Program Files\PowerShell\7\Modules",
            r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.6.0_x64__8wekyb3d8bbwe\Modules",
            r"C:\Users\x\Documents\PowerShell\Modules",
            r"C:\Program Files\PowerShell\Modules",
        ] {
            assert!(is_core_module_dir(entry), "should be Core: {entry}");
        }
    }

    #[test]
    fn windows_powershell_dirs_are_kept() {
        // `Documents\WindowsPowerShell\Modules` differs from the Core
        // scope by one word, so the suffix test has to see the whole
        // segment rather than a trailing `powershell\modules`.
        for entry in [
            r"C:\Windows\system32\WindowsPowerShell\v1.0\Modules",
            r"C:\Program Files\WindowsPowerShell\Modules",
            r"C:\Users\x\Documents\WindowsPowerShell\Modules",
        ] {
            assert!(!is_core_module_dir(entry), "should be kept: {entry}");
        }
    }

    /// A streamed script reads nothing from this process's stdin. The
    /// test runs again as a child whose stdin holds a line; the pwsh that
    /// child streams must get end of input and exit 7, which the child's
    /// error names. A pwsh that inherited the child's stdin would read the
    /// line and exit 0.
    #[test]
    fn a_streamed_script_takes_nothing_from_stdin_and_its_failure_names_the_status() {
        const ROLE: &str = "PWRS_STREAM_PROBE_DIR";
        if let Some(dir) = std::env::var_os(ROLE) {
            let script = super::materialize_script(
                std::path::Path::new(&dir),
                "stdin.ps1",
                "if ($null -eq [Console]::In.ReadLine()) { exit 7 }\nexit 0\n",
            )
            .expect("write the probe script");
            match super::stream_pwsh_script(&script, &[]) {
                Ok(()) => panic!("the streamed pwsh read a line from stdin"),
                Err(e) => {
                    let text = e.to_string();
                    assert!(text.contains("exit code: 7") || text.contains("exit status: 7"), "the error does not name exit 7: {text}");
                }
            }
            return;
        }
        if let Err(e) = super::pshome() {
            eprintln!("skipped: no pwsh to run a script in ({e})");
            return;
        }
        let dir = std::env::temp_dir().join(format!("pwrs-stream-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the probe folder");
        let exe = std::env::current_exe().expect("the test binary's path");
        let mut child = std::process::Command::new(exe)
            .args(["--exact", "pwsh::tests::a_streamed_script_takes_nothing_from_stdin_and_its_failure_names_the_status", "--nocapture"])
            .env(ROLE, &dir)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("start the child test");
        {
            use std::io::Write;
            let mut stdin = child.stdin.take().expect("the child's stdin");
            stdin.write_all(b"a line the streamed pwsh must not see\n").expect("write the child's stdin");
        }
        let status = child.wait().expect("wait for the child test");
        assert!(status.success(), "the child test failed: {status}");
    }
}
