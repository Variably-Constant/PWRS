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
//!
//! The table is a list that only grows: each cmdlet type's entry holds
//! atomic counters and is pushed at the head by compare-exchange the
//! first time the type is seen, and no entry is ever freed or unlinked,
//! so a phase on any thread records with a walk of acquire loads and
//! relaxed read-modify-writes.

use std::path::PathBuf;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::OnceLock;

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

/// One cmdlet type's counters in the table.
struct Entry {
    name: &'static str,
    phases: AtomicU64,
    asks: AtomicU64,
    piped: AtomicU64,
    /// The entry that was the head when this one was pushed.
    next: Option<&'static Entry>,
}

impl Entry {
    fn observed(&self) -> Observed {
        Observed { phases: self.phases.load(Ordering::Relaxed), asks: self.asks.load(Ordering::Relaxed), piped: self.piped.load(Ordering::Relaxed) }
    }
}

/// The newest entry, or null before the first phase is recorded. Every
/// pointer it has held came from `Box::into_raw` and was never freed.
static HEAD: AtomicPtr<Entry> = AtomicPtr::new(std::ptr::null_mut());
static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// The entry `head` points at.
fn at(head: *mut Entry) -> Option<&'static Entry> {
    // SAFETY: `head` is null or a pointer HEAD held, which came from
    // Box::into_raw and is never freed, so it is valid for 'static; the
    // entry was written in full before the release half of the
    // compare-exchange that published it, which the acquire load that
    // read `head` synchronizes with.
    unsafe { head.as_ref() }
}

/// The table's entries, newest first.
fn entries() -> impl Iterator<Item = &'static Entry> {
    std::iter::successors(at(HEAD.load(Ordering::Acquire)), |e| e.next)
}

/// The entry for `name`, pushed when the table has none. A push that
/// loses the compare-exchange looks again from the head it lost to, so
/// two threads seeing a type first at once make one entry between them.
fn entry(name: &'static str) -> &'static Entry {
    let mut head = HEAD.load(Ordering::Acquire);
    let mut made: Option<Box<Entry>> = None;
    loop {
        let first = at(head);
        if let Some(found) = std::iter::successors(first, |e| e.next).find(|e| e.name == name) {
            return found;
        }
        let mut fresh = made.take().unwrap_or_else(|| {
            Box::new(Entry { name, phases: AtomicU64::new(0), asks: AtomicU64::new(0), piped: AtomicU64::new(0), next: None })
        });
        fresh.next = first;
        let raw = Box::into_raw(fresh);
        match HEAD.compare_exchange(head, raw, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_published) => {
                // SAFETY: `raw` came from Box::into_raw above and is now
                // the head, which is never freed.
                return unsafe { &*raw };
            }
            Err(newer) => {
                // SAFETY: `raw` came from Box::into_raw above and the
                // failed compare-exchange did not publish it.
                made = Some(unsafe { Box::from_raw(raw) });
                head = newer;
            }
        }
    }
}

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
    let entry = entry(name);
    entry.phases.fetch_add(1, Ordering::Relaxed);
    if asked {
        entry.asks.fetch_add(1, Ordering::Relaxed);
    }
    if phase == PS_PHASE_PROCESS {
        entry.piped.fetch_or(dirty, Ordering::Relaxed);
    }
}

/// What the table holds for `name`, or nothing when no phase of it was
/// recorded.
#[cfg(test)]
fn observed(name: &str) -> Option<Observed> {
    entries().find(|e| e.name == name).map(Entry::observed)
}

/// The table as the file carries it: one tab-separated line per
/// cmdlet, `name`, `phases`, `asks`, `piped`, ordered by name.
fn table() -> String {
    let mut rows: Vec<(&'static str, Observed)> = entries().map(|e| (e.name, e.observed())).collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    let mut s = String::new();
    for (name, o) in rows {
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
    format!("{}-{:x}.tsv", std::process::id(), &HEAD as *const AtomicPtr<Entry> as usize)
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
        let seen = observed("Test-SurfaceCount").expect("entry");
        assert_eq!(seen.phases, 2);
        assert_eq!(seen.asks, 0);
    }

    #[test]
    fn a_parameter_assigned_at_process_is_piped_and_one_assigned_at_begin_is_not() {
        record("Test-SurfacePiped", pwrs_sys::PS_PHASE_BEGIN, 0b0001, false);
        record("Test-SurfacePiped", PS_PHASE_PROCESS, 0b0100, true);
        let seen = observed("Test-SurfacePiped").expect("entry");
        assert_eq!(seen.piped, 0b0100);
        assert_eq!(seen.asks, 1);
    }

    #[test]
    fn threads_recording_one_new_cmdlet_at_once_make_one_entry_and_lose_no_phase() {
        let threads: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..1000 {
                        record("Test-SurfaceRace", PS_PHASE_PROCESS, 0b1, true);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().expect("a recording thread panicked");
        }
        assert_eq!(entries().filter(|e| e.name == "Test-SurfaceRace").count(), 1);
        let seen = observed("Test-SurfaceRace").expect("entry");
        assert_eq!(seen.phases, 8000);
        assert_eq!(seen.asks, 8000);
        assert_eq!(seen.piped, 0b1);
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
