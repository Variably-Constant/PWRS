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

pub fn new_module(dir: &Path, pwrs_dependency: &str) -> Result<(), Error> {
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
            "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n# The module manifest takes Author from the first of `authors`,\n# Description from `description`, ProjectUri from `repository`, and\n# its tags from `keywords`. `cargo pwrs publish` refuses a module\n# that has not set its own authors and description.\n# authors = [\"Your Name <you@example.com>\"]\ndescription = \"{module}, a PowerShell module written in Rust\"\nkeywords = [\"powershell\"]\n# repository = \"https://example.invalid/{crate_name}\"\n\n# Everything else a module manifest carries, each under the manifest\n# key's own name in kebab case. A cargo manifest has no field for any\n# of them, and one left unset is written nowhere, so the manifest\n# carries no key it has no value for.\n# [package.metadata.pwrs]\n# license-uri = \"https://example.invalid/{crate_name}/blob/main/license\"\n# release-notes = \"First release.\"\n# icon-uri = \"https://example.invalid/{crate_name}/raw/main/icon.png\"\n# company = \"Your Company\"\n# copyright = \"(c) Your Name\"\n# prerelease = \"beta1\"                     # a gallery hides it from a plain install\n# require-license-acceptance = true\n# external-module-dependencies = [\"Pester\"]\n# powershell-version = \"7.2\"               # default 5.1, what the shells target\n# compatible-ps-editions = [\"Core\"]        # default both, one shell each\n# powershell-host-name = \"ConsoleHost\"\n# powershell-host-version = \"5.1\"\n# dotnet-framework-version = \"4.7.2\"       # Windows PowerShell reads these two;\n# clr-version = \"4.0\"                      # PowerShell Core ignores them\n# processor-architecture = \"Amd64\"\n# help-info-uri = \"https://example.invalid/{crate_name}/help\"\n# default-command-prefix = \"Xy\"            # inserted into every exported name\n# required-modules = [\"Storage\"]\n# required-assemblies = [\"System.Xml.dll\"]\n# scripts-to-process = [\"init.ps1\"]\n# types-to-process = [\"{module}.Types.ps1xml\"]\n# nested-modules = [\"Extra.psm1\"]\n# dsc-resources-to-export = [\"YourResource\"]\n# module-list = [\"{module}\"]\n# file-list = [\"readme.md\"]\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\npwrs = {pwrs_dependency}\n\n# Whole-program LTO and one codegen unit so pwrs inlines into the\n# cmdlet body. Keep panic = \"unwind\": pwrs catches panics at the\n# boundary and reports them as errors instead of aborting the host.\n[profile.release]\nlto = \"fat\"\ncodegen-units = 1\npanic = \"unwind\"\n"
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
}
