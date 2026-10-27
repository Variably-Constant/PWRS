---
title: PWRS and PyO3
weight: 5
---

Concept by concept: what is alike, what differs because PowerShell is not CPython, and what each has that the other lacks.

## What is alike

| PyO3 | PWRS | What they have in common |
|---|---|---|
| `#[pymodule]` with `PyInit_` | `export_module!` with the exports it emits | an entry point the host finds by name; here the host finds `pwrs_module_descriptor` at build time and `pwrs_module_init` at import |
| `#[pyfunction]` | `#[cmdlet]` | one attribute turns a Rust item into something the host can call |
| `#[pyclass]` | `#[psclass]` | one attribute turns a Rust struct into a type the host can see |
| `Py<T>` | `PsObject` | a reference to an object in the host's memory, which Rust code can store anywhere and which is released when it is dropped |
| `Python<'py>` | `Pipeline<'ps>` | a `!Send` value that code must have to call into the host; having PyO3's means the thread has the GIL, and having PWRS's means the code runs on the pipeline thread inside a phase |
| `FromPyObject`, `IntoPyObject` | `FromPs`, `IntoPs` | conversion traits that also cover `Option`, `Vec` and maps of the types they convert |
| `PyErr`, `PyResult` | `PsError`, `PsResult` | an error type the host understands, which `?` makes from a `std::io::Error` |
| `abi3` | the append-only vtable with a size and version header | one binary works across host versions |
| `maturin` | `cargo pwrs` | a command-line tool that builds a cargo project into what the host loads |
| `PyBuffer` | `Pinned<'a, T>`, from `PsObject::pin` or a `PsArrayMut<T>` argument | access to an array in the host's memory without copying it |
| a `#[pyclass]` that implements the buffer protocol | `PsMemory<T>` and `PsMemoryView<T>` | memory of the module's own that the host reads without copying it; Windows PowerShell 5.1 gets a copy in an array instead |
| `__len__`, `__getitem__`, `__setitem__`, `__iter__` with `__next__`, `__lt__`, `__eq__` with `__hash__` | `count`, `item`, `set_item`, `next`, `compare`, `equals` with `hash` on a proxy `#[psclass]` | methods that let the host index an object, loop over it, sort it and compare it by value; in PWRS each key makes the class implement the .NET interface PowerShell uses for that |
| `__add__`, `__sub__`, `__mul__`, `__truediv__`, `__mod__` | `add`, `sub`, `mul`, `div`, `rem` on a proxy or copied `#[psclass]` | methods behind the host's arithmetic operators; in PWRS each key makes the class declare the .NET operator that PowerShell's `+`, `-`, `*`, `/` or `%` calls. PowerShell runs unary minus as `0 - value`, so there is no `__neg__` |
| `#[pyclass(extends = Base)]`, and protocol methods such as `__format__` | `base = "System.IO.Stream"` and `implements = ["System.IFormattable"]` on a proxy `#[psclass]`, each member a method marked `#[member]` | a Rust type the host's own code takes as a subclass of a host class, or as implementing an interface; in PWRS the build reads the class and the interfaces from the reference assemblies and declares each member with its own signature |
| `Python::initialize` and embedding | `pwrs-host` | start the host runtime inside a Rust process |

## What differs

- **There is no C API to bind.** PyO3 wraps `Python.h`. PWRS generates a managed shell and defines its own C ABI behind it. The shell is real C#, compiled by a compiler PWRS downloads, because PowerShell finds cmdlets only by reading the attributes of .NET classes.
- **A function has a lifecycle.** A `#[pyfunction]` runs once per call. A cmdlet is created once per invocation and runs `begin`, then `process` once per pipeline record, then `end`, and its parameters can be bound again between records. PWRS sets a parameter again only when its value changed, and stops calling a phase that a cmdlet type leaves empty.
- **Arguments are bound by the host.** PyO3 extracts arguments from Python objects in Rust. PWRS lets PowerShell's own parameter binder convert, check and complete them for the .NET property types the generated shell declares, so the cmdlet behaves like one written in C#, on the command line and in help.
- **Return is a stream.** A cmdlet writes zero or more objects as it goes; there is no single return value. `ps.write` writes output, and writing a `Vec<T>` writes each of its elements.
- **Errors are records, not exceptions.** `WriteError` with a category and an id is how PowerShell reports that one item failed while the command goes on; `ThrowTerminatingError` stops the command, like an exception. A returned `Err` is written with `WriteError` unless it is marked terminating.
- **Two runtimes, one build.** CPython is one interpreter. PowerShell is two hosts on two .NET runtimes, so the runtime assembly is built twice and a script picks the right one at import.
- **The host's thread rule is stricter.** The GIL can be taken from any thread. A cmdlet can write to its streams only from the thread running its phase, so the runtime creates the token for each phase, and code cannot get one any other way.

## What PWRS has that PyO3 does not need

- Providers: a filesystem-like namespace (`New-PSDrive`, `Get-ChildItem`, `Get-Content`) from a Rust trait.
- Argument completers and dynamic parameters, because PowerShell has tab completion and parameter discovery as host features.
- Generated help and default views, because `Get-Help` and formatting are host features that read files beside the assembly.
- Hand-written C# cmdlets beside the Rust ones, because the shell is compiled from C# source.
- `ShouldProcess` and `ShouldContinue`, because `-WhatIf` and `-Confirm` are host conventions.

## What PyO3 has that PWRS does not

- Part of `#[pymethods]`. `#[psmethods]` gives a proxy class instance methods, static methods, and a constructor a script calls as `[Type]::new()`, like PyO3's `#[pymethods]`, `#[staticmethod]` and `#[new]`. A method that takes `self` is an instance method, and one that does not is static. A copied class can have static methods and a constructor but no instance methods, since a copied object has no Rust value behind it. A psobject class can have no methods. Neither has class methods that take the type as an argument.
- Conversion methods: no class declares `op_Implicit` or `op_Explicit`, the .NET counterparts of `__int__` and `__float__`, which a cast such as `[double]$value` would call.
- Buffers of any shape: a buffer PWRS passes is one-dimensional and contiguous, of one primitive element type, with no shape, strides or format.
- Async functions: a cmdlet's phases are synchronous, and there is no `async fn process`. A proxy method can return a .NET `Task` through `PsTask<T>`, which any thread can complete once.
- Maturity and reach: PyO3 has years of use across thousands of crates, and PWRS is new.

## Finding out what type an object is

Neither has one Rust type that can store any value of its host. In PyO3 you take a `&Bound<'py, PyAny>` and call `extract` with the type you expect. In PWRS you take a `PsObject` and convert it with `FromPs` to the type you expect. When you do not know the type in advance, `PsObject::type_tag()` returns a number that says what type the object is, with one call into the host. To turn objects of any type into your own data format, check that number and convert each type the way your format needs.

## Is it a PyO3 for PowerShell?

For the code a module author writes, yes: an attribute on a struct, fields as parameters, a token to write output, conversions, a build tool, and tests in the host. For wrapping an existing C API, no, because PowerShell has no such API. PWRS provides one, with a generated managed shell, a compiler it downloads itself, and a runtime assembly compiled for both hosts.
