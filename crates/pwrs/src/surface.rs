//! What a run exercised of each cmdlet's declared surface.
//!
//! A cmdlet's declarations are promises the engine keeps on its
//! behalf: `SupportsShouldProcess` makes `-WhatIf` and `-Confirm`
//! appear whether or not the body ever asks, and a parameter that
//! declares `ValueFromPipeline` binds whether or not anything is ever
//! piped into it. Neither promise is checkable from the declaration,
//! and neither breaks loudly: `-WhatIf` on a cmdlet that never asks
//! does the thing it was supposed to describe.
//!
//! So the run is the evidence. While `PWRS_SURFACE_DIR` names a
//! directory, every phase call records what it saw against the cmdlet
//! type's PowerShell name, and the table is written to
//! `<dir>/<pid>-<image>.tsv`, one file per native image in each
//! process. `cargo pwrs test` sets the variable, reads every file the
//! hosts leave behind, and reports each declaration nothing exercised.
//!
//! Unset, which is every production call, a phase pays one relaxed
//! load and a branch.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use pwrs_sys::{PsPhase, PS_PHASE_PROCESS};

/// One cmdlet type's observations, summed over the process.
#[derive(Clone, Copy, Default)]
pub struct Observed {
    /// Phase calls of any phase.
    pub phases: u64,
    /// Calls to `should_process` or `should_continue`.
    pub asks: u64,
    /// Bit `i` set when parameter `i` was assigned during a process
    /// phase, which is where a value from the pipeline arrives. A
    /// value given as an argument is assigned before `begin` and sets
    /// no bit here.
    pub piped: u64,
}

static SEEN: Mutex<BTreeMap<&'static str, Observed>> = Mutex::new(BTreeMap::new());
static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// The directory from `PWRS_SURFACE_DIR`, read once and cached.
fn dir() -> Option<&'static PathBuf> {
    DIR.get_or_init(|| match std::env::var("PWRS_SURFACE_DIR") {
        Ok(v) if !v.trim().is_empty() => Some(PathBuf::from(v)),
        _unset_or_empty => None,
    })
    .as_ref()
}

/// Whether this process is recording what it exercised.
#[inline]
pub fn enabled() -> bool {
    dir().is_some()
}

/// Records one phase call. `dirty` is the phase's parameter-assignment
/// word and `asked` whether the body asked the engine to confirm.
pub(crate) fn record(name: &'static str, phase: PsPhase, dirty: u64, asked: bool) {
    let Ok(mut seen) = SEEN.lock() else {
        return;
    };
    let entry = seen.entry(name).or_default();
    entry.phases += 1;
    if asked {
        entry.asks += 1;
    }
    if phase == PS_PHASE_PROCESS {
        entry.piped |= dirty;
    }
}

/// The table as the file carries it: one tab-separated line per
/// cmdlet, `name`, `phases`, `asks`, `piped`.
fn table() -> String {
    let Ok(seen) = SEEN.lock() else {
        return String::new();
    };
    let mut s = String::new();
    for (name, o) in seen.iter() {
        s.push_str(&format!("{}\t{}\t{}\t{}\n", name, o.phases, o.asks, o.piped));
    }
    s
}

/// The file this native image writes: the process, and the address of
/// this image's own table. Two PWRS modules in one process are two
/// images with two tables, and so are two generations of one module
/// after a reload, so each writes its own file and none replaces
/// another's; the tool sums them by cmdlet name.
fn file_name() -> String {
    format!("{}-{:x}.tsv", std::process::id(), &SEEN as *const Mutex<BTreeMap<&'static str, Observed>> as usize)
}

/// Writes this image's table to its file, replacing what was there.
///
/// Called at every instance release rather than at exit, because a
/// library loaded by a host has no exit of its own to run at, and a
/// host that is killed mid-suite still leaves everything observed
/// before it. A failed write is dropped: this is a report about a
/// test run and must not change what the run does.
pub(crate) fn dump() {
    let Some(dir) = dir() else {
        return;
    };
    let _made = std::fs::create_dir_all(dir);
    let _written = std::fs::write(dir.join(file_name()), table());
}

#[cfg(test)]
mod tests {
    use super::*;

    // The table is process-wide, so the cases share it and use names
    // no other test writes.
    #[test]
    fn a_phase_is_counted_against_its_cmdlet() {
        record("Test-SurfaceCount", PS_PHASE_PROCESS, 0, false);
        record("Test-SurfaceCount", PS_PHASE_PROCESS, 0, false);
        let seen = SEEN.lock().expect("table");
        assert_eq!(seen["Test-SurfaceCount"].phases, 2);
        assert_eq!(seen["Test-SurfaceCount"].asks, 0);
    }

    #[test]
    fn a_parameter_assigned_at_process_is_piped_and_one_assigned_at_begin_is_not() {
        record("Test-SurfacePiped", pwrs_sys::PS_PHASE_BEGIN, 0b0001, false);
        record("Test-SurfacePiped", PS_PHASE_PROCESS, 0b0100, true);
        let seen = SEEN.lock().expect("table");
        assert_eq!(seen["Test-SurfacePiped"].piped, 0b0100);
        assert_eq!(seen["Test-SurfacePiped"].asks, 1);
    }

    #[test]
    fn the_table_is_one_line_per_cmdlet() {
        record("Test-SurfaceLine", PS_PHASE_PROCESS, 0b10, false);
        let line = table().lines().find(|l| l.starts_with("Test-SurfaceLine\t")).map(String::from).expect("line");
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields[0], "Test-SurfaceLine");
        assert_eq!(fields[3], "2");
    }

    #[test]
    fn the_file_names_the_process_and_this_image_and_stays_the_same() {
        let name = file_name();
        assert!(name.starts_with(&format!("{}-", std::process::id())), "{name}");
        assert!(name.ends_with(".tsv"), "{name}");
        assert_eq!(name, file_name(), "one image writes one file");
    }
}
