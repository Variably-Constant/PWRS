---
title: cargo pwrs Reference
weight: 4
---

The build tool. Source: `crates/cargo-pwrs/src/*.rs`, `crates/cargo-pwrs/scripts/*.ps1`, `crates/pwrs-build/src/*.rs`, `crates/pwrs-build/scripts/*.ps1`.

## Invocation

```text
cargo pwrs <build|test|publish|sign|new|toolchain> [--release] [--target <triple>] [--features <a,b>] [--all-features] [--debug-symbols] [--package <name>] [--manifest-dir <dir>] [--dry-run] [--cpu-tiers <x86-64,x86-64-v3,...>] [--pwrs <dependency spec>] [<new dir>]
cargo pwrs publish --folder <module dir> [--release] [--dry-run]
cargo pwrs sign [--folder <module dir>] [--pfx <file> | --pem <'["certs", "key"]'> | --store <thumbprint> | --command <'["program", "args"]'> | --pkcs11 <'{ library = "...", token = "...", key = "..." }'>] [--password-env <name>] [--certificate <file>] [--timestamp-url <url>] [--hash <sha256|sha384|sha512>] [--catalog] [--signer <pwrs|openauthenticode>]
cargo pwrs merge <into module dir> <from module dir>...
```

`cargo-pwrs` is a cargo subcommand binary; `cargo pwrs ...` and `cargo-pwrs pwrs ...` are the same. From the PWRS checkout without installing it: `cargo run -p cargo-pwrs -- pwrs ...`.

| Flag | Applies to | Effect |
|---|---|---|
| `--release` | build, test, publish, sign | `cargo build --release`; the library is read from `target/release` instead of `target/debug`. With `publish --folder`, every runtime in the folder must be a release build |
| `--target` | build, test, publish, sign | `cargo build --target <triple>`; the library is read from `target/<triple>/<profile>` and placed under the triple's runtime identifier. A triple that is not the building machine's needs a linker for it in cargo's configuration, and the crate is built for the host as well, since the descriptor is read by loading a library |
| `--features`, `-F` | build, test, publish, sign | passed to `cargo build` and, for `test`, to `cargo test`; comma- or space-separated, repeatable |
| `--all-features` | build, test, publish, sign | passed to `cargo build` and `cargo test` |
| `--debug-symbols` | build, test, publish, sign | writes a `.pdb` beside each managed assembly, and for a Windows target puts the library's and each helper's `.pdb` beside them in `runtimes/<rid>/native/`. A build without `--release` does so anyway; this makes a release build do it too |
| `--package`, `-p` | build, test, publish, sign | which package in the workspace, when the manifest directory is not the package's own |
| `--manifest-dir` | build, test, publish, sign | the crate directory; defaults to the current directory |
| `--cpu-tiers` | test | runs the Pester suites once more under each x86-64 level listed; see [`test`](#test) |
| `--dry-run` | publish | package only |
| `--folder` | publish, sign | the module folder to publish or sign as it stands, such as one `merge` joined from several platforms' builds; nothing is built. `publish --folder` takes only `--release` and `--dry-run` besides; `sign --folder` takes only `--manifest-dir`, `--package` and the signing settings. See [`publish`](#publish) and [`sign`](#sign) |
| `--pfx`, `--pem`, `--store`, `--command`, `--pkcs11`, `--password-env`, `--certificate`, `--timestamp-url`, `--hash`, `--catalog`, `--signer` | sign, publish | each replaces the setting of the same name in `[package.metadata.pwrs.signing]`, and a key flag (the first five) replaces the table's key. A flag takes its value as `Cargo.toml` writes it, and `--catalog` alone means true. See [`sign`](#sign) |
| `--pwrs` | new | the `pwrs` dependency spec written into the new `Cargo.toml`; the default is `{ package = "PoWerRuSt", version = "<this tool's version>" }`, which names the package because crates.io has an unrelated `pwrs`, and takes the version from `cargo-pwrs` itself because the workspace publishes the two together |

A command refuses each flag it does not use, naming the flags and the command, and exits 2 with the usage text. `merge` and `toolchain` take no flag.

With `PWRS_TRACE` set to anything but empty or `0`, every command prints a line to stderr when each process it starts begins and another when it ends; see [How To Trace A Module](../how-to/How-To-Trace-A-Module.md#tracing-cargo-pwrs).

## What each command returns

Every subcommand exits 0 when it succeeds. A failure prints `cargo-pwrs: <what failed>` on standard error and exits 1. An unknown subcommand, an unknown flag, a flag without its value, or a flag the command does not use prints the usage line and exits 2. Each command writes its own progress to standard error, one line per event beginning `pwrs: `, among the lines cargo, the compiler and Pester write themselves. Only `publish` writes to standard output: the result line of its script.

| Command | Exit 0 means | Writes | Its own lines on success |
|---|---|---|---|
| `build` | the module folder is complete | `target/pwrs/<Module>/`, listed under [The generated files](#the-generated-files) | `pwrs: module folder <path>`, last; before it, a warning for each of: extensions compiled in beyond the target's baseline, crates without a license file, a missing `authors` or `description`, `[Cmdlet]` attributes in hand-written C# that export no cmdlet, because they are not on a class or their verb or noun is not a string literal or a `Verbs*` constant, and a class the library writes that `export_module!` does not list |
| `test` | `cargo test` passed, and every Pester suite passed in each host | what `build` writes, and each host's surface file under `target/pwrs/work/surface/` | `pwrs: Pester in pwsh`, and on Windows `pwrs: Pester in Windows PowerShell`, each before that host's Pester output; one pair more per CPU tier; then the surface check's line and a line per finding |
| `publish` | the module was packaged, or published | with `--dry-run`, `target/pwrs/publish/<Module>/<Module>.<version>.nupkg`, or with `--folder`, `publish/<Module>/<Module>.<version>.nupkg` beside the folder | on standard output `packaged <path of the .nupkg>`, or `published <Module> to PSGallery` |
| `sign` | every script and Windows executable in the folder has a valid signature, its own or a new one, and with `catalog` the catalog lists every file as it is | the new signatures, inside the files they sign, and with `catalog`, `<Module>.cat` | `pwrs: signing as <subject>, <hash>, timestamped by <url>` (or `with no timestamp, ...`); a line for each file left as it is or signed; `pwrs: <N> other files take no signature of their own ...`; with `catalog`, `pwrs: made the catalog <Module>.cat ...` or `pwrs: left the catalog <Module>.cat as it is ...` |
| `new` | the crate was created | `<dir>/Cargo.toml`, `src/lib.rs`, `tests/<Module>.Tests.ps1`, `.gitignore` | `pwrs: created <dir> ; next: cargo pwrs test --manifest-dir <dir>` |
| `merge` | every `runtimes/<rid>` of every source folder was copied, and every `<Bundled>/runtimes/<rid>` into the destination's copy of that bundled module | `runtimes/<rid>/` and `<Bundled>/runtimes/<rid>/` in the destination folder | `pwrs: merged runtimes/<rid> from <source folder>`, one per rid, and `pwrs: merged <Bundled>/runtimes/<rid> from <source folder>` per bundled module and rid |
| `toolchain` | the toolchain is in place | the toolchain, when a package is missing (below) | `pwrs: toolchain at <root>`, `pwrs: csc at <folder>`, `pwrs: Pester at <manifest>`, `pwrs: pwsh at <$PSHOME>` |

## `build`

1. `cargo metadata --no-deps` in the manifest directory; the package is the one whose manifest path matches, or `--package`. It must have a `cdylib` target, and each name `[package.metadata.pwrs] helpers` lists must be one of its `[[bin]]` targets; a name that is not stops the build, named. See [How To Ship A Helper Executable](../how-to/How-To-Ship-A-Helper-Executable.md).
2. `cargo build -p <package> [--release] [--target <triple>]`, which builds the package's `[[bin]]` targets with its library, for the same target and profile, and a second build for the host when the triple is not the host's. A GNU/Linux triple with a glibc version after a dot, as cargo-zigbuild spells it (`x86_64-unknown-linux-gnu.2.35`), is built by `cargo zigbuild` instead, which links the library against that glibc's symbols; the other steps use the triple without the version. It needs cargo-zigbuild and zig on `PATH`.
3. Loads the host's library and calls `pwrs_module_descriptor`; the JSON must declare `abi` 1, and two classes it lists under one name, compared without regard to case, stop the build, named with their Rust paths. Then reads every class record out of the built library. Each `#[psclass]` and `#[psenum]` type's id lookup contains `PWRS-CLASS/1`, its name and its Rust path, so the library has one for every class whose values it can write, listed or not. A cross-built library is read the same way. A class the descriptor does not list, under a name another class has, stops the build, naming both; one under a name of its own is named in a warning.
4. Ensures the toolchain (below).
5. Under `target/pwrs/work/<Module>/` writes the runtime and bootstrap sources and the generated `<Module>.Shell.cs`, and adds every `.cs` under `<crate>/src/csharp/`, noting the `Verb-Noun` and the class-level `[Alias]` names of each `[Cmdlet(verb, noun)]` class in them. Compiles `Pwrs.Bootstrap.dll`, `Pwrs.Runtime.dll` and `<Module>.Shell.<stamp>.dll` for `net10.0` and `netstandard2.0` into `target/pwrs/<Module>/<tfm>/`. The folder is cleared first. `<stamp>` is a hash of the managed source the shell is compiled from (the generated `<Module>.Shell.cs`, the `src/csharp/` files and the runtime) and of the `references` named, so a change to any of them gives the shell a new assembly name. The shell's two compiles also reference the assemblies `references` names, looked up separately for each compile, and the build prints `pwrs: references <name> for <halves>`; see [How To Add Hybrid C#](../how-to/How-To-Add-Hybrid-CSharp.md#windows-forms-wpf-and-other-desktop-assemblies).
6. Builds the module of each crate `[package.metadata.pwrs] bundled-modules` names, through this same tool with the same `--release`, `--target` and `--debug-symbols`, `--locked`, into `target/pwrs-bundled/<crate directory name>/` under the bundling module's target directory, so the named tree is only read. An entry is the crate directory as a string, or a table with `path` and any of `on-import-failure` (`"stop"`, the default, or `"warn"`), `features`, `default-features`, `license-files` and `cpu-features`. The bundled build uses the entry's features, and the entry's license files and extensions as well as the bundled crate's own. A path is joined to the crate directory and may leave the repository; one with no `Cargo.toml` stops the build, named, as does a key or value the table does not accept. The bundled module's `Pwrs.Bootstrap.dll` and `Pwrs.Runtime.dll` under both `net10.0` and `netstandard2.0` must match the bundling module's byte for byte, since a Windows PowerShell session loads one runtime and every module built by one tool uses it. A file that differs stops the build, named. The folder is then copied to `target/pwrs/<Module>/<Name>/`. See [How To Bundle A Module](../how-to/How-To-Bundle-A-Module.md).
7. Writes `<tfm>/en-US/<Module>.Shell.<stamp>.dll-Help.xml`. Copies the library and each helper `helpers` names to `runtimes/<rid>/native/`: `<name>.exe` on Windows, `<name>` elsewhere, and on macOS a helper `macos-bundles` names inside its app bundle `<bundle name>.app`. For a Windows target, when the build writes `.pdb` files, it copies the `.pdb` the library and each helper records beside them, under the name recorded. Puts beside them each file `native-files` names for the target, taken from its download as the keys table below says. Then writes `<Module>.Format.ps1xml` when the module declares classes, and `<Module>.psd1` and `<Module>.psm1`. The script imports each bundled module from its folder before it imports its own shell, and leaves one the session already has as it is. A failed import fails the module's import, or, for an entry with `on-import-failure = "warn"`, writes a warning naming the bundled module and the error and continues without it.
8. Reads the instruction-set extensions the library was compiled to require out of the built file: the `pwrs_cpu_requirements` data `export_module!` emits, so a cross-built library is read the same way. Leaves out the ones every CPU of the target has (`rustc --print cfg --target <triple>` with no flags), and warns about the rest unless `[package.metadata.pwrs] cpu-features` lists them. Such a library fails to import on a CPU without them; see [How To Fix A Failure](../how-to/How-To-Fix-A-Failure.md#import-fails).
9. `cargo metadata --filter-platform <triple>`, with the `--target` triple or the host's and the build's feature flags, lists the crates the library links: everything the package's normal dependencies reach, leaving out proc macros, what only they depend on, and build dependencies. `cargo metadata` unifies features across the workspace. So the list has only the crates that `cargo tree --package <name>@<version> --target <triple>` reaches with the same feature flags, which are the ones `cargo build -p` compiles for the package alone. A crate that only another member's features turn on is neither listed nor checked. A crate whose license expression the allowed licenses do not satisfy, or that declares no SPDX expression, stops the build, named with its expression. Otherwise `runtimes/<rid>/THIRD-PARTY-NOTICES.txt` lists each crate with its license and, word for word, the license, copying, notice and copyright files at the top of its source. The folder's own `THIRD-PARTY-NOTICES.txt` covers the managed runtime and, with hand-written C#, the .NET Framework assemblies in `netstandard2.0`. A crate whose source has no such file is named on standard error, unless `[package.metadata.pwrs.license-files]` supplies one for that crate and version, which is then quoted instead. That entry is an unscoped one, or one in a `license-files.'cfg(...)'` table whose expression the target's cfg satisfies, as `rustc --print cfg --target <triple>` prints it; that command runs only when such a table exists. For an entry whose value is a table with `add`, the files it lists are quoted after the crate's own, as notices for code the crate links that is not its own. The files of a `self` entry's `add` list are quoted after every crate, under a section for the module's own crate, as notices for code that crate compiles that is not its own. After them come the files `native-files` shipped beside the library, each under its name with the address it was downloaded from, its SHA-256, its path in the archive, and its license files word for word. See [How To Publish](../how-to/How-To-Publish.md#license-notices).
10. Writes the build's record, `runtimes/<rid>/pwrs-build.json`: the cargo-pwrs that built it, whether it was a release build, the `[package]` keys the manifest has a placeholder for, the extensions step 8 warned about and the crates step 9 named without a license text. `merge` copies it with the runtime, and `publish --folder` refuses what it records; see [`publish`](#publish).

The runtime identifier is the building machine's, or the `--target` triple's: `win`, `linux`, `osx` or `freebsd` and `x64`, `arm64` or `x86` (`x86_64`, `aarch64`, `i686` in the triple).

A first `cargo pwrs build` of a module fresh from `cargo pwrs new output-demo` on Windows x64, exit 0. Cargo's own lines come first; `PoWerRuSt` was a path dependency on a checkout here, and its version is the one that checkout has:

```text
    Updating crates.io index
     Locking 7 packages to latest Rust 1.98.1 compatible versions
   Compiling proc-macro2 v1.0.107
   Compiling quote v1.0.47
   Compiling unicode-ident v1.0.27
   Compiling pwrs-sys v0.8.0 (C:\Projects\pwrs\crates\pwrs-sys)
   Compiling syn v3.0.9
   Compiling pwrs-macros v0.8.0 (C:\Projects\pwrs\crates\pwrs-macros)
   Compiling PoWerRuSt v0.8.0 (C:\Projects\pwrs\crates\pwrs)
   Compiling output-demo v0.1.0 (C:\Temp\cargo-pwrs-outputs\output-demo)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.57s
pwrs: output-demo declares no `authors`; the manifest says Author = 'pwrs', which cargo pwrs publish refuses
pwrs: module folder C:\Temp\cargo-pwrs-outputs\output-demo\target\pwrs\OutputDemo
```

The folder it wrote, with each file's size in bytes. A debug build writes the `.pdb` files; a release build leaves them out unless `--debug-symbols` is given. The `netstandard2.0` ones are larger because on Windows they are in the Windows format:

```text
OutputDemo.psd1 487
OutputDemo.psm1 3286
THIRD-PARTY-NOTICES.txt 7525
net10.0\OutputDemo.Shell.5c121ab70aac8542.dll 6144
net10.0\OutputDemo.Shell.5c121ab70aac8542.pdb 11004
net10.0\Pwrs.Bootstrap.dll 9728
net10.0\Pwrs.Bootstrap.pdb 12328
net10.0\Pwrs.Runtime.dll 99840
net10.0\Pwrs.Runtime.pdb 44472
net10.0\en-US\OutputDemo.Shell.5c121ab70aac8542.dll-Help.xml 2478
netstandard2.0\Microsoft.Win32.Registry.dll 31376
netstandard2.0\OutputDemo.Shell.5c121ab70aac8542.dll 6144
netstandard2.0\OutputDemo.Shell.5c121ab70aac8542.pdb 19968
netstandard2.0\Pwrs.Bootstrap.dll 8704
netstandard2.0\Pwrs.Bootstrap.pdb 22016
netstandard2.0\Pwrs.Runtime.dll 116736
netstandard2.0\Pwrs.Runtime.pdb 253440
netstandard2.0\System.IO.FileSystem.AccessControl.dll 27792
netstandard2.0\System.Security.AccessControl.dll 39048
netstandard2.0\System.Security.Principal.Windows.dll 23688
netstandard2.0\en-US\OutputDemo.Shell.5c121ab70aac8542.dll-Help.xml 2478
runtimes\win-x64\pwrs-build.json 170
runtimes\win-x64\THIRD-PARTY-NOTICES.txt 3012
runtimes\win-x64\native\output_demo.dll 467456
runtimes\win-x64\native\output_demo.pdb 5599232
```

The build's record, `runtimes\win-x64\pwrs-build.json`, with the placeholder author the build warned about:

```json
{
  "built_by": "cargo-pwrs 0.8.0",
  "release": false,
  "missing_metadata": [
    "authors"
  ],
  "undeclared_cpu_features": [],
  "crates_without_license_text": []
}
```

## `merge`

`cargo pwrs merge <into> <from>...` copies every `runtimes/<rid>` folder of the `from` module folders into `into`, so one folder has the native library of every platform it was built on, with the helpers beside each. The `<Module>.psd1` of every folder must have the same text, which builds of one checkout produce on every platform. The GUID is derived from the module name, and the rest of the manifest comes from `Cargo.toml`. The hand-written `.cs` files are read in name order, so the order of the exported cmdlets and aliases does not depend on how a platform lists a folder. A rid already present in `into` is refused rather than overwritten. Nothing else in the folders is touched: each rid folder has its library's own `THIRD-PARTY-NOTICES.txt` and its build's record, which `publish --folder` reads, and the folder's own notices are the same text on every platform built from one checkout. The joined folder is published with `cargo pwrs publish --folder`; a `publish` without it builds again, which clears the folder first.

A bundled module inside a source folder, a subfolder `<Name>/` with `<Name>.psd1` in it, has its `runtimes/<rid>` copied the same way into the destination's `<Name>/`, with the same manifest check. A rid that folder already has is refused the same way, and a destination without that bundled module is refused rather than given one.

Each rid it copies prints `pwrs: merged runtimes/<rid> from <folder>`, or `pwrs: merged <Name>/runtimes/<rid> from <folder>` for a bundled module's. A rid the destination already has stops the command with exit 1 before anything is copied. The hello example built on Windows x64 in `Hello`, with the folder its Linux x64 build made in `linux\Hello`, merged and then merged again:

```text
> cargo pwrs merge Hello linux\Hello
pwrs: merged runtimes/linux-x64 from linux\Hello
pwrs: merged Calc/runtimes/linux-x64 from linux\Hello\Calc
> cargo pwrs merge Hello linux\Hello
cargo-pwrs: Hello already has runtimes/linux-x64; remove it first
```

The first exits 0 and the second 1. `Hello\runtimes` after the first, each Linux file byte-identical to the one it was copied from:

```text
runtimes/linux-x64/THIRD-PARTY-NOTICES.txt               3016 bytes
runtimes/linux-x64/native/hello-helper                 394432 bytes
runtimes/linux-x64/native/libpwrs_example_hello.so    2186752 bytes
runtimes/linux-x64/pwrs-build.json                        152 bytes
runtimes/win-x64/THIRD-PARTY-NOTICES.txt                 3014 bytes
runtimes/win-x64/native/hello-helper.exe               136192 bytes
runtimes/win-x64/native/pwrs_example_hello.dll        1958912 bytes
runtimes/win-x64/pwrs-build.json                          152 bytes
```

## `test`

`cargo test` in the crate (the default profile), then `build`, then, when `<crate>/tests/` exists, `pester.ps1` with the module folder, the tests folder and the Pester path, in `pwsh` and on Windows also in `powershell`. Each host's Pester output reaches the console as the suite writes it, so a test that never returns is on screen by name. A host's error stream reaches it through `cargo pwrs`, which passes plain text on as it arrives. A PowerShell that a test starts with `-EncodedCommand` writes its records to that stream as CLIXML, and `cargo pwrs` writes each one as the host writes it as text. That is an error's lines as they are, and a warning, verbose or debug record after `WARNING: `, `VERBOSE: ` or `DEBUG: `, with progress and information records left out. The Pester is the module `PWRS_PESTER_PATH` names, else Pester 6.2.0, which the toolchain fetches the first time, before `cargo test` runs, and both hosts import from there (see [The toolchain](#the-toolchain)). A Pester that does not load stops the run with one line naming its path and the import's error. `PWRS_MODULE` is set for the tests, and `PWRS_THREAD_CHECK=1`, which makes a `PsObject` method called from a worker thread fail with `PwrsOffThread`. A failed test makes the command fail.

`--cpu-tiers <list>`, or `test-cpu-tiers` under `[package.metadata.pwrs]` when the flag is absent, runs the Pester suites once more for each listed level (`x86-64`, `x86-64-v2`, `x86-64-v3`, `x86-64-v4`, `native`) with `PWRS_CPU_MAX` set to it. So every kernel tier a module dispatches between runs on one machine; see [How To Use Instruction Sets](../how-to/How-To-Use-Instruction-Sets.md). A level outside that list is refused before anything runs.

### The surface check

Afterward the command reports the module's declarations that the suite did not exercise. PowerShell acts on two declarations whether or not the body does its part, and neither shows an error when it does not:

- `supports_should_process` gives a cmdlet `-WhatIf` and `-Confirm` whether or not it ever calls `should_process`, so `-WhatIf` on a cmdlet that never calls it does the very thing it should only describe;
- a parameter that takes pipeline input, by value or by property name, accepts whatever is piped to it whether or not the body handles it.

The declarations alone cannot show either, so the check uses what the tests ran. `cargo pwrs test` sets `PWRS_SURFACE_DIR`, each host writes what its tests ran to its own file there, and the two files are compared with the descriptor. Three things are reported: a cmdlet no test invoked, a cmdlet that declares `supports_should_process` but ran without ever calling `should_process`, and a parameter declared `value_from_pipeline`, `value_from_pipeline_by_property_name` or `literal_path`, which binds a piped object's `PSPath`, that nothing was ever piped into.

A finding never fails the command. Each one is either a missing test or a declaration the body does not follow, and only the author can tell which.

`cargo pwrs test` on the module fresh from `cargo pwrs new output-demo`, exit 0, after the build lines shown under `build`. Pester's own lines are in color on a terminal; they are shown here without the color codes:

```text
pwrs: Pester in pwsh

Running tests from 1 files.
[+] C:\Temp\cargo-pwrs-outputs\output-demo\tests\OutputDemo.Tests.ps1 472ms (2 tests)
Tests completed in 482ms
Tests Passed: 2, Failed: 0, Skipped: 0, Inconclusive: 0, NotRun: 0
pwrs pester host=7.6.7 pester=6.2.0 passed=2 failed=0
pwrs: Pester in Windows PowerShell

Running tests from 1 files.
[+] C:\Temp\cargo-pwrs-outputs\output-demo\tests\OutputDemo.Tests.ps1 565ms (2 tests)
Tests completed in 578ms
Tests Passed: 2, Failed: 0, Skipped: 0, Inconclusive: 0, NotRun: 0
pwrs pester host=5.1.26100.9711 pester=6.2.0 passed=2 failed=0
pwrs: surface check: every cmdlet ran, every pipeline parameter took a record, every ShouldProcess cmdlet called should_process
```

The `pwrs pester` line is each host's summary, with the host's version, the Pester version loaded, and the counts. When the surface check finds something, its line gives the count and each finding follows on a line of its own:

```text
pwrs: surface check: 1 of the module's declarations was not exercised
  Get-OutputDemoGreeting takes -Name from the pipeline and nothing was ever piped into it.
```

## `publish`

`build`, then [`sign`](#sign) when the crate has `[package.metadata.pwrs.signing]` or a signing flag is given. Then `publish.ps1`, which requires pwsh 7, runs `Test-ModuleManifest` and one of two steps:

- With `--dry-run`, it removes the packages an earlier run left in `target/pwrs/publish/<Module>/` and packages the module there with `Compress-PSResource`. Under a pwsh whose PSResourceGet lacks that command, it stops with an error naming the PSResourceGet found and the `Install-PSResource` command that installs one that has it.
- Otherwise it runs `Publish-PSResource -Repository PSGallery -ApiKey $env:PWRS_PSGALLERY_KEY`.

A crate that declares no `authors`, or no `description` under `[package]` or `[package.metadata.pwrs]`, is refused before any of that runs, since the manifest would then say `Author = 'pwrs'` or describe the module by its name alone. `build` reports the same two on standard error and writes the placeholder, so a module under development still imports.

A module whose library is compiled to require extensions beyond its target's baseline is refused too, unless `cpu-features` lists them, because it would fail to import on every CPU without them:

```toml
[package.metadata.pwrs]
cpu-features = ["avx2", "fma"]
```

So is a module whose library links a crate whose source has no license file: its notices could name the crate's license but not quote it.

A bundled module is inside the folder `build` made, so the package includes it under `<Name>/` with no dependency declared for it; `publish` prints `pwrs: the package includes the bundled module <Name>` for each. A bundled module's library gets the same two checks. Extensions it requires that neither the bundled crate's `cpu-features` nor the entry's lists, and crates it links without a license text that neither the bundled crate's `license-files` nor the entry's supplies, make `publish` refuse the package, naming the bundled module.

On success the script writes one line to standard output: with `--dry-run`, `packaged <path>` naming the `.nupkg` it made; otherwise `published <Module> to PSGallery`. A refusal is exit 1 with `cargo-pwrs: <Module> cannot be published: ...`, naming what the crate must declare, the extensions it must list, or the crates without a license file, and for a bundled module's library, which bundled module it is.

### `publish --folder`

`cargo pwrs publish --folder <module dir>` publishes a module folder as it stands, such as one `merge` joined from several platforms' builds: no build runs, and `publish.ps1` takes the folder, packaging a dry run into `publish/<Module>/` beside it in the same way. The folder contains one `<Module>.psd1` and a `runtimes/` folder. Each runtime's `pwrs-build.json`, and each bundled module's runtime's, is read in place of a build. What any of them records is refused as `publish` refuses its own build's, naming the runtime: `its <rid> native library` or `the <rid> native library of its bundled module <Name>`. A runtime with no record is refused, since what its build found cannot be checked, as is a record with a field this cargo-pwrs does not read, which a newer one may have written. With `--release`, a runtime whose build was not a release build is refused too. Every refusal is reported at once, one line each, the module's missing `[package]` keys first. With `--folder`, a flag that changes a build, such as `--target`, `--features` or `--manifest-dir`, is refused, since the folder's builds were already made with theirs. A signing setting is refused too, since `publish --folder` signs nothing; the folder is signed first with `cargo pwrs sign --folder`. See [How To Publish](../how-to/How-To-Publish.md#a-module-built-on-several-platforms).

## `sign`

`cargo pwrs sign` builds the module as `build` does and signs `target/pwrs/<Module>/`, or with `--folder` signs the folder it names as it is, such as one `merge` joined. The key and the options come from `[package.metadata.pwrs.signing]` in the crate's `Cargo.toml`, with each signing flag in place of its setting. With neither, it stops with `there is nothing to sign with`. [How To Sign A Module](../how-to/How-To-Sign-A-Module.md) describes each key source and setting, with a whole run.

It signs every file of the folder that can have an Authenticode signature, the bundled modules' included:

- each `.ps1`, `.psm1`, `.psd1`, `.ps1xml`, `.psc1`, `.cdxml` and `.mof`, with the signature block at the end of the file, in the file's own encoding;
- each `.dll` and `.exe` that is a Windows image, with the signature in its certificate table.

A file whose signature is valid is left as it is, whoever signed it. A file whose signature no longer matches its contents is signed again, with the reason in its line. Before anything is signed, it refuses a script that Windows reads in the reading machine's code page and that has text beyond ASCII, naming it: a script with no byte order mark and no UTF-8 in its first 32 bytes. Nothing is signed while a file cannot be. With `catalog = true`, `<Module>.cat` lists every file of the folder by its hashes, laid out as `New-FileCatalog -CatalogVersion 2` writes one, and is signed. A catalog that still lists the folder's files as they are is left as it is.

`merge` compares manifests without their signature blocks, so a signed folder still takes another platform's runtimes. Signing it again then signs only the files the merge added, and makes the catalog again.

The signer is C# of cargo-pwrs's own, compiled the first time into `target/pwrs/work/signing-<stamp>/` against .NET 8's reference pack and `System.Security.Cryptography.Pkcs` 8.0.2, and run by `sign.ps1` in pwsh 7.4 or later: `PWRS_PWSH` when set, else `pwsh` on `PATH`. A timestamp server's RFC 3161 token goes on each signature as the attribute Windows reads. `signer = "openauthenticode"` signs the scripts and executables through OpenAuthenticode 0.6.3 instead, and the catalog is still made by cargo-pwrs.

## `new <dir>`

Creates `<dir>` (refusing an existing one) with `Cargo.toml`, `src/lib.rs`, `tests/<Module>.Tests.ps1` and `.gitignore`. The crate name is the directory name lowercased with `_` replaced by `-`; the module name is that in PascalCase. `src/lib.rs` declares one cmdlet, `Get-<Module>Greeting`, whose `-Name` takes pipeline input, and the suite calls it by name and pipes two names into it, so the first `cargo pwrs test` exercises everything the module declares.

```text
> cargo pwrs new output-demo
pwrs: created output-demo ; next: cargo pwrs test --manifest-dir output-demo
```

## `toolchain`

Ensures the toolchain, Pester included, and prints where it is, where `csc` is, where the Pester `test` runs suites under is, and which `$PSHOME` was found. On Windows x64 with pwsh from the Microsoft Store:

```text
pwrs: toolchain at C:\Users\<user>\.pwrs\toolchain\5.9.0\v2
pwrs: csc at C:\Users\<user>\.pwrs\toolchain\5.9.0\v2\csc-5.9.0\tasks\netcore\bincore
pwrs: Pester at C:\Users\<user>\.pwrs\toolchain\5.9.0\v2\pester-6.2.0\tools\Pester.psd1
pwrs: pwsh at C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.7.0_x64__8wekyb3d8bbwe
```

## Build settings

`cargo pwrs new` writes a `release` profile with `lto = "fat"`, `codegen-units = 1` and `panic = "unwind"`, and a module ships from it. The exports catch a panic and report it as an error, which needs `panic = "unwind"`; with `panic = "abort"`, a panic ends the PowerShell process.

Two more settings reach a module through `RUSTFLAGS` and need nothing from `cargo pwrs`: `-C target-cpu=native`, for a build that runs only on the machine that made it, and profile-guided optimization through `-Cprofile-generate` and `-Cprofile-use`. Both can make a module's own compute-bound code faster, and PWRS sets neither. A library built for the native CPU fails to import on a CPU that lacks any extension it was compiled for, which `build` warns about and `publish` refuses. To use wide instructions and still run on every CPU, build at the baseline and pick the code path at run time, as [How To Use Instruction Sets](../how-to/How-To-Use-Instruction-Sets.md) shows.

The managed side is compiled with `/optimize+`, the runtime and the generated shell with `[module: SkipLocalsInit]`, and on .NET the runtime calls the library's exports through unmanaged function pointers. Figures are in [Benchmarks](Benchmarks.md).

## The toolchain

Under `$PWRS_HOME` or `~/.pwrs`, at `toolchain/5.9.0/v2/`:

| Folder | Package | Version |
|---|---|---|
| `csc-5.9.0/` | `Microsoft.Net.Compilers.Toolset` | 5.9.0, or whatever `PWRS_TOOLSET` names, which also names the folder above `v2/` |
| `netcoreref-8.0.31/` | `Microsoft.NETCore.App.Ref` | 8.0.31 |
| `sma-7.4.0/` | `System.Management.Automation` | 7.4.0 |
| `netstandard-2.0.3/` | `NETStandard.Library` | 2.0.3 |
| `psstandard-5.1.1/` | `PowerShellStandard.Library` | 5.1.1 |
| `simd-4.6.1/` | `System.Numerics.Vectors` | 4.6.1 |
| `memory-4.6.3/` | `System.Memory` | 4.6.3 |
| `buffers-4.6.1/` | `System.Buffers` | 4.6.1 |
| `unsafe-6.1.2/` | `System.Runtime.CompilerServices.Unsafe` | 6.1.2 |
| `accesscontrol-4.5.0/` | `System.Security.AccessControl` | 4.5.0 |
| `principalwindows-4.5.0/` | `System.Security.Principal.Windows` | 4.5.0 |
| `filesystemaccesscontrol-4.5.0/` | `System.IO.FileSystem.AccessControl` | 4.5.0 |
| `registry-4.5.0/` | `Microsoft.Win32.Registry` | 4.5.0 |
| `desktopref-8.0.31/` | `Microsoft.WindowsDesktop.App.Ref`, fetched only for a module that names `references` | 8.0.31 |
| `netfxref472-1.0.3/` | `Microsoft.NETFramework.ReferenceAssemblies.net472`, fetched only for a module that names `references` or an executable for Windows PowerShell | 1.0.3 |
| `ps51ref-1.0.0/` | `Microsoft.PowerShell.5.1.ReferenceAssemblies`, fetched only for an executable for Windows PowerShell | 1.0.0 |
| `pester-6.2.0/` | `Pester`, fetched only by `test` and `toolchain`; both hosts import `pester-6.2.0/tools/Pester.psd1`, the same files as the module on the PowerShell Gallery | 6.2.0 |
| `pkcs-8.0.2/` | `System.Security.Cryptography.Pkcs`, fetched only when a module is signed; the signer compiles against its `lib/net8.0` build | 8.0.2 |
| `openauthenticode-0.6.3/` | `OpenAuthenticode`, from the PowerShell Gallery, fetched only for `signer = "openauthenticode"` | 0.6.3 |
| `scripts-<stamp>/` | `fetch.ps1`, `csc.ps1`, in a folder named for their text | |
| `toolchain.lock` | package id, version, SHA-512 per fetched archive, one line appended per record | |

`fetch.ps1` downloads each package from the NuGet v3 flat container, and OpenAuthenticode from the PowerShell Gallery, prints the archive's SHA-512, and extracts it; a package whose hash differs from the lock's record is refused, and a folder with a `.complete` marker is not fetched again. Builds fetch side by side and take no lock. A missing package is extracted into a folder of the fetching build's own, marked complete and renamed into place. Of two builds fetching one package, the second to rename leaves the first's folder in place and removes its own. A record is one line appended to `toolchain.lock` in one write, the first line for a package is its record, and the scripts are written under a temporary name and renamed into place. Every package version has a folder of its own and the scripts' folder is named for them, so two builds of this version on one machine share the tree.

`cargo-pwrs` 0.2.1 to 0.4.0 store their packages, scripts and `toolchain.lock` directly in the toolset's folder, fetch into a package's own folder (0.4.0 with `fetch.guard` open there), and rewrite their `toolchain.lock` whole. `cargo-pwrs` 0.2.0 stores its packages in the unversioned `csc/`, `netstandard/`, `psstandard/` and `simd/` there. This version writes nothing outside `v2/` and opens no `fetch.guard`, so on a machine that has their tree it fetches the toolchain again beside that tree. It checks each package it fetches against their `toolchain.lock` as well as its own, reading it and never writing it. Two builds of 0.4.0 and two of this version fetching into one empty `PWRS_HOME` at once all succeeded in each of three runs. `csc.ps1` loads `csc.dll` inside pwsh in an isolated `AssemblyLoadContext` (so it never collides with the Roslyn pwsh ships) and invokes its entry point with a response file. `pwsh` is found on `PATH` or through `PWRS_PWSH`; `$PSHOME` is `PWRS_PSHOME` when it is set, and otherwise what `pwsh -c $PSHOME` prints. On Linux, macOS and FreeBSD, each pwsh that runs `fetch.ps1` or `csc.ps1` or prints `$PSHOME` gets `XDG_CACHE_HOME` set to a folder of its own under the temporary folder, removed when it ends. So builds running side by side never share the startup profile pwsh writes there ([PowerShell/PowerShell#26528](https://github.com/PowerShell/PowerShell/issues/26528)).

The compile options:

- `/nologo /nostdlib+ /target:library /unsafe+ /nullable:enable /langversion:latest /deterministic+ /optimize+ /warnaserror-`;
- `/debug:portable` when a `.pdb` is written, and `/debug-` otherwise. On Windows the `netstandard2.0` assemblies take `/debug:full`, a Windows `.pdb`, since Windows PowerShell reads a stack trace's file and line only from that format. The toolset's .NET Framework compiler, `tasks/net472/csc.exe`, compiles them, because the .NET one cannot write it;
- `/define` for the framework symbols the .NET SDK defines: net8.0's for `net10.0`, netstandard2.0's for `netstandard2.0`;
- `/pathmap`, giving the work folder and the module folder the names `/pwrs/work` and `/pwrs/module` in a `.pdb` and in the assembly's pointer to it;
- a `/reference` per assembly.

`net10.0` references `Microsoft.NETCore.App.Ref`'s `ref/net8.0/*.dll` and the `System.Management.Automation` 7.4.0 reference. `netstandard2.0` references the NETStandard reference assemblies, PowerShellStandard's `System.Management.Automation.dll`, the `lib/net462` builds of `System.Numerics.Vectors`, `System.Memory`, `System.Buffers` and `System.Runtime.CompilerServices.Unsafe`, and the `ref/netstandard2.0` builds of the four access-control packages. A module with hand-written C# also ships the four `lib/net462` builds in its `netstandard2.0` folder, and every module ships the access-control packages' `lib/net461` builds there, which point each of their types to mscorlib's. When a module names `references`, the `net10.0` shell adds what they resolved to in `Microsoft.WindowsDesktop.App.Ref`, each replacing the Core reference of its file name. The `netstandard2.0` shell then compiles against .NET Framework 4.7.2's set in place of the NETStandard reference assemblies: the `build/.NETFramework/v4.7.2` assemblies its `RedistList/FrameworkList.xml` lists, its `Facades/*.dll`, PowerShellStandard's `System.Management.Automation.dll`, the four `lib/net462` builds and the four `lib/net461` access-control builds, with netstandard2.0's symbols still defined. The runtime and bootstrap compiles still use their own sets.

An executable for Windows PowerShell, which `pwrs-build`'s `Toolchain::compile_exe` compiles for a program of your own and `cargo pwrs` never builds, takes the same options with `/target:exe` and net472's symbols. It references the assemblies `Toolchain::framework_exe_references()` returns: the `build/.NETFramework/v4.7.2` assemblies `FrameworkList.xml` lists, their `Facades/*.dll`, and `Microsoft.PowerShell.5.1.ReferenceAssemblies`' `lib/net461/System.Management.Automation.dll` and `Microsoft.PowerShell.ConsoleHost.dll`. Both are 3.0.0.0, the versions in the GAC, and between them define `RunspaceConfiguration` and `ConsoleShell`, which PowerShell Standard lacks. A generated source declaring `[assembly: TargetFramework(".NETFramework,Version=v4.7.2")]` is compiled in, so .NET Framework runs the executable with 4.7.2's defaults rather than 4.0's. In Windows PowerShell's console host, TLS versions are then the system's, an escaped `/` in a URL's path stays escaped and long paths open, as they do in powershell.exe. Sources that declare a `TargetFramework` of their own fail with CS0579.

## `[package.metadata.pwrs]` keys

The keys that change what `build` does. The keys that only fill in the module manifest are listed in [How To Publish](../how-to/How-To-Publish.md#the-manifest).

| Key | Takes | Does |
|---|---|---|
| `helpers` | `[[bin]]` target names | builds each with the library and ships it beside the library in `runtimes/<rid>/native/`; a name that is not a `[[bin]]` target stops the build. See [How To Ship A Helper Executable](../how-to/How-To-Ship-A-Helper-Executable.md) |
| `macos-bundles` | a table from a helper's name to a table with `name`, `identifier`, and optionally `display-name` and `icon` | on macOS, ships that helper as the app bundle `<name>.app` in `runtimes/<rid>/native/`: the executable under `Contents/MacOS`, the `.icns` file `icon` names (from the crate directory) under `Contents/Resources`, and an `Info.plist` with the name, the display name (the name when none is given), the identifier, the package's version and high resolution; other platforms ship the executable. An entry for a target `helpers` does not list, a key it does not accept, a name that is not a valid folder name on Windows, Linux and macOS alike, an identifier that is not parts of letters, digits and hyphens joined by periods, or an icon that is not an existing `.icns` file stops the build on every platform. See [How To Ship A Helper Executable](../how-to/How-To-Ship-A-Helper-Executable.md#ship-it-as-an-app-bundle-on-macos) |
| `bundled-modules` | crate directories, or tables with `path` | builds each crate's module with this tool and copies it into this one; see step 6 above and [How To Bundle A Module](../how-to/How-To-Bundle-A-Module.md) |
| `references` | assembly names | adds them to the shell's compiles, looked up separately for each compile, and compiles the `netstandard2.0` shell against .NET Framework 4.7.2's set, so the module needs .NET Framework 4.7.2 under Windows PowerShell; a name in neither compile's set stops the build. See [How To Add Hybrid C#](../how-to/How-To-Add-Hybrid-CSharp.md#windows-forms-wpf-and-other-desktop-assemblies) |
| `cpu-features` | extension names | extensions the library requires on purpose, which `build` does not warn about and `publish` does not refuse |
| `test-cpu-tiers` | `PWRS_CPU_MAX` levels | the levels `test` runs the suites under again when `--cpu-tiers` names none |
| `signing` | a table of one key (`pfx`, `pem`, `store`, `command` or `pkcs11`) and `password-env`, `certificate`, `timestamp-url`, `hash`, `catalog` and `signer` | what [`sign`](#sign) signs with, and `publish` after its build; see [How To Sign A Module](../how-to/How-To-Sign-A-Module.md#the-settings) |
| `allowed-licenses` | SPDX license names | licenses the library may link crates under beyond the defaults; see [How To Publish](../how-to/How-To-Publish.md#license-notices) |
| `license-files` | a table of `"<crate>@<version>" = "<path>"` or `{ add = ["<path>", ...] }`, and `self = { add = ["<path>", ...] }`, and beside them tables of the same under `cfg(...)` keys | license files the module supplies for crates whose packages ship none, with `add` the notices it adds under a crate for code the crate links that is not its own, and with `self` the notices of code the module's own crate compiles that is not its own, those of a `cfg(...)` table only for a target its expression matches; see [How To Publish](../how-to/How-To-Publish.md#license-notices) |
| `native-files` | a table from a file's name to a table with `url`, `sha256`, and optionally `path`, `licenses` and `license-files`, and beside them tables of the same under `cfg(...)` keys | ships the file in `runtimes/<rid>/native/` beside the library, from a `cfg(...)` table only for a target its expression matches: downloaded with the system's curl once into `<CARGO_HOME>/pwrs/downloads/<sha256>/`, refused when its SHA-256 differs from `sha256`, and taken from the zip or gzip-compressed tar archive at `path`, or used as downloaded when there is no `path`. `licenses` names license files inside the archive and `license-files` files in the crate directory, and the runtime's notices quote them; an entry with none, a name Windows does not allow for a file or one the library or a helper already uses, and two entries naming one file for a target stop the build. The module loads the file from the copy `pwrs::native_file_path` stages. See [How To Ship A Prebuilt Native File](../how-to/How-To-Ship-A-Prebuilt-Native-File.md) |

## The generated files

- **`<Module>.psd1`**:
  - `RootModule`: the `.psm1`;
  - `ModuleVersion`: from `Cargo.toml`;
  - `GUID`: derived from the module name, as two FNV-1a hashes, over the name and over it reversed;
  - `Author`: the first author;
  - `Description`: `[package.metadata.pwrs] description` when set, otherwise the package description;
  - `CompatiblePSEditions`: Desktop and Core;
  - `PowerShellVersion`: 5.1;
  - `FormatsToProcess`: the format file, when one exists;
  - `CmdletsToExport`: the Rust cmdlets, then the hybrid C# ones;
  - `AliasesToExport`: their aliases, in the same order;
  - `Tags`: `pwrs` followed by `[package.metadata.pwrs] tags` when set, otherwise the package keywords, with whitespace in a tag turned into a hyphen, since a gallery tag cannot contain whitespace;
  - `ProjectUri`: the package repository, when it has one.
- **`<Module>.psm1`**: described in [Two Hosts](Two-Hosts.md). Its `Export-ModuleMember` names the cmdlets and the aliases, since PowerShell creates the aliases a cmdlet's `[Alias]` attribute declares inside the nested binary module, and only what the root module exports reaches the session.
- **`<Module>.Shell.cs`**: one namespace `Pwrs.Modules.<Module>` containing the module class, the cmdlets, the completers and the providers; class and enum types in the namespaces their names declare.
- **Help**: MAML with the synopsis, description, syntax, parameters (type, position, pipeline input, aliases), input types from pipeline parameters, return values from `output`, and examples.
- **Format file**: for each copied, proxy or psobject class, the view `<Type>`, a table of its fields when it has five or fewer, else a list. Ahead of it comes the view `<Type>.Columns`, a table of the columns a class names with `columns`, and first the view `<Type>.<Method>`, a custom view writing what a class's `view` method returns. The first view a class has is its default.
