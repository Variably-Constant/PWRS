<div align="center">

<img src="https://github.com/Variably-Constant/PWRS/raw/main/assets/pwrs-logo.svg" alt="PWRS logo" width="200"/>

# pwrs-sys

**The C ABI contract between a PWRS native module and its managed runtime.**

[![crates.io](https://img.shields.io/crates/v/pwrs-sys?style=flat-square)](https://crates.io/crates/pwrs-sys)
[![docs.rs](https://img.shields.io/docsrs/pwrs-sys?style=flat-square)](https://docs.rs/pwrs-sys)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](https://github.com/Variably-Constant/PWRS/blob/main/LICENSE)

**[Documentation](https://variably-constant.github.io/PWRS/)** | [ABI reference](https://variably-constant.github.io/PWRS/docs/reference/abi-reference/) | [API reference](https://docs.rs/pwrs-sys) | [Repository](https://github.com/Variably-Constant/PWRS)

</div>

The host vtable, the handle and string types, the status codes, the type tags, and the names of the exports every module provides: the contract a [PWRS](https://github.com/Variably-Constant/PWRS) native module and the `Pwrs.Runtime` assembly `cargo-pwrs` embeds in the module folder both compile against.

## Quick start

You do not normally depend on this crate. Add [`PoWerRuSt`](https://crates.io/crates/PoWerRuSt) instead, which re-exports it as `pwrs::sys` and gives you a typed surface over it:

```toml
[dependencies]
pwrs = { package = "PoWerRuSt", version = "0.6.0" }
```

Then follow [Getting Started](https://variably-constant.github.io/PWRS/docs/tutorials/getting-started/).

## The table

The host table is append-only, behind an 8-byte header of `size` and `version`. At `pwrs_module_init` a module refuses, with status 4, a table whose `size` is smaller than the one it was compiled against or whose `version` differs, so an older runtime is turned away before any call goes through an entry it lacks, while a newer runtime serves an older module. `version` changes only when an existing entry changes meaning, and then the export names gain the major (`pwrs2_*`).

## Documentation

- [ABI reference](https://variably-constant.github.io/PWRS/docs/reference/abi-reference/): every status code, export, table entry, and parameter and field block.
- [The Bridge](https://variably-constant.github.io/PWRS/docs/explanation/the-bridge/): the generated shell, the append-only table, the native exports, and the two rules that hold at the boundary.

`PoWerRuSt`, `pwrs-sys`, `pwrs-macros`, `pwrs-build` and `cargo-pwrs` are published together at one version. MIT licensed.
