---
title: Why PWRS
weight: 1
---

The problem PWRS solves and the parts it is made of. [PWRS and PyO3](Pwrs-And-PyO3.md) compares it with the binding that does the same job for Python.

## The problem PyO3 does not have

PyO3 works because CPython is a C library. `Python.h` gives an extension a `PyObject*`, reference counts, and a `PyInit_<name>` entry point the interpreter calls after it `dlopen`s the shared object. The binding crate's job is to make that C API pleasant from Rust.

PowerShell's extension surface is the .NET type system. A binary module is a managed assembly whose classes derive from `PSCmdlet`, have `[Cmdlet]` and `[Parameter]` attributes, and are discovered by reflection at `Import-Module`. The parameter binder reads attributes off properties; the help system reads MAML next to the assembly; providers are classes deriving from `NavigationCmdletProvider`; completers implement `IArgumentCompleter`. No native library is loaded anywhere on that path, and nothing in it can be expressed as a C function.

So a Rust binding for PowerShell has to supply two things CPython gives away:

1. **A managed shell**: real CLR types PowerShell can reflect over. PWRS generates them from the Rust source, and the work behind them happens in Rust.
2. **A native bridge**: a C ABI through which the shell calls Rust and Rust calls back into the engine. In PWRS it is a versioned table of function pointers that only grows at the end, plus a fixed set of exports.

Together they are the C API PowerShell never had, and everything PyO3 users expect is built on them: attribute macros, an object handle freed when it is dropped, a thread token, conversions, a build tool. [The Bridge](The-Bridge.md) describes both halves.

## No .NET SDK

The shell is C# that `cargo pwrs build` generates and compiles with the Roslyn `csc` it fetches from NuGet once (`Microsoft.Net.Compilers.Toolset`), run on the .NET runtime pwsh already ships, inside a pwsh process. A module's author needs Rust and PowerShell and nothing else. Because the shell is ordinary C# source, an author may add hand-written C# beside the Rust; see [How To Add Hybrid C#](How-To-Add-Hybrid-CSharp.md).

## One build for both hosts

Windows PowerShell 5.1 is still the default shell on every Windows machine and runs on .NET Framework; PowerShell 7.4 runs on .NET 8, 7.5 on .NET 9 and 7.6 on .NET 10. A module for both editions has to ship a `netstandard2.0` assembly for the first and a Core assembly for the second. PWRS compiles the runtime and the shell twice from the same sources, against `NETStandard.Library` and `PowerShellStandard.Library` for the desktop edition and against .NET 8's reference pack and the `System.Management.Automation` 7.4 reference for Core. The bootstrap script picks the folder at import. Both reference sets come from NuGet rather than from the pwsh doing the build, so a module built on any machine references the same assemblies, and one built on PowerShell 7.6 imports on 7.4. The Rust library is the same file for both. See [Two Hosts](Two-Hosts.md).

## The ABI is a table

The host table has a size and version header, as Python's `abi3` does. A module compiled against version 1 of the table still works when the runtime's table is longer. Entries are only added at the end, the module never uses an entry beyond the table it was built against, and an entry never changes meaning within a major version. Every entry is `extern "C"`, never unwinds, and reports failure through a status code and an exception handle. That is what lets modules built by different PWRS versions share one runtime assembly in a session, which Windows PowerShell needs, since there every PWRS module uses the runtime of the first one imported.

The table is the native half of that contract. The managed half is the set of runtime members a module's shell calls. On Windows PowerShell, when one of two modules in a session was built by `cargo-pwrs` 0.1.8 or earlier and the other by 0.2.0 or later, the second to be imported is refused if it declares classes or enums. [How To Fix A Failure](How-To-Fix-A-Failure.md#two-modules-in-one-windows-powershell-session) has the message and the fix.

## The engine's binder does the work

PWRS binds no parameter itself. Every Rust field type becomes the CLR property type that makes PowerShell's binder handle it correctly: `bool` to `SwitchParameter`, `Option<T>` to a non-mandatory `T`, `Vec<T>` to `T[]`, a `#[psenum]` to a real enum. `[ValidateRange]`, `[ValidateSet]`, `[ValidatePattern]`, `[ValidateNotNullOrEmpty]`, parameter sets, aliases, pipeline binding by value and by property name all come from the attributes the generator writes. So a PWRS cmdlet behaves exactly like a C# cmdlet on the command line, in `Get-Help`, and under tab completion.

## Errors are non-terminating by default

`Err` from a phase becomes `WriteError`, so `-ErrorAction`, `-ErrorVariable` and per-record error handling work the PowerShell way; `.terminating()` uses `ThrowTerminatingError` instead. A panic never leaves Rust: in a phase it is caught and raised as a terminating error with its message, and the host process survives.

## PWRS in use

The `hello` example in this repository is a tour of every mechanism, built to be read. For a module built to be used, [SubEtha](https://github.com/Variably-Constant/SubEtha) puts shared memory between processes on the PowerShell Gallery: rings, queues, channels, locks, atomics, counters and shared collections, each backed by a memory-mapped file that outlives the session that made it. SubEtha 0.6.0 is 139 cmdlets, 121 `#[psclass]` types and 14 `#[psenum]` types in one module, built for Windows x64, Linux x64, macOS arm64 and FreeBSD x64, on both hosts.

It is worth reading for the shapes an example cannot show at that size:

- a `New-` and an `Open-` cmdlet that pair up, so one process makes a thing and another attaches to it;
- an object that offers its operations as proxy methods instead of as more cmdlets;
- a lock that comes back as something `Release()`, `Dispose()` or garbage collection releases.

Bytes pass as a pinned `byte[]` rather than a copy.

Its manifest pins the published crate at an exact version, so it shows the surface of that release rather than of this one.
