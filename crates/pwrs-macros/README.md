# pwrs-macros

The procedural macros for [PWRS](https://github.com/Variably-Constant/PWRS):
`#[cmdlet]`, `#[param]`, `#[psclass]`, `#[psfield]`, `#[psmethods]`,
`#[psenum]`, `#[completer]`, `#[transform]`, `#[dynamic_params]`,
`#[provider]`, `#[on_import]` and `#[on_remove]`.

**You do not normally depend on this crate.** Add `PoWerRuSt`
instead, which re-exports every macro:

```toml
[dependencies]
pwrs = { package = "PoWerRuSt", version = "0.2.3" }
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
```

The macros emit the descriptor `cargo pwrs build` reads to generate
the C# shell PowerShell reflects over, plus the conversion impls each
type needs. An unknown key is a compile error naming it.

`PoWerRuSt`, `pwrs-sys`, `pwrs-macros`, `pwrs-build` and `cargo-pwrs`
are published together at one version.

Every key each attribute accepts is listed in the
[attribute reference](https://variably-constant.github.io/PWRS/docs/reference/attribute-reference/).

MIT licensed.
