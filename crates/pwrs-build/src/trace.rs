//! `PWRS_TRACE` for the processes PWRS's build tools start. With the
//! variable on, each process gets one line on stderr when it starts,
//! naming its pid, program, arguments, the variables set or removed for
//! it and its folder, and one when it ends, with how long it ran and its
//! status. With it off, a process runs exactly as `Command::output` and
//! `Command::status` run it.

use std::ffi::OsStr;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const UNREAD: u8 = u8::MAX;
static LEVEL: AtomicU8 = AtomicU8::new(UNREAD);

/// The level `PWRS_TRACE` sets, read once and cached; 0 is off.
pub fn level() -> u8 {
    let cached = LEVEL.load(Ordering::Relaxed);
    if cached != UNREAD {
        return cached;
    }
    let parsed = match std::env::var("PWRS_TRACE") {
        Ok(v) => parse_level(&v),
        Err(_unset) => 0,
    };
    LEVEL.store(parsed, Ordering::Relaxed);
    parsed
}

/// A `PWRS_TRACE` value as a level, as the module's own trace reads it:
/// a number as written, empty as 0, and any other text as 1.
fn parse_level(value: &str) -> u8 {
    match value.trim() {
        "" => 0,
        text => text.parse::<u8>().unwrap_or(1),
    }
}

/// [`Command::output`], with the trace's two lines when it is on.
pub fn output(cmd: &mut Command) -> std::io::Result<Output> {
    output_traced(cmd, level() > 0)
}

/// [`Command::status`], with the trace's two lines when it is on.
pub fn status(cmd: &mut Command) -> std::io::Result<ExitStatus> {
    status_traced(cmd, level() > 0)
}

/// `cmd`'s output, given as `Command::output` gives it: stdin empty, and
/// stdout and stderr captured.
fn output_traced(cmd: &mut Command, traced: bool) -> std::io::Result<Output> {
    if !traced {
        return cmd.output();
    }
    let (child, at) = start(cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()))?;
    let pid = child.id();
    let out = child.wait_with_output();
    end(pid, at, out.as_ref().map(|o| &o.status));
    out
}

/// `cmd`'s status, with its streams as `Command::status` leaves them.
fn status_traced(cmd: &mut Command, traced: bool) -> std::io::Result<ExitStatus> {
    if !traced {
        return cmd.status();
    }
    let (mut child, at) = start(cmd)?;
    let status = child.wait();
    end(child.id(), at, status.as_ref());
    status
}

/// Starts `cmd` and writes its start line, or a line saying it did not
/// start.
fn start(cmd: &mut Command) -> std::io::Result<(Child, Instant)> {
    match cmd.spawn() {
        Ok(child) => {
            eprintln!("pwrs trace process start pid={} t={} {}", child.id(), now(), describe(cmd));
            Ok((child, Instant::now()))
        }
        Err(e) => {
            eprintln!("pwrs trace process not started t={} {}: {e}", now(), describe(cmd));
            Err(e)
        }
    }
}

/// Writes the end line of process `pid`, started at `at`.
fn end(pid: u32, at: Instant, result: Result<&ExitStatus, &std::io::Error>) {
    let ms = at.elapsed().as_millis();
    match result {
        Ok(status) => eprintln!("pwrs trace process end pid={pid} t={} ms={ms} {status}", now()),
        Err(e) => eprintln!("pwrs trace process end pid={pid} t={} ms={ms} wait failed: {e}", now()),
    }
}

/// `cmd`'s program and arguments, each variable set or removed for it,
/// and its folder when one is set. A variable whose name reads as a
/// secret is named without its value.
fn describe(cmd: &Command) -> String {
    let mut text = word(cmd.get_program());
    for arg in cmd.get_args() {
        text.push(' ');
        text.push_str(&word(arg));
    }
    for (key, value) in cmd.get_envs() {
        let name = key.to_string_lossy();
        match value {
            Some(_) if secret(&name) => text.push_str(&format!(" env {name}=(withheld)")),
            Some(v) => text.push_str(&format!(" env {name}={}", word(v))),
            None => text.push_str(&format!(" env -{name}")),
        }
    }
    if let Some(dir) = cmd.get_current_dir() {
        text.push_str(&format!(" in {}", word(dir.as_os_str())));
    }
    text
}

/// `s` as one word: quoted when it is empty or holds whitespace.
fn word(s: &OsStr) -> String {
    let t = s.to_string_lossy();
    if t.is_empty() || t.chars().any(char::is_whitespace) { format!("\"{t}\"") } else { t.into_owned() }
}

/// Whether a variable named `name` reads as holding a secret.
fn secret(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL"].iter().any(|w| upper.contains(w))
}

/// Seconds since the Unix epoch to the millisecond, so lines from
/// several processes read against one clock.
fn now() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{:03}", d.as_secs(), d.subsec_millis()),
        Err(before) => format!("-{:.3}", before.duration().as_secs_f64()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_reads_as_the_module_trace_reads_it() {
        for (value, level) in [("", 0), (" 0 ", 0), ("1", 1), ("2", 2), ("yes", 1)] {
            assert_eq!(parse_level(value), level, "PWRS_TRACE={value:?}");
        }
    }

    /// The description names the program, each argument as one word, the
    /// variables set and removed with a secret's value withheld, and the
    /// folder.
    #[test]
    fn a_process_is_described_with_its_arguments_environment_and_folder() {
        let mut cmd = Command::new("pwsh");
        cmd.args(["-File", "a b.ps1", ""])
            .env("XDG_CACHE_HOME", "/tmp/c")
            .env("PWRS_PSGALLERY_KEY", "not-to-be-printed")
            .env_remove("PSModulePath")
            .current_dir("/work");
        let text = describe(&cmd);
        assert!(text.starts_with("pwsh -File \"a b.ps1\" \"\""), "{text}");
        assert!(text.contains(" env XDG_CACHE_HOME=/tmp/c"), "{text}");
        assert!(text.contains(" env PWRS_PSGALLERY_KEY=(withheld)") && !text.contains("not-to-be-printed"), "{text}");
        assert!(text.contains(" env -PSModulePath"), "{text}");
        assert!(text.ends_with(" in /work"), "{text}");
    }

    /// Traced, a process gives the output and status it gives untraced,
    /// and one that cannot start is the same error.
    #[test]
    fn a_traced_process_gives_what_it_gives_untraced() {
        let run = |traced: bool| output_traced(Command::new("cargo").arg("--version"), traced).expect("run cargo --version");
        let (plain, traced) = (run(false), run(true));
        assert!(traced.status.success(), "{:?}", traced.status);
        assert_eq!(plain.status.code(), traced.status.code());
        assert_eq!(plain.stdout, traced.stdout);
        let status = status_traced(Command::new("cargo").arg("--version").stdout(Stdio::null()), true).expect("run cargo --version");
        assert!(status.success(), "{status:?}");
        let missing = |traced: bool| output_traced(&mut Command::new("pwrs-no-such-program"), traced).expect_err("a missing program started");
        assert_eq!(missing(true).kind(), missing(false).kind());
    }
}
