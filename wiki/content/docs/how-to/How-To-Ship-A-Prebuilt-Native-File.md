---
title: How To Ship A Prebuilt Native File
weight: 14
---

A module sometimes needs a file it does not build: a library a dependency loads at run time, or a tool it runs, taken from the vendor's own release. `[package.metadata.pwrs.native-files]` names the download that contains the file and the download's SHA-256. `cargo pwrs build` puts the file in `runtimes/<rid>/native/` beside the module's library, with its license texts in the notices of `runtimes/<rid>/`, and the module reaches it through `pwrs::native_file_path`. Source: `crates/cargo-pwrs/src/native_files.rs` and the step in `build` that calls `native_files::ship`; `pwrs::native_file_path` in `crates/pwrs/src/helper.rs`; and `StageFile` in `crates/cargo-pwrs/dotnet/Pwrs.Runtime/NativeModule.cs`.

## Name the file

The worked example is a copy of the hello example with this entry added, which ships ripgrep's Windows x64 build from ripgrep's own release; hello itself ships no such file:

```toml
[package.metadata.pwrs.native-files.'cfg(all(windows, target_arch = "x86_64"))']
"rg.exe" = { url = "https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-pc-windows-msvc.zip", sha256 = "71b2fef860abe467217a538ff31de02f5258807c0129f771846f87bd029aafc5", path = "ripgrep-15.2.0-x86_64-pc-windows-msvc/rg.exe", licenses = ["ripgrep-15.2.0-x86_64-pc-windows-msvc/COPYING", "ripgrep-15.2.0-x86_64-pc-windows-msvc/LICENSE-MIT", "ripgrep-15.2.0-x86_64-pc-windows-msvc/UNLICENSE"] }
```

The key is the file's name in `runtimes/<rid>/native/`. An entry takes:

| Key | Takes |
|---|---|
| `url` | the download's address, anything curl fetches |
| `sha256` | the download's SHA-256, 64 hexadecimal digits |
| `path` | the file's path inside the download, a zip or a gzip-compressed tar archive; without it the download is the file itself |
| `licenses` | license files inside the archive |
| `license-files` | license files in the crate directory, for a download that is the file itself or an archive that has none |

Entries under a `cfg(...)` key apply only to a target whose cfg satisfies it, as `rustc --print cfg --target <triple>` prints it, the way `license-files` scopes its own tables; entries directly under `[package.metadata.pwrs.native-files]` apply to every target. An entry that names no license file, a name Windows cannot use for a file, a name the library or a helper already uses, and two entries naming one file for one target each stop the build, named.

## What the build ships

The first build downloads the archive with the system's curl into `<CARGO_HOME>/pwrs/downloads/<sha256>/`, checks it against the pin, and takes the file and its licenses from it:

```text
pwrs: downloading https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-pc-windows-msvc.zip for rg.exe
```

A download whose SHA-256 differs is refused, naming both hashes, and nothing of it is saved. Later builds take the saved download and print nothing. A release build of the copy on Windows x64:

```text
runtimes/win-x64/native/hello-helper.exe                 150528 bytes
runtimes/win-x64/native/pwrs_example_hello.dll          2080256 bytes
runtimes/win-x64/native/rg.exe                          4218880 bytes
runtimes/win-x64/pwrs-build.json                            152 bytes
runtimes/win-x64/THIRD-PARTY-NOTICES.txt                   6687 bytes
```

The runtime's notices quote the license files after the crates the library compiles in:

```text
Files shipped beside pwrs_example_hello.dll
Each was downloaded from the address given, checked against the SHA-256
given, and is quoted below with its license files.

================================================================================
rg.exe
From: https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-pc-windows-msvc.zip
SHA-256: 71b2fef860abe467217a538ff31de02f5258807c0129f771846f87bd029aafc5
Taken from: ripgrep-15.2.0-x86_64-pc-windows-msvc/rg.exe
--------------------------------------------------------------------------------
ripgrep-15.2.0-x86_64-pc-windows-msvc/COPYING
--------------------------------------------------------------------------------
This project is dual-licensed under the Unlicense and MIT licenses.

You may use this code under the terms of either license.
```

`cargo pwrs merge` copies each `runtimes/<rid>` folder whole, and `publish` packages the module folder as built, so both include the file and its notices.

## Load it from Rust

```rust
let rg = pwrs::native_file_path("rg.exe")?;
let out = std::process::Command::new(&rg).arg("--version").output()?;
```

`pwrs::native_file_path` takes the file's own name, extension included, and returns the path of a copy staged for the process by the rules a helper is staged by, at `<temp>/pwrs-load/<pid>/<key>/native/<file>.<mark>/<file>`. PWRS loads the module's library from a copy of its own there, so the library's folder at run time does not have the file. Load or start the file from the path this returns and never from the module folder: a loaded library goes on using its file, and the next build clears that folder. `Get-HelloNativeFile` in the hello example returns the path; in the copy, `Get-HelloNativeFile rg.exe` returned

```text
<temp>/pwrs-load/<pid>/<key>/native/rg.exe.8df33f0e01a4793-406000/rg.exe
```

and that copy printed `ripgrep 15.2.0 (rev e89fff89ac)`. A name the module ships no file of is an error naming the file and the folder searched. `examples/hello/tests/Helper.Tests.ps1` checks the staging with the library's own file, which every build ships, and checks the error.

The cache under `<CARGO_HOME>/pwrs/downloads` has one folder per SHA-256, and nothing removes it; delete a folder to fetch that download again.
