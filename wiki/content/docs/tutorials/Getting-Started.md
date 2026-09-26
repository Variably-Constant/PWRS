---
title: Getting Started
weight: 1
---

From a scaffolded crate to a cmdlet imported in both PowerShell hosts. Everything on this page is what `crates/cargo-pwrs/src/scaffold.rs`, `build.rs` and `test.rs` do.

## Prerequisites

- Rust 1.88 or later (`rustup` stable is fine), for the naked functions `export_module!` emits so the runtime can ask the CPU what it offers before any other module code runs.
- PowerShell 7 (`pwsh`) on `PATH`. The build tool runs the C# compiler inside a pwsh process, so pwsh is needed on every platform, including when the module you build targets only Windows PowerShell 5.1.
- On Windows, Windows PowerShell 5.1 (`powershell.exe`) if you want the 5.1 half of the test run; it ships with the OS.
- No .NET SDK. The first build fetches the compiler from NuGet.

crates.io already carries an unrelated crate named `pwrs`, so the package is registered as [`PoWerRuSt`](https://crates.io/crates/PoWerRuSt) and the library keeps the short name: a module writes `use pwrs::prelude::*` either way. The scaffold writes the dependency for you.

## Scaffold

```text
cargo install cargo-pwrs
cargo pwrs new C:\src\greeter
```

To work against a checkout of PWRS instead of the registry, pass the dependency:

```text
cargo run -p cargo-pwrs -- pwrs new C:\src\greeter --pwrs '{ path = "C:/src/pwrs/crates/pwrs" }'
```

`new` takes the target directory and refuses to overwrite one that exists. The crate name is the directory name lowercased with underscores turned into dashes (`greeter`), and the module name is that in PascalCase (`Greeter`). It writes four files:

- `Cargo.toml` with `crate-type = ["cdylib"]`, the `pwrs` dependency, a `description` and `keywords` the manifest will carry into a gallery listing, and a `release` profile with `lto = "fat"`, `codegen-units = 1` and `panic = "unwind"`.
- `src/lib.rs` with one cmdlet, `Get-GreeterGreeting`.
- `tests/Greeter.Tests.ps1` with two Pester tests, one naming `-Name` and one piping names in.
- `.gitignore` containing `/target`.

`src/lib.rs`:

```rust
use pwrs::prelude::*;

/// Says hello.
///
/// # Examples
/// Get-GreeterGreeting -Name World
/// 'Ada', 'Bob' | Get-GreeterGreeting
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

Line by line:

- `#[cmdlet(verb, noun, output)]` names the cmdlet `Get-GreeterGreeting` and declares its `[OutputType]`. The doc comment's first paragraph, up to the first blank line, is the help synopsis; what follows is the description, and the lines under `# Examples` become `Get-Help` examples.
- `#[derive(Default)]` is required: the runtime constructs one instance per invocation with `Default::default()`.
- `#[param(mandatory, position = 0, value_from_pipeline)]` makes `-Name` mandatory, positional, and bindable from the pipeline. The field's doc comment is the parameter's help message.
- `impl Cmdlet` needs `process`; `begin` and `end` have empty defaults.
- `ps.write(value)` writes any `IntoPs` value to the output stream. Here a `String` takes the direct string entry: one native crossing, no handle.
- `export_module!` emits the native exports the runtime binds and names the module.

## Build

```text
cd C:\src\greeter
cargo run --manifest-path C:\src\pwrs\Cargo.toml -p cargo-pwrs -- pwrs build --release
```

(Install `cargo-pwrs` with `cargo install --path C:\src\pwrs\crates\cargo-pwrs` to shorten that to `cargo pwrs build --release`.)

What `build` does, in order (`crates/cargo-pwrs/src/build.rs`):

1. `cargo metadata` to find the package, its `cdylib` target, version and authors.
2. `cargo build --release -p <package>`.
3. Loads the built library and calls its `pwrs_module_descriptor` export, which returns the JSON the macros embedded: every cmdlet, parameter, class, enum, completer and provider.
4. Ensures the toolchain under `~/.pwrs/toolchain/5.9.0/`: the compiler, `Microsoft.Net.Compilers.Toolset` 5.9.0, and the reference packages both compiles use, fetched from `api.nuget.org` once, each archive's SHA-512 recorded in `toolchain.lock` and checked on any later fetch. [The toolchain](../reference/Cargo-Pwrs-Reference.md#the-toolchain) lists them.
5. Generates `Greeter.Shell.cs`, then compiles `Pwrs.Bootstrap`, `Pwrs.Runtime` and the shell twice: for `net10.0` against .NET 8's reference pack and the `System.Management.Automation` 7.4 reference, so the module imports on PowerShell 7.4 and later whichever pwsh built it, and for `netstandard2.0` against `NETStandard.Library` and `PowerShellStandard.Library`. The compiler is `csc.dll` run inside pwsh in an isolated `AssemblyLoadContext`.
6. Writes the MAML help, the format file when the module declares classes, the manifest and the bootstrap `.psm1`, and copies the native library to `runtimes/<rid>/native/`.

The last line of output names the folder:

```text
pwrs: module folder C:\src\greeter\target\pwrs\Greeter
```

## Test

```text
cargo pwrs test --release
```

`test` runs `cargo test` in the crate, then `build`, then the Pester suite under `tests/` in pwsh and, on Windows, again in Windows PowerShell. Each host prints one summary line, and the surface check reports last:

```text
pwrs pester host=7.6.6 pester=5.7.1 passed=2 failed=0
pwrs pester host=5.1.26100.9444 pester=5.7.1 passed=2 failed=0
pwrs: surface check: every cmdlet ran, every pipeline parameter took a record, every ShouldProcess asked
```

The runner sets `PWRS_MODULE` to the module folder, which the scaffolded test imports:

```powershell
BeforeAll {
    Import-Module (Join-Path $env:PWRS_MODULE 'Greeter.psd1') -Force -ErrorAction Stop
}

Describe 'Get-GreeterGreeting' {
    It 'greets by name' {
        Get-GreeterGreeting -Name x | Should -Be 'Hello, x!'
    }

    It 'greets each name piped to it' {
        'Ada', 'Bob' | Get-GreeterGreeting | Should -Be @('Hello, Ada!', 'Hello, Bob!')
    }
}
```

The second test is what the surface check needs: `-Name` declares `value_from_pipeline`, and a parameter that promises pipeline input and never receives any is reported after the run (see [How To Test A Module](../how-to/How-To-Test-A-Module.md#what-the-suite-did-not-reach)).

Pester 4 or later is required. Windows ships Pester 3.4.0 on the module path both hosts search, which is too old; save a newer one with `Save-PSResource -Name Pester -Path target/pester` (the runner looks for `target/pester/Pester` beside the module folder) or point `PWRS_PESTER_PATH` at one.

## Import and use

```powershell
Import-Module .\target\pwrs\Greeter\Greeter.psd1
Get-GreeterGreeting World
'a', 'b' | Get-GreeterGreeting
Get-Help Get-GreeterGreeting -Full
```

The same folder imports into Windows PowerShell 5.1; the `.psm1` picks `netstandard2.0` there and `net10.0` on pwsh.

## Where to go next

- [Hello Tour](Hello-Tour.md) walks the example module that uses every mechanism.
- [How To Write A Cmdlet](How-To-Write-A-Cmdlet.md) covers parameters, phases, errors and streams in full.
- [Attribute Reference](Attribute-Reference.md) lists every key the attributes accept.
