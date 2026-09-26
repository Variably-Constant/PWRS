---
title: Environment Variables
weight: 5
---

Every variable the tools and a running module read. All optional.

| Variable | Read by | Effect |
|---|---|---|
| `PWRS_PWSH` | `pwrs-build` (`pwsh.rs`) | the pwsh executable to run, instead of `pwsh` on `PATH` |
| `PWRS_PSHOME` | `pwrs-build` (`pwsh.rs`) | `$PSHOME`, instead of asking pwsh for it; `pwrs-host` starts the runtime through the `hostfxr` under it, and `cargo pwrs toolchain` prints it |
| `PWRS_HOME` | `pwrs-build` (`toolchain.rs`) | the directory holding `toolchain/`, instead of `~/.pwrs` (from `USERPROFILE` or `HOME`) |
| `PWRS_TOOLSET` | `pwrs-build` (`toolchain.rs`) | the `Microsoft.Net.Compilers.Toolset` version to fetch, instead of the pinned one. Each version gets its own tree under `toolchain/`, so switching neither refetches nor mixes two compilers under one lock |
| `PWRS_PESTER_PATH` | `cargo-pwrs` (`test.rs`) | a Pester module to `Import-Module`, instead of `<target>/pester/Pester` or the host's module path |
| `PWRS_PSGALLERY_KEY` | `publish.ps1` | the PowerShell Gallery API key; required unless `--dry-run` |
| `RUSTC` | `cargo-pwrs` (`cpu.rs`) | the compiler `cargo pwrs build` asks, with `rustc --print cfg` and `--target` for a cross build, which extensions the target assumes, so a library compiled for more is warned about; `rustc` on `PATH` otherwise, as cargo does |
| `PWRS_MODULE` | your Pester tests | set by `pester.ps1` to the built module folder |
| `PWRS_SURFACE_DIR` | a running module (`surface.rs`), set by `cargo pwrs test` | the directory each native image in each process writes `<pid>-<image>.tsv` to, so two PWRS modules in one process keep separate files, recording per cmdlet how many phases ran, how many asked the engine to confirm, and which parameters took a value from the pipeline. Unset, which is every production call, a phase pays one relaxed load and a branch. `cargo pwrs test` sets it, clears the directory first, and reads what both hosts leave |
| `PWRS_THREAD_CHECK` | a running module (`host.rs`), set by `cargo pwrs test` | `1` makes a `PsObject` method called from a thread the host did not call the module on (a worker the module started) return an error with id `PwrsOffThread` instead of running. A debug build checks whatever it says; a release build checks only when it is `1` |
| `PWRS_CPU_MAX` | a running module (`cpu.rs`, `CpuCheck.cs`) | caps the x86-64 extensions a module may use at one psABI level: `x86-64`, `x86-64-v2`, `x86-64-v3` or `x86-64-v4`, with `native` or unset for no cap. `pwrs::cpu::has` answers under it, and the import check refuses a library compiled for more than it allows, so one machine can run every tier of a module. Any other value refuses the import |
| `PWRS_TRACE` | a running module (`trace.rs`, `Native.cs`) | `1` prints both sides' counters to stderr every 10000 events; `2` prints every event on the Rust side; unset or `0` prints nothing and skips the counter sites. Has to be in the environment the process inherits: assigning it inside PowerShell through `$env:` was observed on Linux to reach the managed counters only |

Others are read as well, none of them a setting of PWRS's own. `USERPROFILE` or `HOME` places `~/.pwrs`. `PSModulePath` is handed to a Windows PowerShell run with PowerShell 7's module directories taken out. `CLR_ICU_VERSION_OVERRIDE` is set by `pwrs-host`, when the pwsh it hosts is a Linux snap, to the version of the ICU the snap carries, unless it is already set. Nothing else is read from the environment. The toolchain fetch uses `Invoke-WebRequest`, which honors the host's proxy settings.
