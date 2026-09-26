# pwrs-build

The build machinery [PWRS](https://github.com/Variably-Constant/PWRS)
tooling shares: finding pwsh and its `$PSHOME`, fetching and locking
the C# compiler toolset and the reference packages, and compiling C#
against them: the .NET 8 reference pack and
`System.Management.Automation` 7.4 for PowerShell 7, and .NET Standard
2.0 with PowerShell Standard for Windows PowerShell.

**You do not normally depend on this crate.** It is what
[`cargo-pwrs`](https://crates.io/crates/cargo-pwrs) is built from; to
build a module, install that:

```text
cargo install cargo-pwrs
```

No .NET SDK is involved anywhere. `Microsoft.Net.Compilers.Toolset`
is fetched once into `~/.pwrs/toolchain/`, and `csc.dll` runs inside
the installed pwsh, in an assembly load context of its own. Each
toolset version gets its own tree under a lock, so switching versions
neither refetches nor mixes two compilers, and each package in it gets
a folder named for its version, so two releases of these tools share a
tree without replacing each other's references.

`PWRS_PWSH`, `PWRS_PSHOME`, `PWRS_HOME` and `PWRS_TOOLSET` change
what it finds and where it puts things; see the
[environment variables reference](https://variably-constant.github.io/PWRS/docs/reference/environment-variables/).

`PoWerRuSt`, `pwrs-sys`, `pwrs-macros`, `pwrs-build` and `cargo-pwrs`
are published together at one version.

MIT licensed.
