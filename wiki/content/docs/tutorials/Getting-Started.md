---
title: Getting Started
weight: 1
---

This tutorial builds one PowerShell module in Rust, **FileFacts**: commands that report facts about files, such as how many lines and words they have, their hashes, and which of them are duplicates. Each chapter adds to the same module. This first one installs the tools, makes the module, and runs its first command in PowerShell 7 and in Windows PowerShell 5.1.

## What you need

- **Rust 1.88 or later.** Install it with [rustup](https://rustup.rs/).
- **PowerShell 7.6 or later**, as `pwsh` on your `PATH`. `cargo pwrs` runs the C# compiler inside it, so you need it even for a module meant only for Windows PowerShell. With PowerShell 7.5, set the environment variable `PWRS_TOOLSET` to `5.3.0` before you build.
- **On Windows, Windows PowerShell 5.1**, which comes with Windows, if you want to run the module there too.
- **No .NET SDK.** The first build downloads the compiler and the reference assemblies it needs, once, into a folder under your home folder.

The modules you build import into PowerShell 7.4 and later and into Windows PowerShell 5.1.

## Install cargo pwrs

`cargo pwrs` is the tool that turns a Rust crate into a PowerShell module. Install it once:

```text
PS> cargo install cargo-pwrs
```

It adds a `pwrs` command to `cargo`.

## Make the module

```text
PS> cargo pwrs new file-facts
pwrs: created file-facts ; next: cargo pwrs test --manifest-dir file-facts
```

`new` makes a Rust crate in a new folder. The crate is named after the folder, `file-facts`, and the module it builds is that name in PascalCase, `FileFacts`. The folder has four files:

| File | What it is |
|---|---|
| `Cargo.toml` | The crate's manifest. Besides Rust's own settings it sets what the module's manifest will say: its description, its tags, and lines you can fill in later. |
| `src/lib.rs` | The module's code: one command to start with. |
| `tests/FileFacts.Tests.ps1` | Two Pester tests of that command. Chapter 8 runs them. |
| `.gitignore` | Lists the build output, so git leaves it out of version control. |

This is `src/lib.rs`:

```rust
use pwrs::prelude::*;

/// Says hello.
///
/// # Examples
/// Get-FileFactsGreeting -Name World
/// 'Ada', 'Bob' | Get-FileFactsGreeting
#[cmdlet(verb = "Get", noun = "FileFactsGreeting", output = ["System.String"])]
#[derive(Default)]
pub struct GetGreeting {
    /// Who to greet.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub name: String,
}

impl Cmdlet for GetGreeting {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write(format!("Hello, {}!", self.name))
    }
}

pwrs::export_module! {
    name: "FileFacts",
    cmdlets: [GetGreeting],
}
```

Each part does one thing:

- `use pwrs::prelude::*;` brings in everything a module uses.
- `#[cmdlet(...)]` makes the struct a PowerShell command. Its name is the verb and the noun joined by a dash, `Get-FileFactsGreeting`. `output` says what type of object it writes, which `Get-Help` shows.
- The `///` comment above the struct becomes the command's help: its first paragraph is the synopsis, and each line under `# Examples` is an example.
- `#[derive(Default)]` is required. PowerShell makes a new instance for each call, starting from `Default`.
- `#[param(...)]` makes a field a parameter. The field `name` becomes `-Name`. `mandatory` means PowerShell prompts for it when it is missing, `position = 0` lets you give it without its name, and `value_from_pipeline` lets it come from the pipeline. The field's `///` comment is the parameter's help.
- `impl Cmdlet` is the command's body. `process` runs once for each object that comes down the pipeline, or once when nothing is piped in. `ps.write` writes an object to the output.
- `export_module!` names the module and lists its commands.

## Build it

```text
PS> cd file-facts
PS> cargo pwrs build
```

Cargo prints its own lines while it compiles. The last two lines are from `cargo pwrs`:

```text
pwrs: file-facts declares no `authors`; the manifest says Author = 'pwrs', which cargo pwrs publish refuses
pwrs: module folder C:\Temp\pwrs-tutorial\file-facts\target\pwrs\FileFacts
```

The first line is a warning. The module's manifest takes its author from `authors` in `Cargo.toml`, which the new crate leaves commented out, so the manifest names `pwrs` instead, and publishing refuses a module like that. It does not stop anything until then; chapter 9 sets it.

The second line names the module folder. It has everything PowerShell needs:

| In the folder | What it is |
|---|---|
| `FileFacts.psd1` | The module manifest. |
| `FileFacts.psm1` | The script that loads the module. |
| `net10.0\` | The command shell PowerShell 7 loads, with its help file. |
| `netstandard2.0\` | The command shell Windows PowerShell 5.1 loads, with its help file, and four .NET Framework files every module includes. |
| `runtimes\win-x64\` | The Rust library in `native\`, built for this computer's platform, with the licenses of the crates it was built from and a record of the build. On Linux the folder is `linux-x64`. |
| `THIRD-PARTY-NOTICES.txt` | The licenses of PWRS's own .NET runtime and of those four .NET Framework files. |

## Run it

Import the module folder and run the command:

```text
PS> Import-Module .\target\pwrs\FileFacts\FileFacts.psd1
PS> Get-FileFactsGreeting World
Hello, World!
PS> 'Ada', 'Bob' | Get-FileFactsGreeting
Hello, Ada!
Hello, Bob!
```

The same folder imports into Windows PowerShell 5.1:

```text
PS> powershell -NoProfile -Command "Import-Module .\target\pwrs\FileFacts\FileFacts.psd1; Get-FileFactsGreeting World"
Hello, World!
```

## A first command of your own

Now make the module do its job. Replace `src/lib.rs` with a command that counts the lines of a file:

```rust
use pwrs::prelude::*;

/// Counts the lines of a text file.
///
/// # Examples
/// Measure-FileFactsText fruit.txt
#[cmdlet(verb = "Measure", noun = "FileFactsText", output = ["System.Int64"])]
#[derive(Default)]
pub struct MeasureText {
    /// The file to read.
    #[param(mandatory, position = 0)]
    pub path: String,
}

impl Cmdlet for MeasureText {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        for file in ps.resolve_path(&self.path, true)? {
            let text = std::fs::read_to_string(&file)?;
            ps.write(text.lines().count() as i64)?;
        }
        Ok(())
    }
}

pwrs::export_module! {
    name: "FileFacts",
    cmdlets: [MeasureText],
}
```

What is new:

- `ps.resolve_path(&self.path, true)` gets the full path of the file from PowerShell. PowerShell has a current folder of its own, which `cd` changes and the process's current folder does not follow. So PowerShell has to turn a file name given to a command into a full path, as it does for its own commands. `true` takes the name as written; chapter 2 lets it expand wildcards. It returns a list, with one path here.
- `std::fs::read_to_string(&file)?` reads the file. The `?` passes a failure on: a Rust error becomes a PowerShell error, which chapter 3 looks at.
- `ps.write(... as i64)` writes the count as a number.

Build again. In a PowerShell window that already imported the module, `Import-Module ... -Force` loads the new build in place of the old one:

```text
PS> cargo pwrs build
PS> Import-Module .\target\pwrs\FileFacts\FileFacts.psd1 -Force
```

Then give it a file. This writes one with three lines, each ending in a line feed, the same bytes on every system:

```powershell
Set-Content fruit.txt "apple`nbanana`ncherry`n" -NoNewline
Measure-FileFactsText fruit.txt
```

```text
3
```

## The whole module so far

<details>
<summary>The files of the crate after this chapter</summary>

<!-- file: Cargo.toml -->
```toml
[package]
name = "file-facts"
version = "0.1.0"
edition = "2024"
# The module manifest takes Author from the first of `authors`,
# Description from `description`, ProjectUri from `repository`, and
# its tags from `keywords`, unless [package.metadata.pwrs] below sets
# the gallery's own. `cargo pwrs publish` refuses a module that has
# not set its own authors and description.
# authors = ["Your Name <you@example.com>"]
description = "FileFacts, a PowerShell module written in Rust"
keywords = ["powershell"]
# repository = "https://example.invalid/file-facts"

# Every other key a module manifest has, each under the manifest
# key's own name in kebab case, since a cargo manifest has no field
# for them, and a description and tags the gallery shows in place of
# the ones above. A key left unset is not written, so the manifest has
# no key without a value.
# [package.metadata.pwrs]
# description = "What the gallery shows about FileFacts."
# tags = ["Windows", "Linux", "MacOS"]
# license-uri = "https://example.invalid/file-facts/blob/main/license"
# release-notes = "First release."
# icon-uri = "https://example.invalid/file-facts/raw/main/icon.png"
# company = "Your Company"
# copyright = "(c) Your Name"
# prerelease = "beta1"                     # a gallery hides it from a plain install
# require-license-acceptance = true
# external-module-dependencies = ["Pester"]
# powershell-version = "7.2"               # default 5.1, what the shells target
# compatible-ps-editions = ["Core"]        # default both, one shell each
# powershell-host-name = "ConsoleHost"
# powershell-host-version = "5.1"
# dotnet-framework-version = "4.7.2"       # Windows PowerShell reads these two;
# clr-version = "4.0"                      # PowerShell Core ignores them
# processor-architecture = "Amd64"
# help-info-uri = "https://example.invalid/file-facts/help"
# default-command-prefix = "Xy"            # inserted into every exported name
# required-modules = ["Storage"]
# required-assemblies = ["System.Xml.dll"]
# scripts-to-process = ["init.ps1"]
# types-to-process = ["FileFacts.Types.ps1xml"]
# nested-modules = ["Extra.psm1"]
# dsc-resources-to-export = ["YourResource"]
# module-list = ["FileFacts"]
# file-list = ["readme.md"]

[lib]
crate-type = ["cdylib"]

[dependencies]
pwrs = { package = "PoWerRuSt", version = "0.8.0" }

# Whole-program LTO and one codegen unit so pwrs inlines into the
# cmdlet body. Leave panic = "unwind" as it is: pwrs catches panics at the
# boundary and reports them as errors instead of aborting the host.
[profile.release]
lto = "fat"
codegen-units = 1
panic = "unwind"
```

<!-- file: src/lib.rs -->
```rust
use pwrs::prelude::*;

/// Counts the lines of a text file.
///
/// # Examples
/// Measure-FileFactsText fruit.txt
#[cmdlet(verb = "Measure", noun = "FileFactsText", output = ["System.Int64"])]
#[derive(Default)]
pub struct MeasureText {
    /// The file to read.
    #[param(mandatory, position = 0)]
    pub path: String,
}

impl Cmdlet for MeasureText {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        for file in ps.resolve_path(&self.path, true)? {
            let text = std::fs::read_to_string(&file)?;
            ps.write(text.lines().count() as i64)?;
        }
        Ok(())
    }
}

pwrs::export_module! {
    name: "FileFacts",
    cmdlets: [MeasureText],
}
```

</details>

The Cargo.toml above leaves out the commented lines `cargo pwrs new` writes for the module manifest's other settings; they change nothing until you fill them in.

Next: [Parameters and the pipeline](../parameters-and-the-pipeline/).
