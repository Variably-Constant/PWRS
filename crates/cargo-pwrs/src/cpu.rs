//! The instruction-set extensions a built native library was compiled
//! to require, read out of the file, and the ones its target assumes
//! of every CPU anyway.
//!
//! The list is the `pwrs_cpu_requirements` data `pwrs::cpu` builds:
//! `PWRS-CPU/1`, then a space and one comma-separated entry per
//! extension, name first, then NUL. It is read from the bytes rather
//! than by loading the library, so a library built for another target
//! reads the same way.

use std::process::Command;

use crate::Error;

const MARKER: &[u8] = b"PWRS-CPU/1";

/// The extension names the library in `bytes` lists, or `None` when it
/// carries no list, which a library built before the list existed does
/// not.
pub fn required(bytes: &[u8]) -> Result<Option<Vec<String>>, Error> {
    let mut found: Option<Vec<String>> = None;
    let mut from = 0;
    while let Some(offset) = bytes[from..].windows(MARKER.len()).position(|w| w == MARKER) {
        let start = from + offset + MARKER.len();
        let rest = &bytes[start..];
        let end = rest.iter().position(|&b| b == 0).ok_or_else(|| Error::msg("the CPU requirement list has no terminating NUL"))?;
        let text = std::str::from_utf8(&rest[..end]).map_err(|e| Error::msg(format!("the CPU requirement list is not ASCII: {e}")))?;
        let names: Vec<String> = text
            .split(' ')
            .filter(|e| !e.is_empty())
            .map(|e| match e.split_once(',') {
                Some((name, _fields)) => name.to_string(),
                None => e.to_string(),
            })
            .collect();
        if let Some(earlier) = &found {
            if *earlier != names {
                return Err(Error::msg("the library carries two different CPU requirement lists"));
            }
        }
        found = Some(names);
        from = start + end;
    }
    Ok(found)
}

/// The target features rustc enables for `triple` (the building
/// machine's when `None`) with no flags at all: what the target assumes
/// of every CPU it runs on. `rustc` itself reads neither `RUSTFLAGS`
/// nor a cargo config, so a machine's own `target-cpu` does not reach
/// this answer.
pub fn target_baseline(triple: Option<&str>) -> Result<Vec<String>, Error> {
    let rustc = rustc()?;
    let mut cmd = Command::new(&rustc);
    cmd.args(["--print", "cfg"]);
    if let Some(t) = triple {
        cmd.args(["--target", t]);
    }
    let out = cmd.output().map_err(|e| Error::msg(format!("cannot run {rustc} --print cfg: {e}")))?;
    if !out.status.success() {
        return Err(Error::msg(format!("{rustc} --print cfg failed: {}", String::from_utf8_lossy(&out.stderr))));
    }
    Ok(features_in_cfg(&String::from_utf8_lossy(&out.stdout)))
}

/// The rustc cargo would run: `RUSTC` when set, else `rustc` on PATH.
fn rustc() -> Result<String, Error> {
    match std::env::var("RUSTC") {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        Ok(_empty) => Ok("rustc".to_string()),
        Err(std::env::VarError::NotPresent) => Ok("rustc".to_string()),
        Err(std::env::VarError::NotUnicode(raw)) => Err(Error::msg(format!("RUSTC is not valid Unicode: {}", raw.to_string_lossy()))),
    }
}

/// The building machine's own target triple, as `rustc -vV` names it.
pub fn host_triple() -> Result<String, Error> {
    let rustc = rustc()?;
    let out = Command::new(&rustc).arg("-vV").output().map_err(|e| Error::msg(format!("cannot run {rustc} -vV: {e}")))?;
    if !out.status.success() {
        return Err(Error::msg(format!("{rustc} -vV failed: {}", String::from_utf8_lossy(&out.stderr))));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    match text.lines().find_map(|l| l.strip_prefix("host: ")) {
        Some(host) => Ok(host.trim().to_string()),
        None => Err(Error::msg(format!("{rustc} -vV names no host"))),
    }
}

/// The `target_feature` values in `rustc --print cfg` output.
fn features_in_cfg(cfg: &str) -> Vec<String> {
    cfg.lines().filter_map(|l| l.trim().strip_prefix("target_feature=")).map(|v| v.trim_matches('"').to_string()).collect()
}

/// The extensions in `required` beyond `baseline` and not in
/// `declared`: the ones a build warns about and a publish refuses.
pub fn undeclared(required: &[String], baseline: &[String], declared: &[String]) -> Vec<String> {
    required.iter().filter(|n| !baseline.contains(n) && !declared.contains(n)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::{features_in_cfg, required, undeclared};

    fn library_with(list: &str) -> Vec<u8> {
        let mut bytes = b"\x7fELF...code...".to_vec();
        bytes.extend_from_slice(list.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(b"...more sections...");
        bytes
    }

    #[test]
    fn the_names_come_out_of_the_bytes() {
        let lib = library_with("PWRS-CPU/1 avx2,3,7,0,1,5,6 avx512f,4,7,0,1,16,230");
        assert_eq!(required(&lib).expect("reads"), Some(vec!["avx2".to_string(), "avx512f".to_string()]));
        assert_eq!(required(&library_with("PWRS-CPU/1")).expect("reads"), Some(Vec::new()), "a baseline build lists nothing");
        assert_eq!(required(b"a library built before the list existed").expect("reads"), None);
    }

    #[test]
    fn two_lists_that_disagree_are_refused() {
        let mut lib = library_with("PWRS-CPU/1 avx2,3,7,0,1,5,6");
        lib.extend_from_slice(b"PWRS-CPU/1 fma,3,1,0,2,12,6\0");
        assert!(required(&lib).is_err());
    }

    #[test]
    fn the_baseline_and_the_declared_ones_are_not_warned_about() {
        let cfg = "debug_assertions\ntarget_arch=\"x86_64\"\ntarget_feature=\"cmpxchg16b\"\ntarget_feature=\"sse3\"\ntarget_os=\"windows\"\n";
        let baseline = features_in_cfg(cfg);
        assert_eq!(baseline, vec!["cmpxchg16b".to_string(), "sse3".to_string()]);
        let req: Vec<String> = ["sse3", "avx2", "avx512f"].iter().map(|s| s.to_string()).collect();
        assert_eq!(undeclared(&req, &baseline, &["avx2".to_string()]), vec!["avx512f".to_string()]);
    }
}
