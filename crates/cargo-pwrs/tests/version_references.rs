//! The version appears in prose that nothing else checks.
//!
//! Everything else takes it from the workspace or from
//! `CARGO_PKG_VERSION`, but a README tells the reader what to write in
//! their own manifest and has to say a literal. crates.io freezes a
//! README per version, so a stale literal there cannot be corrected
//! without releasing again.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/cargo-pwrs.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Every PoWerRuSt version a README's dependency lines name.
fn versions_named(readme: &str) -> Vec<&str> {
    readme
        .match_indices("PoWerRuSt\", version = \"")
        .map(|(i, m)| {
            let rest = &readme[i + m.len()..];
            &rest[..rest.find('"').unwrap_or(0)]
        })
        .collect()
}

#[test]
fn the_readme_tells_the_reader_the_version_that_is_being_released() {
    let version = env!("CARGO_PKG_VERSION");
    let readme = std::fs::read_to_string(repo_root().join("README.md")).expect("read README.md");

    let wanted = format!("pwrs = {{ package = \"PoWerRuSt\", version = \"{version}\" }}");
    assert!(
        readme.contains(&wanted),
        "README.md does not carry the dependency line for {version}.\n\
         Looked for: {wanted}\n\
         crates.io freezes the README of each version, so a reader of {version} would be told to depend on something else."
    );

    // Any other three-part version in the README is a different claim
    // than the one above and is stale by construction.
    let stale: Vec<&str> = versions_named(&readme).into_iter().filter(|v| *v != version).collect();
    assert!(stale.is_empty(), "README.md names other PoWerRuSt versions: {stale:?}");
}

/// crates.io shows each crate its own README, and one that tells the
/// reader to depend on PoWerRuSt names a version as literally as the
/// root README does.
#[test]
fn every_crate_readme_names_the_version_that_is_being_released() {
    let version = env!("CARGO_PKG_VERSION");
    let crates = repo_root().join("crates");
    let mut naming = Vec::new();
    let mut stale = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("read crates/") {
        let path = entry.expect("read an entry of crates/").path().join("README.md");
        let readme = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("read {}: {e}", path.display()),
        };
        let named = versions_named(&readme);
        if named.is_empty() {
            continue;
        }
        naming.push(path.display().to_string());
        for v in named {
            if v != version {
                stale.push(format!("{} names {v}", path.display()));
            }
        }
    }
    assert!(!naming.is_empty(), "no README under {} names a PoWerRuSt version, so nothing was checked", crates.display());
    assert!(stale.is_empty(), "crates.io freezes each crate's README per version, and these name something other than {version}: {stale:?}");
}

/// A crate of this workspace requires its siblings at exactly the
/// version being released: `version = "0.2.3"` is ^0.2.3 to cargo, and a
/// later pwrs-sys whose host table grew would then build into a
/// PoWerRuSt that does not match it, or not build at all.
#[test]
fn every_sibling_requirement_pins_the_version_being_released_exactly() {
    let version = env!("CARGO_PKG_VERSION");
    let crates = repo_root().join("crates");
    let mut pins = Vec::new();
    let mut loose = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("read crates/") {
        let path = entry.expect("read an entry of crates/").path().join("Cargo.toml");
        let manifest = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("read {}: {e}", path.display()),
        };
        for line in manifest.lines().filter(|l| l.contains("path = \"../") && l.contains("version = \"")) {
            pins.push(format!("{}: {line}", path.display()));
            if !line.contains(&format!("version = \"={version}\"")) {
                loose.push(format!("{}: {line}", path.display()));
            }
        }
    }
    assert!(!pins.is_empty(), "no crate under {} requires a sibling with a version, so nothing was checked", crates.display());
    assert!(loose.is_empty(), "these sibling requirements do not read version = \"={version}\": {loose:?}");
}
