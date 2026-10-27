---
title: Shipping
weight: 9
---

FileFacts works, and its tests say so in both hosts. This last chapter turns it into a package for the PowerShell Gallery, the one `Install-PSResource` installs from. It covers what the package says about its author, the release build, the license notices that go with the library, a dry run that packages without publishing, and publishing from one computer or from several.

## Who made it

Every build so far has warned that the crate declares no `authors`. The module's manifest takes its `Author` from the first of `authors`, its `Description` from `description`, its `ProjectUri` from `repository` and its tags from `keywords`, and `cargo pwrs publish` refuses a module that has not set its own author and description. In `Cargo.toml`, uncomment `authors` and `repository` and put your own name, address and repository in them:

```toml
authors = ["Your Name <you@example.com>"]
description = "FileFacts, a PowerShell module written in Rust"
keywords = ["powershell"]
repository = "https://example.invalid/file-facts"
```

The commented keys under `[package.metadata.pwrs]` set the rest of the manifest, such as `license-uri` and `release-notes`. Its `description` and `tags` take the place of `description` and `keywords` in the manifest, so a gallery listing can say more than crates.io does. A key left commented is not written into the manifest at all.

## A release build

```text
PS> cargo pwrs build --release
```

`--release` builds the library with optimizations and with the whole-program LTO that the scaffold's `[profile.release]` turns on, and writes no `.pdb` files. With the author set, the warning is gone, and the last line is the module folder:

```text
pwrs: module folder C:\Temp\pwrs-tutorial\file-facts\target\pwrs\FileFacts
```

The library is compiled for the processors its target names, so it loads on any x86-64 computer. A build compiled for this computer's own instruction sets, with `-C target-cpu=native` in `RUSTFLAGS`, would fail to import on a computer without them. So the build warns about it, and `publish` refuses it unless `cpu-features` under `[package.metadata.pwrs]` lists them. [How To Use Instruction Sets](../../how-to/how-to-use-instruction-sets/) shows how a module can use those instruction sets and still load on every computer.

## What goes with the library

A native library contains every crate it links, so a package that ships the library also passes those crates on, and most of their licenses require their text to go with them. The build writes `runtimes/win-x64/THIRD-PARTY-NOTICES.txt` beside the library, listing each crate with the license it declares and, word for word, the license files in its source. FileFacts' library links eleven:

| Crate | Version | License |
|---|---|---|
| PoWerRuSt | 0.8.0 | MIT |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| const-oid | 0.10.2 | Apache-2.0 OR MIT |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 |
| digest | 0.11.3 | MIT OR Apache-2.0 |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 |
| pwrs-sys | 0.8.0 | MIT |
| sha2 | 0.11.1 | MIT OR Apache-2.0 |
| typenum | 1.20.1 | MIT OR Apache-2.0 |

The build refuses a crate whose license is not among those a module may ship without saying so, the permissive licenses such as MIT and Apache-2.0, until `allowed-licenses` under `[package.metadata.pwrs]` names it. [How To Publish](../../how-to/how-to-publish/#license-notices) covers the rest, such as supplying the text for a crate whose source has none.

## A dry run

```text
PS> cargo pwrs publish --release --dry-run
    Finished `release` profile [optimized] target(s) in 0.02s
pwrs: module folder C:\Temp\pwrs-tutorial\file-facts\target\pwrs\FileFacts
packaged C:\Temp\pwrs-tutorial\file-facts\target\pwrs\publish\FileFacts\FileFacts.0.1.0.nupkg
```

`publish` builds the module again, checks its manifest with `Test-ModuleManifest`, and with `--dry-run` packages it with `Compress-PSResource`, as the gallery would receive it, and sends nothing. The package's version is the crate's, `0.1.0`. The dry run runs in PowerShell 7 and needs the `Compress-PSResource` command of PSResourceGet 1.1 or later, which Microsoft's PowerShell 7.4 and later include.

## Publishing

```text
PS> $env:PWRS_PSGALLERY_KEY = '<your API key>'
PS> cargo pwrs publish --release
```

This runs the same checks, then `Publish-PSResource`, with the API key from your PowerShell Gallery account, and prints `published FileFacts to PSGallery`. This tutorial does not run it: FileFacts is the tutorial's own, not a module for the gallery. Once a module is published, `Install-PSResource -Name <Module>` installs it in PowerShell 7 and `Install-Module -Name <Module>` in Windows PowerShell.

## More than one platform

`publish` packages the library of the computer it runs on. For a module that should load on Linux or macOS as well, build it on each, join the module folders with `cargo pwrs merge` on one of them, and publish the joined folder as it stands with `--folder`:

```text
cargo pwrs build --release                                # on each platform
cargo pwrs merge target/pwrs/FileFacts <module folder>... # on one, with the other platforms' folders
cargo pwrs publish --release --folder target/pwrs/FileFacts --dry-run
cargo pwrs publish --release --folder target/pwrs/FileFacts
```

Each build records what it found that `publish` refuses, such as a missing author or an instruction set the module does not declare, and `publish --folder` refuses what any platform's build found, naming the platform. [How To Publish](../../how-to/how-to-publish/#a-module-built-on-several-platforms) has the details.

## Where to go from here

That is the whole of FileFacts, from a scaffold to a package. The [how-to guides](../../how-to/) go further: a helper executable shipped beside the library, a module bundled inside another, C# beside the Rust, a thread that runs a window, a provider for a drive of your own, and code that picks an instruction set at run time. The [reference](../../reference/) lists every attribute, every type and every `cargo pwrs` command and flag.

## The whole module so far

<details>
<summary>Cargo.toml after this chapter</summary>

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
authors = ["Your Name <you@example.com>"]
description = "FileFacts, a PowerShell module written in Rust"
keywords = ["powershell"]
repository = "https://example.invalid/file-facts"

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
sha2 = "0.11.1"

# Whole-program LTO and one codegen unit so pwrs inlines into the
# cmdlet body. Leave panic = "unwind" as it is: pwrs catches panics at the
# boundary and reports them as errors instead of aborting the host.
[profile.release]
lto = "fat"
codegen-units = 1
panic = "unwind"
```

</details>
