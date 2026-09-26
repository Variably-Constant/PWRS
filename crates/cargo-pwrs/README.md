# cargo-pwrs

The build tool for [PWRS](https://github.com/Variably-Constant/PWRS):
it turns a Rust crate into a PowerShell module folder that
`Import-Module` loads in PowerShell 7 from 7.4 on and in Windows
PowerShell 5.1.

```text
cargo install cargo-pwrs
cargo pwrs new ../greeter
cargo pwrs build --release
```

No .NET SDK is installed anywhere. The C# compiler is fetched once
into `~/.pwrs/toolchain/` and runs on the runtime pwsh already ships.

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

A module is built from two halves that must be the same version: the
`PoWerRuSt` crate your code links, and the C# runtime this tool
embeds in the module folder. `PoWerRuSt`, `pwrs-sys`, `pwrs-macros`,
`pwrs-build` and `cargo-pwrs` are published together at one version.
Bumping one alone gives a module that refuses to import with
`pwrs_module_init failed with status 4`; `cargo install cargo-pwrs
--force` is the other half.

## Documentation

[variably-constant.github.io/PWRS](https://variably-constant.github.io/PWRS/),
in particular the
[cargo pwrs reference](https://variably-constant.github.io/PWRS/docs/reference/cargo-pwrs-reference/)
for every flag and
[how to fix a failure](https://variably-constant.github.io/PWRS/docs/how-to/how-to-fix-a-failure/)
when something goes wrong.

MIT licensed.
