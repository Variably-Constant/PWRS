# pwrs-sys

The C ABI contract between a [PWRS](https://github.com/Variably-Constant/PWRS)
native module and the `Pwrs.Runtime` managed support assembly:
the host vtable, the handle and string types, the status codes, the
type tags, and the names of the exports every module provides.

**You do not normally depend on this crate.** Add `PoWerRuSt`
instead, which re-exports it as `pwrs::sys` and gives you a typed
surface over it.

```toml
[dependencies]
pwrs = { package = "PoWerRuSt", version = "0.2.3" }
```

The table is append-only with an 8-byte header of `size` and
`version`. A module checks `size` before using an entry newer than
the version it was compiled against, so an older runtime is refused
at `pwrs_module_init` rather than by calling through a slot that is
not there. `version` changes only when an existing entry changes
meaning, and then the export names gain the major (`pwrs2_*`).

`PoWerRuSt`, `pwrs-sys`, `pwrs-macros`, `pwrs-build` and `cargo-pwrs`
are published together at one version.

Every entry and export is listed in the
[ABI reference](https://variably-constant.github.io/PWRS/docs/reference/abi-reference/).

MIT licensed.
