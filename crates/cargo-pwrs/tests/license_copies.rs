//! Every published crate carries the repository's license text.
//!
//! A package holds only what sits in its own folder, so the root
//! LICENSE reaches crates.io only through a copy in each crate. A module
//! built on these crates quotes that copy in its notices, and cargo-pwrs
//! embeds its own for the managed runtime it compiles into every module.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/cargo-pwrs.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn every_published_crate_carries_the_repository_license() {
    let root = repo_root();
    let license = std::fs::read(root.join("LICENSE")).expect("read the repository LICENSE");
    let mut checked = Vec::new();
    let mut wrong = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).expect("read crates/") {
        let dir = entry.expect("read an entry of crates/").path();
        let manifest = match std::fs::read_to_string(dir.join("Cargo.toml")) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("read {}: {e}", dir.join("Cargo.toml").display()),
        };
        if manifest.lines().any(|l| l.split_whitespace().collect::<String>() == "publish=false") {
            continue;
        }
        let copy = dir.join("LICENSE");
        match std::fs::read(&copy) {
            Ok(bytes) if bytes == license => checked.push(copy.display().to_string()),
            Ok(_differs) => wrong.push(format!("{} differs from the repository LICENSE", copy.display())),
            Err(e) => wrong.push(format!("{}: {e}", copy.display())),
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    assert_eq!(checked.len(), 5, "the five published crates, found {checked:#?}");
}
