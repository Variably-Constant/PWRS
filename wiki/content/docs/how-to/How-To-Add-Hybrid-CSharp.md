---
title: How To Add Hybrid C#
weight: 8
---

Hand-written C# beside the generated shell. Source: `crates/cargo-pwrs/src/build.rs`, the `hybrid` block.

## What is compiled

`cargo pwrs build` compiles every `.cs` file directly under `src/csharp/` in the crate into the shell assembly, together with the generated `<Module>.Shell.cs`, for both target frameworks. The files see the same references the shell sees: .NET 8's reference pack (`Microsoft.NETCore.App.Ref` 8.0.31) and the `System.Management.Automation` 7.4.0 reference for `net10.0`, whichever pwsh runs the build; and `NETStandard.Library` 2.0.3, `PowerShellStandard.Library` 5.1.1 and the .NET Framework builds of `System.Numerics.Vectors` 4.6.1, `System.Memory` 4.6.3, `System.Buffers` 4.6.1 and `System.Runtime.CompilerServices.Unsafe` 6.1.2 for `netstandard2.0`. `Pwrs.Runtime.dll` is referenced too. So hand-written C# gets .NET 8's API on PowerShell 7, and spans, `MemoryMarshal` and `Vector<T>` on Windows PowerShell.

Each build defines the preprocessor symbols the .NET SDK defines for its framework, so code guarded the way an SDK project guards it compiles the same way here. The pwsh half is net8.0: `NET`, `NETCOREAPP`, `NET8_0`, `NET5_0_OR_GREATER` through `NET8_0_OR_GREATER`, and `NETCOREAPP1_0_OR_GREATER` through `NETCOREAPP3_1_OR_GREATER`. The Windows PowerShell half is netstandard2.0: `NETSTANDARD`, `NETSTANDARD2_0`, and `NETSTANDARD1_0_OR_GREATER` through `NETSTANDARD2_0_OR_GREATER`. `NETFRAMEWORK` and `NET472` are not defined, since that half is a .NET Standard build; code a .NET Framework project keeps under them needs `NETSTANDARD2_0` added to its guard. The configuration symbols `DEBUG`, `RELEASE` and `TRACE` are not defined in either.

## A cmdlet in C#

```csharp
// src/csharp/GetHybrid.cs
using System.Management.Automation;

namespace MyModule
{
    [Cmdlet(VerbsCommon.Get, "Hybrid")]
    public sealed class GetHybridCommand : PSCmdlet
    {
        [Parameter(Mandatory = true, Position = 0)]
        public string Name { get; set; } = string.Empty;

        protected override void ProcessRecord() => WriteObject("hybrid " + Name);
    }
}
```

`Import-Module -Assembly` in the bootstrap `.psm1` registers every `[Cmdlet]` class in the shell assembly, so the cmdlet is discovered like the generated ones.

## Exporting it

`cargo pwrs build` reads the attributes of every class declared in the files under `src/csharp/`. A class carrying `[Cmdlet(verb, noun, ...)]` contributes `Verb-Noun` to the manifest's `CmdletsToExport` and the `.psm1`'s `Export-ModuleMember -Cmdlet`, after the Rust cmdlets, and every name in an `[Alias(...)]` on the same class joins `AliasesToExport` and `Export-ModuleMember -Alias`. The alias matters because the engine creates a cmdlet's alias members inside the nested binary module, so the root module has to name them or they never reach the session.

The verb and noun are read as string literals (`"Get"`) or as `Verbs*.Name` constants (`VerbsCommon.Get`); named arguments after them are ignored. Only a class's own attribute list is read, so an `[Alias]` on a parameter stays a parameter alias and never becomes a cmdlet alias, and an attribute inside a comment or a string literal is not read at all. `[Alias]` may sit before or after `[Cmdlet]`, or beside it in one bracket group.

A `[Cmdlet]` attribute the scan cannot attach to a class declaration is counted and named on standard error during the build, with the file it is in. The class still compiles into the shell assembly; what it loses is the manifest entry, so the cmdlet exists in the assembly and is not exported. The build does not fail on it.

The hello example carries three of these. `Get-RustHybrid` declares the alias `grhyb` and gives its `-Name` parameter the alias `n`. `Get-RustHybridInfo` declares the same two attributes in the other accepted forms: the alias ahead of the cmdlet attribute, the cmdlet attribute fully qualified, and the alias names as an array. `Hybrid.Tests.ps1` checks in both hosts that each cmdlet runs, that each and its alias are exported by the manifest and the module, that `grhyb -n y` binds through both aliases, that `n` is not exported as a cmdlet alias, and that the two cmdlets appear in the manifest in file-name order. The third, `Measure-RustHybridDot`, is the vector example below.

## Hardware vectors on both hosts

`System.Numerics.Vector<T>` is the one hardware vector type hand-written C# can use on both hosts, because `System.Runtime.Intrinsics` does not exist on .NET Framework, and spans are how C# walks an array in `Vector<T>`-sized steps. PowerShell 7 carries both; .NET Framework carries neither. So a module with any hand-written C# ships the .NET Framework builds of the four packages named above in its `netstandard2.0` folder, the same builds that half is compiled against, and on Windows PowerShell the `.psm1` copies them beside the staged shell, where .NET Framework looks for a shell's dependencies. Each package's .NET Standard build carries a lower assembly version than its .NET Framework build (`System.Memory` 4.0.2.0 against 4.0.5.0), and .NET Framework binds only the version an assembly references, which is why the compile references the .NET Framework builds.

`System.Runtime.Intrinsics` (`Vector256`, `Avx2` and the rest) is for the pwsh half alone. Put it under `#if NET8_0_OR_GREATER` with a `Vector<T>` path after it, the way a .NET SDK project targeting both frameworks does: the pwsh half compiles both paths and the Windows PowerShell half compiles the second.

`Measure-RustHybridDot` in the hello example is that shape. It takes the dot product of two `int[]` through `Vector256<int>` when the build defines `NET8_0_OR_GREATER` and the hardware accelerates it, otherwise through `Vector<int>` over spans cast with `MemoryMarshal.Cast`, then a scalar loop for whatever the lanes leave over. It answers the sum, the path it took (`Vector256`, `Vector` or `Scalar`), that path's lane count, and whether the intrinsics path was compiled in. `Hybrid.Tests.ps1` checks the sum in both hosts, and that the intrinsics path is compiled into the pwsh half and not into the Windows PowerShell half.

## Sharing the runtime

Hybrid code may use `Pwrs.Native`, `Pwrs.PsStr16` and the other public runtime types, since the shell references `Pwrs.Runtime.dll`. The methods that call into the Rust library are `internal` to `Pwrs.Runtime` and are not reachable from hybrid code; a hybrid cmdlet talks to Rust by invoking a Rust cmdlet, not by calling exports itself.
