---
title: cargo pwrs Reference
weight: 4
---

The build tool. Source: `crates/cargo-pwrs/src/*.rs`, `crates/cargo-pwrs/scripts/*.ps1`, `crates/pwrs-build/src/*.rs`, `crates/pwrs-build/scripts/*.ps1`.

## Invocation

```text
cargo pwrs <build|test|publish|new|toolchain> [--release] [--target <triple>] [--features <a,b>] [--all-features] [--package <name>] [--manifest-dir <dir>] [--dry-run] [--pwrs <dependency spec>] [<new dir>]
cargo pwrs merge <into module dir> <from module dir>...
```

`cargo-pwrs` is a cargo subcommand binary; `cargo pwrs ...` and `cargo-pwrs pwrs ...` are the same. From the PWRS checkout without installing it: `cargo run -p cargo-pwrs -- pwrs ...`.

| Flag | Applies to | Effect |
|---|---|---|
| `--release` | build, test, publish | `cargo build --release`; the library is read from `target/release` instead of `target/debug` |
| `--target` | build, test, publish | `cargo build --target <triple>`; the library is read from `target/<triple>/<profile>` and placed under the triple's runtime identifier. A triple that is not the building machine's needs a linker for it in cargo's configuration, and the crate is built for the host as well, since the descriptor is read by loading a library |
| `--features`, `-F` | build, test, publish | passed to `cargo build` and, for `test`, to `cargo test`; comma- or space-separated, repeatable |
| `--all-features` | build, test, publish | passed to `cargo build` and `cargo test` |
| `--debug-symbols` | build, test, publish | writes a `.pdb` beside each managed assembly. A build without `--release` writes them anyway; this asks a release build for them too |
| `--package`, `-p` | build, test, publish | which package in the workspace, when the manifest directory is not the package's own |
| `--manifest-dir` | build, test, publish | the crate directory; defaults to the current directory |
| `--dry-run` | publish | package only |
| `--pwrs` | new | the `pwrs` dependency spec written into the new `Cargo.toml`; the default is `{ package = "PoWerRuSt", version = "<this tool's version>" }`, which names the package because crates.io carries an unrelated `pwrs`, and takes the version from `cargo-pwrs` itself because the workspace publishes the two together |

## What each command returns

Every subcommand exits 0 when it succeeds. A failure prints `cargo-pwrs: <what failed>` on standard error and exits 1. An unknown subcommand, an unknown flag, or a flag without its value prints the usage line and exits 2. Each command writes its own progress to standard error, one line per event beginning `pwrs: `, among the lines cargo, the compiler and Pester write themselves. Only `publish` writes to standard output: the result line of its script.

| Command | Exit 0 means | Writes | Its own lines on success |
|---|---|---|---|
| `build` | the module folder is complete | `target/pwrs/<Module>/`, listed under [The generated files](#the-generated-files) | `pwrs: module folder <path>`, last; before it, a warning for each of: extensions compiled in beyond the target's baseline, crates without a license file, a missing `authors` or `description`, and `[Cmdlet]` attributes in hand-written C# that sit on no class |
| `test` | `cargo test` passed, and every Pester suite passed in each host | what `build` writes, and each host's surface file under `target/pwrs/work/surface/` | `pwrs: Pester in pwsh`, and on Windows `pwrs: Pester in Windows PowerShell`, each before that host's Pester output; one pair more per CPU tier; then the surface check's line and a line per finding |
| `publish` | the module was packaged, or published | with `--dry-run`, `target/pwrs/publish/<Module>/<Module>.<version>.nupkg` | on standard output `packaged <path of the .nupkg>`, or `published <Module> to PSGallery` |
| `new` | the crate was created | `<dir>/Cargo.toml`, `src/lib.rs`, `tests/<Module>.Tests.ps1`, `.gitignore` | `pwrs: created <dir> ; next: cargo pwrs test --manifest-dir <dir>` |
| `merge` | every `runtimes/<rid>` of every source folder was copied, and every `<Bundled>/runtimes/<rid>` into the destination's copy of that bundled module | `runtimes/<rid>/` and `<Bundled>/runtimes/<rid>/` in the destination folder | `pwrs: merged runtimes/<rid> from <source folder>`, one per rid, and `pwrs: merged <Bundled>/runtimes/<rid> from <source folder>` per bundled module and rid |
| `toolchain` | the toolchain is in place | the toolchain, when a package is missing (below) | `pwrs: toolchain at <root>`, `pwrs: csc at <folder>`, `pwrs: pwsh at <$PSHOME>` |

## `build`

1. `cargo metadata --no-deps` in the manifest directory; the package is the one whose manifest path matches, or `--package`. It must have a `cdylib` target, and each name `[package.metadata.pwrs] helpers` lists must be one of its `[[bin]]` targets; a name that is not stops the build, named. See [How To Ship A Helper Executable](../how-to/How-To-Ship-A-Helper-Executable.md).
2. `cargo build -p <package> [--release] [--target <triple>]`, which builds the package's `[[bin]]` targets with its library, for the same target and profile, and a second build for the host when the triple is not the host's. A GNU/Linux triple with a glibc version after a dot, as cargo-zigbuild spells it (`x86_64-unknown-linux-gnu.2.35`), is built by `cargo zigbuild` instead, which links the library against that glibc's symbols; the triple without the version is what the other steps receive. It needs cargo-zigbuild and zig on `PATH`.
3. Loads the host's library and calls `pwrs_module_descriptor`; the JSON must declare `abi` 1.
4. Ensures the toolchain (below).
5. Under `target/pwrs/work/<Module>/` writes the runtime and bootstrap sources and the generated `<Module>.Shell.cs`, adds every `.cs` under `<crate>/src/csharp/` (noting the `Verb-Noun` and the class-level `[Alias]` names of each `[Cmdlet(verb, noun)]` class in them), and compiles `Pwrs.Bootstrap.dll`, `Pwrs.Runtime.dll` and `<Module>.Shell.<stamp>.dll` for `net10.0` and `netstandard2.0` into `target/pwrs/<Module>/<tfm>/`. The folder is cleared first. `<stamp>` is a hash of the managed source the shell is compiled from (the generated `<Module>.Shell.cs`, the `src/csharp/` files and the runtime), so a changed surface produces an assembly identity of its own.
6. Builds the module of each crate `[package.metadata.pwrs] bundled-modules` names, through this same tool with the same `--release`, `--target` and `--debug-symbols`, `--locked`, into `target/pwrs-bundled/<crate directory name>/` under the bundling module's target directory, so the named tree is only read. An entry is the crate directory as a string, or a table with `path` and any of `on-import-failure` (`"stop"`, the default, or `"warn"`), `features`, `default-features`, `license-files` and `cpu-features`: the bundled build takes the entry's features, and its license files and extensions beside the bundled crate's own. A path is joined to the crate directory and may leave the repository; one holding no `Cargo.toml` stops the build, named, as does a key or value the table does not take. The bundled module's `Pwrs.Bootstrap.dll` and `Pwrs.Runtime.dll` under both `net10.0` and `netstandard2.0` must match the bundling module's byte for byte, since one Windows PowerShell session keeps one runtime for every module built by one tool; a file that differs stops the build, named. The folder is then copied to `target/pwrs/<Module>/<Name>/`. See [How To Bundle A Module](../how-to/How-To-Bundle-A-Module.md).
7. Writes `<tfm>/en-US/<Module>.Shell.<stamp>.dll-Help.xml`, copies the library, and each helper `helpers` names (`<name>.exe` on Windows, `<name>` elsewhere), to `runtimes/<rid>/native/`, writes `<Module>.Format.ps1xml` when the module declares classes, then `<Module>.psd1` and `<Module>.psm1`. The script imports each bundled module into the session, from its folder, before its own shell, and leaves one the session already holds as it is; a failed import fails the module's import, or, for an entry with `on-import-failure = "warn"`, writes a warning naming the bundled module and the error and goes on without it.
8. Reads the instruction-set extensions the library was compiled to require out of the built file (the `pwrs_cpu_requirements` data `export_module!` emits, so a cross-built library reads the same way), subtracts what the target assumes of every CPU (`rustc --print cfg --target <triple>` with no flags), and warns about the rest unless `[package.metadata.pwrs] cpu-features` lists them. Such a library refuses to import on a CPU without them; see [How To Fix A Failure](../how-to/How-To-Fix-A-Failure.md#import-fails).
9. `cargo metadata --filter-platform <triple>` (the `--target` triple or the host's, with the build's feature flags) for the crates the library links: everything the package's normal dependencies reach, leaving out proc macros, what only they depend on, and build dependencies. A crate whose license expression the allowed licenses do not satisfy, or that declares no SPDX expression, stops the build, named with its expression. Otherwise `runtimes/<rid>/THIRD-PARTY-NOTICES.txt` lists each crate with its license and, word for word, the license, copying, notice and copyright files at the top of its source, and the folder's own `THIRD-PARTY-NOTICES.txt` covers the managed runtime and, with hand-written C#, the .NET Framework assemblies in `netstandard2.0`. A crate whose source carries no such file is named on standard error, unless `[package.metadata.pwrs.license-files]` supplies one for that crate and version, which is then quoted instead. See [How To Publish](../how-to/How-To-Publish.md#license-notices).

The runtime identifier is the building machine's, or the `--target` triple's: `win`, `linux`, `osx` or `freebsd` and `x64`, `arm64` or `x86` (`x86_64`, `aarch64`, `i686` in the triple).

A first `cargo pwrs build` of a module fresh from `cargo pwrs new output-demo` on Windows x64, exit 0. Cargo's own lines come first; `PoWerRuSt` was a path dependency on a checkout here, and its version is the one that checkout carries:

```text
    Updating crates.io index
     Locking 7 packages to latest compatible versions
   Compiling proc-macro2 v1.0.107
   Compiling quote v1.0.47
   Compiling unicode-ident v1.0.26
   Compiling pwrs-sys v0.2.1 (C:\Temp\pwrs-wip\crates\pwrs-sys)
   Compiling syn v3.0.6
   Compiling pwrs-macros v0.2.1 (C:\Temp\pwrs-wip\crates\pwrs-macros)
   Compiling PoWerRuSt v0.2.1 (C:\Temp\pwrs-wip\crates\pwrs)
   Compiling output-demo v0.1.0 (C:\Temp\cargo-pwrs-outputs\output-demo)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.97s
pwrs: output-demo declares no `authors`; the manifest says Author = 'pwrs', which cargo pwrs publish refuses
pwrs: module folder C:\Temp\cargo-pwrs-outputs\output-demo\target\pwrs\OutputDemo
```

The folder it wrote, with each file's size in bytes. A debug build writes the `.pdb` files; a release build leaves them out unless `--debug-symbols` asks for them:

```text
OutputDemo.psd1 484
OutputDemo.psm1 1583
THIRD-PARTY-NOTICES.txt 1760
net10.0\OutputDemo.Shell.a5bbe119072b4333.dll 6144
net10.0\OutputDemo.Shell.a5bbe119072b4333.pdb 11056
net10.0\Pwrs.Bootstrap.dll 8192
net10.0\Pwrs.Bootstrap.pdb 11708
net10.0\Pwrs.Runtime.dll 57856
net10.0\Pwrs.Runtime.pdb 29944
net10.0\en-US\OutputDemo.Shell.a5bbe119072b4333.dll-Help.xml 2478
netstandard2.0\OutputDemo.Shell.a5bbe119072b4333.dll 6656
netstandard2.0\OutputDemo.Shell.a5bbe119072b4333.pdb 8444
netstandard2.0\Pwrs.Bootstrap.dll 7680
netstandard2.0\Pwrs.Bootstrap.pdb 8820
netstandard2.0\Pwrs.Runtime.dll 73216
netstandard2.0\Pwrs.Runtime.pdb 28608
netstandard2.0\en-US\OutputDemo.Shell.a5bbe119072b4333.dll-Help.xml 2478
runtimes\win-x64\THIRD-PARTY-NOTICES.txt 3023
runtimes\win-x64\native\output_demo.dll 432640
```

## `merge`

`cargo pwrs merge <into> <from>...` copies every `runtimes/<rid>` folder of the `from` module folders into `into`, so one folder carries the native library of every platform it was built on, with the helpers beside each. The folders must hold the same `<Module>.psd1` text, which builds of one checkout produce on every platform: the GUID is derived from the module name, the rest of the manifest comes from `Cargo.toml`, and the hand-written `.cs` files are read in name order so the exported cmdlets and aliases do not follow a directory listing. A rid already present in `into` is refused rather than overwritten. Nothing else in the folders is touched: each rid folder carries its library's own `THIRD-PARTY-NOTICES.txt`, and the folder's own is the same text on every platform built from one checkout.

A bundled module inside a source folder, a subfolder `<Name>/` holding `<Name>.psd1`, has its `runtimes/<rid>` copied the same way into the destination's `<Name>/`, under the same manifest check and the same refusal of a rid already held; a destination without that bundled module is refused rather than given one.

Each rid it copies prints `pwrs: merged runtimes/<rid> from <folder>`, or `pwrs: merged <Name>/runtimes/<rid> from <folder>` for a bundled module's. A rid the destination already holds stops the command with exit 1 before anything is copied. The hello example built on Windows x64 in `Hello`, with the folder its Linux x64 build made in `linux\Hello`, merged and then merged again:

```text
> cargo pwrs merge Hello linux\Hello
pwrs: merged runtimes/linux-x64 from linux\Hello
> cargo pwrs merge Hello linux\Hello
cargo-pwrs: Hello already holds runtimes/linux-x64; remove it first
```

The first exits 0 and the second 1. `Hello\runtimes` after the first, each Linux file byte-identical to the one it was copied from:

```text
runtimes/linux-x64/THIRD-PARTY-NOTICES.txt               3027 bytes
runtimes/linux-x64/native/hello-helper                 392152 bytes
runtimes/linux-x64/native/libpwrs_example_hello.so    1376080 bytes
runtimes/win-x64/THIRD-PARTY-NOTICES.txt               3025 bytes
runtimes/win-x64/native/hello-helper.exe             134656 bytes
runtimes/win-x64/native/pwrs_example_hello.dll      1105920 bytes
```

## `test`

`cargo test` in the crate (the default profile), then `build`, then, when `<crate>/tests/` exists, `pester.ps1` with the module folder, the tests folder and the Pester path, in `pwsh` and on Windows also in `powershell`. Each host's Pester output reaches the console as the suite writes it, so a test that never returns is on screen by name. Pester comes from `PWRS_PESTER_PATH`, else `<target>/pester/Pester` when it exists, else `Import-Module Pester -MinimumVersion 4.0`. `PWRS_MODULE` is set for the tests, and `PWRS_THREAD_CHECK=1`, which makes a `PsObject` method called from a worker thread fail with `PwrsOffThread`. A failed test makes the command fail.

`--cpu-tiers <list>`, or `test-cpu-tiers` under `[package.metadata.pwrs]` when the flag is absent, runs the Pester suites once more for each listed level (`x86-64`, `x86-64-v2`, `x86-64-v3`, `x86-64-v4`, `native`) with `PWRS_CPU_MAX` set to it, so every kernel tier a module dispatches between runs on one machine; see [How To Use Instruction Sets](../how-to/How-To-Use-Instruction-Sets.md). A level outside that list is refused before anything runs.

### The surface check

Afterwards the command reports what the suite left unexercised of what the module declares. Two declarations are promises the engine keeps whether or not the body holds up its end, and neither breaks loudly: `SupportsShouldProcess` gives a cmdlet `-WhatIf` and `-Confirm` whether or not it ever calls `should_process`, so `-WhatIf` on a cmdlet that never asks does the thing it was supposed to describe; and a parameter declaring `ValueFromPipeline` binds whether or not a record is ever piped into it.

Neither is decidable from the declaration, so the run is the evidence. `cargo pwrs test` sets `PWRS_SURFACE_DIR`, each host writes what it exercised to its own file there, and the two are read against the descriptor. Three things are reported: a cmdlet no test invoked, a cmdlet that ran without ever asking though it declares `SupportsShouldProcess`, and a pipeline parameter nothing was ever piped into.

A finding never fails the command. Each one is either a test that is missing or a promise the body does not keep, and only the author knows which.

`cargo pwrs test` on the module fresh from `cargo pwrs new output-demo`, exit 0, after the build lines shown under `build`. Pester's own lines are in color on a terminal; they are shown here without the color codes:

```text
pwrs: Pester in pwsh

Starting discovery in 1 files.
Discovery found 2 tests in 181ms.
Running tests.
[+] C:\Temp\cargo-pwrs-outputs\output-demo\tests\OutputDemo.Tests.ps1 733ms (270ms|307ms)
Tests completed in 748ms
Tests Passed: 2, Failed: 0, Skipped: 0, Inconclusive: 0, NotRun: 0
pwrs pester host=7.6.6 pester=5.7.1 passed=2 failed=0
pwrs: Pester in Windows PowerShell

Starting discovery in 1 files.
Discovery found 2 tests in 753ms.
Running tests.
[+] OutputDemo.Tests.ps1 3.47s (998ms|1.81s)
Tests completed in 3.55s
Tests Passed: 2, Failed: 0, Skipped: 0, Inconclusive: 0, NotRun: 0
pwrs pester host=5.1.26100.9444 pester=5.7.1 passed=2 failed=0
pwrs: surface check: every cmdlet ran, every pipeline parameter took a record, every ShouldProcess asked
```

The `pwrs pester` line is each host's summary, with the host's version, the Pester version loaded, and the counts. When the surface check finds something, its line gives the count and each finding follows on a line of its own:

```text
pwrs: surface check: 1 of the module's declarations were not exercised
  Get-OutputDemoGreeting takes -Name from the pipeline and nothing was ever piped into it.
```

## `publish`

`build`, then `publish.ps1` (requires pwsh 7): `Test-ModuleManifest`; with `--dry-run`, `Compress-PSResource` into `target/pwrs/publish/<Module>/`; otherwise `Publish-PSResource -Repository PSGallery -ApiKey $env:PWRS_PSGALLERY_KEY`.

A crate that declares no `authors` or no `description` is refused before any of that runs. The manifest would carry `Author = 'pwrs'` or describe the module by name alone, and a module on a gallery names its own author and describes itself. `build` reports the same two on standard error and carries the placeholder, so a module under development still imports.

A module whose library is compiled to require extensions beyond its target's baseline is refused too, unless `cpu-features` lists them, because it would refuse to import on every CPU without them:

```toml
[package.metadata.pwrs]
cpu-features = ["avx2", "fma"]
```

So is a module whose library links a crate whose source carries no license file: its notices could name the crate's license but not quote it.

A bundled module is inside the folder `build` made, so the package carries it under `<Name>/` with no dependency declared for it; `publish` prints `pwrs: the package carries the bundled module <Name>` for each. A bundled module's library is held to the same two checks: extensions it requires that neither the bundled crate's `cpu-features` nor the entry's lists, and crates it links without a license text that neither the bundled crate's `license-files` nor the entry's supplies, refuse the package, naming the bundled module.

On success the script writes one line to standard output: with `--dry-run`, `packaged <path>` for each `.nupkg` in `target/pwrs/publish/<Module>/`; otherwise `published <Module> to PSGallery`. A refusal is exit 1 with `cargo-pwrs: <Module> cannot be published: ...`, naming what the crate must declare, the extensions it must list, or the crates without a license file, and for a bundled module's library, which bundled module it is.

## `new <dir>`

Creates `<dir>` (refusing an existing one) with `Cargo.toml`, `src/lib.rs`, `tests/<Module>.Tests.ps1` and `.gitignore`. The crate name is the directory name lowercased with `_` replaced by `-`; the module name is that in PascalCase. `src/lib.rs` declares one cmdlet, `Get-<Module>Greeting`, whose `-Name` takes pipeline input, and the suite calls it by name and pipes two names into it, so the first `cargo pwrs test` exercises everything the module declares.

```text
> cargo pwrs new output-demo
pwrs: created output-demo ; next: cargo pwrs test --manifest-dir output-demo
```

## `toolchain`

Ensures the toolchain and prints where it is, where `csc` is, and which `$PSHOME` was found. On Windows x64 with pwsh from the Microsoft Store:

```text
pwrs: toolchain at C:\Users\<user>\.pwrs\toolchain\5.9.0
pwrs: csc at C:\Users\<user>\.pwrs\toolchain\5.9.0\csc-5.9.0\tasks\netcore\bincore
pwrs: pwsh at C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.6.0_x64__8wekyb3d8bbwe
```

## Build settings

`cargo pwrs new` writes a `release` profile with `lto = "fat"`, `codegen-units = 1` and `panic = "unwind"`, and a module ships from it. Unwinding stays on because the exports catch panics at the boundary and report them as errors; `panic = "abort"` takes the host process down with the module.

Two further levers reach a module through `RUSTFLAGS` and need nothing from `cargo pwrs`: `-C target-cpu=native`, for a build that stays on the machine that made it, and profile-guided optimization through `-Cprofile-generate` and `-Cprofile-use`. Both pay in a module's own compute-bound code, and PWRS sets neither for distributable builds. A library built for the native CPU refuses to import on a CPU that lacks any extension it was compiled for, which `build` warns about and `publish` refuses; a module that wants wide instructions and runs everywhere builds at the baseline and chooses kernels at run time, as [How To Use Instruction Sets](../how-to/How-To-Use-Instruction-Sets.md) shows.

The managed side is compiled with `/optimize+`, calls its exports through unmanaged function pointers on .NET, and carries `[module: SkipLocalsInit]` over the runtime and the generated shell. Figures are in [Benchmarks](Benchmarks.md).

## The toolchain

Under `$PWRS_HOME` or `~/.pwrs`, at `toolchain/5.9.0/`:

| Folder | Package | Version |
|---|---|---|
| `csc-5.9.0/` | `Microsoft.Net.Compilers.Toolset` | 5.9.0, or whatever `PWRS_TOOLSET` names, which also names the root |
| `netcoreref-8.0.31/` | `Microsoft.NETCore.App.Ref` | 8.0.31 |
| `sma-7.4.0/` | `System.Management.Automation` | 7.4.0 |
| `netstandard-2.0.3/` | `NETStandard.Library` | 2.0.3 |
| `psstandard-5.1.1/` | `PowerShellStandard.Library` | 5.1.1 |
| `simd-4.6.1/` | `System.Numerics.Vectors` | 4.6.1 |
| `memory-4.6.3/` | `System.Memory` | 4.6.3 |
| `buffers-4.6.1/` | `System.Buffers` | 4.6.1 |
| `unsafe-6.1.2/` | `System.Runtime.CompilerServices.Unsafe` | 6.1.2 |
| `scripts-<stamp>/` | `fetch.ps1`, `csc.ps1`, in a folder named for their text | |
| `toolchain.lock` | package id, version, SHA-512 per fetched archive | |

`fetch.ps1` downloads each package from the NuGet v3 flat container, prints the archive's SHA-512, and extracts it; a package whose hash differs from the lock's record is refused, and a folder holding a `.complete` marker is not fetched again. Every package version has a folder of its own and the scripts' folder is named for them, so versions of `cargo-pwrs` that share a machine share this tree without writing a folder another version reads. `cargo-pwrs` 0.2.0 keeps its packages in the unversioned `csc/`, `netstandard/`, `psstandard/` and `simd/` beside these. `csc.ps1` loads `csc.dll` inside pwsh in an isolated `AssemblyLoadContext` (so it never collides with the Roslyn pwsh ships) and invokes its entry point with a response file. `pwsh` is found on `PATH` or through `PWRS_PWSH`; `$PSHOME` is asked of it or taken from `PWRS_PSHOME`.

The compile options: `/nostdlib+ /target:library /unsafe+ /nullable:enable /langversion:latest /deterministic+ /optimize+ /debug:portable /warnaserror- /nowarn:CS1591,CS8981`, plus `/define` for the framework symbols the .NET SDK defines (net8.0's for `net10.0`, netstandard2.0's for `netstandard2.0`) and a `/reference` per assembly. `net10.0` references `Microsoft.NETCore.App.Ref`'s `ref/net8.0/*.dll` and the `System.Management.Automation` 7.4.0 reference; `netstandard2.0` references the NETStandard reference assemblies, PowerShellStandard's `System.Management.Automation.dll`, and the `lib/net462` builds of `System.Numerics.Vectors`, `System.Memory`, `System.Buffers` and `System.Runtime.CompilerServices.Unsafe`, which a module with hand-written C# also ships in its `netstandard2.0` folder.

## The generated files

- **`<Module>.psd1`**: `RootModule` the `.psm1`, `ModuleVersion` from `Cargo.toml`, a `GUID` derived from the module name (two FNV-1a hashes, over the name and over it reversed), `Author` from the first author, `Description` from the package description, `CompatiblePSEditions` Desktop and Core, `PowerShellVersion` 5.1, `FormatsToProcess` when a format file exists, `CmdletsToExport` (the Rust cmdlets, then the hybrid C# ones), `AliasesToExport` (their aliases in the same order), `Tags` of `pwrs` followed by the package keywords (whitespace in a keyword becomes a hyphen, since a gallery tag holds none), and `ProjectUri` from the package repository when it has one.
- **`<Module>.psm1`**: described in [Two Hosts](Two-Hosts.md). Its `Export-ModuleMember` names the cmdlets and the aliases, since the engine creates a cmdlet's `[Alias]` members inside the nested binary module and the root module is what carries them into the session.
- **`<Module>.Shell.cs`**: one namespace `Pwrs.Modules.<Module>` holding the module class, the cmdlets, the completers and the providers; class and enum types in the namespaces their names declare.
- **Help**: MAML with the synopsis, description, syntax, parameters (type, position, pipeline input, aliases), input types from pipeline parameters, return values from `output`, and examples.
- **Format file**: one view per copied, proxy or psobject class; a table for five or fewer fields, else a list.
