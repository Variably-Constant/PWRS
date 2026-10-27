//! The surface check: what the suite exercised of what each cmdlet
//! declares.
//!
//! PowerShell acts on two declarations whether or not the body does its
//! part, and neither shows an error when it does not.
//! `SupportsShouldProcess` gives a cmdlet `-WhatIf` and `-Confirm`
//! whether or not it ever calls `should_process`, so `-WhatIf` on a
//! cmdlet that never calls it does the very thing it should only
//! describe. A parameter that takes pipeline input, by value or by
//! property name, accepts whatever is piped to it whether or not the
//! body handles it, so a cmdlet can offer pipeline input that no test
//! ever sent it. A `literal_path` parameter is one of these: the shell
//! declares it `ValueFromPipelineByPropertyName`, bound from `PSPath`.
//!
//! The declarations alone cannot show either, so the check uses what
//! the tests ran: `pwrs::surface` records what each phase did while
//! `PWRS_SURFACE_DIR` is set, both hosts leave a file behind, and this
//! compares them with the descriptor.
//!
//! A finding is about the suite as much as the cmdlet. A cmdlet that
//! never called `should_process` either has a body that does not call
//! it or was never tested on the branch that does, and the wording says
//! so: either a test or a `should_process` call is missing, and only
//! the author can tell which.

use std::collections::BTreeMap;
use std::path::Path;

use crate::descriptor::Module;
use crate::Error;

/// One cmdlet's observations, summed over every host that ran.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Exercised {
    /// Phase calls of any phase.
    pub phases: u64,
    /// Calls to `should_process` or `should_continue`.
    pub asks: u64,
    /// Bit `i % 64` of word `i / 64` set when parameter `i` was assigned
    /// during a process phase, which is where a value from the pipeline
    /// arrives.
    pub piped: Vec<u64>,
}

impl Exercised {
    /// Whether parameter `index` took a value from the pipeline.
    fn piped_into(&self, index: u32) -> bool {
        let index = index as usize;
        match self.piped.get(index / 64) {
            Some(word) => word & (1u64 << (index % 64)) != 0,
            None => false,
        }
    }
}

/// Sums every `.tsv` a run left in `dir`, one per native image in each
/// host process, keyed by cmdlet name, and adds to `unread` a note for
/// each line it could not read.
/// A directory with no files is an empty table, which reports every
/// cmdlet as never invoked.
pub fn read(dir: &Path, unread: &mut Vec<String>) -> Result<BTreeMap<String, Exercised>, Error> {
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
        unread.extend(merge(&text, &path.display().to_string(), &mut all));
    }
    Ok(all)
}

/// One line of a file: the cmdlet's name, its phase calls, its asks, and
/// its piped bits, one decimal word per 64 parameters joined by commas.
fn parse_line(line: &str) -> Result<(&str, u64, u64, Vec<u64>), String> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [name, phases, asks, piped, ..] = fields.as_slice() else {
        return Err(format!("it has {} of its 4 fields", fields.len()));
    };
    let phases = phases.parse::<u64>().map_err(|e| format!("its phase count {phases:?} is not a count: {e}"))?;
    let asks = asks.parse::<u64>().map_err(|e| format!("its ask count {asks:?} is not a count: {e}"))?;
    let words = piped
        .split(',')
        .map(|word| word.parse::<u64>().map_err(|e| format!("its piped word {word:?} is not a number: {e}")))
        .collect::<Result<Vec<u64>, String>>()?;
    Ok((name, phases, asks, words))
}

/// Adds the lines of `file`, whose text is `text`, to the table, and
/// returns a note for each line it could not read, such as the last line
/// of a host killed while it wrote.
fn merge(text: &str, file: &str, all: &mut BTreeMap<String, Exercised>) -> Vec<String> {
    let mut unread = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let (name, phases, asks, words) = match parse_line(line) {
            Ok(parsed) => parsed,
            Err(why) => {
                unread.push(format!("pwrs: surface check: {file} line {} could not be read, so its counts are left out: {why}", n + 1));
                continue;
            }
        };
        let entry = all.entry(name.to_string()).or_default();
        entry.phases += phases;
        entry.asks += asks;
        if entry.piped.len() < words.len() {
            entry.piped.resize(words.len(), 0);
        }
        for (word, bits) in entry.piped.iter_mut().zip(&words) {
            *word |= bits;
        }
    }
    unread
}

/// One line per declaration nothing exercised, in the order the
/// module declares its cmdlets.
pub fn findings(module: &Module, seen: &BTreeMap<String, Exercised>) -> Vec<String> {
    let mut out = Vec::new();
    let never = Exercised::default();
    for c in &module.cmdlets {
        let e = match seen.get(&c.name) {
            Some(e) => e,
            None => &never,
        };
        if e.phases == 0 {
            out.push(format!("{} was never invoked, so none of its declarations were checked.", c.name));
            continue;
        }
        if c.should_process && e.asks == 0 {
            out.push(format!(
                "{} declares SupportsShouldProcess and ran {} times without calling should_process. Either its body never calls it, so -WhatIf and -Confirm are accepted and the change still happens, or no test reached the branch that calls it.",
                c.name, e.phases
            ));
        }
        for p in &c.params {
            let takes_pipeline = p.pipeline || p.pipeline_by_name || p.literal_path;
            if takes_pipeline && !e.piped_into(p.index) {
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
    fn a_cmdlet_that_ran_called_should_process_and_was_piped_into_reports_nothing() {
        let mut seen = BTreeMap::new();
        seen.insert("Remove-Thing".to_string(), Exercised { phases: 3, asks: 1, piped: vec![0b1] });
        assert!(findings(&module(ONE_CMDLET), &seen).is_empty());
    }

    #[test]
    fn a_pipeline_parameter_past_the_first_64_is_read_from_its_own_word() {
        let wide = ONE_CMDLET.replace(r#""index": 0"#, r#""index": 65"#);
        let mut seen = BTreeMap::new();
        seen.insert("Remove-Thing".to_string(), Exercised { phases: 3, asks: 1, piped: vec![0, 0b10] });
        assert!(findings(&module(&wide), &seen).is_empty());
        seen.insert("Remove-Thing".to_string(), Exercised { phases: 3, asks: 1, piped: vec![0b10] });
        let found = findings(&module(&wide), &seen);
        assert_eq!(found.len(), 1, "a word the table does not have is no record: {found:?}");
        assert!(found[0].contains("-InputObject"), "{}", found[0]);
    }

    #[test]
    fn should_process_declared_and_never_called_is_reported() {
        let mut seen = BTreeMap::new();
        seen.insert("Remove-Thing".to_string(), Exercised { phases: 3, asks: 0, piped: vec![0b1] });
        let found = findings(&module(ONE_CMDLET), &seen);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("SupportsShouldProcess"), "{}", found[0]);
        assert!(found[0].contains("ran 3 times"), "{}", found[0]);
    }

    #[test]
    fn a_pipeline_parameter_nothing_was_piped_into_is_reported() {
        let mut seen = BTreeMap::new();
        seen.insert("Remove-Thing".to_string(), Exercised { phases: 3, asks: 2, piped: vec![0] });
        let found = findings(&module(ONE_CMDLET), &seen);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("-InputObject"), "{}", found[0]);
    }

    #[test]
    fn a_literal_path_parameter_is_checked_as_one_taken_by_property_name() {
        let literal = ONE_CMDLET
            .replace(r#""pipeline": true"#, r#""pipeline": false"#)
            .replace(r#""literal_path": false"#, r#""literal_path": true"#);
        let mut seen = BTreeMap::new();
        seen.insert("Remove-Thing".to_string(), Exercised { phases: 3, asks: 1, piped: vec![0] });
        let found = findings(&module(&literal), &seen);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("-InputObject from the pipeline"), "{}", found[0]);
        seen.insert("Remove-Thing".to_string(), Exercised { phases: 3, asks: 1, piped: vec![0b1] });
        assert!(findings(&module(&literal), &seen).is_empty());
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
        assert!(merge("Get-Thing\t4\t0\t1\n", "a.tsv", &mut all).is_empty());
        assert!(merge("Get-Thing\t6\t2\t4\nGet-Other\t1\t0\t0\n", "b.tsv", &mut all).is_empty());
        assert_eq!(all["Get-Thing"], Exercised { phases: 10, asks: 2, piped: vec![0b101] });
        assert_eq!(all["Get-Other"], Exercised { phases: 1, asks: 0, piped: vec![0] });
    }

    #[test]
    fn piped_words_past_the_first_join_word_by_word() {
        let mut all = BTreeMap::new();
        assert!(merge("Get-Wide\t2\t0\t1,0,8\n", "a.tsv", &mut all).is_empty());
        assert!(merge("Get-Wide\t1\t0\t2\n", "b.tsv", &mut all).is_empty());
        assert_eq!(all["Get-Wide"], Exercised { phases: 3, asks: 0, piped: vec![0b11, 0, 8] });
    }

    #[test]
    fn a_line_that_cannot_be_read_is_left_out_with_a_note_naming_it() {
        let mut all = BTreeMap::new();
        let unread = merge("truncated\nGet-Thing\tx\t0\t0\nGet-Thing\t1\t0\t0\nGet-Thing\t1\t0\t1,y\n", "a.tsv", &mut all);
        assert_eq!(all["Get-Thing"], Exercised { phases: 1, asks: 0, piped: vec![0] });
        assert_eq!(unread.len(), 3, "{unread:?}");
        assert!(unread[0].contains("a.tsv line 1 could not be read") && unread[0].contains("it has 1 of its 4 fields"), "{}", unread[0]);
        assert!(unread[1].contains("line 2") && unread[1].contains("its phase count \"x\" is not a count"), "{}", unread[1]);
        assert!(unread[2].contains("line 4") && unread[2].contains("its piped word \"y\" is not a number"), "{}", unread[2]);
    }
}
