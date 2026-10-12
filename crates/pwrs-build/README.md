<div align="center">

<img src="https://github.com/Variably-Constant/PWRS/raw/main/assets/pwrs-logo.svg" alt="PWRS logo" width="200"/>

# pwrs-build

**The C# toolchain behind PWRS's build tool, with no .NET SDK.**

[![crates.io](https://img.shields.io/crates/v/pwrs-build?style=flat-square)](https://crates.io/crates/pwrs-build)
[![docs.rs](https://img.shields.io/docsrs/pwrs-build?style=flat-square)](https://docs.rs/pwrs-build)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](https://github.com/Variably-Constant/PWRS/blob/main/LICENSE)

**[Documentation](https://variably-constant.github.io/PWRS/)** | [cargo pwrs reference](https://variably-constant.github.io/PWRS/docs/reference/cargo-pwrs-reference/) | [API reference](https://docs.rs/pwrs-build) | [Repository](https://github.com/Variably-Constant/PWRS)

</div>

What [PWRS](https://github.com/Variably-Constant/PWRS) tooling shares to compile a module's managed side: finding pwsh and its `$PSHOME`, fetching the C# compiler toolset and the reference packages, and compiling C# against them: the .NET 8 reference pack and `System.Management.Automation` 7.4 for PowerShell 7, and .NET Standard 2.0 with PowerShell Standard for Windows PowerShell.

## Quick start

You do not normally depend on this crate. It is what [`cargo-pwrs`](https://crates.io/crates/cargo-pwrs) is built from; to build a module, install that:

```text
cargo install cargo-pwrs
cargo pwrs toolchain
```

`cargo pwrs toolchain` fetches the toolset if it is missing and prints where it is, which C# compiler it runs and which pwsh it runs it in.

## How it compiles

No .NET SDK is involved anywhere. `Microsoft.Net.Compilers.Toolset` and the reference packages are fetched once from NuGet into `~/.pwrs/toolchain/<toolset>/v2/`, each package extracted into a folder of the fetching build's own and renamed into place, with its SHA-512 hash recorded in `toolchain.lock`, so builds fetching at once never meet in a half-written folder and two releases of these tools share the tree without replacing each other's packages. `csc.dll` runs inside the installed pwsh, in an assembly load context of its own.

`PWRS_PWSH`, `PWRS_PSHOME`, `PWRS_HOME` and `PWRS_TOOLSET` change what it finds and where it puts things; see the [environment variables reference](https://variably-constant.github.io/PWRS/docs/reference/environment-variables/).

## Documentation

- [cargo pwrs reference](https://variably-constant.github.io/PWRS/docs/reference/cargo-pwrs-reference/): the fetched toolchain, every subcommand and flag, and the module folder.
- [Two Hosts](https://variably-constant.github.io/PWRS/docs/explanation/two-hosts/): how one build serves PowerShell 7 and Windows PowerShell 5.1.

`PoWerRuSt`, `pwrs-sys`, `pwrs-macros`, `pwrs-build` and `cargo-pwrs` are published together at one version. MIT licensed.
