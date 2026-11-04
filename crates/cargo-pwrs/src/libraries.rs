//! Libraries of its workspace that a module ships beside its own, from
//! `[package.metadata.pwrs.libraries]`. Each key names a package with a
//! cdylib target, which is built with the module, for the same target and
//! profile, and written to `runtimes/<rid>/<folder>`. The crates it links
//! are held to the module's allowed licenses and listed in the module's
//! notices, and what it is compiled to require counts as the module's
//! library's does.

use std::path::PathBuf;

use crate::Error;

/// A library a module ships from another package of its workspace.
#[derive(Debug, Clone)]
pub struct Library {
    /// The package that builds it, the entry's key.
    pub package: String,
    /// The package's cdylib target, as the built file names it.
    pub lib_name: String,
    /// The package's manifest.
    pub manifest_path: PathBuf,
    /// The folder under `runtimes/<rid>` it is written to.
    pub folder: String,
    /// The Cargo features its build turns on.
    pub features: Vec<String>,
    /// Whether its build takes the package's default features.
    pub default_features: bool,
    /// The expression of the `cfg(...)` table the entry is written in,
    /// which the build's target must satisfy for the entry to apply;
    /// `None` for an entry of the unscoped table.
    pub scope: Option<crate::cfg::Expr>,
}

impl Library {
    /// The arguments that select its features, which `cargo build` and the
    /// notices' `cargo metadata` both take.
    pub fn feature_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if !self.default_features {
            args.push("--no-default-features".to_string());
        }
        if !self.features.is_empty() {
            args.push("--features".to_string());
            args.push(self.features.join(","));
        }
        args
    }
}

/// The table's name in refusals.
const SOURCE: &str = "[package.metadata.pwrs] libraries";

/// The keys an entry's table takes.
const KEYS: [&str; 3] = ["folder", "features", "default-features"];

/// The entries of a `libraries` table: each key a package of the workspace
/// other than `module_package`, each value a table of `folder`, and
/// `features` and `default-features`; and beside them tables under
/// `cfg(...)` keys, as `native-files` scopes its own, whose entries take
/// the same form. `packages` is the workspace's, as `cargo metadata`
/// lists them. An absent table is no entries.
pub fn from_metadata(table: &serde_json::Value, module_package: &str, packages: &[serde_json::Value]) -> Result<Vec<Library>, Error> {
    if table.is_null() {
        return Ok(Vec::new());
    }
    let map = table.as_object().ok_or_else(|| Error::msg(format!("{SOURCE} is a table of packages, each with its folder")))?;
    let mut out = Vec::with_capacity(map.len());
    for (key, value) in map {
        let Some(text) = crate::cfg::key_expression(key) else {
            out.push(entry(key, value, module_package, packages, SOURCE, None)?);
            continue;
        };
        let expr = crate::cfg::parse(text).map_err(|why| Error::msg(format!("{SOURCE} key `{key}` is not a cfg expression: {why}")))?;
        if let Some(name) = crate::cfg::non_target_name(&expr) {
            return Err(Error::msg(format!(
                "{SOURCE} key `{key}` names `{name}`, which describes the compilation rather than the target, and a table applies by target alone"
            )));
        }
        let entries = value.as_object().ok_or_else(|| Error::msg(format!("{SOURCE} key `{key}` is not a table of packages")))?;
        let scoped = format!("{SOURCE}.'{key}'");
        for (name, value) in entries {
            out.push(entry(name, value, module_package, packages, &scoped, Some(&expr))?);
        }
    }
    Ok(out)
}

/// One entry of a `libraries` table, or of one of its `cfg(...)` tables
/// when `scope` is that table's expression.
fn entry(
    name: &str,
    value: &serde_json::Value,
    module_package: &str,
    packages: &[serde_json::Value],
    source: &str,
    scope: Option<&crate::cfg::Expr>,
) -> Result<Library, Error> {
    if name == module_package {
        return Err(Error::msg(format!("{source} names {name}, the module's own package, whose library the module ships already")));
    }
    let table = value.as_object().ok_or_else(|| Error::msg(format!("{source} `{name}` is not a table; it takes {}", KEYS.join(", "))))?;
    if let Some(other) = table.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(Error::msg(format!("{source} `{name}` sets `{other}`; an entry takes {}", KEYS.join(", "))));
    }
    let folder = match table.get("folder") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s.clone(),
        Some(other) => return Err(Error::msg(format!("{source} `{name}` sets `folder` to {other}; it takes text"))),
        None => return Err(Error::msg(format!("{source} `{name}` has no `folder`, the folder under runtimes/<rid> it is written to"))),
    };
    if let Some(why) = crate::native_files::name_refusal(&folder) {
        return Err(Error::msg(format!("{source} `{name}` sets `folder` to {folder}, which {why}")));
    }
    let features = match table.get("features") {
        None => Vec::new(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(String::from).ok_or_else(|| Error::msg(format!("{source} `{name}` lists {v} in `features`, which takes strings"))))
            .collect::<Result<Vec<_>, _>>()?,
        Some(other) => return Err(Error::msg(format!("{source} `{name}` sets `features` to {other}; it takes a list of strings"))),
    };
    let default_features = match table.get("default-features") {
        None => true,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(other) => return Err(Error::msg(format!("{source} `{name}` sets `default-features` to {other}; it takes true or false"))),
    };
    let package = packages
        .iter()
        .find(|p| p["name"].as_str() == Some(name))
        .ok_or_else(|| Error::msg(format!("{source} names {name}, which is not a package of the workspace")))?;
    let targets = package["targets"].as_array().ok_or_else(|| Error::msg(format!("package {name} has no targets")))?;
    let cdylib = targets
        .iter()
        .find(|t| t["kind"].as_array().is_some_and(|kinds| kinds.iter().any(|k| k.as_str() == Some("cdylib"))))
        .ok_or_else(|| Error::msg(format!("{source} names {name}, which has no cdylib target; add `crate-type = [\"cdylib\"]`")))?;
    let lib_name = cdylib["name"].as_str().ok_or_else(|| Error::msg(format!("package {name} has a cdylib target without a name")))?.replace('-', "_");
    let manifest_path = PathBuf::from(package["manifest_path"].as_str().ok_or_else(|| Error::msg(format!("package {name} has no manifest path")))?);
    Ok(Library { package: name.to_string(), lib_name, manifest_path, folder, features, default_features, scope: scope.cloned() })
}

/// The entries that apply to a target whose cfg is `target`: the unscoped
/// table's, and each `cfg(...)` table's whose expression the target
/// satisfies. A package two of them name is refused.
pub fn for_target<'a>(libraries: &'a [Library], target: &[crate::cfg::Cfg]) -> Result<Vec<&'a Library>, Error> {
    let applying: Vec<&Library> = libraries.iter().filter(|l| l.scope.as_ref().is_none_or(|e| e.matches(target))).collect();
    for (i, library) in applying.iter().enumerate() {
        if applying[..i].iter().any(|earlier| earlier.package == library.package) {
            return Err(Error::msg(format!("{SOURCE} names {} twice for this target", library.package)));
        }
    }
    Ok(applying)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The workspace's packages: `demo`, the module, `plugin`, with a
    /// cdylib target, and `tool`, with none.
    fn packages() -> Vec<serde_json::Value> {
        vec![
            json!({ "name": "demo", "manifest_path": "/w/demo/Cargo.toml", "targets": [{ "name": "demo", "kind": ["cdylib"] }] }),
            json!({ "name": "plugin", "manifest_path": "/w/plugin/Cargo.toml", "targets": [{ "name": "gst-plugin", "kind": ["cdylib", "rlib"] }] }),
            json!({ "name": "tool", "manifest_path": "/w/tool/Cargo.toml", "targets": [{ "name": "tool", "kind": ["bin"] }] }),
        ]
    }

    fn read(table: serde_json::Value) -> Result<Vec<Library>, Error> {
        from_metadata(&table, "demo", &packages())
    }

    fn refused(table: serde_json::Value, wanted: &str) {
        match read(table) {
            Ok(entries) => panic!("expected a refusal naming {wanted:?}, got {entries:?}"),
            Err(e) => assert!(e.to_string().contains(wanted), "{e}"),
        }
    }

    #[test]
    fn an_absent_table_is_no_libraries() {
        assert!(read(serde_json::Value::Null).expect("no table").is_empty());
    }

    #[test]
    fn an_entry_reads_its_package_target_folder_and_features() {
        let entries = read(json!({ "plugin": { "folder": "gstreamer-1.0", "features": ["tracer"], "default-features": false } })).expect("one entry");
        let library = &entries[0];
        assert_eq!(library.package, "plugin");
        assert_eq!(library.lib_name, "gst_plugin");
        assert_eq!(library.manifest_path, PathBuf::from("/w/plugin/Cargo.toml"));
        assert_eq!(library.folder, "gstreamer-1.0");
        assert!(library.scope.is_none());
        assert_eq!(library.feature_args(), ["--no-default-features", "--features", "tracer"]);
    }

    #[test]
    fn an_entry_takes_the_default_features_unless_it_says_otherwise() {
        let entries = read(json!({ "plugin": { "folder": "plugins" } })).expect("one entry");
        assert!(entries[0].feature_args().is_empty());
    }

    #[test]
    fn a_cfg_table_applies_only_to_a_target_that_satisfies_it() {
        let table = json!({ "cfg(unix)": { "plugin": { "folder": "plugins" } } });
        let entries = read(table).expect("one scoped entry");
        assert!(entries[0].scope.is_some());
        let linux = crate::cfg::target("unix\ntarget_os=\"linux\"\n").expect("a target");
        let windows = crate::cfg::target("windows\ntarget_os=\"windows\"\n").expect("a target");
        assert_eq!(for_target(&entries, &linux).expect("applies").len(), 1);
        assert!(for_target(&entries, &windows).expect("does not apply").is_empty());
    }

    #[test]
    fn a_package_two_entries_name_for_one_target_is_refused() {
        let table = json!({ "plugin": { "folder": "a" }, "cfg(unix)": { "plugin": { "folder": "b" } } });
        let entries = read(table).expect("two entries");
        let linux = crate::cfg::target("unix\n").expect("a target");
        match for_target(&entries, &linux) {
            Ok(applying) => panic!("expected a refusal, got {applying:?}"),
            Err(e) => assert!(e.to_string().contains("names plugin twice"), "{e}"),
        }
    }

    #[test]
    fn what_cannot_be_shipped_is_refused() {
        refused(json!({ "missing": { "folder": "plugins" } }), "missing, which is not a package of the workspace");
        refused(json!({ "tool": { "folder": "plugins" } }), "tool, which has no cdylib target");
        refused(json!({ "demo": { "folder": "plugins" } }), "the module's own package");
    }

    #[test]
    fn an_entry_of_the_wrong_shape_is_refused() {
        refused(json!({ "plugin": { } }), "has no `folder`");
        refused(json!({ "plugin": { "folder": 3 } }), "it takes text");
        refused(json!({ "plugin": { "folder": "a/b" } }), "which contains");
        refused(json!({ "plugin": { "folder": ".." } }), "which is not a file name");
        refused(json!({ "plugin": { "folder": "plugins", "path": "x" } }), "sets `path`");
        refused(json!({ "plugin": { "folder": "plugins", "features": "tracer" } }), "it takes a list of strings");
        refused(json!({ "plugin": { "folder": "plugins", "default-features": "no" } }), "it takes true or false");
        refused(json!({ "cfg(feature = \"x\")": { "plugin": { "folder": "plugins" } } }), "describes the compilation");
        refused(json!(["plugin"]), "is a table of packages");
    }
}
