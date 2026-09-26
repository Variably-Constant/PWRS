---
title: How To Publish
weight: 9
---

Packaging and publishing a built module with PSResourceGet. Source: `crates/cargo-pwrs/src/main.rs` (`publish`) and `crates/cargo-pwrs/scripts/publish.ps1`.

## What the crate must declare

`publish` refuses a crate whose `Cargo.toml` sets no `authors` or no `description`, before it builds anything for the gallery. Those two become the module's `Author` and `Description`, and a published module carries its own, not a placeholder from the framework. `build` reports both on standard error and carries a placeholder so a module under development still imports.

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

Builds the module, runs `Test-ModuleManifest` on it, and packages it with `Compress-PSResource` into `target/pwrs/publish/<Module>/`. The output names the `.nupkg`.

## Publishing

```text
$env:PWRS_PSGALLERY_KEY = '<your API key>'
cargo pwrs publish --release
```

Runs the same validation, then `Publish-PSResource -Path <module> -Repository PSGallery -ApiKey $env:PWRS_PSGALLERY_KEY`. The script requires PowerShell 7 and the `Microsoft.PowerShell.PSResourceGet` module that pwsh 7.4 and later ship. Without the key it fails before contacting the gallery.

## What ships

The module folder as built: manifest, bootstrap script, format file, both framework folders, and `runtimes/<rid>/native/` for the platform that built it, holding the library and the helper executables `[package.metadata.pwrs] helpers` names (see [How To Ship A Helper Executable](How-To-Ship-A-Helper-Executable.md)), and a `<Name>/` folder for each module `bundled-modules` names, which the package carries with no dependency declared for it (see [How To Bundle A Module](How-To-Bundle-A-Module.md)). A release build writes no `.pdb` files; pass `--debug-symbols` to keep them, which roughly doubles the managed half of the folder. A module for several platforms is built on each of them (or with `--target` and a cross linker) and the folders joined with `cargo pwrs merge <into> <from>...` before publishing; see [cargo pwrs Reference](Cargo-Pwrs-Reference.md).

## License notices

A native library carries every crate it links, so a published module redistributes each of them in binary form, and most licenses ask for their text to go along. `cargo pwrs build` works out which crates the library links for its target: everything its normal dependencies reach, leaving out proc macros, what only they depend on, and build dependencies, none of which end up in the library. Beside the library it writes `runtimes/<rid>/THIRD-PARTY-NOTICES.txt`, listing each crate with the license it declares and, word for word, every license, copying, notice and copyright file at the top of its source. `cargo pwrs merge` copies each `runtimes/<rid>` folder whole, so a folder joined from several platforms keeps each library's own list. The list covers the module's helper executables too: a helper is one of the package's `[[bin]]` targets and links only crates among the package's own dependencies.

The module folder's own `THIRD-PARTY-NOTICES.txt` covers what cargo-pwrs puts there: `Pwrs.Bootstrap.dll`, `Pwrs.Runtime.dll` and the shell, under PWRS's license, and, for a module with hand-written C#, the four .NET Framework assemblies it ships in `netstandard2.0`, each with the copyright line its NuGet package declares and the MIT terms the package names.

A crate whose license the module does not allow stops the build, named with its license expression. An expression is allowed when some choice among its `OR` alternatives uses only allowed licenses; a crate that declares no SPDX expression is refused. The allowed licenses are permissive ones whose terms the notices meet: `0BSD`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `BSL-1.0`, `CC0-1.0`, `CDLA-Permissive-2.0`, `ISC`, `MIT`, `MIT-0`, `Unicode-3.0`, `Unicode-DFS-2016`, `Unlicense` and `Zlib`. A module that accepts another license, and whatever its terms ask beyond the notices, lists it:

```toml
[package.metadata.pwrs]
allowed-licenses = ["MPL-2.0"]
```

A crate whose source carries no license file is listed with its license and no text. `build` names it on standard error, and `publish` refuses the module, because the notices could name that license but not quote it.

The module can supply that file itself, pinned to the crate's version, with a path in its own repository, typically the license text from the crate's upstream repository:

```toml
[package.metadata.pwrs.license-files]
"alloc-stdlib@0.3.0" = "licenses/alloc-stdlib-LICENSE.txt"
```

The notices then quote the file under that crate, saying the module supplies it, and `publish` accepts the module. The build refuses an entry that names a crate the library does not link, a version it does not link, a crate whose package ships its own license file, or a file that is missing or empty; the crate's license expression still has to be an allowed one. A new version of the crate therefore needs its entry again, and its terms another look.

A bundled module's library is checked the same way, and `publish` refuses the package over its crates as over the module's own. The bundling module can supply their files in that module's `bundled-modules` entry, whose `license-files` takes the same table; see [How To Bundle A Module](How-To-Bundle-A-Module.md).

## The manifest

`cargo pwrs build` writes the manifest from the crate's metadata: `ModuleVersion` from `Cargo.toml`, `Author` from the first of `authors`, `Description` from the package description, `ProjectUri` from its repository, `Tags` of `pwrs` plus the package keywords, a `GUID` derived deterministically from the module name so a rebuilt manifest keeps its identity, and the cmdlets and their aliases to export. Set `description`, `repository` and `keywords` in `Cargo.toml` and the listing follows the crate.

Every other property a module manifest can carry comes from `[package.metadata.pwrs]`, under the manifest key's own name in kebab case, because a cargo manifest has no field for any of them. A property left unset is written nowhere, so the manifest never carries a key it has no value for, which a gallery would read as deliberately empty.

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

Two have defaults rather than nothing, because the two generated shells decide them: `CompatiblePSEditions` names `Desktop` and `Core`, and `PowerShellVersion` says `5.1`. Set either to narrow a module to one host. `DotNetFrameworkVersion` and `ClrVersion` are read by Windows PowerShell alone; PowerShell Core ignores both. `default-command-prefix` is inserted into every exported name at import, so a caller can load a module beside another that uses the same nouns.

`FormatsToProcess` is not set by hand: `cargo pwrs build` writes it when the module declares output classes and generates the `.ps1xml` beside it.
