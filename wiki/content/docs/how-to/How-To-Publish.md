---
title: How To Publish
weight: 9
---

Packaging and publishing a built module with PSResourceGet. Source: `crates/cargo-pwrs/src/main.rs` (`publish` and `publish_folder`), `crates/cargo-pwrs/src/build_record.rs` and `crates/cargo-pwrs/scripts/publish.ps1`.

## What the crate must declare

`publish` refuses a crate whose `Cargo.toml` sets no `authors` or no `description`, before it builds anything for the gallery. Those two become the module's `Author` and `Description`, and a published module has its own, not a placeholder from the framework. A `description` under `[package.metadata.pwrs]` counts too, as [The manifest](#the-manifest) shows. `build` reports both on standard error and writes a placeholder so a module under development still imports.

```toml
[package]
authors = ["Your Name <you@example.com>"]
description = "What the module does, in one sentence."
repository = "https://github.com/you/your-module"   # becomes ProjectUri
keywords = ["thing", "automation"]                   # become gallery tags
```

## Dry run

```text
cargo pwrs publish --release --dry-run
```

Builds the module, runs `Test-ModuleManifest` on it, and packages it with `Compress-PSResource` into `target/pwrs/publish/<Module>/`, after removing the packages an earlier run left there. The output names the `.nupkg` it made.

`Compress-PSResource` is in the `Microsoft.PowerShell.PSResourceGet` 1.1.1 that Microsoft's pwsh 7.4.20 and 7.5.11 ship and the 1.2.0 in pwsh 7.6. FreeBSD's pwsh 7.5.5 package has 1.0.3, which lacks it, so there the dry run stops with a message naming `Install-PSResource -Name Microsoft.PowerShell.PSResourceGet -Scope CurrentUser -TrustRepository`, which installed 1.2.0 for the user.

## What the gallery analyzes

The PowerShell Gallery runs PSScriptAnalyzer over every package it takes, with the rules of its `PSGallery` settings. The manifest and script `cargo pwrs build` writes pass them with no findings. A script the module ships itself, such as one `scripts-to-process` or `nested-modules` names, is the module's own to check. PSScriptAnalyzer installs with `Install-PSResource PSScriptAnalyzer`, and this runs the gallery's rules over every script and manifest in the built folder:

```powershell
Get-ChildItem target/pwrs/<Module> -Recurse -Include *.ps1, *.psm1, *.psd1 |
    ForEach-Object { Invoke-ScriptAnalyzer -Path $_.FullName -Settings PSGallery }
```

## Publishing

```text
$env:PWRS_PSGALLERY_KEY = '<your API key>'
cargo pwrs publish --release
```

Runs the same validation, then `Publish-PSResource -Path <module> -Repository PSGallery -ApiKey $env:PWRS_PSGALLERY_KEY`. The script requires PowerShell 7 and the `Microsoft.PowerShell.PSResourceGet` module that pwsh 7.4 and later ship. Without the key it fails before contacting the gallery.

## A module built on several platforms

`publish` builds the module for the platform it runs on. So a module that ships for several is built on each of them (or with `--target` and a cross linker), the folders are joined with `cargo pwrs merge`, and the joined folder is published as it stands with `--folder`:

```text
cargo pwrs build --release                                # on each platform
cargo pwrs merge target/pwrs/<Module> <module folder>...  # on one, with the other platforms' folders
cargo pwrs publish --release --folder target/pwrs/<Module> --dry-run
cargo pwrs publish --release --folder target/pwrs/<Module>
```

With `--folder` no build runs. Each build leaves `runtimes/<rid>/pwrs-build.json` beside its library, recording what it found that `publish` refuses: a placeholder for `authors` or `description`, extensions its library requires that `cpu-features` does not declare, and crates whose license text the notices cannot quote. `merge` copies each runtime folder whole, its record included, and `publish --folder` refuses what any runtime's build found, naming the runtime, every refusal at once. It refuses a runtime whose build left no record, and with `--release` one that is not a release build. A bundled module's runtimes are checked as the module's own are. An option that changes a build, such as `--target` or `--features`, is refused with `--folder`, since the folder is already built. The package goes to `publish/<Module>/` beside the folder, as a single build's does.

## Signing

`publish` signs the module after its build when the crate has `[package.metadata.pwrs.signing]`, and takes the signing flags, so the files it packages are signed. `publish --folder` signs nothing and refuses a signing flag, so a merged folder is signed with `cargo pwrs sign --folder` first:

```text
cargo pwrs sign --folder target/pwrs/<Module>
cargo pwrs publish --release --folder target/pwrs/<Module>
```

[How To Sign A Module](How-To-Sign-A-Module.md) covers the keys, the settings and checking the result.

## What ships

The module folder as built:

- the manifest, the bootstrap script, the format file and both framework folders;
- `runtimes/<rid>/` for the platform that built it, with the build's record and, in `native/`, the library and the helper executables `[package.metadata.pwrs] helpers` names (see [How To Ship A Helper Executable](How-To-Ship-A-Helper-Executable.md));
- a `<Name>/` folder for each module `bundled-modules` names, which the package includes with no dependency declared for it (see [How To Bundle A Module](How-To-Bundle-A-Module.md)).

A release build writes no `.pdb` files; pass `--debug-symbols` to write them, which roughly doubles the managed half of the folder. For a Windows target it also puts the library's `.pdb` and each helper's in `native/`. hello's release build gains 1,380,352 bytes there beside its 1,683,456-byte library, and 921,600 for its helper. With the default release profile, that `.pdb` has no path of the crate's own source files. A folder joined from several platforms' builds has a `runtimes/<rid>/` for each, as the section above shows; see [cargo pwrs Reference](Cargo-Pwrs-Reference.md).

## License notices

A native library contains every crate it links, so a published module redistributes each of them in binary form, and most licenses require their text to go with it. `cargo pwrs build` works out which crates the library links for its target: everything its normal dependencies reach, leaving out proc macros, what only they depend on, and build dependencies, none of which end up in the library. Beside the library it writes `runtimes/<rid>/THIRD-PARTY-NOTICES.txt`, listing each crate with the license it declares and, word for word, every license, copying, notice and copyright file at the top of its source. `cargo pwrs merge` copies each `runtimes/<rid>` folder whole, so a folder joined from several platforms has each library's own list. The list covers the module's helper executables too: a helper is one of the package's `[[bin]]` targets and links only crates among the package's own dependencies.

The module folder's own `THIRD-PARTY-NOTICES.txt` covers what cargo-pwrs puts there: `Pwrs.Bootstrap.dll`, `Pwrs.Runtime.dll` and the shell, under PWRS's license. It also covers the .NET Framework assemblies the module ships in `netstandard2.0`, each from a NuGet package under MIT. Every module ships four access-control assemblies, from 4.5.0 packages whose own `LICENSE.TXT` the notices quote word for word. A module with hand-written C# ships four more, each with the copyright line its package declares and the MIT terms.

A crate whose license the module does not allow stops the build, named with its license expression. An expression is allowed when some choice among its `OR` alternatives uses only allowed licenses; a crate that declares no SPDX expression is refused. The allowed licenses are permissive ones whose terms the notices meet: `0BSD`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `BSL-1.0`, `CC0-1.0`, `CDLA-Permissive-2.0`, `ISC`, `MIT`, `MIT-0`, `Unicode-3.0`, `Unicode-DFS-2016`, `Unlicense` and `Zlib`. A module that accepts another license, and whatever its terms require beyond the notices, lists it:

```toml
[package.metadata.pwrs]
allowed-licenses = ["MPL-2.0"]
```

A crate whose source has no license file is listed with its license and no text. `build` names it on standard error, and `publish` refuses the module, because the notices could name that license but not quote it.

The module can supply that file itself, pinned to the crate's version, with a path in its own repository, typically the license text from the crate's upstream repository:

```toml
[package.metadata.pwrs.license-files]
"alloc-stdlib@0.3.0" = "licenses/alloc-stdlib-LICENSE.txt"
```

The notices then quote the file under that crate, saying the module supplies it, and `publish` accepts the module. The build refuses an entry that names a crate the library does not link, a version it does not link, a crate whose package ships its own license file, or a file that is missing or empty. The crate's license expression still has to be an allowed one. A new version of the crate therefore needs a new entry, and its license terms need checking again.

A crate the library links on some targets and not others takes its entry in a table under a `cfg(...)` key, as cargo scopes `[target.'cfg(...)'.dependencies]`:

```toml
[package.metadata.pwrs.license-files.'cfg(windows)']
"clipboard-win@5.4.1" = "licenses/clipboard-win-5.4.1.txt"

[package.metadata.pwrs.license-files.'cfg(any(target_os = "linux", target_os = "freebsd"))']
"accesskit_unix@0.21.1" = "licenses/accesskit_unix-0.21.1.txt"
```

A build takes the unscoped entries and those of each table whose expression the target satisfies, read and matched as cargo reads and matches one, against what `rustc --print cfg --target <triple>` prints for the `--target` triple or the building machine's own. It skips the other tables. An entry in a table the build takes is refused as an unscoped one is, so the build for a target the table covers catches a stale entry. A table under any other key, an expression cargo would not parse, and one naming `test`, `debug_assertions`, `proc_macro` or `feature`, which describe the compilation rather than the target, stop the build on every target.

A crate can link code that is not its own, such as a C or C++ library its build script compiles or downloads, whose license and notices are not in the crate's own license file. The module adds that code's notices under the crate with a table whose `add` lists their files. A module named Shaders, for one, links DXC, Microsoft's shader compiler, on 64-bit Windows through `mach-dxcompiler-rs`, whose package ships only its own MIT license, so its manifest adds DXC's notices, in a `cfg(...)` table for the targets that link that crate:

```toml
[package.metadata.pwrs.license-files.'cfg(all(windows, not(target_arch = "aarch64"), target_env = "msvc"))']
"mach-dxcompiler-rs@0.1.6+2026.09.16-48d5a66.1" = { add = [
    "licenses/dxcompiler-2026.09.16-48d5a66/LICENSE",
    "licenses/dxcompiler-2026.09.16-48d5a66/LICENSE-APACHE",
    "licenses/dxcompiler-2026.09.16-48d5a66/LICENSE-MIT",
    "licenses/dxcompiler-2026.09.16-48d5a66/DirectXShaderCompiler-LICENSE.TXT",
    "licenses/dxcompiler-2026.09.16-48d5a66/DirectXShaderCompiler-ThirdPartyNotices.txt",
] }
```

The notices quote the crate's own license files first, as for any crate:

```text
mach-dxcompiler-rs 0.1.6+2026.09.16-48d5a66.1
License: MIT
Repository: https://github.com/DouglasDwyer/mach-dxcompiler-rs
--------------------------------------------------------------------------------
LICENSE
--------------------------------------------------------------------------------
Copyright (c) 2021 Hexops Contributors (given via the Git commit history).
```

After their terms, a line says the crate links code that is not its own, and each listed file follows, in the list's order:

```text
The crate links code that is not its own; the Shaders module adds that code's notices.
--------------------------------------------------------------------------------
licenses/dxcompiler-2026.09.16-48d5a66/LICENSE
--------------------------------------------------------------------------------
Copyright 2021, Hexops Contributors (given via the Git commit history).
```

An `add` list is taken whether or not the crate ships a license file, and beside a path entry for the same crate in another table. The build refuses one that names a crate or version the library does not link, lists no file or one file twice, names a file that is missing or empty, or adds to a crate another `add` list already adds to. The license the build checks against the allowed ones is still the crate's own expression; the module is responsible for meeting the terms of the code a list adds.

Code the module's own crate compiles, such as a C or C++ library its build script builds from sources vendored in its repository, belongs to no crate the library links, so no `name@version` entry can list its notices. A `self` entry does, with an `add` list of their files. For a module named Textures whose build script compiles a vendored copy of Basis Universal and the zstd inside it, the manifest adds theirs this way:

```toml
[package.metadata.pwrs.license-files]
self = { add = [
    "vendor/basis_universal/LICENSE",
    "vendor/basis_universal/NOTICE",
    "vendor/basis_universal/zstd/LICENSE",
] }
```

The notices quote them last, after every crate, under a section for the module's own crate, in the list's order:

```text
================================================================================
The Textures module's own crate
Its build compiles code that is not its own; the Textures module adds that code's notices.
--------------------------------------------------------------------------------
vendor/basis_universal/LICENSE
--------------------------------------------------------------------------------
```

`self` takes only a table with `add`, in the unscoped table or in a `cfg(...)` table for code the build script compiles on some targets alone. The build refuses a `self` list for the same reasons as an `add` list: it lists no file or one file twice, names a file that is missing or empty, or is a second `self` list for the same target. Since it names no version, a new version of the module needs no change to it.

A bundled module's library is checked the same way, and `publish` refuses the package over its crates as over the module's own. The bundling module can supply their files in that module's `bundled-modules` entry, whose `license-files` takes the same table, `cfg(...)` tables included, and where `self` is the bundled module's own crate; see [How To Bundle A Module](How-To-Bundle-A-Module.md).

## The manifest

`cargo pwrs build` writes the manifest from the crate's metadata:

- `ModuleVersion` from `Cargo.toml`;
- `Author` from the first of `authors`;
- `Description` from `[package.metadata.pwrs] description` when set, otherwise the package description;
- `ProjectUri` from its repository;
- `Tags`: `pwrs`, then `[package.metadata.pwrs] tags` when set, otherwise the package keywords;
- a `GUID` derived from the module name, so a rebuilt manifest has the same identity;
- the cmdlets and their aliases to export.

Set `description`, `repository` and `keywords` in `Cargo.toml` and the listing follows the crate. A crate on crates.io as well can give the gallery other text: crates.io shows the package's `description` and at most five `keywords`, and the manifest takes `description` and `tags` from `[package.metadata.pwrs]` in their place:

```toml
[package]
description = "Greetings from PowerShell."
keywords = ["powershell", "greeting"]

[package.metadata.pwrs]
description = "Greets people by name in PowerShell 7 and Windows PowerShell, from a Rust library."
tags = ["greeting", "text", "Windows", "Linux", "MacOS"]
```

`description` there is text and `tags` a list of text; a value of another shape stops the build, named. An empty `tags` list gives the module no tags but `pwrs`.

Every other property a module manifest can have comes from `[package.metadata.pwrs]`, under the manifest key's own name in kebab case, because a cargo manifest has no field for any of them. A property left unset is written nowhere, so the manifest never has a key with no value.

```toml
[package.metadata.pwrs]
license-uri = "https://example.invalid/greeter/blob/main/license"
release-notes = "First release."
icon-uri = "https://example.invalid/greeter/raw/main/icon.png"
company = "Your Company"
copyright = "(c) Your Name"
prerelease = "beta1"
require-license-acceptance = true
external-module-dependencies = ["Pester"]
powershell-version = "7.2"
compatible-ps-editions = ["Core"]
powershell-host-name = "ConsoleHost"
powershell-host-version = "5.1"
dotnet-framework-version = "4.7.2"
clr-version = "4.0"
processor-architecture = "Amd64"
help-info-uri = "https://example.invalid/greeter/help"
default-command-prefix = "Gr"
required-modules = ["Storage"]
required-assemblies = ["System.Xml.dll"]
scripts-to-process = ["init.ps1"]
types-to-process = ["Greeter.Types.ps1xml"]
nested-modules = ["Extra.psm1"]
dsc-resources-to-export = ["GreeterResource"]
module-list = ["Greeter"]
file-list = ["readme.md"]
```

Two have defaults instead of nothing, because the two generated shells set them: `CompatiblePSEditions` names `Desktop` and `Core`, and `PowerShellVersion` says `5.1`. Set either to narrow a module to one host. `DotNetFrameworkVersion` and `ClrVersion` are read by Windows PowerShell alone; PowerShell Core ignores both. `default-command-prefix` is inserted into every exported name at import, so a caller can load a module beside another that uses the same nouns.

`FormatsToProcess` is not set by hand: `cargo pwrs build` writes it when the module declares output classes and generates the `.ps1xml` beside it.
