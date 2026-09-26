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

The runner imports Pester from `PWRS_PESTER_PATH` when set, else from `target/pester/Pester` when that folder exists (`Save-PSResource -Name Pester -Path target/pester` creates it), else `Import-Module Pester -MinimumVersion 4.0` from the host's module path.

A test file imports the module from the environment variable:

```powershell
BeforeAll {
    Import-Module (Join-Path $env:PWRS_MODULE 'Hello.psd1') -Force -ErrorAction Stop
}
```

Write tests in Pester 4 syntax so they run unchanged on the version either host loads. The `examples/hello/tests` files are the reference: they cover output, streams (`4>&1` for verbose), errors (`-ErrorVariable`, `-ErrorAction Stop`, `Should -Throw`), completion (`TabExpansion2`), dynamic parameters, classes (`GetType().FullName`, `Get-Member`), providers, enums, and pipeline stopping.

## What the suite did not reach

After the Pester runs, `cargo pwrs test` reports declarations nothing exercised:

```
pwrs: surface check: 1 of the module's declarations were not exercised
  Get-RustColor was never invoked, so nothing here was checked of it.
```

Three things are reported: a cmdlet no test invoked, a cmdlet that ran without ever asking though it declares `supports_should_process`, and a parameter declaring `value_from_pipeline` that nothing was ever piped into. The engine keeps both promises whether or not the body holds up its end, which is why they are worth reporting: `-WhatIf` on a cmdlet that never calls `should_process` does the thing it was supposed to describe, and neither case fails a test that does not exist.

The finding above is the real one from this repository's own hello module: its `Get-RustColor` existed to have its `-Name` completed, and completing a parameter does not run the command it belongs to. A finding never fails the command, because each is either a missing test or a promise the body does not keep and only the author knows which.

## Rust unit tests against the fake host

`pwrs::testing` is a vtable implemented in Rust: objects live in a table in the process, strings, numbers, arrays and `PSObject`s behave like the managed host for conversion purposes, and the cmdlet-stream entries record what was written. Nothing touches .NET, so `cargo test` runs it anywhere.

```rust
#[test]
fn writes_three_items() {
    let _host = pwrs::testing::install();          // installs the fake table once; serializes tests that share it
    let stopping = std::sync::atomic::AtomicBool::new(false);
    let ps = unsafe { pwrs::Pipeline::new(pwrs::sys::PsHandle::NULL, &stopping) };
    pwrs::testing::take_output();
    ps.write(vec![1i64, 2, 3]).unwrap();
    let out = pwrs::testing::take_output();
    assert_eq!(out.len(), 3);
}
```

`install()` returns a guard; hold it for the length of the test, because the table is process-global and two tests writing at once would interleave. `take_output`, `take_streams` and `take_errors` return and clear what was recorded; `object(Value)` makes a handle to a fake value; `live_handles()` counts handles, which is how leaks are asserted. Entries the fake cannot provide (script blocks, dynamic calls, pinning, factories) fail with a clear message. `crates/pwrs/src/convert_tests.rs` is the reference.

## The real engine in-process

`pwrs-host` starts the .NET runtime that pwsh ships through `hostfxr` inside the test process, loads `Pwrs.TestHost.dll` (compiled on first use with the same fetched toolchain), and runs scripts in one runspace:

```rust
let dll = pwrs_host::build_testhost(&out_dir)?;
let session = pwrs_host::Session::start(None, &dll)?;   // None: the pwsh on PATH
session.import_module(&module_dir)?;
let r = session.run("'a','b' | Get-Greeting")?;
assert_eq!(r.output, vec!["Hello, a!", "Hello, b!"]);
assert!(r.errors.is_empty());
```

`RunResult` carries `output`, `errors`, `verbose`, `warning`, `information` and `terminating` (an exception that escaped `Invoke`). One runtime per process; a second `Session::start` reuses it. A pwsh installed from the Microsoft Store lives under `WindowsApps`, whose files cannot be mapped as executables by another process, so such an install is mirrored once into `~/.pwrs/pshome/<folder>` and hosted from there. A pwsh installed as a snap runs on the ICU the snap bundles, which only the snap's own executable and launcher know how to find, so before the runtime starts the snap's ICU is loaded by full path and `CLR_ICU_VERSION_OVERRIDE` names its version, unless it is already set; without that, .NET ends the process with "Couldn't find a valid ICU package installed on the system". `crates/pwrs-host/tests/inproc.rs` skips when no pwsh is on `PATH`. Windows PowerShell 5.1 has no `hostfxr`; the Pester runner covers it by spawning `powershell.exe`.

## Where the suites run

The Rust tests and `cargo pwrs test` for each example run on Windows under both hosts, on Linux and on FreeBSD, on machines of the project's own; [`docs/PLATFORMS.md`](https://github.com/Variably-Constant/PWRS/blob/main/docs/PLATFORMS.md) records each. `.github/workflows/ci.yml` runs the same suites on `macos-latest` (arm64, the runner's own pwsh 7), the one platform none of those machines runs. Each platform builds its own native library; nothing is cross-compiled.

Three repository-level gates run alongside them. `python tools/wire_audit.py` fails the build on a function nothing in production, the generated C# or a suite calls. `tools/hot_reload_gate.ps1` builds one module three times in one host process and checks that a rebuilt surface takes its cmdlets over, that a rebuilt body does not disturb the types, and that each generation's objects come from that generation's own shell; see [How To Reload A Module](How-To-Reload-A-Module.md). `tools/coload_gate.ps1` builds one fixture twice under two names and imports both into one session in each order, checking that each module's copied objects, proxies and enum values are its own; that is the case Windows PowerShell's single load context puts at risk. Both run in both Windows hosts and in pwsh on Linux, FreeBSD and macOS, and both take `-Target <triple>`, handed to `cargo pwrs build --target`, so the modules they build load under a pwsh on an older glibc than the building machine's, such as the PowerShell snap's.
