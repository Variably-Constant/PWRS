//! Locating and running the PowerShell hosts.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::clixml::Decoder;
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

/// Cache folders this process has made for the pwsh processes it starts.
static CACHES: AtomicU64 = AtomicU64::new(0);

/// The cache folder of a pwsh that PWRS starts for one of its own steps:
/// on Unix a new folder under the temporary folder that no other pwsh
/// uses, removed when this is dropped, so the step neither reads nor
/// writes the startup profile that pwsh processes share in
/// `$XDG_CACHE_HOME/powershell` (PowerShell/PowerShell#26528); on Windows
/// none, since pwsh there takes its cache folder from the user's profile
/// rather than the environment.
pub struct OwnCache(Option<PathBuf>);

impl OwnCache {
    /// Makes the folder, on Unix.
    pub fn new() -> Result<OwnCache, Error> {
        if !cfg!(unix) {
            return Ok(OwnCache(None));
        }
        let dir = std::env::temp_dir().join(format!("pwrs-pwsh-cache-{}-{}", std::process::id(), CACHES.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).map_err(|e| Error::msg(format!("cannot create {}: {e}", dir.display())))?;
        Ok(OwnCache(Some(dir)))
    }

    /// Points `cmd`'s `XDG_CACHE_HOME` at the folder, when there is one.
    pub fn give(&self, cmd: &mut Command) {
        if let Some(dir) = &self.0 {
            cmd.env("XDG_CACHE_HOME", dir);
        }
    }
}

impl Drop for OwnCache {
    fn drop(&mut self) {
        if let Some(dir) = &self.0
            && let Err(e) = std::fs::remove_dir_all(dir)
        {
            eprintln!("pwrs: cannot remove pwsh's cache folder {}: {e}", dir.display());
        }
    }
}

/// `$PSHOME` of the pwsh on PATH, or `PWRS_PSHOME`.
pub fn pshome() -> Result<PathBuf, Error> {
    if let Some(p) = env_opt("PWRS_PSHOME") {
        return Ok(PathBuf::from(p));
    }
    let cache = OwnCache::new()?;
    let mut cmd = Command::new(pwsh_exe());
    cache.give(&mut cmd);
    let out = crate::trace::output(cmd.args(["-NoProfile", "-NonInteractive", "-c", "$PSHOME"]))
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
/// an error that includes stdout and stderr.
pub fn run_pwsh_script(script: &Path, args: &[String]) -> Result<String, Error> {
    run_host(&pwsh_exe(), script, args)
}

/// Runs a script file in pwsh for one of PWRS's own steps and returns
/// stdout, as [`run_pwsh_script`] does, with an [`OwnCache`].
pub fn run_pwsh_tool(script: &Path, args: &[String]) -> Result<String, Error> {
    let exe = pwsh_exe();
    let cache = OwnCache::new()?;
    let mut cmd = host_command(&exe, script, args);
    cache.give(&mut cmd);
    output_of(&exe, script, cmd)
}

/// Runs a script file in Windows PowerShell 5.1.
pub fn run_winps_script(script: &Path, args: &[String]) -> Result<String, Error> {
    run_host("powershell", script, args)
}

/// Runs a script file in pwsh with its stdout and stderr going to this
/// process's own as the script writes them, so a run that never ends
/// has already shown how far it got. Its stderr is read on a thread of
/// its own and written on, with the CLIXML a PowerShell it starts may
/// put there as plain lines. Stdin is empty. A non-zero exit is an error
/// naming the script and the status.
pub fn stream_pwsh_script(script: &Path, args: &[String]) -> Result<(), Error> {
    stream_host(&pwsh_exe(), script, args)
}

/// Runs a script file in Windows PowerShell 5.1, its output streamed as
/// [`stream_pwsh_script`] streams it.
pub fn stream_winps_script(script: &Path, args: &[String]) -> Result<(), Error> {
    stream_host("powershell", script, args)
}

/// Runs a script file in pwsh for one of PWRS's own steps, its output
/// streamed as [`stream_pwsh_script`] streams it, with an [`OwnCache`].
pub fn stream_pwsh_tool(script: &Path, args: &[String]) -> Result<(), Error> {
    let exe = pwsh_exe();
    let cache = OwnCache::new()?;
    let mut cmd = host_command(&exe, script, args);
    cache.give(&mut cmd);
    stream_command(&exe, script, cmd)
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
    if exe == "powershell"
        && let Some(p) = winps_module_path()
    {
        cmd.env("PSModulePath", p);
    }
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(script).args(args);
    cmd
}

/// Runs `script` in `exe` with stdout inherited, stderr read by
/// [`forward`] on a thread of its own, and stdin null, as
/// `Command::output` gives it, so a script reading the console gets end
/// of input instead of waiting on the terminal. The thread ends before
/// this returns.
fn stream_host(exe: &str, script: &Path, args: &[String]) -> Result<(), Error> {
    stream_command(exe, script, host_command(exe, script, args))
}

/// Runs `cmd`, which runs `script` in `exe`, as [`stream_host`] does.
fn stream_command(exe: &str, script: &Path, mut cmd: Command) -> Result<(), Error> {
    let mut started = crate::trace::spawn(cmd.stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::piped()))
        .map_err(|e| Error::msg(format!("cannot run {exe}: {e}")))?;
    let reader = started.child.stderr.take().map(|errors| std::thread::Builder::new().spawn(move || forward(errors)));
    let waited = started.wait();
    let forwarded = match reader {
        Some(Ok(reader)) => match reader.join() {
            Ok(forwarded) => forwarded,
            Err(panic) => Err(std::io::Error::other(format!("the thread reading it panicked: {}", panic_text(&*panic)))),
        },
        Some(Err(e)) => Err(e),
        None => Err(std::io::Error::other("it has no error stream to read")),
    };
    let status = match (waited, forwarded) {
        (Ok(status), Ok(())) => status,
        (Ok(status), Err(e)) => {
            return Err(Error::msg(format!("{exe} {} exited with {status}, and its error stream could not be written here: {e}", script.display())));
        }
        (Err(e), Ok(())) => return Err(Error::msg(format!("cannot wait for {exe} {}: {e}", script.display()))),
        (Err(e), Err(lost)) => {
            return Err(Error::msg(format!(
                "cannot wait for {exe} {}: {e}; its error stream could not be written here either: {lost}",
                script.display()
            )));
        }
    };
    if !status.success() {
        return Err(Error::msg(format!("{exe} {} exited with {status}; its output is above", script.display())));
    }
    Ok(())
}

/// Reads a host's error stream, `errors`, to its end, and writes it to
/// this process's own as [`Decoder`] turns it into plain text, each read
/// as it arrives. A write that fails ends the writing but not the
/// reading, so the host never waits on a full pipe; that failure is
/// returned once the stream ends.
fn forward(mut errors: impl Read) -> std::io::Result<()> {
    let mut decoder = Decoder::new();
    let mut buffer = vec![0; 1 << 16];
    let mut text = Vec::new();
    let mut writer = stderr_writer();
    let read = loop {
        let n = match errors.read(&mut buffer) {
            Ok(0) => break Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => break Err(e),
        };
        decoder.read(&buffer[..n], &mut text);
        writer = writer.and_then(|mut w| w.write_all(&text).map(|()| w));
        text.clear();
    };
    decoder.end(&mut text);
    writer = writer.and_then(|mut w| w.write_all(&text).map(|()| w));
    match (read, writer) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(e), Ok(_)) | (Ok(()), Err(e)) => Err(e),
        (Err(read), Err(write)) => Err(std::io::Error::new(read.kind(), format!("{read}, after a write that failed: {write}"))),
    }
}

/// This process's standard error, to be written byte for byte. Rust's own
/// handle to a Windows console takes UTF-8 only, so on a console a copy
/// of the console's handle is written as a file instead, which the
/// console reads in its output code page, as it reads a host's own
/// writes to it.
fn stderr_writer() -> std::io::Result<Box<dyn Write>> {
    #[cfg(windows)]
    {
        use std::io::IsTerminal;
        use std::os::windows::io::AsHandle;
        let stderr = std::io::stderr();
        if stderr.is_terminal() {
            return Ok(Box::new(std::fs::File::from(stderr.as_handle().try_clone_to_owned()?)));
        }
    }
    Ok(Box::new(std::io::stderr()))
}

/// The text a thread panicked with, when it is text.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(text) = payload.downcast_ref::<&str>() {
        text
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text
    } else {
        "a value that is not text"
    }
}

fn run_host(exe: &str, script: &Path, args: &[String]) -> Result<String, Error> {
    output_of(exe, script, host_command(exe, script, args))
}

/// Runs `cmd`, which runs `script` in `exe`, and returns its stdout; a
/// non-zero exit is an error that includes stdout and stderr.
fn output_of(exe: &str, script: &Path, mut cmd: Command) -> Result<String, Error> {
    let out = crate::trace::output(&mut cmd).map_err(|e| Error::msg(format!("cannot run {exe}: {e}")))?;
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
/// returns its path. A file there whose text is already `body` is left
/// as it is; otherwise the script is written under a temporary name and
/// renamed into place, so a tool running the script meanwhile reads a
/// whole one, and a rename that fails counts only when the file's text
/// is still not `body`.
pub fn materialize_script(dir: &Path, name: &str, body: &str) -> Result<PathBuf, Error> {
    std::fs::create_dir_all(dir).map_err(|e| Error::msg(format!("cannot create {}: {e}", dir.display())))?;
    let path = dir.join(name);
    if holds(&path, body)? {
        return Ok(path);
    }
    match crate::write_whole(&path, body) {
        Ok(()) => Ok(path),
        Err(_written_by_another_tool) if holds(&path, body)? => Ok(path),
        Err(e) => Err(e),
    }
}

/// Whether the file at `path` exists and its text is `body`.
fn holds(path: &Path, body: &str) -> Result<bool, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text == body),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Error::msg(format!("cannot read {}: {e}", path.display()))),
    }
}

#[cfg(test)]
mod tests {
    use super::is_core_module_dir;

    /// A script is written once, left as it is while its text is the
    /// same, and written again whole when its text differs.
    #[test]
    fn a_script_is_written_whole_once_and_again_only_when_it_differs() {
        let dir = std::env::temp_dir().join(format!("pwrs-build-script-{}", std::process::id()));
        let path = super::materialize_script(&dir, "s.ps1", "'one'").expect("write the script");
        let written = std::fs::metadata(&path).expect("stat").modified().expect("modified time");
        std::thread::sleep(std::time::Duration::from_millis(20));
        super::materialize_script(&dir, "s.ps1", "'one'").expect("leave the script");
        assert_eq!(std::fs::metadata(&path).expect("stat").modified().expect("modified time"), written);
        super::materialize_script(&dir, "s.ps1", "'two'").expect("write the other text");
        assert_eq!(std::fs::read_to_string(&path).expect("read the script"), "'two'");
        let left: Vec<_> = std::fs::read_dir(&dir).expect("list").map(|e| e.expect("entry").file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("s.ps1")], "no temporary file stays");
        std::fs::remove_dir_all(&dir).expect("remove the scratch folder");
    }

    /// On Unix a pwsh given an own cache writes its startup profile in that
    /// folder, the folder goes with the cache, and no two caches share a
    /// folder; on Windows there is no folder to give.
    #[test]
    fn a_step_writes_its_startup_profile_in_a_cache_folder_of_its_own() {
        if let Err(e) = super::pshome() {
            eprintln!("skipped: no pwsh to start ({e})");
            return;
        }
        let first = super::OwnCache::new().expect("make a cache");
        let second = super::OwnCache::new().expect("make another cache");
        let dir = match (&first.0, &second.0) {
            (Some(a), Some(b)) => {
                assert_ne!(a, b, "two caches share a folder");
                a.clone()
            }
            (None, None) if !cfg!(unix) => return,
            other => panic!("unexpected cache folders {other:?}"),
        };
        let mut cmd = std::process::Command::new(super::pwsh_exe());
        first.give(&mut cmd);
        let out = cmd
            .args(["-NoProfile", "-NonInteractive", "-c", "Start-Sleep -Milliseconds 500; $env:XDG_CACHE_HOME"])
            .output()
            .expect("run pwsh");
        assert!(out.status.success(), "pwsh failed: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), dir.display().to_string());
        let profile = dir.join("powershell").join("StartupProfileData-NonInteractive");
        assert!(profile.is_file(), "pwsh wrote no startup profile at {}", profile.display());
        drop(first);
        assert!(!dir.exists(), "the cache folder outlived its cache");
    }

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
    fn windows_powershell_dirs_stay() {
        // `Documents\WindowsPowerShell\Modules` differs from the Core
        // scope by one word, so the suffix test has to see the whole
        // segment rather than a trailing `powershell\modules`.
        for entry in [
            r"C:\Windows\system32\WindowsPowerShell\v1.0\Modules",
            r"C:\Program Files\WindowsPowerShell\Modules",
            r"C:\Users\x\Documents\WindowsPowerShell\Modules",
        ] {
            assert!(!is_core_module_dir(entry), "should stay: {entry}");
        }
    }

    /// A streamed script reads nothing from this process's stdin. The
    /// test runs again as a child whose stdin has a line; the pwsh that
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
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("start the child test");
        {
            use std::io::Write;
            let mut stdin = child.stdin.take().expect("the child's stdin");
            stdin.write_all(b"a line the streamed pwsh must not see\n").expect("write the child's stdin");
        }
        let out = child.wait_with_output().expect("wait for the child test");
        assert!(
            out.status.success(),
            "the child test failed: {}\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A script that starts a pwsh with -EncodedCommand, which writes a
    /// progress record, a warning and a line straight to the error stream
    /// it shares with the script, and then writes a line of its own there.
    const CLIXML_SCRIPT: &str = "$body = \"Write-Progress -Activity 'child work' -Status 'half way' -PercentComplete 50; \
        Write-Warning 'a warning from the child'; [Console]::Error.WriteLine('a line straight from the child')\"\n\
        $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($body))\n\
        $null = & (Get-Process -Id $PID).Path -NoProfile -NonInteractive -EncodedCommand $encoded\n\
        [Console]::Error.WriteLine('a line after the child')\n";

    /// A streamed script's error stream reaches this process's own as
    /// plain lines, with nothing of the CLIXML a pwsh it started wrote
    /// there. The script's error stream read as it is must have CLIXML
    /// first, or the rest shows nothing. The test then runs again as a
    /// child whose error stream it reads.
    #[test]
    fn a_streamed_scripts_error_stream_reaches_stderr_as_plain_lines() {
        const ROLE: &str = "PWRS_CLIXML_PROBE_DIR";
        if let Some(dir) = std::env::var_os(ROLE) {
            let script = super::materialize_script(std::path::Path::new(&dir), "clixml.ps1", CLIXML_SCRIPT).expect("write the probe script");
            super::stream_pwsh_script(&script, &[]).expect("run the probe script");
            return;
        }
        if let Err(e) = super::pshome() {
            eprintln!("skipped: no pwsh to run a script in ({e})");
            return;
        }
        let dir = std::env::temp_dir().join(format!("pwrs-clixml-probe-{}", std::process::id()));
        let script = super::materialize_script(&dir, "clixml.ps1", CLIXML_SCRIPT).expect("write the probe script");
        let raw = super::host_command(&super::pwsh_exe(), &script, &[]).output().expect("run the probe script");
        let raw_stderr = String::from_utf8_lossy(&raw.stderr);
        assert!(raw_stderr.contains("#< CLIXML"), "the child pwsh wrote no CLIXML, so this test shows nothing:\n{raw_stderr}");
        let exe = std::env::current_exe().expect("the test binary's path");
        let out = std::process::Command::new(exe)
            .args(["--exact", "pwsh::tests::a_streamed_scripts_error_stream_reaches_stderr_as_plain_lines", "--nocapture"])
            .env(ROLE, &dir)
            .output()
            .expect("run the child test");
        std::fs::remove_dir_all(&dir).expect("remove the probe folder");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "the child test failed: {}\n{stderr}", out.status);
        for line in ["WARNING: a warning from the child", "a line straight from the child", "a line after the child"] {
            assert!(stderr.contains(line), "{line:?} is not in:\n{stderr}");
        }
        assert!(!stderr.contains("CLIXML") && !stderr.contains("<Objs"), "CLIXML reached the error stream:\n{stderr}");
    }
}
