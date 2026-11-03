//! `cargo pwrs new <name>`: a crate with one cmdlet and its Pester
//! suite, ready for `cargo pwrs test`.

use std::path::Path;

use crate::Error;

fn write(path: &Path, body: &str) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::msg(format!("cannot create {}: {e}", parent.display())))?;
    }
    std::fs::write(path, body).map_err(|e| Error::msg(format!("cannot write {}: {e}", path.display())))
}

fn pascal(s: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' {
            up = true;
        } else if up {
            out.extend(ch.to_uppercase());
            up = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// A TOML basic string containing `s`.
fn basic(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether `s` is a version requirement cargo reads: one or more,
/// separated by commas, each an optional operator, one to three numbers
/// separated by dots, a later one of which may be `*`, and an optional
/// pre-release or build suffix.
fn is_version(s: &str) -> bool {
    !s.trim().is_empty()
        && s.split(',').all(|part| {
            let part = part.trim().trim_start_matches(['=', '^', '~', '<', '>']).trim_start();
            let (core, suffix) = match part.find(['-', '+']) {
                Some(i) => (&part[..i], Some(&part[i + 1..])),
                None => (part, None),
            };
            let numbers: Vec<&str> = core.split('.').collect();
            (1..=3).contains(&numbers.len())
                && numbers.iter().enumerate().all(|(i, n)| (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) || (i > 0 && *n == "*"))
                && suffix.is_none_or(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'+'))
        })
}

/// The refusal of a `--pwrs` value, naming the three forms it takes, and
/// what is wrong with the value when that is known.
fn refusal(given: &str, why: Option<&str>) -> Error {
    let why = match why {
        Some(w) => format!(" ({w})"),
        None => String::new(),
    };
    Error::msg(format!(
        "--pwrs '{given}' is not a dependency on PoWerRuSt{why}: give a TOML dependency value such as '{{ path = \"../pwrs/crates/pwrs\" }}' or '\"1.0.0\"', the path of the PoWerRuSt crate's folder, or a version such as 1.0.0"
    ))
}

/// What follows `pwrs = ` in the new crate's Cargo.toml, from the value
/// `--pwrs` gave. The library's package is PoWerRuSt, imported as `pwrs`,
/// so every form names it. A TOML dependency value is written as given
/// when its table names a package, gains `package = "PoWerRuSt"` when it
/// does not, and a quoted version becomes a table with that version. A
/// path to the crate's folder, one with a Cargo.toml, is written as an
/// absolute `path`, since cargo reads a relative one from the new crate's
/// folder. A version is written as `version`. Anything else is refused,
/// before any file is written.
pub fn dependency(given: &str) -> Result<String, Error> {
    let value = given.trim();
    let named = |key: &str, text: &str| format!("{{ package = \"PoWerRuSt\", {key} = {} }}", basic(text));
    if value.starts_with(['{', '"', '\'']) {
        let parsed: toml::Table = toml::from_str(&format!("pwrs = {value}")).map_err(|e| refusal(given, Some(e.message())))?;
        return match parsed.get("pwrs") {
            Some(toml::Value::String(version)) if is_version(version) => Ok(named("version", version)),
            Some(toml::Value::String(other)) => Err(refusal(given, Some(&format!("'{other}' is not a version")))),
            Some(toml::Value::Table(table)) if ["path", "version", "git"].iter().any(|key| table.contains_key(*key)) => {
                if table.contains_key("package") {
                    Ok(value.to_string())
                } else {
                    Ok(format!("{{ package = \"PoWerRuSt\",{}", &value[1..]))
                }
            }
            Some(toml::Value::Table(_)) => Err(refusal(given, Some("the table has no path, version or git"))),
            Some(other) => Err(refusal(given, Some(&format!("a TOML {} is not a dependency", other.type_str())))),
            None => Err(refusal(given, None)),
        };
    }
    if is_version(value) {
        return Ok(named("version", value));
    }
    let path = Path::new(value);
    if !value.is_empty() && path.join("Cargo.toml").is_file() {
        let absolute = std::path::absolute(path).map_err(|e| Error::msg(format!("--pwrs '{given}': {e}")))?;
        return Ok(named("path", &absolute.to_string_lossy()));
    }
    if !value.is_empty() && (path.exists() || value.contains(['/', '\\'])) {
        return Err(Error::msg(format!("--pwrs '{given}' has no Cargo.toml: give the folder of the PoWerRuSt crate, such as ../pwrs/crates/pwrs")));
    }
    Err(refusal(given, None))
}

pub fn new_module(dir: &Path, pwrs_dependency: &str) -> Result<(), Error> {
    let pwrs_dependency = dependency(pwrs_dependency)?;
    if dir.exists() {
        return Err(Error::msg(format!("{} already exists", dir.display())));
    }
    let crate_name = match dir.file_name() {
        Some(n) => n.to_string_lossy().to_lowercase().replace('_', "-"),
        None => return Err(Error::msg("target directory has no name")),
    };
    let module = pascal(&crate_name);
    write(
        &dir.join("Cargo.toml"),
        &format!(
            "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n# The module manifest takes Author from the first of `authors`,\n# Description from `description`, ProjectUri from `repository`, and\n# its tags from `keywords`, unless [package.metadata.pwrs] below sets\n# the gallery's own. `cargo pwrs publish` refuses a module that has\n# not set its own authors and description.\n# authors = [\"Your Name <you@example.com>\"]\ndescription = \"{module}, a PowerShell module written in Rust\"\nkeywords = [\"powershell\"]\n# repository = \"https://example.invalid/{crate_name}\"\n\n# Every other key a module manifest has, each under the manifest\n# key's own name in kebab case, since a cargo manifest has no field\n# for them, and a description and tags the gallery shows in place of\n# the ones above. A key left unset is not written, so the manifest has\n# no key without a value.\n# [package.metadata.pwrs]\n# description = \"What the gallery shows about {module}.\"\n# tags = [\"Windows\", \"Linux\", \"MacOS\"]\n# license-uri = \"https://example.invalid/{crate_name}/blob/main/license\"\n# release-notes = \"First release.\"\n# icon-uri = \"https://example.invalid/{crate_name}/raw/main/icon.png\"\n# company = \"Your Company\"\n# copyright = \"(c) Your Name\"\n# prerelease = \"beta1\"                     # a gallery hides it from a plain install\n# require-license-acceptance = true\n# external-module-dependencies = [\"Pester\"]\n# powershell-version = \"7.2\"               # default 5.1, what the shells target\n# compatible-ps-editions = [\"Core\"]        # default both, one shell each\n# powershell-host-name = \"ConsoleHost\"\n# powershell-host-version = \"5.1\"\n# dotnet-framework-version = \"4.7.2\"       # Windows PowerShell reads these two;\n# clr-version = \"4.0\"                      # PowerShell Core ignores them\n# processor-architecture = \"Amd64\"\n# help-info-uri = \"https://example.invalid/{crate_name}/help\"\n# default-command-prefix = \"Xy\"            # inserted into every exported name\n# required-modules = [\"Storage\"]\n# required-assemblies = [\"System.Xml.dll\"]\n# scripts-to-process = [\"init.ps1\"]\n# types-to-process = [\"{module}.Types.ps1xml\"]\n# nested-modules = [\"Extra.psm1\"]\n# dsc-resources-to-export = [\"YourResource\"]\n# module-list = [\"{module}\"]\n# file-list = [\"readme.md\"]\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\npwrs = {pwrs_dependency}\n\n# Whole-program LTO and one codegen unit so pwrs inlines into the\n# cmdlet body. Leave panic = \"unwind\" as it is: pwrs catches panics at the\n# boundary and reports them as errors instead of aborting the host.\n[profile.release]\nlto = \"fat\"\ncodegen-units = 1\npanic = \"unwind\"\n"
        ),
    )?;
    write(
        &dir.join("src").join("lib.rs"),
        &format!(
            "use pwrs::prelude::*;\n\n/// Says hello.\n///\n/// # Examples\n/// Get-{module}Greeting -Name World\n/// 'Ada', 'Bob' | Get-{module}Greeting\n#[cmdlet(verb = \"Get\", noun = \"{module}Greeting\", output = [\"System.String\"])]\n#[derive(Default)]\npub struct GetGreeting {{\n    /// Who to greet.\n    #[param(mandatory, position = 0, value_from_pipeline)]\n    pub name: String,\n}}\n\nimpl Cmdlet for GetGreeting {{\n    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {{\n        ps.write(format!(\"Hello, {{}}!\", self.name))\n    }}\n}}\n\npwrs::export_module! {{\n    name: \"{module}\",\n    cmdlets: [GetGreeting],\n}}\n"
        ),
    )?;
    write(
        &dir.join("tests").join(format!("{module}.Tests.ps1")),
        &format!(
            "BeforeAll {{\n    Import-Module (Join-Path $env:PWRS_MODULE '{module}.psd1') -Force -ErrorAction Stop\n}}\n\nDescribe 'Get-{module}Greeting' {{\n    It 'greets by name' {{\n        Get-{module}Greeting -Name x | Should -Be 'Hello, x!'\n    }}\n\n    It 'greets each name piped to it' {{\n        'Ada', 'Bob' | Get-{module}Greeting | Should -Be @('Hello, Ada!', 'Hello, Bob!')\n    }}\n}}\n"
        ),
    )?;
    write(&dir.join(".gitignore"), "/target\n")?;
    eprintln!("pwrs: created {} ; next: cargo pwrs test --manifest-dir {}", dir.display(), dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scaffolded_suite_pipes_into_the_cmdlet_it_declares_as_taking_pipeline_input() {
        let dir = std::env::temp_dir().join(format!("pwrs-scaffold-test-{}", std::process::id())).join("demo-thing");
        new_module(&dir, "{ package = \"PoWerRuSt\", version = \"0.0.0\" }").expect("scaffold");
        let lib = std::fs::read_to_string(dir.join("src").join("lib.rs")).expect("lib.rs");
        let suite = std::fs::read_to_string(dir.join("tests").join("DemoThing.Tests.ps1")).expect("the suite");
        assert!(lib.contains("value_from_pipeline"), "{lib}");
        assert!(suite.contains("'Ada', 'Bob' | Get-DemoThingGreeting"), "{suite}");
        let parent = dir.parent().expect("the scratch folder");
        std::fs::remove_dir_all(parent).expect("remove the scratch folder");
    }

    /// The table in `pwrs = <written>`, as cargo reads it.
    fn read(written: &str) -> toml::Table {
        let parsed: toml::Table = toml::from_str(&format!("pwrs = {written}")).unwrap_or_else(|e| panic!("{written} is not TOML: {e}"));
        parsed["pwrs"].as_table().unwrap_or_else(|| panic!("{written} is not a table")).clone()
    }

    #[test]
    fn a_toml_value_is_written_naming_the_package() {
        let given = "{ package = \"PoWerRuSt\", version = \"0.8.0\" }";
        assert_eq!(dependency(given).expect("a table naming the package"), given);
        let unnamed = read(&dependency("{ path = \"../pwrs/crates/pwrs\" }").expect("a table with a path"));
        assert_eq!(unnamed["package"].as_str(), Some("PoWerRuSt"));
        assert_eq!(unnamed["path"].as_str(), Some("../pwrs/crates/pwrs"));
        let quoted = read(&dependency("\"0.8.0\"").expect("a quoted version"));
        assert_eq!((quoted["package"].as_str(), quoted["version"].as_str()), (Some("PoWerRuSt"), Some("0.8.0")));
    }

    #[test]
    fn a_bare_version_is_written_as_the_version() {
        for version in ["0.8.0", "0.8", "^0.8.0", "=0.8.0", "~0.8", "0.9.0-beta.1", ">=0.8, <0.9", "0.8.*"] {
            let table = read(&dependency(version).unwrap_or_else(|e| panic!("{version}: {e}")));
            assert_eq!((table["package"].as_str(), table["version"].as_str()), (Some("PoWerRuSt"), Some(version)));
        }
    }

    #[test]
    fn a_bare_path_is_written_absolute() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let table = read(&dependency(&here.to_string_lossy()).expect("this crate's folder"));
        assert_eq!(table["package"].as_str(), Some("PoWerRuSt"));
        assert_eq!(Path::new(table["path"].as_str().expect("a path")), here);
        // cargo test runs a crate's tests in its folder, so "." is it.
        let relative = read(&dependency(".").expect("the current folder"));
        assert_eq!(Path::new(relative["path"].as_str().expect("a path")), std::path::absolute(".").expect("absolute"));
    }

    #[test]
    fn anything_else_is_refused_naming_the_three_forms() {
        for bad in ["", "latest", "v0.8", "0.8.x", "pwrs = \"0.8\"", "{ path = ", "{}", "{ features = [\"x\"] }", "\"latest\"", "[1, 2]"] {
            let refused = dependency(bad).expect_err(bad).to_string();
            assert!(refused.contains(&format!("--pwrs '{bad}'")), "{refused}");
            assert!(refused.contains("a TOML dependency value") && refused.contains("the path of the PoWerRuSt crate's folder") && refused.contains("a version such as 1.0.0"), "{refused}");
        }
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let refused = dependency(&folder.to_string_lossy()).expect_err("a folder with no Cargo.toml").to_string();
        assert!(refused.contains("has no Cargo.toml"), "{refused}");
    }

    #[test]
    fn a_refused_value_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("pwrs-scaffold-refused-{}", std::process::id())).join("demo-thing");
        new_module(&dir, "latest").expect_err("refused");
        assert!(!dir.exists(), "{} was made", dir.display());
    }
}
