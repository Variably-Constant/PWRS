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
| `__int__`, `__float__`, `__index__` | `implicit`, `explicit` and `converter` on a proxy or copied `#[psclass]` | methods the host calls to convert an object; in PWRS `implicit` and `explicit` make the class declare .NET conversion operators, from or to any one type, and `converter` a type converter for any value and any type, which casts and the parameter binder call |
| `#[pyclass(subclass)]` | `inheritable` on a proxy or copied `#[psclass]` | a host class may derive from the Rust type and override its methods; in PWRS the class is unsealed, its methods virtual, and a proxy class has a protected constructor whose Rust value is its `Default` |
| `#[pyclass(extends = Base)]`, and protocol methods such as `__format__` | `base = "System.IO.Stream"` and `implements = ["System.IFormattable"]` on a proxy `#[psclass]`, each member a method marked `#[member]` | a Rust type the host's own code takes as a subclass of a host class, or as implementing an interface; in PWRS the build reads the class and the interfaces from the reference assemblies and declares each member with its own signature. PyO3's base is a class defined in Rust or a builtin, and PWRS's any .NET class that is not sealed, as [the section below](#what-pwrs-does-that-pyo3-has-no-counterpart-for) says |
| `Python::initialize` and embedding | `pwrs-host` | start the host runtime inside a Rust process |

## What differs

- **There is no C API to bind.** PyO3 wraps `Python.h`. PWRS generates a managed shell and defines its own C ABI behind it. The shell is real C#, compiled by a compiler PWRS downloads, because PowerShell finds cmdlets only by reading the attributes of .NET classes.
- **A function has a lifecycle.** A `#[pyfunction]` runs once per call. A cmdlet is created once per invocation and runs `begin`, then `process` once per pipeline record, then `end`, and its parameters can be bound again between records. PWRS sets a parameter again only when its value changed, and stops calling a phase that a cmdlet type leaves empty.
- **Arguments are bound by the host.** PyO3 extracts arguments from Python objects in Rust. PWRS lets PowerShell's own parameter binder convert, check and complete them for the .NET property types the generated shell declares, so the cmdlet behaves like one written in C#, on the command line and in help.
- **Return is a stream.** A cmdlet writes zero or more objects as it goes; there is no single return value. `ps.write` writes output, and writing a `Vec<T>` writes each of its elements.
- **Errors are records, not exceptions.** `WriteError` with a category and an id is how PowerShell reports that one item failed while the command goes on; `ThrowTerminatingError` stops the command, like an exception. A returned `Err` is written with `WriteError` unless it is marked terminating.
- **Two runtimes, one build.** CPython is one interpreter. PowerShell is two hosts on two .NET runtimes, so the runtime assembly is built twice and a script picks the right one at import.
- **The host's thread rule is stricter.** The GIL can be taken from any thread. A cmdlet can write to its streams only from the thread running its phase, so the runtime creates the token for each phase, and code cannot get one any other way.

## What PWRS does that PyO3 has no counterpart for

Each row is checked against PyO3's own guide.

| What PWRS does | Why PyO3 has no counterpart | Where |
|---|---|---|
| A class derives from a .NET class that is not sealed, such as `System.IO.Stream`, `System.Exception` or `System.Reflection.Binder`, and overrides its virtual and abstract members with Rust methods | a `#[pyclass]` extends only classes defined in Rust and the builtins PyO3 provides, a native type such as `PyDict` not when building for the limited API, and inheriting a class defined in Python is not supported ([PyO3's guide, Inheritance](https://pyo3.rs/latest/class.html#inheritance), [issue 991](https://github.com/PyO3/pyo3/issues/991)) | [How To Return Objects](../how-to/How-To-Return-Objects.md#a-net-base-class-and-net-interfaces) |
| A member taking arguments passed `out`, `ref` or `in`, or a generic one, is backed by a Rust method that returns the out arguments after its result, takes a ref argument as `&mut T` and the type arguments as `&[PsType]`; a member the class does not need throws `NotSupportedException` through `stubs` | Python has no out, ref or in arguments, and its generics are annotations, with no type arguments passed at a call; a `#[pyclass]` cannot inherit an abstract base class defined in Python, such as `collections.abc.Mapping`, whose members a stub would stand in for | [How To Return Objects](../how-to/How-To-Return-Objects.md#out-ref-and-generic-members-and-stubs) |
| A class declares and raises .NET events, its own and its interfaces' (`INotifyPropertyChanged`, `ICommand`), from any thread, and Rust handles any .NET object's events, on the raising thread or through the runspace's event queue in the order raised | Python's object model has no event member, so a `#[pyclass]` has none to declare or raise, and PyO3 none to subscribe to | [How To Raise And Handle Object Events](../how-to/How-To-Raise-And-Handle-Object-Events.md) |
| `#[pwrs::tiered]` compiles a function body for each instruction-set tier and runs the copy the CPU supports, `pwrs::simd` has kernels built that way, and import refuses a library compiled for instruction sets the CPU lacks, naming them | neither PyO3's guide nor maturin's documentation describes compiling for each CPU tier, choosing code by the CPU at run time or checking the CPU at import, so an extension compiled for instructions the CPU lacks crashes with an illegal instruction when it runs one ([PyO3's guide, Building and distribution](https://pyo3.rs/latest/building-and-distribution.html), [maturin, Distribution](https://www.maturin.rs/distribution.html)) | [How To Use Instruction Sets](../how-to/How-To-Use-Instruction-Sets.md) |

## What PWRS has that PyO3 does not need

- Providers: a filesystem-like namespace (`New-PSDrive`, `Get-ChildItem`, `Get-Content`) from a Rust trait.
- Argument completers and dynamic parameters, because PowerShell has tab completion and parameter discovery as host features.
- Generated help and default views, because `Get-Help` and formatting are host features that read files beside the assembly.
- Hand-written C# cmdlets beside the Rust ones, because the shell is compiled from C# source.
- [`ShouldProcess` and `ShouldContinue`](../how-to/How-To-Write-A-Cmdlet.md#confirmation) in every form the engine has, Yes to All among them, because `-WhatIf` and `-Confirm` are host conventions.
- [Jobs](../how-to/How-To-Run-Work-As-A-Job.md): Rust work that `Get-Job`, `Receive-Job` and `Stop-Job` work on, and `-AsJob`, because jobs are a host feature.
- [Transactions](../how-to/How-To-Write-A-Cmdlet.md#transactions) for a cmdlet and a provider, in Windows PowerShell 5.1, because transactions are a host feature.
- [Paging](../how-to/How-To-Write-A-Cmdlet.md#paging) with `-First`, `-Skip` and `-IncludeTotalCount`, [an argument checked by a Rust function](../how-to/How-To-Write-A-Cmdlet.md#checking-an-argument-in-rust) before the cmdlet runs, and [`[Credential()]`](../how-to/How-To-Write-A-Cmdlet.md#prompting-the-person), because parameter binding is the host's.
- [Wrapping another command](../how-to/How-To-Write-A-Cmdlet.md#wrap-another-command), record by record or as a proxy of it, because steppable pipelines and proxy commands are host features.
- [.NET attributes](../how-to/How-To-Write-A-Cmdlet.md#net-attributes) on cmdlets, parameters, classes and their members, because PowerShell acts on the attributes of a cmdlet and its parameters.
- [COM objects](../how-to/How-To-Call-DotNet-From-Rust.md#com-objects) and [their events](../how-to/How-To-Raise-And-Handle-Object-Events.md#a-com-objects-events), because `New-Object -ComObject` is built into PowerShell.

## What PyO3 has that PWRS does not

- Part of `#[pymethods]`. `#[psmethods]` gives a proxy class instance methods, static methods, and a constructor a script calls as `[Type]::new()`, like PyO3's `#[pymethods]`, `#[staticmethod]` and `#[new]`. A method that takes `self` is an instance method, and one that does not is static. A copied class can have static methods and a constructor but no instance methods, since a copied object has no Rust value behind it. A psobject class can have no methods. Neither has class methods that take the type as an argument.
- Buffers of any shape: a buffer PWRS passes is one-dimensional and contiguous, of one primitive element type, with no shape, strides or format.
- Async functions: a cmdlet's phases are synchronous, and there is no `async fn process`. A proxy method can return a .NET `Task` through `PsTask<T>`, which any thread can complete once.
- Maturity and reach: PyO3 has years of use across thousands of crates, and PWRS is new.

## Finding out what type an object is

Neither has one Rust type that can store any value of its host. In PyO3 you take a `&Bound<'py, PyAny>` and call `extract` with the type you expect. In PWRS you take a `PsObject` and convert it with `FromPs` to the type you expect. When you do not know the type in advance, `PsObject::type_tag()` returns a number that says what type the object is, with one call into the host. To turn objects of any type into your own data format, check that number and convert each type the way your format needs.

## Is it a PyO3 for PowerShell?

For the code a module author writes, yes: an attribute on a struct, fields as parameters, a token to write output, conversions, a build tool, and tests in the host. For wrapping an existing C API, no, because PowerShell has no such API. PWRS provides one, with a generated managed shell, a compiler it downloads itself, and a runtime assembly compiled for both hosts.
