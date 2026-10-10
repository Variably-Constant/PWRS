//! The surface check: what the suite exercised of what each cmdlet
//! declares.
//!
//! Two declarations are promises the engine keeps whether or not the
//! body holds up its end, and neither breaks loudly.
//! `SupportsShouldProcess` gives a cmdlet `-WhatIf` and `-Confirm`
//! whether or not it ever calls `should_process`, so `-WhatIf` on a
//! cmdlet that never asks does the thing it was supposed to
//! describe. A parameter that declares `ValueFromPipeline` binds
//! whether or not anything is ever piped into it, so a cmdlet can
//! advertise a pipeline it has never been handed a record through.
//!
//! Neither is decidable from the declaration, so the run is the
//! evidence: `pwrs::surface` records what each phase saw while
//! `PWRS_SURFACE_DIR` is set, both hosts leave a file behind, and
//! this reads them against the descriptor.
//!
//! A finding is about the suite as much as the cmdlet. "Never asked"
//! is either a body that does not ask or a test that never reached
//! the branch that does, and the wording says so: what answers it is
//! a test or a `should_process`, and only the author knows which.

use std::collections::BTreeMap;
use std::path::Path;

use crate::descriptor::Module;
use crate::Error;

/// One cmdlet's observations, summed over every host that ran.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Exercised {
    /// Phase calls of any phase.
    pub phases: u64,
    /// Calls to `should_process` or `should_continue`.
    pub asks: u64,
    /// Bit `i` set when parameter `i` was assigned during a process
    /// phase, which is where a value from the pipeline arrives.
    pub piped: u64,
}

/// Sums every `.tsv` a run left in `dir`, one per native image in each
/// host process, keyed by cmdlet name.
/// A directory with no files is an empty table, which reports every
/// cmdlet as never invoked.
pub fn read(dir: &Path) -> Result<BTreeMap<String, Exercised>, Error> {
    let mut all: BTreeMap<String, Exercised> = BTreeMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_absent) => return Ok(all),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("tsv") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| Error::msg(format!("cannot read {}: {e}", path.display())))?;
        merge(&text, &mut all);
    }
    Ok(all)
}

/// Adds one file's lines to the table.
fn merge(text: &str, all: &mut BTreeMap<String, Exercised>) {
    for line in text.lines() {
        let mut fields = line.split('\t');
        let (Some(name), Some(phases), Some(asks), Some(piped)) = (fields.next(), fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        let (Ok(phases), Ok(asks), Ok(piped)) = (phases.parse::<u64>(), asks.parse::<u64>(), piped.parse::<u64>()) else {
            continue;
        };
        let entry = all.entry(name.to_string()).or_default();
        entry.phases += phases;
        entry.asks += asks;
        entry.piped |= piped;
    }
}

/// One line per declaration nothing exercised, in the order the
/// module declares its cmdlets.
pub fn findings(module: &Module, seen: &BTreeMap<String, Exercised>) -> Vec<String> {
    let mut out = Vec::new();
    for c in &module.cmdlets {
        let e = seen.get(&c.name).copied().unwrap_or_default();
        if e.phases == 0 {
            out.push(format!("{} was never invoked, so nothing here was checked of it.", c.name));
            continue;
        }
        if c.should_process && e.asks == 0 {
            out.push(format!(
                "{} declares SupportsShouldProcess and ran {} times without asking. Either the body never calls should_process, and -WhatIf and -Confirm accept and then do the thing, or no test reached the branch that asks.",
                c.name, e.phases
            ));
        }
        for p in &c.params {
            if (p.pipeline || p.pipeline_by_name) && e.piped & (1u64 << p.index) == 0 {
                out.push(format!("{} takes -{} from the pipeline and nothing was ever piped into it.", c.name, p.name));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(json: &str) -> Module {
        serde_json::from_str(json).expect("descriptor")
    }

    const ONE_CMDLET: &str = r#"{
        "abi": 1,
        "name": "Demo",
        "cmdlets": [{
            "id": 0, "verb": "Remove", "noun": "Thing", "name": "Remove-Thing", "rust": "RemoveThing",
            "should_process": true, "confirm_impact": null, "default_set": null,
            "aliases": [], "output_types": [], "synopsis": "", "description": "",
            "params": [{
                "name": "InputObject", "rust": "input_object", "index": 0, "clr": "System.Object",
                "slot": "handle", "optional": false, "mandatory": true, "position": null, "set": null,
                "pipeline": true, "pipeline_by_name": false, "remaining": false, "aliases": [],
                "help": "", "validate_set": [], "validate_range": null, "validate_pattern": null,
                "not_null_or_empty": false, "dont_show": false, "literal_path": false
            }]
        }]
    }"#;

    #[test]
    fn a_cmdlet_that_ran_and_asked_and_was_piped_into_reports_nothing() {
        let mut seen = BTreeMap::new();
        seen.insert("Remove-Thing".to_string(), Exercised { phases: 3, asks: 1, piped: 0b1 });
        assert!(findings(&module(ONE_CMDLET), &seen).is_empty());
    }

    #[test]
    fn should_process_declared_and_never_asked_is_reported() {
        let mut seen = BTreeMap::new();
        seen.insert("Remove-Thing".to_string(), Exercised { phases: 3, asks: 0, piped: 0b1 });
        let found = findings(&module(ONE_CMDLET), &seen);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("SupportsShouldProcess"), "{}", found[0]);
        assert!(found[0].contains("ran 3 times"), "{}", found[0]);
    }

    #[test]
    fn a_pipeline_parameter_nothing_was_piped_into_is_reported() {
        let mut seen = BTreeMap::new();
        seen.insert("Remove-Thing".to_string(), Exercised { phases: 3, asks: 2, piped: 0 });
        let found = findings(&module(ONE_CMDLET), &seen);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("-InputObject"), "{}", found[0]);
    }

    #[test]
    fn a_cmdlet_no_test_invoked_is_reported_once_and_not_for_each_declaration() {
        let found = findings(&module(ONE_CMDLET), &BTreeMap::new());
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("never invoked"), "{}", found[0]);
    }

    #[test]
    fn the_files_of_both_hosts_sum_and_their_piped_bits_join() {
        let mut all = BTreeMap::new();
        merge("Get-Thing\t4\t0\t1\n", &mut all);
        merge("Get-Thing\t6\t2\t4\nGet-Other\t1\t0\t0\n", &mut all);
        assert_eq!(all["Get-Thing"], Exercised { phases: 10, asks: 2, piped: 0b101 });
        assert_eq!(all["Get-Other"], Exercised { phases: 1, asks: 0, piped: 0 });
    }

    #[test]
    fn a_line_that_is_not_four_numbers_is_skipped_rather_than_failing_the_run() {
        let mut all = BTreeMap::new();
        merge("truncated\nGet-Thing\tx\t0\t0\nGet-Thing\t1\t0\t0\n", &mut all);
        assert_eq!(all["Get-Thing"], Exercised { phases: 1, asks: 0, piped: 0 });
    }
}
