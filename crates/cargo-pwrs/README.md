<div align="center">

<img src="https://github.com/Variably-Constant/PWRS/raw/main/assets/pwrs-logo.svg" alt="PWRS logo" width="240"/>

# cargo-pwrs

**The build tool for PWRS: a Rust crate in, a PowerShell module folder out.**

[![crates.io](https://img.shields.io/crates/v/cargo-pwrs?style=flat-square)](https://crates.io/crates/cargo-pwrs)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](https://github.com/Variably-Constant/PWRS/blob/main/LICENSE)
[![PowerShell](https://img.shields.io/badge/PowerShell-7.4%2B%20%7C%205.1-5391FE.svg?style=flat-square)](https://learn.microsoft.com/powershell/)

**[Documentation](https://variably-constant.github.io/PWRS/)** | [Getting started](https://variably-constant.github.io/PWRS/docs/tutorials/getting-started/) | [cargo pwrs reference](https://variably-constant.github.io/PWRS/docs/reference/cargo-pwrs-reference/) | [Repository](https://github.com/Variably-Constant/PWRS) | [Changelog](https://github.com/Variably-Constant/PWRS/blob/main/CHANGELOG.md)

`cargo pwrs` turns a crate written with [PoWerRuSt](https://crates.io/crates/PoWerRuSt) into a module folder that `Import-Module` loads in PowerShell 7 from 7.4 on and in Windows PowerShell 5.1. No .NET SDK is installed anywhere: the C# compiler is fetched once into `~/.pwrs/toolchain/` and runs on the runtime pwsh already ships.

</div>

## Quick start

```text
cargo install cargo-pwrs
cargo pwrs new ../greeter
cd ../greeter
cargo pwrs test --release
```

`new` writes a crate with one cmdlet and one Pester test. `test` runs `cargo test`, builds the module into `target/pwrs/Greeter/`, and runs the Pester suite in pwsh and, on Windows, in Windows PowerShell. Then:

```powershell
Import-Module ./target/pwrs/Greeter/Greeter.psd1
Get-GreeterGreeting -Name World
```

## Commands

| Command | What it does |
|---|---|
| `cargo pwrs new <dir>` | scaffolds a crate with one cmdlet and one Pester test |
| `cargo pwrs build` | reads the crate's descriptor, generates the C# shell, compiles it, and writes `target/pwrs/<Module>/`; warns when the library is compiled for CPU extensions beyond its target's baseline |
| `cargo pwrs test` | `cargo test`, then the module's Pester suite in pwsh and, on Windows, in Windows PowerShell, each host's output shown as the suite writes it, then a report of what the suite left unexercised; `--cpu-tiers` runs the suites again under each x86-64 level given |
| `cargo pwrs publish` | `Test-ModuleManifest`, then `Publish-PSResource` to the PowerShell Gallery; `--dry-run` packs without publishing. Refuses a library compiled for CPU extensions the manifest does not declare |
| `cargo pwrs merge <into> <from>...` | joins module folders built on different platforms into one |
| `cargo pwrs toolchain` | fetches or reports the C# toolset |

## The other half

A module is built from two halves that must be the same version: the `PoWerRuSt` crate your code links, and the C# runtime this tool embeds in the module folder. `PoWerRuSt`, `pwrs-sys`, `pwrs-macros`, `pwrs-build` and `cargo-pwrs` are published together at one version. Bumping the crate alone gives a module that refuses to import with `pwrs_module_init failed with status 4`; `cargo install cargo-pwrs --force` is the other half.

## Documentation

- [Getting Started](https://variably-constant.github.io/PWRS/docs/tutorials/getting-started/): scaffold, build, test and import a module.
- [cargo pwrs reference](https://variably-constant.github.io/PWRS/docs/reference/cargo-pwrs-reference/): every subcommand and flag, the fetched toolchain, the module folder and its manifest.
- [How To Test A Module](https://variably-constant.github.io/PWRS/docs/how-to/how-to-test-a-module/), [How To Publish](https://variably-constant.github.io/PWRS/docs/how-to/how-to-publish/), [How To Bundle A Module](https://variably-constant.github.io/PWRS/docs/how-to/how-to-bundle-a-module/), [How To Ship A Helper Executable](https://variably-constant.github.io/PWRS/docs/how-to/how-to-ship-a-helper-executable/), [How To Use Instruction Sets](https://variably-constant.github.io/PWRS/docs/how-to/how-to-use-instruction-sets/).
- [Environment variables](https://variably-constant.github.io/PWRS/docs/reference/environment-variables/), and [How To Fix A Failure](https://variably-constant.github.io/PWRS/docs/how-to/how-to-fix-a-failure/) when something goes wrong.

MIT licensed.
