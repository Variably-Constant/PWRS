---
title: PWRS and PyO3
weight: 5
---

Concept by concept: what carried over from PyO3, what had to change because PowerShell is not CPython, and what each has that the other lacks.

## What carried over

| PyO3 | PWRS | Same idea because |
|---|---|---|
| `#[pymodule]` with `PyInit_` | `export_module!` with the exports it emits | one entry point the host finds by name; here the host finds `pwrs_module_descriptor` at build time and `pwrs_module_init` at import |
| `#[pyfunction]` | `#[cmdlet]` | one attribute turns a Rust item into something the host can call |
| `#[pyclass]` | `#[psclass]` | one attribute turns a Rust struct into a host-visible type |
| `Py<T>` | `PsObject` | an owned reference into the host's heap that can live anywhere and is released on drop |
| `Python<'py>` | `Pipeline<'ps>` | a `!Send` token proving the holder is allowed to touch the host right now; PyO3's proves the GIL is held, PWRS's proves you are on the pipeline thread inside a phase |
| `FromPyObject`, `IntoPy` | `FromPs`, `IntoPs` | conversion traits with blanket impls for `Option`, `Vec`, maps |
| `PyErr`, `PyResult` | `PsError`, `PsResult` | an error type the host understands, with `?` from `std::io::Error` |
| `abi3` | the append-only vtable with a size and version header | one binary works across host versions |
| `maturin` | `cargo pwrs` | a cargo subcommand that produces the artifact the host loads and runs the host-side tests |
| `PyBuffer` | `Pinned<'a, T>` | zero-copy access to a host-owned contiguous buffer |
| `pyo3::prepare_freethreaded_python` and embedding | `pwrs-host` | start the host runtime inside a Rust process |

## What had to change

- **There is no C API to bind.** PyO3 wraps `Python.h`; PWRS generates a managed shell and invents the C ABI behind it. The shell is real C# compiled by a fetched compiler, because PowerShell discovers cmdlets by reflection over CLR attributes and nothing else.
- **A function has a lifecycle.** A `#[pyfunction]` is called once per call. A cmdlet is constructed per invocation and runs `begin`, then `process` per pipeline record, then `end`; parameters can be rebound between records. PWRS binds on change and learns which phases a type leaves empty.
- **Arguments are bound by the host.** PyO3 extracts arguments from Python objects in Rust. PWRS lets PowerShell's binder coerce, validate and complete them against the CLR property types the generator declared, so the cmdlet behaves like a native one on the command line and in help.
- **Return is a stream.** A cmdlet writes zero or more objects as it goes; there is no single return value. `ps.write` is the whole output API and `Vec<T>` enumerates by default.
- **Errors are records, not exceptions.** `WriteError` with a category and an id is the PowerShell way to report a per-item failure without stopping; `ThrowTerminatingError` is the exception-like path. `Err` maps to the first by default.
- **Two runtimes, one build.** CPython is one interpreter. PowerShell is two hosts on two .NET runtimes, so the runtime assembly builds twice and a bootstrap script picks one at import.
- **The host's thread rule is stricter.** The GIL can be acquired from any thread. A cmdlet's streams are valid only on the thread running its phase, so the token is created per phase by the runtime and cannot be acquired.

## What PWRS has that PyO3 does not need

- Providers: a filesystem-like namespace (`New-PSDrive`, `Get-ChildItem`, `Get-Content`) from a Rust trait.
- Argument completers and dynamic parameters, because PowerShell has tab completion and parameter discovery as host features.
- Generated help and default views, because `Get-Help` and formatting are host features that read files beside the assembly.
- Hybrid C#, because the shell is source.
- `ShouldProcess` and `ShouldContinue`, because `-WhatIf` and `-Confirm` are host conventions.

## What PyO3 has that PWRS does not

- Part of `#[pymethods]`. `#[psmethods]` gives a proxy class instance methods, static methods, and a constructor a script reaches as `[Type]::new()`, which are PyO3's `#[pymethods]`, `#[staticmethod]` and `#[new]` with the receiver deciding which; what it does not give is methods on a copied or psobject class, which hold no Rust state to call into, or class methods that take the type as an argument.
- Subclassing host types, dunder protocols, and iterators as objects. The buffer protocol as a producer has its counterpart: `PsMemory<T>` hands a `Memory<T>` over Rust memory to the engine through `memory_view_new`.
- Async: PowerShell cmdlets are synchronous; there is no `async fn process`.
- Maturity and reach: PyO3 has years of use across thousands of crates, and PWRS is new.

## A borrowed decision: a tag, not a value type

PyO3 does not give you a sum type over every Python value. It gives you `PyAny` and `extract`, and lets the caller say what it expects. PWRS took the same line when a consumer asked for a decoded-value type: what shipped was `PsObject::type_tag()`, one crossing answering a `u32`, and not a `PsValue` enum.

The reason is the same reason PyO3 has none. The arms of such a type are policy, not mechanism: whether `Decimal` gets one, whether `Version` does, how deep a nested table nests. Those answers belong to the code decoding the values, and a consumer that needs a value tree writes the one its own wire format wants. What it cannot write for itself is a cheap way to ask what an object is, because that needs an entry in the vtable. So PWRS supplies the part only PWRS can, and leaves the shape to the caller.

## Is it a PyO3 for PowerShell?

For the part of PyO3 that is about the surface an author writes, yes: attribute on a struct, fields as parameters, a token to write output, conversions, a build tool, tests in the host. For the part of PyO3 that is about wrapping an existing C API, no, because that API does not exist; PWRS builds it, which is why it needs a generated managed shell, a compiler it fetches itself, and a runtime assembly compiled for two hosts.
