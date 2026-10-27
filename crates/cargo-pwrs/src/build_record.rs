//! What a build found that `cargo pwrs publish` refuses, stored beside the
//! library it built as `runtimes/<rid>/pwrs-build.json`: the manifest
//! metadata it stood placeholders in for, the instruction-set extensions
//! the library requires that the module does not declare, and the crates
//! the library links whose source has no license text. `merge` copies
//! each runtime folder whole, so a folder joined from several platforms
//! has every build's record, and `publish --folder` refuses what any of
//! them found as `publish` refuses what its own build finds.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Error;

/// The record's file name in a runtime folder.
pub const FILE: &str = "pwrs-build.json";

/// One build's record. A record with a field this version of
/// cargo-pwrs does not define is refused rather than read without it,
/// since the field may be a refusal this version cannot check.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildRecord {
    /// The cargo-pwrs that built the runtime, as `cargo-pwrs <version>`.
    pub built_by: String,
    /// Whether it was a release build, which `publish --folder --release`
    /// requires of every runtime.
    pub release: bool,
    /// The `[package]` keys the manifest has a placeholder for.
    pub missing_metadata: Vec<String>,
    /// Extensions the library requires beyond its target's baseline that
    /// the module's `cpu-features` does not list.
    pub undeclared_cpu_features: Vec<String>,
    /// The linked crates whose source has no license file and for
    /// which the module supplies none, as `name version`.
    pub crates_without_license_text: Vec<String>,
}

impl BuildRecord {
    /// Writes the record into `runtime_dir`.
    pub fn write(&self, runtime_dir: &Path) -> Result<(), Error> {
        let path = runtime_dir.join(FILE);
        let text = serde_json::to_string_pretty(self).map_err(|e| Error::msg(format!("cannot write {}: {e}", path.display())))?;
        std::fs::write(&path, text + "\n").map_err(|e| Error::msg(format!("cannot write {}: {e}", path.display())))
    }

    /// The record in `runtime_dir`.
    pub fn read(runtime_dir: &Path) -> Result<BuildRecord, Error> {
        let path = runtime_dir.join(FILE);
        let text = std::fs::read_to_string(&path).map_err(|e| Error::msg(format!("cannot read {}: {e}", path.display())))?;
        serde_json::from_str(&text).map_err(|e| {
            Error::msg(format!(
                "{} is not a build record this cargo-pwrs {} reads: {e}. A cargo-pwrs newer than this one may have written it; publish with that one",
                path.display(),
                env!("CARGO_PKG_VERSION")
            ))
        })
    }
}

/// A runtime folder of a module folder, by its runtime identifier, and
/// the record its build left there, when it left one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub rid: String,
    pub record: Option<BuildRecord>,
}

/// The runtime folders in `module_dir/runtimes`, in identifier order.
pub fn runtimes(module_dir: &Path) -> Result<Vec<Runtime>, Error> {
    let dir = module_dir.join("runtimes");
    let rd = std::fs::read_dir(&dir).map_err(|e| Error::msg(format!("cannot read {}: {e}", dir.display())))?;
    let mut out = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", dir.display())))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let record = if path.join(FILE).is_file() { Some(BuildRecord::read(&path)?) } else { None };
        out.push(Runtime { rid: entry.file_name().to_string_lossy().into_owned(), record });
    }
    out.sort_by(|a, b| a.rid.cmp(&b.rid));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cargo-pwrs-build-record-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("clear the scratch folder");
        }
        std::fs::create_dir_all(&dir).expect("make the scratch folder");
        dir
    }

    #[test]
    fn a_record_reads_back_as_written_and_each_runtime_is_found_with_or_without_one() {
        let module = scratch("runtimes");
        let record = BuildRecord {
            built_by: String::from("cargo-pwrs 0.7.0"),
            release: true,
            missing_metadata: vec![String::from("authors")],
            undeclared_cpu_features: Vec::new(),
            crates_without_license_text: vec![String::from("spirv 0.4.0+sdk-1.4.341.0")],
        };
        for rid in ["win-x64", "osx-arm64", "linux-x64"] {
            std::fs::create_dir_all(module.join("runtimes").join(rid).join("native")).expect("make a runtime folder");
        }
        record.write(&module.join("runtimes").join("osx-arm64")).expect("write the record");
        record.write(&module.join("runtimes").join("win-x64")).expect("write the record");
        std::fs::write(module.join("runtimes").join("README.txt"), "not a runtime").expect("write a stray file");
        let found = runtimes(&module).expect("read the runtimes");
        let rids: Vec<&str> = found.iter().map(|r| r.rid.as_str()).collect();
        assert_eq!(rids, ["linux-x64", "osx-arm64", "win-x64"], "the folders, in order, and no file");
        assert_eq!(found[0].record, None, "a runtime whose build left no record has none");
        assert_eq!(found[1].record.as_ref(), Some(&record));
        std::fs::remove_dir_all(&module).expect("remove the scratch folder");
    }

    #[test]
    fn a_record_with_a_field_this_cargo_pwrs_does_not_read_is_refused() {
        let runtime = scratch("unknown");
        std::fs::write(
            runtime.join(FILE),
            r#"{"built_by":"cargo-pwrs 9.0.0","release":true,"missing_metadata":[],"undeclared_cpu_features":[],"crates_without_license_text":[],"unsigned_binaries":["x"]}"#,
        )
        .expect("write a newer record");
        let refused = match BuildRecord::read(&runtime) {
            Ok(r) => panic!("a record with an unknown field was read as {r:?}"),
            Err(e) => e.to_string(),
        };
        assert!(refused.contains("is not a build record this cargo-pwrs") && refused.contains("unsigned_binaries"), "{refused}");
        std::fs::remove_dir_all(&runtime).expect("remove the scratch folder");
    }
}
