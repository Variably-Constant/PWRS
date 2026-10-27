---
title: How To Test A Module
weight: 7
---

Three layers: Pester against the built module in both hosts, Rust unit tests against a fake host, and Rust tests that drive the real engine in-process. Source: `crates/cargo-pwrs/src/test.rs` and `crates/cargo-pwrs/scripts/pester.ps1`, `crates/pwrs/src/testing.rs`, `crates/pwrs-host/src/lib.rs` and `crates/pwrs-host/dotnet/Pwrs.TestHost/Host.cs`.

## Pester in both hosts

```text
cargo pwrs test --release [--manifest-dir <crate>]
```

1. `cargo test` runs in the crate.
2. The module is built.
3. If the crate has a `tests/` directory, `pester.ps1` runs `Invoke-Pester -Path tests` in `pwsh`, and on Windows again in `powershell.exe`, with `PWRS_MODULE` set to the module folder. Each host prints `pwrs pester host=<version> pester=<version> passed=<n> failed=<n>` and exits with the failed count.

The suites run under Pester 6.2.0, which `cargo pwrs test` fetches into the toolchain the first time, before anything builds, and imports in both hosts from there, so there is nothing to install and both hosts run the same Pester. `PWRS_PESTER_PATH` names another Pester module to import in its place, and with it set nothing is fetched.

A test file imports the module from the environment variable:

```powershell
BeforeAll {
    Import-Module (Join-Path $env:PWRS_MODULE 'Hello.psd1') -Force -ErrorAction Stop
}
```

Write the tests for Pester 6: setup in `BeforeAll`, as above, and assertions with `Should`. The `examples/hello/tests` files are the reference: they cover output, streams (`4>&1` for verbose), errors (`-ErrorVariable`, `-ErrorAction Stop`, `Should -Throw`), completion (`TabExpansion2`), dynamic parameters, classes (`GetType().FullName`, `Get-Member`), providers, enums, and pipeline stopping.

## What the suite did not reach

After the Pester runs, `cargo pwrs test` reports declarations nothing exercised:

```
pwrs: surface check: 1 of the module's declarations was not exercised
  Get-RustColor was never invoked, so none of its declarations were checked.
```

Three things are reported: a cmdlet no test invoked, a cmdlet that declares `supports_should_process` but ran without ever calling `should_process`, and a parameter declared `value_from_pipeline`, `value_from_pipeline_by_property_name` or `literal_path`, which binds a piped object's `PSPath`, that nothing was ever piped into. PowerShell acts on both declarations whether or not the body does its part: `-WhatIf` on a cmdlet that never calls `should_process` does the very thing it should only describe, and neither case fails a test that does not exist.

The finding above is the one the check makes of a cmdlet like hello's `Get-RustColor` when the suite only tests that its `-Name` completes: completing a parameter does not run the command it belongs to. hello's suite also runs it once, so hello's own check reports nothing. A finding never fails the command. Each one is either a test to add or a declaration the body does not follow.

## Rust unit tests against the fake host

`pwrs::testing` is a vtable implemented in Rust: objects live in the process, strings, numbers, arrays and `PSObject`s behave like the managed host for conversion purposes, and the cmdlet-stream entries record what was written. Nothing calls .NET, so `cargo test` runs it anywhere.

```rust
#[test]
fn writes_three_items() {
    let _host = pwrs::testing::install();          // installs the fake host once; this test gets a context of its own
    let stopping = std::sync::atomic::AtomicBool::new(false);
    let scratch = std::cell::Cell::new(Vec::new());
    let ps = unsafe { pwrs::Pipeline::new(pwrs::sys::PsHandle::NULL, &stopping, &scratch) };
    pwrs::testing::take_output();
    ps.write(vec![1i64, 2, 3]).unwrap();
    let out = pwrs::testing::take_output();
    assert_eq!(out.len(), 3);
}
```

`install()` returns a guard and gives the calling test a context of its own. What the entries record, the handles it has and the streams it reports as enabled belong to that test, so tests run side by side as `cargo test` runs them. Let the guard live for the whole test. A handle made on the test's thread is read and released on any thread, and a task finished or an event raised through one on a worker thread is recorded for the test that made it. `take_output`, `take_streams`, `take_errors`, `take_errors_with_details` and `take_progress` return and clear what the test recorded, `take_progress` as the `Progress` records the managed runtime would build. `object(Value)` makes a handle to a fake value. `live_handles()` counts the handles the test has open, which is how a test checks for leaks. `PsObject::get` reads a note property of a fake `PSObject`, so a value read by property name, such as a `PsCredential`, converts from one. Entries the fake cannot provide (script blocks, session-state variables, setting a member, method calls, constructors, generated classes and proxy objects) fail with a clear message, and it pins only arrays it made itself. `crates/pwrs/src/convert_tests.rs` is the reference.

## The real engine in-process

`pwrs-host` starts the .NET runtime that pwsh ships, through `hostfxr`, inside the test process, and loads `Pwrs.TestHost.dll`, compiled on first use with the same fetched toolchain. It runs each session's scripts in a runspace of the thread that runs them, so tests on different threads run side by side and a session stays on the thread that started it:

```rust
let dll = pwrs_host::build_testhost(&out_dir)?;
let session = pwrs_host::Session::start(None, &dll)?;   // None: the pwsh on PATH
session.import_module(&module_dir)?;
let r = session.run("'a','b' | Get-Greeting")?;
assert_eq!(r.output, vec!["Hello, a!", "Hello, b!"]);
assert!(r.errors.is_empty());
```

`RunResult` has `output`, `errors`, `verbose`, `warning`, `information` and `terminating` (an exception that escaped `Invoke`). One runtime per process; a second `Session::start` reuses it. A pwsh installed from the Microsoft Store lives under `WindowsApps`, whose files cannot be mapped as executables by another process, so such an install is mirrored once into `~/.pwrs/pshome/<folder>` and hosted from there. A pwsh installed as a snap runs on the ICU (the Unicode library .NET uses for culture data on Linux) that the snap bundles, which only the snap's own executable and launcher can find. So before the runtime starts, the snap's ICU is loaded by full path, and `CLR_ICU_VERSION_OVERRIDE` names its version unless it is already set. Without that, .NET ends the process with "Couldn't find a valid ICU package installed on the system". `crates/pwrs-host/tests/inproc.rs` skips when no pwsh is on `PATH`. Windows PowerShell 5.1 has no `hostfxr`; the Pester runner covers it by spawning `powershell.exe`.

## Where the suites run

The Rust tests and `cargo pwrs test` for each example run on Windows under both hosts, on Linux and on FreeBSD, on machines of the project's own; [`docs/PLATFORMS.md`](https://github.com/Variably-Constant/PWRS/blob/main/docs/PLATFORMS.md) records each. `.github/workflows/ci.yml` runs the same suites on `macos-latest` (arm64, the runner's own pwsh 7), the one platform none of those machines runs. Each platform builds its own native library; nothing is cross-compiled.

Five repository-level gates run alongside them. `python tools/wire_audit.py` fails the build on a function that nothing in production, the C# sources or a suite calls, unless an attribute such as `#[completer]` or `#[member]` has the generated code call it. `tools/gallery_rules_gate.ps1` checks the script and manifest cargo-pwrs wrote for each built example against the rules the PowerShell Gallery runs on every package, PSScriptAnalyzer with its `PSGallery` settings, and fails on any finding; see [How To Publish](How-To-Publish.md). `tools/hot_reload_gate.ps1` builds one module three times in one host process and checks that a rebuilt surface takes its cmdlets over, that a rebuilt body does not disturb the types, and that each generation's objects come from that generation's own shell. See [How To Reload A Module](How-To-Reload-A-Module.md). `tools/coload_gate.ps1` builds one fixture twice under two names and imports both into one session in each order, checking that each module's copied objects, proxies and enum values are its own. That is the case Windows PowerShell's single load context puts at risk. Both run in both Windows hosts and in pwsh on Linux, FreeBSD and macOS. Both take `-Target <triple>`, passed to `cargo pwrs build --target`, so the modules they build load under a pwsh on an older glibc than the building machine's, such as the PowerShell snap's. `tools/tutorial_gate.ps1` builds the module each tutorial chapter describes and runs the chapter's examples in both hosts on Windows and in pwsh elsewhere. It fails when an example prints something other than what the page shows, or when the last chapter's files differ from `examples/file-facts`.
