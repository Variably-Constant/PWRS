---
title: PWRS Wiki
toc: false
---

<p align="center">
  <img src="pwrs-logo.svg" alt="PWRS logo" width="240" />
</p>

Rust bindings for PowerShell binary modules, in the spirit of [PyO3](https://github.com/PyO3/pyo3). A cmdlet is a Rust struct, and its parameters are that struct's fields, coerced, validated and completed by PowerShell's own binder before your code runs. `cargo pwrs build` produces a module folder that imports into PowerShell 7 from 7.4 on and Windows PowerShell 5.1 with no .NET SDK on the machine.

{{< cards >}}
  {{< card link="docs/tutorials/" title="Tutorials" subtitle="Learning-oriented. Getting Started takes a scaffolded crate to an imported module in ten minutes; the Hello Tour walks every mechanism the example module uses." icon="academic-cap" >}}
  {{< card link="docs/how-to/" title="How-to" subtitle="Task-oriented recipes: writing a cmdlet, returning objects, providers, completers, transforms and dynamic parameters, dynamic .NET, threads, testing, hybrid C#, publishing, tracing, fixing a failure." icon="cog" >}}
  {{< card link="docs/explanation/" title="Explanation" subtitle="Why the design is what it is: the problem PyO3 does not have, the bridge, the call path, how PWRS relates to PyO3, the two hosts." icon="book-open" >}}
  {{< card link="docs/reference/" title="Reference" subtitle="Look things up: the attribute grammar, conversions, the Pipeline API, cargo pwrs, environment variables, the ABI, benchmarks, glossary." icon="document-text" >}}
{{< /cards >}}

## Quick links

- **New here?** [Getting Started](Getting-Started.md) - from `cargo pwrs new` to `Import-Module`.
- **Want to see everything at once?** [Hello Tour](Hello-Tour.md) - the `examples/hello` module, cmdlet by cmdlet, with the Pester tests that pin each behavior.
- **Choosing a type for a parameter or an output?** [Conversions Reference](Conversions-Reference.md).
- **Writing to the pipeline, the streams, or session state?** [Pipeline Reference](Pipeline-Reference.md).
- **Need to know what crosses the boundary and how?** [The Bridge](The-Bridge.md) and [ABI Reference](ABI-Reference.md).
- **Need numbers?** [Benchmarks](Benchmarks.md) - what was measured, on which box, with which controls.

## What PWRS is

| PyO3 | PWRS |
|---|---|
| `#[pymodule]` and `PyInit_` | `pwrs::export_module!` and the generated module folder |
| `#[pyfunction]` | `#[cmdlet]` on a struct implementing `Cmdlet` |
| function arguments | `#[param]` fields, bound by the engine's own binder |
| return value | the output stream, `ps.write(value)` |
| a Python exception | `PsError` with an `ErrorCategory`, an id, and a terminating flag |
| `#[pyclass]` | `#[psclass]` in copied, proxy, or psobject mode; `#[psenum]` for enums |
| `PyObject` | `PsObject`, an owned `GCHandle` |
| `Python<'py>` | `Pipeline<'ps>`, the `!Send` pipeline-thread token |
| `abi3` | the append-only host vtable |
| `maturin` | `cargo pwrs` |
| embedding | `pwrs-host` |

See [PWRS and PyO3](Pwrs-And-PyO3.md) for the full comparison, including what PWRS has that PyO3 does not (providers, completers, dynamic parameters, two hosts from one build) and what PyO3 has that PWRS does not.

## What PWRS is not

- **Not a way to run Rust inside PowerShell script.** The Rust code is compiled ahead of time into a native library that a generated binary module loads. There is no REPL and no scripting bridge.
- **Not a C# replacement.** The shell is generated C#, and a module may add hand-written C# cmdlets beside the Rust ones ([Hybrid C#](How-To-Add-Hybrid-CSharp.md)).
- **Not a scheduler.** Off-thread work uses std threads and channels through `stream_from_thread`; bring your own thread pool.

## Wiki contents

### Tutorials

- [Getting Started](Getting-Started.md) - scaffold, build, test, import.
- [Hello Tour](Hello-Tour.md) - every cmdlet in `examples/hello`, what it exercises, and its test.

### How-to

- [How To Write A Cmdlet](How-To-Write-A-Cmdlet.md) - parameters, phases, output, errors, streams, confirmation, cancellation.
- [How To Return Objects](How-To-Return-Objects.md) - copied, proxy and psobject classes, enums, arrays, hashtables.
- [How To Write A Provider](How-To-Write-A-Provider.md) - a `NavigationCmdletProvider` from a Rust trait, with the in-memory filesystem as the worked example.
- [How To Add Completers, Transforms And Dynamic Parameters](How-To-Add-Completers-And-Dynamic-Parameters.md) - tab completion from Rust, an argument changed before the binder coerces it, and parameters that appear depending on what was bound.
- [How To Call .NET From Rust](How-To-Call-DotNet-From-Rust.md) - properties, methods, statics, constructors, script blocks, pinned arrays.
- [How To Use Threads](How-To-Use-Threads.md) - `stream_from_thread` and the pipeline-thread rule.
- [How To Test A Module](How-To-Test-A-Module.md) - Pester in both hosts, the fake host for Rust unit tests, the in-process engine.
- [How To Add Hybrid C#](How-To-Add-Hybrid-CSharp.md).
- [How To Publish](How-To-Publish.md).
- [How To Trace A Module](How-To-Trace-A-Module.md) - `PWRS_TRACE` and what its lines mean.
- [How To Fix A Failure](How-To-Fix-A-Failure.md) - what each failure means, keyed by the text you see, starting with the crate and `cargo-pwrs` having to move together.

### Explanation

- [Why PWRS](Why-Pwrs.md) - the problem PyO3 does not have and the four ways it could have been solved.
- [The Bridge](The-Bridge.md) - the generated shell, the vtable, the exports, and the two rules at the boundary.
- [The Call Path](The-Call-Path.md) - what one invocation costs and why.
- [Two Hosts](Two-Hosts.md) - PowerShell 7 from 7.4 on and Windows PowerShell 5.1 from one build.
- [PWRS and PyO3](Pwrs-And-PyO3.md).

### Reference

- [Attribute Reference](Attribute-Reference.md) - `#[cmdlet]`, `#[param]`, `#[psclass]`, `#[psfield]`, `#[psmethods]`, `#[psenum]`, `#[completer]`, `#[dynamic_params]`, `#[provider]`, `export_module!`.
- [Conversions Reference](Conversions-Reference.md) - every Rust type that crosses, and the CLR type it becomes.
- [Pipeline Reference](Pipeline-Reference.md) - every method on `Pipeline<'ps>`, `PsObject`, `PsType`, the wrapper types (`PsHashtable`, `PsScriptBlock`, `PsBigInt`, `PsSecureString`, `PsCredential`), the value types (`PsDateTime`, `PsTimeSpan`, `PsGuid`), `PsError`.
- [cargo pwrs Reference](Cargo-Pwrs-Reference.md) - subcommands, flags, the toolchain, the module folder.
- [Environment Variables](Environment-Variables.md).
- [ABI Reference](ABI-Reference.md) - status codes, exports, vtable slots, parameter blocks.
- [Benchmarks](Benchmarks.md).
- [Glossary](Glossary.md).

## License

MIT - see [LICENSE](https://github.com/Variably-Constant/pwrs/blob/main/LICENSE) on the repository.
