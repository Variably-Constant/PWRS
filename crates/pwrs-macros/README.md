<div align="center">

<img src="https://github.com/Variably-Constant/PWRS/raw/main/assets/pwrs-logo.svg" alt="PWRS logo" width="200"/>

# pwrs-macros

**The attribute macros of PWRS: Rust items in, PowerShell cmdlets, classes and providers out.**

[![crates.io](https://img.shields.io/crates/v/pwrs-macros?style=flat-square)](https://crates.io/crates/pwrs-macros)
[![docs.rs](https://img.shields.io/docsrs/pwrs-macros?style=flat-square)](https://docs.rs/pwrs-macros)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](https://github.com/Variably-Constant/PWRS/blob/main/LICENSE)

**[Documentation](https://variably-constant.github.io/PWRS/)** | [Attribute reference](https://variably-constant.github.io/PWRS/docs/reference/attribute-reference/) | [API reference](https://docs.rs/pwrs-macros) | [Repository](https://github.com/Variably-Constant/PWRS)

</div>

`#[cmdlet]`, `#[param]`, `#[psclass]`, `#[psfield]`, `#[psmethods]`, `#[psenum]`, `#[completer]`, `#[transform]`, `#[dynamic_params]`, `#[provider]`, `#[on_import]` and `#[on_remove]`. They emit the descriptor `cargo pwrs build` reads to generate the C# shell PowerShell reflects over, and the conversions each type needs. An unknown key is a compile error naming it.

## Quick start

You do not normally depend on this crate. Add [`PoWerRuSt`](https://crates.io/crates/PoWerRuSt) instead, which re-exports every macro along with `export_module!`:

```toml
[dependencies]
pwrs = { package = "PoWerRuSt", version = "0.6.0" }
```

```rust
use pwrs::prelude::*;

/// Says hello.
#[cmdlet(verb = "Get", noun = "Greeting", output = ["System.String"])]
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

`cargo install cargo-pwrs`, then `cargo pwrs build` turns the crate into a module folder; [Getting Started](https://variably-constant.github.io/PWRS/docs/tutorials/getting-started/) walks through it.

## Documentation

- [Attribute reference](https://variably-constant.github.io/PWRS/docs/reference/attribute-reference/): every key each attribute takes, and the `export_module!` grammar.
- [How To Write A Cmdlet](https://variably-constant.github.io/PWRS/docs/how-to/how-to-write-a-cmdlet/), [How To Return Objects](https://variably-constant.github.io/PWRS/docs/how-to/how-to-return-objects/), [How To Add Completers, Transforms And Dynamic Parameters](https://variably-constant.github.io/PWRS/docs/how-to/how-to-add-completers-and-dynamic-parameters/), [How To Write A Provider](https://variably-constant.github.io/PWRS/docs/how-to/how-to-write-a-provider/).

`PoWerRuSt`, `pwrs-sys`, `pwrs-macros`, `pwrs-build` and `cargo-pwrs` are published together at one version. MIT licensed.
