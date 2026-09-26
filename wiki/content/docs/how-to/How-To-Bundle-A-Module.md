---
title: How To Bundle A Module
weight: 49
---

A module can depend on another PWRS module the way it depends on a crate: shipped inside it, imported when it is imported, never installed separately. `cargo pwrs build` builds the other module with the same tool, lays its folder inside the module folder, and the module's own script imports it into the session. Source: `bundle_module` and `bundled_entries` in `crates/cargo-pwrs/src/build.rs`, `bundled_step` in `crates/cargo-pwrs/src/generate.rs`, the bundled refusals of `publish` in `crates/cargo-pwrs/src/main.rs`, and `crates/cargo-pwrs/src/merge.rs`. The worked example is `examples/hello`, which bundles `examples/calc`, and `tests/Bundled.Tests.ps1`.

## Declare the bundled crate

```toml
[package.metadata.pwrs]
bundled-modules = ["../calc"]
```

Each entry is a crate directory, joined to the module's own crate directory; it may leave the repository, as a pinned checkout of another project does. An entry whose directory holds no `Cargo.toml` stops the build, named:

```text
[package.metadata.pwrs] bundled-modules names ../missing, and <dir>/missing holds no Cargo.toml
```

An entry can also be a table whose `path` is that directory and whose other keys say what a failed import does, and what the bundled crate is built and published with. The hello example's:

```toml
[package.metadata.pwrs]
bundled-modules = [{ path = "../calc", on-import-failure = "warn" }]
```

| Key | Takes | When absent | Does |
|---|---|---|---|
| `path` | the crate directory | (required) | what the string form names |
| `on-import-failure` | `"stop"` or `"warn"` | `"stop"` | `"stop"` fails the bundling module's import with the bundled module's error; `"warn"` writes a warning naming the bundled module and the error, and the bundling module imports without it |
| `features` | a list of cargo features | none | passed to the bundled crate's build as `--features` |
| `default-features` | `true` or `false` | `true` | `false` builds the bundled crate with `--no-default-features` |
| `license-files` | a table of `"name@version" = "path"` | none | license texts for crates the bundled library links, the paths relative to the bundling crate's directory, checked and quoted as [the module's own](How-To-Publish.md#license-notices) are |
| `cpu-features` | a list of extensions | none | extensions the bundled library requires on purpose, as the module's own `cpu-features` declares them for its library |

The bundled crate's own `license-files` and `cpu-features` still apply; one crate and version supplied from both places stops the build. A key the table does not take, a value of another type, or an entry that is neither a string nor a table stops the build, named:

```text
[package.metadata.pwrs] the bundled-modules entry ../calc sets `optional`, which is not one of path, on-import-failure, features, default-features, license-files, cpu-features
```

## What the build does

For each entry, `cargo pwrs build` runs its own build of that crate: the same `--release`, `--target` and `--debug-symbols`, `--locked`, the entry's `features` and `default-features`, into `target/pwrs-bundled/<crate directory name>/` under the bundling module's target directory. The named tree is only read, so two checkouts bundling one tree never share a target directory, and a `Cargo.lock` that the build would change stops it, with the file named:

```text
cargo build failed with exit code: 101; the build ran --locked, so <dir>/Cargo.lock must already agree with Cargo.toml
```

The bundled module's `Pwrs.Bootstrap.dll` and `Pwrs.Runtime.dll`, under `net10.0` and under `netstandard2.0`, must match the bundling module's byte for byte. One Windows PowerShell session keeps one `Pwrs.Runtime` for every module built by one tool, and the managed compiles are deterministic, so two modules built by one `cargo-pwrs` on one toolchain match; a file that differs stops the build:

```text
pwrs-example-hello cannot bundle Calc: net10.0/Pwrs.Runtime.dll differs between the two builds, and one Windows PowerShell session keeps one runtime for both; build both with one cargo-pwrs and one toolchain
```

The folder is then copied to `<Module>/<Name>/`, whole: its manifest, script, assemblies, help, format file, notices and `runtimes/<rid>/native/`. The build prints `pwrs: building the module of <dir> to bundle` before and `pwrs: bundled <Name> at <Module>/<Name>` after. The hello example's folder, built on Windows x64, holds `Calc/` beside `net10.0/`, `netstandard2.0/` and `runtimes/`, with `Calc/Calc.psd1`, `Calc/Calc.psm1`, `Calc/net10.0/`, `Calc/netstandard2.0/` and `Calc/runtimes/win-x64/native/pwrs_example_calc.dll` inside it.

## What the script does at import

The generated `<Module>.psm1` imports the bundled modules, in the order the entries list them, before it loads its own shell. When every entry stops on a failed import, the step is:

```powershell
foreach ($bundled in @('Calc')) {
if (-not (Get-Module -Name $bundled)) {
Import-Module ([System.IO.Path]::Combine($PSScriptRoot, $bundled, $bundled + '.psd1')) -Global -DisableNameChecking -ErrorAction Stop
}
}
```

When an entry warns, the import runs under a `try` whose `catch` rethrows the error of every bundled module that does not warn, so those still fail the import, and writes one warning for one that does. The hello example's:

```powershell
foreach ($bundled in @('Calc')) {
if (-not (Get-Module -Name $bundled)) {
try {
Import-Module ([System.IO.Path]::Combine($PSScriptRoot, $bundled, $bundled + '.psd1')) -Global -DisableNameChecking -ErrorAction Stop
}
catch {
if (@('Calc') -notcontains $bundled) { throw }
Write-Warning ('Hello' + ' imports without its bundled module ' + $bundled + ', whose import failed: ' + $_.Exception.Message)
}
}
}
```

The warning is the only trace: `-WarningAction` and `$WarningPreference` govern it like any other, and nothing else reports the missing module. The bundling module's own code finds the bundled module absent with `Get-Module` and takes whatever course it has without it.

A module the session already holds under the name is the one kept, whichever folder it came from. That is what makes the import work at all on PowerShell 7, which refuses a second folder of a loaded module because its cmdlet names are taken; on Windows PowerShell 5.1 a second folder would import beside the first. `-Global` puts the import where a user's own `Import-Module` would, so the bundled module's cmdlets are callable from the session and not only from the script. `-DisableNameChecking` keeps the bundled module's unapproved-verb warnings, which are addressed to its author, out of the bundling module's import; importing the bundled module on its own still shows them.

The manifest carries no `RequiredModules` and no `NestedModules` for it: `RequiredModules` with a relative path does not resolve on Windows PowerShell 5.1, and a `NestedModules` entry keeps the inner module out of `Get-Module`, so its cmdlets could not be reached.

`examples/hello/tests/Bundled.Tests.ps1` checks the folder, the byte-identical runtime assemblies, that `Get-Module Calc` answers from the bundled folder after `Import-Module Hello` with `Add-CalcNumber 2 3` giving 5, and, in a child host of the same edition, that a `Calc` imported first is the one the session keeps when `Hello` is imported after it: one module, the same instance, both modules' cmdlets running. In another child host it imports a copy of the module folder whose `Calc` has lost its native library: `Hello` imports with one warning naming `Calc` and its error, `Get-Module Calc` finds nothing, and `Get-Greeting` runs. A cargo-pwrs test runs both forms of the step, as generated, in pwsh and in Windows PowerShell against a bundled module whose import fails: the plain entry fails the bundling module's import with that module's error, and the warning entry imports it with one warning.

## Publish and merge

`cargo pwrs publish` packages the folder `build` made, so the package carries the bundled module under `<Name>/`, with no dependency declared for it; the tool prints `pwrs: the package carries the bundled module <Name>` for each. It refuses the package when a bundled module's library links a crate without a license text, or requires extensions beyond its target's baseline that neither the bundled crate nor the entry declares, as it refuses the module's own library, naming the bundled module and the setting that lets it through. With the calc example made to depend on cranelift-bitset, whose package ships no license file, `cargo pwrs publish --release --dry-run` of the hello example on Windows x64 said:

```text
cargo-pwrs: Hello cannot be published: the native library of its bundled module Calc links cranelift-bitset 0.136.1, wasmtime-internal-core 49.0.1, whose source carries no license file, so the notices beside that library name their licenses but cannot quote them. Supply each file under the license-files of Calc's bundled-modules entry, or build it without the crates that need them.
```

With both crates in the entry's `license-files`, it packaged `Hello.0.2.3.nupkg`, and Calc's `runtimes/win-x64/THIRD-PARTY-NOTICES.txt` quoted the file under each crate:

```toml
bundled-modules = [{ path = "../calc", on-import-failure = "warn", license-files = { "cranelift-bitset@0.136.1" = "licenses/wasmtime-LICENSE.txt", "wasmtime-internal-core@49.0.1" = "licenses/wasmtime-LICENSE.txt" } }]
```

```text
cranelift-bitset 0.136.1
License: Apache-2.0 WITH LLVM-exception
Repository: https://github.com/bytecodealliance/wasmtime
No license file ships in the crate's package; the Hello module supplies it.
```

Built with `RUSTFLAGS='-C target-cpu=x86-64 -C target-feature=+avx2'` and Hello's own `cpu-features` listing what that brings, the refusal named Calc's library, and the same list in the entry's `cpu-features` let it through:

```text
cargo-pwrs: Hello cannot be published: the native library of its bundled module Calc is compiled to require ssse3, sse4.1, sse4.2, avx, avx2, and a CPU without them refuses to import it. Build with RUSTFLAGS='-C target-cpu=x86-64', or list them under the cpu-features of Calc's bundled-modules entry if every CPU the module is meant for has them.
``` `cargo pwrs merge` copies a bundled module's `runtimes/<rid>` into the destination's copy of it, beside the bundling module's own, printing `pwrs: merged <Name>/runtimes/<rid> from <folder>`; a destination that lacks the bundled module is refused. Both modules must therefore be built from one checkout on every platform merged, as the manifests being identical text checks.

## What it costs

A bundled module is built once per build of the bundling module, into its own target directory, so the first build compiles the bundled crate's dependencies from scratch and later builds reuse them. At import the session pays the bundled module's own import, once, when it does not hold the module already.
