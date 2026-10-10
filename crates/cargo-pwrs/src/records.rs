//! The classes a built native library carries, read out of the file.
//!
//! Every `#[psclass]` and `#[psenum]` type whose class id the library can
//! ask for leaves a record there: `PWRS-CLASS/1`, a tab, its PowerShell
//! name, a tab, its Rust path, then NUL. They are read from the bytes,
//! as the CPU requirements are, so a library built for another target
//! reads the same way, and a type `export_module!` does not list is found
//! beside the ones it does.

use crate::descriptor::Class;
use crate::Error;

const MARKER: &[u8] = b"PWRS-CLASS/1\t";

/// One class a library carries: its PowerShell name and its Rust path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub path: String,
}

/// Every distinct record in `bytes`, in the order first found.
pub fn read(bytes: &[u8]) -> Result<Vec<Record>, Error> {
    let mut out: Vec<Record> = Vec::new();
    let mut from = 0;
    while let Some(offset) = bytes[from..].windows(MARKER.len()).position(|w| w == MARKER) {
        let start = from + offset + MARKER.len();
        let rest = &bytes[start..];
        let end = rest.iter().position(|&b| b == 0).ok_or_else(|| Error::msg("a class record has no terminating NUL"))?;
        let text = std::str::from_utf8(&rest[..end]).map_err(|e| Error::msg(format!("a class record is not UTF-8: {e}")))?;
        let (name, path) = text.split_once('\t').ok_or_else(|| Error::msg(format!("the class record {text:?} carries no Rust path")))?;
        let record = Record { name: name.to_string(), path: path.to_string() };
        if !out.contains(&record) {
            out.push(record);
        }
        from = start + end;
    }
    Ok(out)
}

/// The library's records against the classes `export_module!` lists,
/// matched by Rust path. A record no listed class has the path of, under
/// a name a listed class or another such record carries, compared
/// without regard to case, is refused, naming both. One under a name of
/// its own is answered as a warning line naming it.
pub fn check(records: &[Record], listed: &[Class]) -> Result<Vec<String>, Error> {
    let unlisted: Vec<&Record> = records.iter().filter(|r| !listed.iter().any(|c| c.path == r.path)).collect();
    let mut warnings = Vec::new();
    for (i, r) in unlisted.iter().enumerate() {
        if let Some(c) = listed.iter().find(|c| c.name.eq_ignore_ascii_case(&r.name)) {
            return Err(Error::msg(format!(
                "{} is declared as {}, which export_module! lists for {}; PowerShell knows one type by one name, compared without regard to case, so give one of them another name",
                r.path, r.name, c.path
            )));
        }
        if let Some(other) = unlisted[..i].iter().find(|o| o.name.eq_ignore_ascii_case(&r.name)) {
            return Err(Error::msg(format!(
                "{} and {} are both declared as {}, and export_module! lists neither; PowerShell knows one type by one name, compared without regard to case, so give one of them another name",
                other.path, r.path, r.name
            )));
        }
        warnings.push(format!(
            "pwrs: {} is declared as {} and export_module! does not list it, so writing one fails with PwrsUnknownClass",
            r.path, r.name
        ));
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, path: &str) -> Vec<u8> {
        format!("PWRS-CLASS/1\t{name}\t{path}\0").into_bytes()
    }

    fn listed(name: &str, path: &str) -> Class {
        serde_json::from_str(&format!(r#"{{"id": 0, "name": "{name}", "rust": "", "path": "{path}", "mode": "copied", "description": "", "fields": []}}"#))
            .expect("a class")
    }

    /// Records are found wherever they sit among other bytes, each once
    /// however often it appears.
    #[test]
    fn every_distinct_record_is_read_out_of_the_bytes() {
        let mut bytes = b"\x7fELF...code...".to_vec();
        bytes.extend(record("Trex.Segment", "trex::recurrence::TrexSegment"));
        bytes.extend(b"...data...PWRS-CLASS/1 without a tab is not a record\0...");
        bytes.extend(record("Trex.Tiling", "trex::grammar::TrexTiling"));
        bytes.extend(record("Trex.Segment", "trex::recurrence::TrexSegment"));
        let found = read(&bytes).expect("the records");
        assert_eq!(
            found,
            vec![
                Record { name: "Trex.Segment".into(), path: "trex::recurrence::TrexSegment".into() },
                Record { name: "Trex.Tiling".into(), path: "trex::grammar::TrexTiling".into() }
            ]
        );
        assert!(read(b"PWRS-CLASS/1\tTrex.Segment").is_err(), "a record without its NUL is refused");
        assert!(read(b"PWRS-CLASS/1\tTrex.Segment\0").is_err(), "a record without a path is refused");
    }

    /// trex's case: a type the module does not list, under the name of
    /// one it does, compared without regard to case, stops the build
    /// naming both; two unlisted under one name stop it too; one under a
    /// name of its own is a warning; listed types alone give nothing.
    #[test]
    fn an_unlisted_namesake_is_refused_and_an_unlisted_class_named() {
        let segment = listed("Trex.Segment", "trex::recurrence::TrexSegment");
        let records = |extra: &[(&str, &str)]| -> Vec<Record> {
            std::iter::once(("Trex.Segment", "trex::recurrence::TrexSegment"))
                .chain(extra.iter().copied())
                .map(|(name, path)| Record { name: name.into(), path: path.into() })
                .collect()
        };
        let refused = check(&records(&[("trex.segment", "trex::grammar::TrexSegment")]), std::slice::from_ref(&segment)).expect_err("an unlisted namesake");
        assert!(
            refused.to_string().contains("trex::grammar::TrexSegment is declared as trex.segment, which export_module! lists for trex::recurrence::TrexSegment"),
            "{refused}"
        );
        let both = check(&records(&[("Trex.Tiling", "trex::grammar::A"), ("TREX.TILING", "trex::grammar::B")]), std::slice::from_ref(&segment))
            .expect_err("two unlisted namesakes");
        assert!(both.to_string().contains("trex::grammar::A and trex::grammar::B are both declared as TREX.TILING, and export_module! lists neither"), "{both}");
        let warned = check(&records(&[("Trex.Tiling", "trex::grammar::TrexTiling")]), std::slice::from_ref(&segment)).expect("a class of its own name");
        assert_eq!(warned, vec!["pwrs: trex::grammar::TrexTiling is declared as Trex.Tiling and export_module! does not list it, so writing one fails with PwrsUnknownClass".to_string()]);
        assert!(check(&records(&[]), std::slice::from_ref(&segment)).expect("listed alone").is_empty());
    }
}
