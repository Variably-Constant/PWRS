---
title: PWRS Wiki
toc: false
---

<p align="center">
  <img src="pwrs-logo.svg" alt="PWRS logo" width="240" />
</p>

Rust bindings for PowerShell binary modules, in the spirit of [PyO3](https://github.com/PyO3/pyo3). A cmdlet is a Rust struct, and its parameters are that struct's fields, converted, validated and completed by PowerShell's own binder before your code runs. `cargo pwrs build` produces a module folder that imports into PowerShell 7 from 7.4 on and Windows PowerShell 5.1 with no .NET SDK on the machine.

{{< cards >}}
  {{< card link="docs/tutorials/" title="Tutorials" subtitle="Nine chapters build one module, FileFacts, from cargo pwrs new to the PowerShell Gallery." icon="academic-cap" >}}
  {{< card link="docs/how-to/" title="How-to" subtitle="One task per page: cmdlets, objects, threads, tests, speed, shipping, and fixing failures." icon="cog" >}}
  {{< card link="docs/explanation/" title="Explanation" subtitle="How PWRS works: the bridge, the call path, the two hosts, and how it compares with PyO3." icon="book-open" >}}
  {{< card link="docs/reference/" title="Reference" subtitle="Attributes, conversions, the Pipeline API, cargo pwrs, the ABI, and the Hello Tour." icon="document-text" >}}
{{< /cards >}}

## Quick links

- **New here?** [Getting Started](Getting-Started.md), the first of the nine tutorial chapters - from `cargo pwrs new` to `Import-Module`.
- **Want to see everything at once?** [Hello Tour](Hello-Tour.md) - the `examples/hello` module, cmdlet by cmdlet, with the Pester tests that check each behavior.
- **Choosing a type for a parameter or an output?** [Conversions Reference](Conversions-Reference.md).
- **Writing to the pipeline, the streams, or session state?** [Pipeline Reference](Pipeline-Reference.md).
- **Need to know what passes between Rust and .NET, and how?** [The Bridge](The-Bridge.md) and [ABI Reference](ABI-Reference.md).
- **Need numbers?** [Benchmarks](Benchmarks.md) - what was measured, on which machine, with which controls.

## What PWRS is

| PyO3 | PWRS |
|---|---|
| `#[pymodule]` and `PyInit_` | `pwrs::export_module!` and the generated module folder |
| `#[pyfunction]` | `#[cmdlet]` on a struct implementing `Cmdlet` |
| function arguments | `#[param]` fields, bound by PowerShell's own binder |
| return value | the output stream, `ps.write(value)` |
| a Python exception | `PsError` with an `ErrorCategory`, an id, and a terminating flag |
| `#[pyclass]` | `#[psclass]` in copied, proxy, or psobject mode; `#[psenum]` for enums |
| `Py<PyAny>` | `PsObject`, a `GCHandle` freed when it is dropped |
| `Python<'py>` | `Pipeline<'ps>`, the `!Send` pipeline-thread token |
| `abi3` | the append-only host vtable |
| `maturin` | `cargo pwrs` |
| embedding | `pwrs-host` |

See [PWRS and PyO3](Pwrs-And-PyO3.md) for the full comparison, including what PWRS has that PyO3 does not need (providers, completers and dynamic parameters, generated help and default views, hybrid C#, `-WhatIf` and `-Confirm`) and what PyO3 has that PWRS does not.

## What PWRS is not

- **Not a way to run Rust inside PowerShell script.** The Rust code is compiled ahead of time into a native library that a generated binary module loads. There is no REPL and no scripting bridge.
- **Not a C# replacement.** The shell is generated C#, and a module may add hand-written C# cmdlets beside the Rust ones ([Hybrid C#](How-To-Add-Hybrid-CSharp.md)).
- **Not an async runtime.** Work off the pipeline thread runs on threads: one through `stream_from_thread` or `stream_from_worker`, or a pool through `par_map` and `par_for_each`. The pool is as wide as the machine's processors or as `workers(n)` sets, on std threads or, under the `parallel` feature, on Flynnel's work-stealing pool. The pipeline thread writes every record those threads make.

## Wiki contents

### Tutorials

1. [Getting Started](Getting-Started.md) - install cargo pwrs, make the module, run its first command in both hosts.
2. [Parameters and the Pipeline](Parameters-And-The-Pipeline.md) - paths, wildcards, pipeline input, parameter sets.
3. [Talking to the User](Talking-To-The-User.md) - errors, warnings, verbose text, progress, console colors, `-WhatIf`.
4. [Objects Out](Objects-Out.md) - the module's own classes, table views, an enum, an object stored in Rust.
5. [Objects In](Objects-In.md) - script blocks, hashtables and .NET objects passed to the module.
6. [Help and the Binder](Help-And-The-Binder.md) - help, an alias, completion, dynamic parameters, a transform.
7. [Long Work](Long-Work.md) - parallel work, progress from workers, Ctrl+C, an engine event.
8. [Testing](Testing.md) - Rust unit tests, Pester in both hosts, what no test exercised.
9. [Shipping](Shipping.md) - the author, the release build, license notices, publishing.

### How-to

- [How To Write A Cmdlet](How-To-Write-A-Cmdlet.md) - parameters, phases, output, streams and progress, errors, confirmation, prompts, cancellation, session state.
- [How To Return Objects](How-To-Return-Objects.md) - copied, proxy and psobject classes, enums, arrays, hashtables, objects taken back as parameters, default views.
- [How To Write A Provider](How-To-Write-A-Provider.md) - a `NavigationCmdletProvider` from a Rust trait, with the in-memory filesystem as the worked example.
- [How To Add Completers, Transforms And Dynamic Parameters](How-To-Add-Completers-And-Dynamic-Parameters.md) - tab completion from Rust, an argument changed before the binder converts it, and parameters that appear depending on what was bound.
- [How To Call .NET From Rust](How-To-Call-DotNet-From-Rust.md) - properties, methods, statics, constructors, script blocks, commands by name, pinned arrays, Rust memory as `Memory<T>`.
- [How To Use Threads](How-To-Use-Threads.md) - the pipeline-thread rule, `stream_from_thread`, workers that write every stream, `par_map` and `par_for_each`, cancellation.
- [How To Test A Module](How-To-Test-A-Module.md) - Pester in both hosts, the fake host for Rust unit tests, the in-process engine.
- [How To Add Hybrid C#](How-To-Add-Hybrid-CSharp.md) - hand-written C# cmdlets compiled into the same shell assembly, and the desktop assemblies they can use.
- [How To Publish](How-To-Publish.md) - `cargo pwrs publish`, the dry run, a module built on several platforms, license notices.
- [How To Make A Module Fast](How-To-Make-A-Module-Fast.md) - pipeline input, the phase mask, bulk data through a pin, and what each is worth.
- [How To Use Instruction Sets](How-To-Use-Instruction-Sets.md) - AVX2, AVX-512 and the rest chosen at run time, and every tier tested on one machine.
- [How To Pass Native Data Between Cmdlets](How-To-Pass-Native-Data.md) - one object per stage taken by type, the object's gate, and what the rows cost.
- [How To Ship A Helper Executable](How-To-Ship-A-Helper-Executable.md) - a `[[bin]]` target built with the library, shipped beside it, and started from a copy staged for the process.
- [How To Ship A Prebuilt Native File](How-To-Ship-A-Prebuilt-Native-File.md) - a file from a pinned download shipped beside the library, its licenses in the notices.
- [How To Bundle A Module](How-To-Bundle-A-Module.md) - another PWRS module placed inside this one, imported with it, and included by publish and merge.
- [How To Raise Engine Events](How-To-Raise-Engine-Events.md) - `PsEvents` from any thread, read in script by `Wait-Event` and `Get-Event`, or taken by a `Register-EngineEvent -Action` subscription.
- [How To Trace A Module](How-To-Trace-A-Module.md) - `PWRS_TRACE`, the counters on both sides, and what a line means.
- [How To Reload A Module](How-To-Reload-A-Module.md) - picking up a rebuild in a live session, and what it costs to leave the old one loaded.
- [How To Run PowerShell In A Rust Program](How-To-Run-PowerShell-In-A-Rust-Program.md) - `pwrs-host`: the installed pwsh found, its runtime started in the process, an assembly's entry point called.
- [How To Use A Module In Runspaces, Jobs And Remote Sessions](How-To-Use-Runspaces-Jobs-And-Remote-Sessions.md) - the module imported in each runspace, objects shared within a process and copied between processes, and the remoting transports run.
- [How To Debug A Module](How-To-Debug-A-Module.md) - breakpoints in a module's Rust code with WinDbg, gdb and lldb, attaching to a running shell, and finding where a panic started.
- [How To Fix A Failure](How-To-Fix-A-Failure.md) - what each failure means, keyed by the text you see, starting with the crate and `cargo-pwrs` at different versions.

### Explanation

- [Why PWRS](Why-Pwrs.md) - the problem PyO3 does not have, and the shell, the bridge and the build that solve it.
- [The Bridge](The-Bridge.md) - the generated shell, the exports, the vtable, what passes between Rust and .NET, the two rules at the boundary, and the pipeline thread.
- [The Call Path](The-Call-Path.md) - what one invocation costs, piece by piece, and how each piece was measured.
- [Two Hosts](Two-Hosts.md) - PowerShell 7 from 7.4 on and Windows PowerShell 5.1 from one build, and what 5.1 cannot do.
- [PWRS and PyO3](Pwrs-And-PyO3.md) - concept by concept: what is alike, what differs, and what each has that the other lacks.

### Reference

- [Attribute Reference](Attribute-Reference.md) - `#[cmdlet]`, `#[param]`, `#[psclass]` and `#[psfield]`, `#[psenum]`, `#[psmethods]`, `#[completer]`, `#[transform]`, `#[dynamic_params]`, `#[on_load]`, `#[on_import]`, `#[on_remove]` and `#[on_unload]`, `#[provider]`, `export_module!`.
- [Conversions Reference](Conversions-Reference.md) - every Rust type PWRS converts, the CLR type it becomes, and where the two hosts differ.
- [Pipeline Reference](Pipeline-Reference.md) - every method on `Pipeline<'ps>` and `PsObject`; progress, workers, pinned memory, tasks, engine events and `HostUi`; `PsType`; the wrapper types (`PsHashtable`, `PsReadOnlyTable`, `PsScriptBlock`, `PsBigInt`, `PsSecureString`, `PsCredential`); the value types (`PsDateTime`, `PsTimeSpan`, `PsGuid`); `PsError`; and the traits and types behind cmdlets, completers, dynamic parameters and providers.
- [cargo pwrs Reference](Cargo-Pwrs-Reference.md) - subcommands and their flags, what each returns, the toolchain, the `[package.metadata.pwrs]` keys, the generated files.
- [Environment Variables](Environment-Variables.md) - every variable the tools and a running module read.
- [ABI Reference](ABI-Reference.md) - status codes, native exports, vtable slots, parameter, field and method blocks, the error flow through a phase.
- [Benchmarks](Benchmarks.md) - what was measured, on which machine, with which controls.
- [Glossary](Glossary.md) - terms specific to PWRS and the PowerShell vocabulary it relies on.
- [Hello Tour](Hello-Tour.md) - every command and class in `examples/hello`, the Rust behind it, and the test that checks it.

## License

MIT - see [LICENSE](https://github.com/Variably-Constant/pwrs/blob/main/LICENSE) on the repository.
