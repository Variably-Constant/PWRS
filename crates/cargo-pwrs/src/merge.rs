//! `cargo pwrs merge`: one module folder carrying the native libraries
//! of several builds, one `runtimes/<rid>/native/` per platform.

use std::path::{Path, PathBuf};

use crate::Error;

/// The one `<Name>.psd1` in a module folder, and the name.
fn manifest_of(dir: &Path) -> Result<(PathBuf, String), Error> {
    let rd = std::fs::read_dir(dir).map_err(|e| Error::msg(format!("cannot read {}: {e}", dir.display())))?;
    let mut found = Vec::new();
    for entry in rd {
        let p = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", dir.display())))?.path();
        if p.extension().is_some_and(|x| x == "psd1") {
            found.push(p);
        }
    }
    match found.as_slice() {
        [one] => {
            let name = match one.file_stem() {
                Some(stem) => stem.to_string_lossy().into_owned(),
                None => return Err(Error::msg(format!("{} has no file stem", one.display()))),
            };
            Ok((one.clone(), name))
        }
        [] => Err(Error::msg(format!("{} holds no .psd1, so it is not a module folder", dir.display()))),
        many => Err(Error::msg(format!("{} holds {} .psd1 files; a module folder has one", dir.display(), many.len()))),
    }
}

pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(to).map_err(|e| Error::msg(format!("cannot create {}: {e}", to.display())))?;
    let rd = std::fs::read_dir(from).map_err(|e| Error::msg(format!("cannot read {}: {e}", from.display())))?;
    for entry in rd {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", from.display())))?;
        let src = entry.path();
        let dest = to.join(entry.file_name());
        if src.is_dir() {
            copy_tree(&src, &dest)?;
        } else {
            std::fs::copy(&src, &dest).map_err(|e| Error::msg(format!("cannot copy {} to {}: {e}", src.display(), dest.display())))?;
        }
    }
    Ok(())
}

/// The bundled modules inside a module folder: each subfolder holding
/// `<Name>/<Name>.psd1`, by name.
fn bundled_in(dir: &Path) -> Result<Vec<String>, Error> {
    let rd = std::fs::read_dir(dir).map_err(|e| Error::msg(format!("cannot read {}: {e}", dir.display())))?;
    let mut names = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().join(format!("{name}.psd1")).is_file() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Copies every `runtimes/<rid>` of `src` into `into`, both folders of
/// the module `label` names in the messages (empty for the outer
/// module, `<Name>/` for a bundled one). The two must hold the same
/// manifest text; a rid `into` already holds is refused before anything
/// is copied.
fn merge_runtimes(into: &Path, src: &Path, label: &str) -> Result<(), Error> {
    let (into_manifest, name) = manifest_of(into)?;
    let into_text = std::fs::read_to_string(&into_manifest).map_err(|e| Error::msg(format!("cannot read {}: {e}", into_manifest.display())))?;
    let (src_manifest, src_name) = manifest_of(src)?;
    if src_name != name {
        return Err(Error::msg(format!("{} is module {src_name}, not {name}", src.display())));
    }
    let src_text = std::fs::read_to_string(&src_manifest).map_err(|e| Error::msg(format!("cannot read {}: {e}", src_manifest.display())))?;
    if src_text != into_text {
        return Err(Error::msg(format!(
            "the manifests of {} and {} differ; build both from the same checkout",
            src.display(),
            into.display()
        )));
    }
    let runtimes = src.join("runtimes");
    let rd = std::fs::read_dir(&runtimes).map_err(|e| Error::msg(format!("cannot read {}: {e}", runtimes.display())))?;
    for entry in rd {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", runtimes.display())))?;
        let rid = entry.file_name().to_string_lossy().into_owned();
        let dest = into.join("runtimes").join(&rid);
        if dest.exists() {
            return Err(Error::msg(format!("{} already holds {label}runtimes/{rid}; remove it first", into.display())));
        }
        copy_tree(&entry.path(), &dest)?;
        eprintln!("pwrs: merged {label}runtimes/{rid} from {}", src.display());
    }
    Ok(())
}

/// Copies every `runtimes/<rid>` of the `from` folders into `into`,
/// and every `<Bundled>/runtimes/<rid>` into the destination's copy of
/// that bundled module. The folders must be builds of one module from
/// one source, which the manifests being identical text checks, the
/// bundled ones included; a rid already present in `into` is an error
/// rather than an overwrite, and a bundled module `into` lacks is an
/// error rather than a folder created.
pub fn merge(into: &Path, from: &[PathBuf]) -> Result<(), Error> {
    for src in from {
        merge_runtimes(into, src, "")?;
        for bundled in bundled_in(src)? {
            let into_bundled = into.join(&bundled);
            if !into_bundled.join(format!("{bundled}.psd1")).is_file() {
                return Err(Error::msg(format!(
                    "{} carries the bundled module {bundled} and {} does not; build both from the same checkout",
                    src.display(),
                    into.display()
                )));
            }
            merge_runtimes(&into_bundled, &src.join(&bundled), &format!("{bundled}/"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module_folder(root: &Path, rid: &str, lib: &str, manifest: &str) -> PathBuf {
        let native = root.join("runtimes").join(rid).join("native");
        std::fs::create_dir_all(&native).expect("create native dir");
        std::fs::write(native.join(lib), b"library bytes").expect("write library");
        std::fs::write(root.join("Demo.psd1"), manifest).expect("write manifest");
        root.to_path_buf()
    }

    #[test]
    fn merges_rids_and_refuses_duplicates_and_mismatches() {
        let scratch = std::env::temp_dir().join(format!("pwrs-merge-test-{}", std::process::id()));
        let into = module_folder(&scratch.join("win"), "win-x64", "demo.dll", "@{ ModuleVersion = '0.0.1' }");
        let linux = module_folder(&scratch.join("linux"), "linux-x64", "libdemo.so", "@{ ModuleVersion = '0.0.1' }");
        std::fs::write(linux.join("runtimes/linux-x64/native/demo-helper"), b"helper bytes").expect("write helper");
        merge(&into, std::slice::from_ref(&linux)).expect("merge linux");
        assert!(into.join("runtimes/win-x64/native/demo.dll").is_file());
        assert!(into.join("runtimes/linux-x64/native/libdemo.so").is_file());
        assert!(into.join("runtimes/linux-x64/native/demo-helper").is_file(), "a helper beside the library is merged with it");
        let again = merge(&into, std::slice::from_ref(&linux)).expect_err("linux-x64 is already there");
        assert!(again.to_string().contains("already holds runtimes/linux-x64"), "{again}");
        let other = module_folder(&scratch.join("other"), "osx-arm64", "libdemo.dylib", "@{ ModuleVersion = '0.0.2' }");
        let differs = merge(&into, &[other]).expect_err("the manifests differ");
        assert!(differs.to_string().contains("manifests"), "{differs}");
        std::fs::remove_dir_all(&scratch).expect("remove the scratch folder");
    }

    /// A bundled module's runtimes are merged into the destination's
    /// copy of it, with the same refusal for a rid already held, and a
    /// bundled module the destination lacks is refused rather than made.
    #[test]
    fn merges_bundled_runtimes_beside_the_outer_ones() {
        let scratch = std::env::temp_dir().join(format!("pwrs-merge-bundled-test-{}", std::process::id()));
        let into = module_folder(&scratch.join("win"), "win-x64", "demo.dll", "@{ ModuleVersion = '0.0.1' }");
        let linux = module_folder(&scratch.join("linux"), "linux-x64", "libdemo.so", "@{ ModuleVersion = '0.0.1' }");
        let inner_manifest = "@{ ModuleVersion = '0.0.3' }";
        for (root, rid, lib) in [(&into, "win-x64", "inner.dll"), (&linux, "linux-x64", "libinner.so")] {
            let native = root.join("Inner").join("runtimes").join(rid).join("native");
            std::fs::create_dir_all(&native).expect("create the bundled native dir");
            std::fs::write(native.join(lib), b"inner bytes").expect("write the bundled library");
            std::fs::write(root.join("Inner").join("Inner.psd1"), inner_manifest).expect("write the bundled manifest");
        }
        merge(&into, std::slice::from_ref(&linux)).expect("merge linux with its bundled module");
        assert!(into.join("runtimes/linux-x64/native/libdemo.so").is_file());
        assert!(into.join("Inner/runtimes/linux-x64/native/libinner.so").is_file(), "the bundled module's linux library is merged");
        assert!(into.join("Inner/runtimes/win-x64/native/inner.dll").is_file(), "the bundled module's own rid stays");
        let again = merge(&into, std::slice::from_ref(&linux)).expect_err("linux-x64 is already there");
        assert!(again.to_string().contains("already holds runtimes/linux-x64"), "{again}");

        let bare = module_folder(&scratch.join("bare"), "osx-arm64", "libdemo.dylib", "@{ ModuleVersion = '0.0.1' }");
        let missing = merge(&bare, std::slice::from_ref(&linux)).expect_err("bare has no Inner");
        assert!(missing.to_string().contains("carries the bundled module Inner"), "{missing}");
        std::fs::remove_dir_all(&scratch).expect("remove the scratch folder");
    }
}
