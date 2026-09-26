---
title: Why PWRS
weight: 1
---

The problem PWRS solves, the ways it could have been solved, and the one it chose.

## The problem PyO3 does not have

PyO3 works because CPython is a C library. `Python.h` gives an extension a `PyObject*`, reference counts, and a `PyInit_<name>` entry point the interpreter calls after it `dlopen`s the shared object. The binding crate's job is to make that C API pleasant from Rust.

PowerShell's extension surface is the .NET type system. A binary module is a managed assembly whose classes derive from `PSCmdlet`, carry `[Cmdlet]` and `[Parameter]` attributes, and are discovered by reflection at `Import-Module`. The parameter binder reads attributes off properties; the help system reads MAML next to the assembly; providers are classes deriving from `NavigationCmdletProvider`; completers implement `IArgumentCompleter`. No native library is loaded anywhere on that path, and nothing in it can be expressed as a C function.

So a Rust binding for PowerShell has to supply two things CPython gives away:

1. **A managed shell**: real CLR types PowerShell can reflect over. In PWRS they are generated from the Rust source and hold no logic.
2. **A native bridge**: a C ABI through which the shell calls Rust and Rust calls back into the engine. In PWRS it is a versioned, append-only table of function pointers plus a fixed set of exports.

Together they are the C API PowerShell never had, and everything PyO3 users expect sits on top: attribute macros, an owned object handle, a thread token, conversions, a build tool.

## How the shell could be produced

Four mechanisms were considered.

- **Generated C# compiled by a full .NET SDK.** It works, and it makes the SDK a dependency of every module author, which is the burden PyO3 removed from Python extension authors. Rejected as the default.
- **Runtime `Reflection.Emit`.** No toolchain at all, but the module then needs a script entry point to run the emitter, the types are invisible to static tooling and to `Get-Help` before import, and every import pays code generation. Rejected.
- **Rust-side ECMA-335 emission.** Pure, but a metadata writer to own and maintain, and unnecessary once compilation without an SDK is solved another way. Rejected.
- **A mixed-mode single PE.** Windows-only on .NET Core. Rejected.

The chosen mechanism: generated C#, compiled by the Roslyn `csc` that `cargo pwrs` fetches from NuGet once (`Microsoft.Net.Compilers.Toolset`) and runs on the .NET runtime pwsh already ships, inside a pwsh process. Module authors need Rust and PowerShell and nothing else. Because the shell is ordinary C# source, an author may add hand-written C# beside the Rust.

## Why two hosts from one build

Windows PowerShell 5.1 is still the default shell on every Windows machine and runs on .NET Framework; PowerShell 7.4 runs on .NET 8, 7.5 on .NET 9 and 7.6 on .NET 10. A module that serves both editions has to ship a `netstandard2.0` assembly for the first and a Core assembly for the second. PWRS compiles the runtime and the shell twice from the same sources, against `NETStandard.Library` and `PowerShellStandard.Library` for the desktop edition and against .NET 8's reference pack and the `System.Management.Automation` 7.4 reference for Core, and the bootstrap script picks the folder at import. Both reference sets come from NuGet rather than from the pwsh doing the build, so a module built on any machine references the same assemblies, and one built on PowerShell 7.6 imports on 7.4. The Rust library is the same file for both. See [Two Hosts](Two-Hosts.md).

## Why the ABI is a table

A function table with a size and version header is the shape `abi3` proved: a module compiled against version 1 of the table keeps working when the runtime grows the table, because it never touches an entry past the size it was built for, and an entry never changes meaning within a major version. Every entry is `extern "C"`, never unwinds, and reports failure through a status code and an exception handle. That is what lets one runtime assembly serve modules built by different PWRS versions in one session, which Windows PowerShell asks of it, since there the first PWRS module's runtime serves every module after it. The table is the native half of that contract. The managed half, the runtime members a module's shell calls, changed in 0.2.0 for modules that declare classes or enums, and on Windows PowerShell such a module does not import after one built on the other side of that release; see [How To Fix A Failure](../how-to/How-To-Fix-A-Failure.md).

## Why the engine's binder does the work

PWRS does not parse, coerce, validate or complete parameter values. Every Rust field type lowers to the CLR property type that makes the engine's binder do the right thing: `bool` to `SwitchParameter`, `Option<T>` to a non-mandatory `T`, `Vec<T>` to `T[]`, a `#[psenum]` to a real enum. `[ValidateRange]`, `[ValidateSet]`, `[ValidatePattern]`, `[ValidateNotNullOrEmpty]`, parameter sets, aliases, pipeline binding by value and by property name all come from the attributes the generator writes, so a PWRS cmdlet behaves exactly like a C# cmdlet on the command line, in `Get-Help`, and under tab completion.

## Why errors are non-terminating by default

`Err` from a phase becomes `WriteError`, so `-ErrorAction`, `-ErrorVariable` and per-record error handling work the PowerShell way; `.terminating()` opts into `ThrowTerminatingError`. A panic is always terminating and never crosses the boundary: it is caught, reported with its message, and the host process survives.

## What is decided

Fifty decisions are locked. The first twenty settle the shape: the name and license, the two hosts, no SDK, zero-reflection marshalling with one native call per phase, fat packaging under `runtimes/<rid>/native`, `Vec<T>` enumerating and `PsArray<T>` opting out, non-terminating errors by default, cooperative cancellation, std threads in core with `par_map` over owned data and Flynnel behind a feature, the three class modes, dynamic .NET access, enums as CLR enums, providers, completers, dynamic parameters, format files and publishing in version one, MAML help from doc comments, the three test layers, the frozen bootstrap and per-module `AssemblyLoadContext`, the append-only ABI, the gated platforms, and a fair bench for every primitive.

The eight since then settle the surface: the typed value crossings for `DateTime`, `TimeSpan`, `Guid`, `char`, `SecureString` and `PSCredential`; methods on proxy classes; one provider instance per drive; classes as parameter and field types; `#[psfield(skip)]` for state with no PowerShell face; reaching the proxy base through `base.PwrsGet` and `base.PwrsCall` so a module may name a method anything; one pin in each direction for a `Vec` of a primitive; and `#[param(raw)]` for an argument the engine's binder should not coerce.

The seven after those settle the edit loop: a reload loads the new library beside the old and frees nothing; the load a value came from is counted on the managed side and checked before that value crosses; nothing is ever unloaded, shell or native, and a rebuilt shell takes the command over by carrying a name of its own; both result orders are offered and chosen per call; that name is stamped on the assembly rather than the namespace, because a binder failure names the type and scripts match on it; every load is taken from a copy outside the module folder, so a build can rewrite a module a session is holding; and the copy of the library is named after the file it came from rather than a count of reloads. See [How To Reload A Module](../how-to/How-To-Reload-A-Module.md).

The four after those settle the conversion surface. A scalar arrives as the CLR type of the Rust type it was given rather than widened to `Int64` or `Double`, because the engine types every operator's answer by its operands' widths; that costs a handle for every width but the three the direct write entries build. An object's own type can be asked for as a tag in one crossing, so code dispatching on a type it does not know at compile time need not read `GetType().FullName` and compare strings. A `System.Decimal` crosses as a scalar and a `Decimal[]` pins as a block, with the four words named in both the order `Decimal.GetBits` reports and the order they sit in memory, and the first pin in a process proving the second against the first. And `DateTimeOffset` joins the typed crossings, because a `DateTime`'s kind says only which clock a value belongs to.

The eight most recent settle what a cmdlet can say at the edges of a call. A module may declare a function that runs at import and one at removal, because nothing is unloaded and state held across calls has nowhere else to be released. A `pub fn` with no receiver is a static on the generated class and `new` is its constructor, so a value type needs no `New-` cmdlet to come into being. A cmdlet composes with another command by name, resolved and run in the current runspace rather than through a script block nobody needed to parse. The host's own prompts are typed, with no vtable entry added, for the question the engine has no parameter for. An `ErrorRecord` is read as a typed value and only read, since a cmdlet raises by returning an `Err` and the engine builds the record. `cargo pwrs test` reports what the suite left unexercised, because `SupportsShouldProcess` and `ValueFromPipeline` are promises the engine keeps whether or not the body does. An argument may be transformed before the binder coerces it, which is the only point at which a refusal can name the parameter. And a Rust enum may mirror a CLR enum that already exists, declaring nothing, so the parameter is the real type.

The three after those settle what two modules, and two commands, can rely on about each other. A copied class takes a constructor, so a value type no longer has to become a proxy, with every property read crossing the boundary, just to be made from script; declaring one removes the public constructor that fills CLR zeros, and the factory builds through a constructor of its own that no Rust signature can reach, so a declared `new()` cannot call itself through it. Each module owns its class factories, because a class id is only a position in one module's list: on Windows PowerShell, where one runtime serves every module in the process, each module's library is handed its own copy of the host table, which needs no change to the ABI or to the frozen bootstrap, where renaming the runtime per build would have needed both. And a cmdlet can read its own `InvocationInfo`, its place in the pipeline included, so cooperating commands can tell their neighbors are their own.

What was declined is part of the same decision: PWRS does not ship a sum type over every PowerShell value. See [PWRS and PyO3](Pwrs-And-PyO3.md).

## PWRS in use

The `hello` example in this repository is a tour of every mechanism, built to be read. For a module built to be used, [SubEtha](https://github.com/Variably-Constant/SubEtha) puts shared memory between processes on the PowerShell Gallery: rings, queues, channels, locks, atomics, counters and shared collections, each backed by a memory-mapped file that outlives the session that made it. It is 135 cmdlets, 117 `#[psclass]` types and 14 `#[psenum]` types in one module, Windows x64 and Linux x64 together, on both hosts.

It is worth reading for the shapes an example cannot show at that size: how a `New-` and an `Open-` cmdlet pair up so one process makes a thing and another attaches to it, how an object hands back its operations as proxy methods rather than as more cmdlets, and how a lock comes back as something that releases on `Release()`, `Dispose()` or collection. Bytes cross as a pinned `byte[]` rather than a copy.

It is built against the published crate at the version its own lock file names, so it shows the surface of that release rather than of this one.
