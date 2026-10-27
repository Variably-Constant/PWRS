<div align="center">

<img src="https://github.com/Variably-Constant/PWRS/raw/main/assets/pwrs-logo.svg" alt="PWRS logo" width="200"/>

# pwrs-build

**The C# toolchain behind PWRS's build tool, with no .NET SDK.**

[![crates.io](https://img.shields.io/crates/v/pwrs-build?style=flat-square)](https://crates.io/crates/pwrs-build)
[![docs.rs](https://img.shields.io/docsrs/pwrs-build?style=flat-square)](https://docs.rs/pwrs-build)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](https://github.com/Variably-Constant/PWRS/blob/main/LICENSE)

**[Documentation](https://variably-constant.github.io/PWRS/)** | [cargo pwrs reference](https://variably-constant.github.io/PWRS/docs/reference/cargo-pwrs-reference/) | [API reference](https://docs.rs/pwrs-build) | [Repository](https://github.com/Variably-Constant/PWRS)

</div>

What [PWRS](https://github.com/Variably-Constant/PWRS) tooling shares to compile a module's managed side: finding pwsh and its `$PSHOME`, fetching the C# compiler toolset and the reference packages, and compiling C# against them: the .NET 8 reference pack and `System.Management.Automation` 7.4 for PowerShell 7, and .NET Standard 2.0 with PowerShell Standard for Windows PowerShell. It also compiles an executable for Windows PowerShell 5.1, against .NET Framework 4.7.2 and Windows PowerShell's own engine and console host, for a program that starts Windows PowerShell inside its own process.

## Quick start

You do not normally depend on this crate. It is what [`cargo-pwrs`](https://crates.io/crates/cargo-pwrs) is built from; to build a module, install that:

```text
cargo install cargo-pwrs
cargo pwrs toolchain
```

`cargo pwrs toolchain` fetches the toolset if it is missing and prints where it is, which C# compiler it runs, the Pester `cargo pwrs test` runs a module's suites under, and which pwsh it runs them in.

## How it compiles

No .NET SDK is involved anywhere. `Microsoft.Net.Compilers.Toolset`, the reference packages and Pester 6.2.0 are fetched once from NuGet into `~/.pwrs/toolchain/<toolset>/v2/`. Each version of a package has a folder of its own, so two releases of the PWRS tools can share that folder without replacing each other's packages. A package is fetched into a folder that only the fetching tool uses, then renamed into place, so two tools fetching at the same time never see a half-written folder. The SHA-512 hash of each package is recorded in `toolchain.lock`.

`csc.dll` runs inside the installed pwsh, in an assembly load context of its own. That is a separate space for loading .NET libraries, so the compiler's libraries never clash with the copy of the C# compiler that pwsh ships.

`PWRS_PWSH`, `PWRS_PSHOME`, `PWRS_HOME` and `PWRS_TOOLSET` change what it finds and where it puts things; see the [environment variables reference](https://variably-constant.github.io/PWRS/docs/reference/environment-variables/).

## Documentation

- [cargo pwrs reference](https://variably-constant.github.io/PWRS/docs/reference/cargo-pwrs-reference/): the fetched toolchain, every subcommand and flag, and the module folder.
- [Two Hosts](https://variably-constant.github.io/PWRS/docs/explanation/two-hosts/): how one build runs in PowerShell 7 and in Windows PowerShell 5.1.

`PoWerRuSt`, `pwrs-sys`, `pwrs-macros`, `pwrs-build`, `pwrs-host` and `cargo-pwrs` are published together at one version. MIT licensed.
