<div align="center">

<img src="https://github.com/Variably-Constant/PWRS/raw/main/assets/pwrs-logo.svg" alt="PWRS logo" width="300"/>

# PWRS (PoWerRuSt)

**Rust bindings for PowerShell binary modules, in the spirit of PyO3.**

[![crates.io](https://img.shields.io/crates/v/PoWerRuSt?style=flat-square)](https://crates.io/crates/PoWerRuSt)
[![docs.rs](https://img.shields.io/docsrs/PoWerRuSt?style=flat-square)](https://docs.rs/PoWerRuSt)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](https://github.com/Variably-Constant/PWRS/blob/main/LICENSE)
[![PowerShell](https://img.shields.io/badge/PowerShell-7.4%2B%20%7C%205.1-5391FE.svg?style=flat-square)](https://learn.microsoft.com/powershell/)
[![.NET](https://img.shields.io/badge/.NET-8%2B-512BD4.svg?style=flat-square)](https://dotnet.microsoft.com/)

**[Documentation](https://variably-constant.github.io/PWRS/)** | [Getting started](https://variably-constant.github.io/PWRS/docs/tutorials/getting-started/) | [API reference](https://docs.rs/PoWerRuSt) | [Repository](https://github.com/Variably-Constant/PWRS) | [Changelog](https://github.com/Variably-Constant/PWRS/blob/main/CHANGELOG.md)

A cmdlet is a Rust struct and its fields are the parameters, coerced and validated by PowerShell's own binder before your code runs. `cargo pwrs build` turns the crate into a module folder `Import-Module` loads in PowerShell 7 from 7.4 on and in Windows PowerShell 5.1. No .NET SDK is installed anywhere: the C# compiler is fetched once and runs on the runtime pwsh already ships.

</div>

## Features

- A cmdlet is a Rust struct, and PowerShell's own binder coerces, validates and completes its `#[param]` fields.
- [Output classes, enums, completers, dynamic parameters and providers](https://variably-constant.github.io/PWRS/docs/reference/attribute-reference/), generated from Rust and reflected over like any SDK module.
- One native call per phase, and [zero-copy borrows](https://variably-constant.github.io/PWRS/docs/how-to/how-to-call-dotnet-from-rust/) of managed arrays in both directions.
- [Values keep their CLR type](https://variably-constant.github.io/PWRS/docs/reference/conversions-reference/): an `i32` arrives as a `System.Int32`, not a widened `Int64`.
- Writing to a stream off the pipeline thread is a compile error, since `Pipeline<'ps>` is `!Send`.
- [Reload a rebuilt module in a live session](https://variably-constant.github.io/PWRS/docs/how-to/how-to-reload-a-module/), on both hosts.
- [PowerShell 7.4 and later, and Windows PowerShell 5.1](https://variably-constant.github.io/PWRS/docs/explanation/two-hosts/), from one build on any machine, with no .NET SDK installed.
- [Hand-written C#](https://variably-constant.github.io/PWRS/docs/how-to/how-to-add-hybrid-csharp/) under `src/csharp/` compiles into the same shell assembly.
- `cargo pwrs build | test | publish | new | merge | toolchain`, with Pester in both hosts, and [`PWRS_TRACE`](https://variably-constant.github.io/PWRS/docs/how-to/how-to-trace-a-module/) for both sides' counters in a running module and every process `cargo pwrs` starts.

## Quick start

Install the build command, then scaffold a module:

```text
cargo install cargo-pwrs
cargo pwrs new ../greeter
```

crates.io already carries an unrelated crate named `pwrs`, so the package is registered as `PoWerRuSt` and the library keeps the short name. The scaffold writes the dependency for you; a crate adding it by hand writes:

```toml
[dependencies]
pwrs = { package = "PoWerRuSt", version = "0.6.0" }
```

The crate and `cargo-pwrs` move together: a module is built from the crate your code links and the C# runtime `cargo-pwrs` embeds in the module folder, and the two must be the same version. Bumping the crate alone gives a module that refuses to import with `pwrs_module_init failed with status 4`; `cargo install cargo-pwrs --force` is the other half.

The scaffold writes a crate with one cmdlet and one Pester test:

```rust
use pwrs::prelude::*;

/// Says hello.
///
/// # Examples
/// Get-GreeterGreeting -Name World
#[cmdlet(verb = "Get", noun = "GreeterGreeting", output = ["System.String"])]
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
    name: "Greeter",
    cmdlets: [GetGreeting],
}
```

Build it, test it, import it:

```text
cargo pwrs build --release     # target/pwrs/Greeter/, importable in both hosts
cargo pwrs test --release      # cargo test, then tests/*.Tests.ps1 under Pester in pwsh and, on Windows, powershell.exe
```

```powershell
Import-Module ./target/pwrs/Greeter/Greeter.psd1
'World', 'pwrs' | Get-GreeterGreeting
Get-Help Get-GreeterGreeting -Examples
```

The doc comment becomes the help text and the `# Examples` lines become `Get-Help` examples.

## Documentation

The [documentation site](https://variably-constant.github.io/PWRS/) has every page; these are the ones most readers start from.

- **Tutorials:** [Getting Started](https://variably-constant.github.io/PWRS/docs/tutorials/getting-started/), [Hello Tour](https://variably-constant.github.io/PWRS/docs/tutorials/hello-tour/).
- **How-to guides:** [write a cmdlet](https://variably-constant.github.io/PWRS/docs/how-to/how-to-write-a-cmdlet/), [return objects](https://variably-constant.github.io/PWRS/docs/how-to/how-to-return-objects/), [write a provider](https://variably-constant.github.io/PWRS/docs/how-to/how-to-write-a-provider/), [add completers and dynamic parameters](https://variably-constant.github.io/PWRS/docs/how-to/how-to-add-completers-and-dynamic-parameters/), [call .NET from Rust](https://variably-constant.github.io/PWRS/docs/how-to/how-to-call-dotnet-from-rust/), [use threads](https://variably-constant.github.io/PWRS/docs/how-to/how-to-use-threads/), [test a module](https://variably-constant.github.io/PWRS/docs/how-to/how-to-test-a-module/), [publish](https://variably-constant.github.io/PWRS/docs/how-to/how-to-publish/), [fix a failure](https://variably-constant.github.io/PWRS/docs/how-to/how-to-fix-a-failure/), and [the rest](https://variably-constant.github.io/PWRS/docs/how-to/).
- **Explanation:** [Why PWRS](https://variably-constant.github.io/PWRS/docs/explanation/why-pwrs/), [The Bridge](https://variably-constant.github.io/PWRS/docs/explanation/the-bridge/), [Two Hosts](https://variably-constant.github.io/PWRS/docs/explanation/two-hosts/), [PWRS and PyO3](https://variably-constant.github.io/PWRS/docs/explanation/pwrs-and-pyo3/).
- **Reference:** [attributes](https://variably-constant.github.io/PWRS/docs/reference/attribute-reference/), [conversions](https://variably-constant.github.io/PWRS/docs/reference/conversions-reference/), [the `Pipeline` API](https://variably-constant.github.io/PWRS/docs/reference/pipeline-reference/), [`cargo pwrs`](https://variably-constant.github.io/PWRS/docs/reference/cargo-pwrs-reference/), [environment variables](https://variably-constant.github.io/PWRS/docs/reference/environment-variables/), [the ABI](https://variably-constant.github.io/PWRS/docs/reference/abi-reference/), [benchmarks](https://variably-constant.github.io/PWRS/docs/reference/benchmarks/).

## The crates

| Crate | What it is |
|---|---|
| `PoWerRuSt` | this crate: the API a module's code uses, imported as `pwrs` |
| [`cargo-pwrs`](https://crates.io/crates/cargo-pwrs) | the build tool: scaffold, build, test, publish, merge |
| [`pwrs-macros`](https://crates.io/crates/pwrs-macros) | the attribute macros, re-exported here |
| [`pwrs-sys`](https://crates.io/crates/pwrs-sys) | the C ABI contract, re-exported here as `pwrs::sys` |
| [`pwrs-build`](https://crates.io/crates/pwrs-build) | the C# compiler toolchain `cargo-pwrs` builds with |

All five are published together at one version.

## Platforms

Windows x64 in PowerShell 7.4 and later and in Windows PowerShell 5.1, and Linux x64, FreeBSD x64 and macOS arm64 in PowerShell 7.4 and later, each with every suite of the repository passing; [`docs/PLATFORMS.md`](https://github.com/Variably-Constant/PWRS/blob/main/docs/PLATFORMS.md) records the runs. A module needs Rust 1.98 or later, edition 2024.

## License

MIT. See [LICENSE](https://github.com/Variably-Constant/PWRS/blob/main/LICENSE).
