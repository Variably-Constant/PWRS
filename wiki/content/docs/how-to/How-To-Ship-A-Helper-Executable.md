---
title: How To Ship A Helper Executable
weight: 13
---

A module sometimes needs a process of its own: a sandbox per guest, a worker whose crash must not end the session, a tool that controls a device. PWRS ships one as a helper executable, a `[[bin]]` target of the module's crate that `cargo pwrs build` builds with the library and puts beside it, and the module starts it from Rust. Source: `crates/cargo-pwrs/src/build.rs` (`check_helpers`, `macos_bundles_of`, `lay_out_bundle` and the copy in `build`), `crates/pwrs/src/helper.rs`, and `StageHelper` and `StageBundle` in `crates/cargo-pwrs/dotnet/Pwrs.Runtime/NativeModule.cs`. The worked example is `examples/hello`: `src/bin/hello-helper.rs`, the `Invoke-HelloHelper`, `Start-HelloHelper` and `Stop-HelloHelper` cmdlets, and `tests/Helper.Tests.ps1`.

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
runtimes/win-x64/native/hello-helper.exe                 136192 bytes
runtimes/win-x64/native/pwrs_example_hello.dll          1958912 bytes
runtimes/win-x64/pwrs-build.json                            152 bytes
runtimes/win-x64/THIRD-PARTY-NOTICES.txt                   3014 bytes
```

`publish` ships the module folder as built, so it includes the helpers. A module made with `cargo pwrs new` and given a helper this way packaged, through the two steps `publish --dry-run` takes (`Test-ModuleManifest`, then `Compress-PSResource`), to a `.nupkg` containing `runtimes/win-x64/native/demo-helper.exe` beside the module's library. The notices beside the library cover a helper's crates as well, because a `[[bin]]` target links only crates among its package's own dependencies; see [How To Publish](How-To-Publish.md#license-notices).

## One folder for every platform

`cargo pwrs merge` copies each `runtimes/<rid>` folder whole, so the helper a Linux build ships ends up beside the Windows one; the [cargo pwrs reference](../reference/Cargo-Pwrs-Reference.md#merge) lists hello's folder after its Linux x64 build is merged into its Windows x64 one. A `.nupkg` records no Unix mode for its entries, so a helper packaged on Windows installs on Linux without its execute permission. Packaged with `Compress-PSResource` on Windows and saved with `Save-PSResource` on Ubuntu 24.04, hello's helper is:

```text
-rw-rw-r--  runtimes/linux-x64/native/hello-helper
```

The copy whose path `pwrs::helper_path` returns is readable and executable by its owner whatever mode the shipped file has, and nothing in the module folder is changed to make it so. Installed that way, hello starts its helper, and its suite passes 467 of 467 from the installed folder in pwsh 7.6.6.

## Start it from Rust

```rust
let helper = pwrs::helper_path("hello-helper")?;
let out = std::process::Command::new(&helper).args(&self.argument_list).output()?;
```

`pwrs::helper_path` takes the target's name without `.exe` and returns the path to start. It can be called from any thread, since it reaches no runspace. Start the helper from that path and never from the module folder: [a helper started from the module folder stops the next build](#a-helper-started-from-the-module-folder-stops-the-next-build).

For a helper that runs past one call, the module stores its `std::process::Child` and waits for it. `Start-HelloHelper` stores each child it spawns, and `Stop-HelloHelper` closes the child's input and calls `wait_with_output`. On Linux and FreeBSD an exited child that nobody waits for stays in the process table until the session ends.

## A helper started from the module folder stops the next build

Measured on Windows 11 (build 26200): while an executable runs, its file can be renamed and a new file written under its old name. Deleting it, overwriting it in place and deleting the renamed file are all refused until it exits. `cargo pwrs build` clears the module folder before writing it, and that is refused too, so a helper started straight from the module folder stops the next build:

```text
cargo-pwrs: cannot clear C:\Temp\pwrs-baseline-target\pwrs\Hello: Access is denied. (os error 5)
```

Started through `pwrs::helper_path`, the same helper runs from the process's staging folder instead. With one running under each host, pwsh 7.6.7 and Windows PowerShell 5.1, `cargo pwrs build --release` of the hello module succeeded, and each helper then ran to completion when released.

## Where the copy lives

The copy goes into the folder the process stages the module's library in, one folder per copy:

```text
<temp>/pwrs-load/<pid>/<key>/native/<file>.<mark>/<file>
C:\Users\<user>\AppData\Local\Temp\pwrs-load\34436\1434073889\native\hello-helper.exe.8df33f020fac841-21400\hello-helper.exe
```

`<mark>` is the shipped file's write time and length in hex (`0x21400` is the 136192 bytes above). A rebuilt helper therefore stages beside a running one rather than over it, and the file is staged under its own name, so a process listing shows the name its author gave it. The first request for a helper's bytes copies them under a temporary name and moves the copy into place, so no caller gets a file another thread is still writing. Every later request returns the same path. The staging folder is cleaned up with the session's others: the next session to start deletes the staging folders of sessions that have ended, and leaves one that is still in use for the session after.

## Ship it as an app bundle on macOS

On macOS a process outside an app bundle has no bundle identity: `UNUserNotificationCenter` refuses it with "bundleProxyForCurrentProcess is nil", and its windows show the executable's file name in the Dock. `[package.metadata.pwrs.macos-bundles]` ships a helper inside a bundle of its own:

```toml
[package.metadata.pwrs.macos-bundles.hello-helper]
name = "Hello Helper"
identifier = "io.github.variably-constant.pwrs.hello-helper"
```

The key is a name `helpers` lists. `name` is the bundle's folder name without `.app` and its `CFBundleName`, and `identifier` its `CFBundleIdentifier`. `display-name` sets `CFBundleDisplayName`, the name when it is not given, and `icon` names an `.icns` file from the crate directory, which ships under `Contents/Resources` with `CFBundleIconFile` naming it. The table is read on every platform, so a build on Windows or Linux stops on an entry no Mac could use, naming the entry and what is wrong with it.

A macOS build lays the bundle out beside the library, and every other platform ships the executable as before. The hello example's release build on macOS arm64, in CI:

```text
runtimes/osx-arm64/pwrs-build.json                                              152 bytes
runtimes/osx-arm64/THIRD-PARTY-NOTICES.txt                                     3019 bytes
runtimes/osx-arm64/native/libpwrs_example_hello.dylib                       1908672 bytes
runtimes/osx-arm64/native/Hello Helper.app/Contents/Info.plist                  802 bytes
runtimes/osx-arm64/native/Hello Helper.app/Contents/MacOS/hello-helper       380544 bytes
```

with this `Info.plist`, the versions being the package's:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>Hello Helper</string>
	<key>CFBundleExecutable</key>
	<string>hello-helper</string>
	<key>CFBundleIdentifier</key>
	<string>io.github.variably-constant.pwrs.hello-helper</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Hello Helper</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>0.8.0</string>
	<key>CFBundleVersion</key>
	<string>0.8.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
```

`NSHighResolutionCapable` is there so the bundle's windows draw at the display's resolution, as the bare executable's do.

`pwrs::helper_path` finds the bundle by the executable in its `Contents/MacOS`, so the module needs no list of bundles. It stages the whole folder by the rules above: the folder is copied under a temporary name and moved into place, named by the latest write time and the total length of its files, and its executable is made readable and executable by its owner. It returns the path of the executable inside the copy:

```text
<temp>/pwrs-load/<pid>/<key>/native/<name>.app.<mark>/<name>.app/Contents/MacOS/<file>
```

macOS reads the bundle a process runs in from the folders around its executable, so the helper started from that path runs as the bundle. `merge` copies the bundle with the rest of `runtimes/osx-arm64`, and `publish` packages it whole. The packaging `publish --dry-run` does, run in CI over hello's macOS build, made a package whose runtime entries are:

```text
runtimes/osx-arm64/native/Hello Helper.app/Contents/Info.plist
runtimes/osx-arm64/native/Hello Helper.app/Contents/MacOS/hello-helper
runtimes/osx-arm64/native/libpwrs_example_hello.dylib
runtimes/osx-arm64/pwrs-build.json
runtimes/osx-arm64/THIRD-PARTY-NOTICES.txt
```

Saved from that package as `Install-PSResource` saves a module, the helper arrived without execute permission:

```text
Hello/0.8.0/runtimes/osx-arm64/native/Hello Helper.app/Contents/MacOS/hello-helper OtherRead, GroupRead, UserWrite, UserRead
```

The saved module started it all the same. `Invoke-HelloHelper where` and `Invoke-HelloHelper bundle` printed the staged copy's path, `TMPDIR` written as `$TMPDIR`, and the identifier of the bundle the helper runs in, which `hello-helper bundle` reads from CoreFoundation's main bundle, the same bundle `NSBundle.mainBundle` returns:

```text
$TMPDIR/pwrs-load/15239/352112702/native/Hello Helper.app.8df33f734763300-5d1a2/Hello Helper.app/Contents/MacOS/hello-helper
io.github.variably-constant.pwrs.hello-helper
```

`0x5d1a2` is 381346, the executable's 380544 bytes and the `Info.plist`'s 802.

## Reference

| Call | Returns | Fails |
|---|---|---|
| `pwrs::helper_path(name: &str)` | `PsResult<std::path::PathBuf>`: the staged copy's path; for a helper shipped as an app bundle, its executable inside the staged copy of the bundle | a `PsError` with id `PwrsRuntimeError` whose message is the runtime's exception. For a helper the module does not ship, it names the file, the runtime identifier and the folder searched; for a name with a folder or a separator in it, it says the name is not a bare file name |

The hello cmdlets built on it, with output from Windows x64:

| Command | Output type | Output |
|---|---|---|
| `Invoke-HelloHelper echo hello world` | `System.String` | `hello world` |
| `Invoke-HelloHelper where` | `System.String` | the staged path, as in the listing above |
| `Invoke-HelloHelper bundle` | `System.String` | on macOS the bundle's identifier, `io.github.variably-constant.pwrs.hello-helper`; elsewhere the error `HelloHelperFailed`, ending `exited with exit code: 1: hello-helper: runs in no app bundle that names an identifier` |
| `Start-HelloHelper` | `System.Int64` | the helper's process id, such as `12644` |
| `Stop-HelloHelper` | `System.String` | `released`, once per helper started |
