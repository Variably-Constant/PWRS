---
title: How To Ship A Helper Executable
weight: 48
---

A module sometimes needs a process of its own: a sandbox per guest, a worker the session must outlive, a tool that owns a device. PWRS ships one as a helper executable, a `[[bin]]` target of the module's crate that `cargo pwrs build` builds with the library and puts beside it, and the module starts it from Rust. Source: `crates/cargo-pwrs/src/build.rs` (`check_helpers` and the copy in `build`), `crates/pwrs/src/helper.rs`, and `StageHelper` in `crates/cargo-pwrs/dotnet/Pwrs.Runtime/NativeModule.cs`. The worked example is `examples/hello`: `src/bin/hello-helper.rs`, the `Invoke-HelloHelper`, `Start-HelloHelper` and `Stop-HelloHelper` cmdlets, and `tests/Helper.Tests.ps1`.

## Declare the helper

```toml
[[bin]]
name = "hello-helper"
path = "src/bin/hello-helper.rs"

[package.metadata.pwrs]
helpers = ["hello-helper"]
```

Only the targets `helpers` names ship; the package's other binaries stay out of the module. A name that is not one of the package's `[[bin]]` targets stops the build before anything is compiled:

```text
[package.metadata.pwrs] helpers names missing, which is not a [[bin]] target of demo
```

## What the build ships

`cargo pwrs build` runs `cargo build -p <package>`, which builds the package's binaries with its library, for the same `--target` and profile, and copies each named helper beside the library: `<name>.exe` on Windows, `<name>` elsewhere. The hello example's release build on Windows x64:

```text
runtimes/win-x64/THIRD-PARTY-NOTICES.txt               3025 bytes
runtimes/win-x64/native/hello-helper.exe             134656 bytes
runtimes/win-x64/native/pwrs_example_hello.dll      1086976 bytes
```

`publish` ships the module folder as built, so it carries the helpers. A module made with `cargo pwrs new` and given a helper this way packaged, through the two steps `publish --dry-run` takes (`Test-ModuleManifest`, then `Compress-PSResource`), to a `.nupkg` holding `runtimes/win-x64/native/demo-helper.exe` beside the module's library. The notices beside the library cover a helper's crates as well, because a `[[bin]]` target links only crates among its package's own dependencies; see [How To Publish](How-To-Publish.md#license-notices).

## One folder for every platform

`cargo pwrs merge` copies each `runtimes/<rid>` folder whole, so the helper a Linux build ships lands beside the Windows one; the [cargo pwrs reference](../reference/Cargo-Pwrs-Reference.md#merge) lists hello's folder after its Linux x64 build is merged into its Windows x64 one. A `.nupkg` records no Unix mode for its entries, so a helper packaged on Windows installs on Linux without its execute permission. Packaged with `Compress-PSResource` on Windows and saved with `Save-PSResource` on Ubuntu 24.04, hello's helper is:

```text
-rw-rw-r--  runtimes/linux-x64/native/hello-helper
```

The copy `pwrs::helper_path` answers is readable and executable by its owner whatever mode the shipped file has, and nothing in the module folder is changed to make it so. Installed that way, hello starts its helper, and its suite passes 282 of 282 from the installed folder in pwsh 7.6.5.

## Start it from Rust

```rust
let helper = pwrs::helper_path("hello-helper")?;
let out = std::process::Command::new(&helper).args(&self.argument_list).output()?;
```

`pwrs::helper_path` takes the target's name without `.exe` and answers the path to start. It can be called from any thread, since it reaches no runspace. Start the helper from that path and never from the module folder, for the reason below.

A helper that runs past one call keeps its `std::process::Child` and is waited for. `Start-HelloHelper` stores each child it spawns, and `Stop-HelloHelper` closes the child's input and calls `wait_with_output`. On Linux and FreeBSD an exited child that nobody waits for stays in the process table until the session ends.

## Why the helper runs from a copy

Measured on Windows 11 (build 26100): while an executable runs, its file can be renamed and a new file written under its old name. Deleting it, overwriting it in place, deleting the renamed file and removing its folder are all refused until it exits. `cargo pwrs build` clears the module folder before writing it, so a helper started straight from the module folder stops the next build:

```text
cargo-pwrs: cannot clear C:\Temp\pwrs-deps-target\pwrs\Hello: Access is denied. (os error 5)
```

Started through `pwrs::helper_path`, the same helper runs from the process's staging folder instead. With one running under each host, pwsh 7.6.6 and Windows PowerShell 5.1, `cargo pwrs build --release` of the hello module succeeded, and each helper then ran to completion when released.

## Where the copy lives

The copy goes into the folder the process stages the module's library in, one folder per copy:

```text
<temp>/pwrs-load/<pid>/<key>/native/<file>.<mark>/<file>
C:\Users\<user>\AppData\Local\Temp\pwrs-load\4024\385520155\native\hello-helper.exe.8df1a185c68349a-20e00\hello-helper.exe
```

`<mark>` is the shipped file's write time and length in hex (`0x20e00` is the 134656 bytes above). A rebuilt helper therefore stages beside a running one rather than over it, and the file keeps its own name, so a process listing shows the name its author gave it. The first request for a helper's bytes copies them under a temporary name and moves the copy into place, so no caller is answered a file another thread is still writing. Every later request answers the same path. The staging folder goes with the session's others: the next session to start deletes the staging folders of sessions that have ended, and leaves one that something still holds for the session after.

## Reference

| Call | Returns | Fails |
|---|---|---|
| `pwrs::helper_path(name: &str)` | `PsResult<std::path::PathBuf>`: the staged copy's path | a `PsError` with id `PwrsRuntimeError` whose message is the runtime's exception. For a helper the module does not ship, it names the file, the runtime identifier and the folder searched; for a name with a folder or a separator in it, it says the name is not a bare file name |

The hello cmdlets built on it, with output from Windows x64:

| Command | Output type | Output |
|---|---|---|
| `Invoke-HelloHelper echo hello world` | `System.String` | `hello world` |
| `Invoke-HelloHelper where` | `System.String` | the staged path, as in the listing above |
| `Start-HelloHelper` | `System.Int64` | the helper's process id, such as `12644` |
| `Stop-HelloHelper` | `System.String` | `released`, once per helper started |
