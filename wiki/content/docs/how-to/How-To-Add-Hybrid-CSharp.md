---
title: How To Add Hybrid C#
weight: 8
---

Hand-written C# beside the generated shell. Source: `crates/cargo-pwrs/src/build.rs`, the `hybrid` block.

## What is compiled

`cargo pwrs build` compiles every `.cs` file directly under `src/csharp/` in the crate into the shell assembly, together with the generated `<Module>.Shell.cs`, for both target frameworks. The files see the same references the shell sees: .NET 8's reference pack (`Microsoft.NETCore.App.Ref` 8.0.31) and the `System.Management.Automation` 7.4.0 reference for `net10.0`, whichever pwsh runs the build; and `NETStandard.Library` 2.0.3, `PowerShellStandard.Library` 5.1.1 and the .NET Framework builds of `System.Numerics.Vectors` 4.6.1, `System.Memory` 4.6.3, `System.Buffers` 4.6.1 and `System.Runtime.CompilerServices.Unsafe` 6.1.2 for `netstandard2.0`. `Pwrs.Runtime.dll` is referenced too. So hand-written C# gets .NET 8's API on PowerShell 7, and spans, `MemoryMarshal` and `Vector<T>` on Windows PowerShell. A module that names [`references`](#windows-forms-wpf-and-other-desktop-assemblies) compiles its `netstandard2.0` shell against the .NET Framework 4.7.2 reference assemblies in place of `NETStandard.Library`'s.

Each build defines the preprocessor symbols the .NET SDK defines for its framework, so code guarded the way an SDK project guards it compiles the same way here. The pwsh half is net8.0: `NET`, `NETCOREAPP`, `NET8_0`, `NET5_0_OR_GREATER` through `NET8_0_OR_GREATER`, and `NETCOREAPP1_0_OR_GREATER` through `NETCOREAPP3_1_OR_GREATER`. The Windows PowerShell half is netstandard2.0: `NETSTANDARD`, `NETSTANDARD2_0`, and `NETSTANDARD1_0_OR_GREATER` through `NETSTANDARD2_0_OR_GREATER`. `NETFRAMEWORK` and `NET472` are not defined, even when `references` has that half's shell compiled against .NET Framework, so naming references changes no guard; code a .NET Framework project keeps under them needs `NETSTANDARD2_0` added to its guard. The configuration symbols `DEBUG`, `RELEASE` and `TRACE` are not defined in either.

A debug build, and a release build with `--debug-symbols`, writes a `.pdb` beside each assembly. The generated sources and the module folder are recorded in it under the fixed names `/pwrs/work` and `/pwrs/module`, so the runtime assemblies a bundled module compiles are the same bytes as the bundling module's; the files under `src/csharp/` keep their own paths, which is where a debugger finds them.

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

The hello example carries four of these. `Get-RustHybrid` declares the alias `grhyb` and gives its `-Name` parameter the alias `n`. `Get-RustHybridInfo` declares the same two attributes in the other accepted forms: the alias ahead of the cmdlet attribute, the cmdlet attribute fully qualified, and the alias names as an array. `Hybrid.Tests.ps1` checks in both hosts that each cmdlet runs, that each and its alias are exported by the manifest and the module, that `grhyb -n y` binds through both aliases, that `n` is not exported as a cmdlet alias, and that the two cmdlets appear in the manifest in file-name order. The third, `Measure-RustHybridDot`, is the vector example below, and the fourth, `Get-RustHybridDesktop`, the desktop example after it.

## Hardware vectors on both hosts

`System.Numerics.Vector<T>` is the one hardware vector type hand-written C# can use on both hosts, because `System.Runtime.Intrinsics` does not exist on .NET Framework, and spans are how C# walks an array in `Vector<T>`-sized steps. PowerShell 7 carries both; .NET Framework carries neither. So a module with any hand-written C# ships the .NET Framework builds of the four packages named above in its `netstandard2.0` folder, the same builds that half is compiled against, and on Windows PowerShell the `.psm1` copies them beside the staged shell, where .NET Framework looks for a shell's dependencies. Each package's .NET Standard build carries a lower assembly version than its .NET Framework build (`System.Memory` 4.0.2.0 against 4.0.5.0), and .NET Framework binds only the version an assembly references, which is why the compile references the .NET Framework builds.

`System.Runtime.Intrinsics` (`Vector256`, `Avx2` and the rest) is for the pwsh half alone. Put it under `#if NET8_0_OR_GREATER` with a `Vector<T>` path after it, the way a .NET SDK project targeting both frameworks does: the pwsh half compiles both paths and the Windows PowerShell half compiles the second.

`Measure-RustHybridDot` in the hello example is that shape. It takes the dot product of two `int[]` through `Vector256<int>` when the build defines `NET8_0_OR_GREATER` and the hardware accelerates it, otherwise through `Vector<int>` over spans cast with `MemoryMarshal.Cast`, then a scalar loop for whatever the lanes leave over. It answers the sum, the path it took (`Vector256`, `Vector` or `Scalar`), that path's lane count, and whether the intrinsics path was compiled in. `Hybrid.Tests.ps1` checks the sum in both hosts, and that the intrinsics path is compiled into the pwsh half and not into the Windows PowerShell half.

## Windows Forms, WPF and other desktop assemblies

`references` under `[package.metadata.pwrs]` names assemblies the shell's compile references beyond its set, each resolved for each half on its own:

```toml
[package.metadata.pwrs]
references = ["System.Windows.Forms"]
```

The pwsh half takes a name from `Microsoft.WindowsDesktop.App.Ref` 8.0.31, the reference pack of the Windows desktop framework at the version of .NET 8's own pack, and an assembly from it replaces one of the same name in .NET 8's pack, as its `WindowsBase` replaces the smaller one there. The Windows PowerShell half finds it in the .NET Framework 4.7.2 reference assemblies, `Microsoft.NETFramework.ReferenceAssemblies.net472` 1.0.3, and once a module names any, that half's shell compiles against the whole of that set in place of netstandard 2.0's: every assembly its `FrameworkList.xml` lists, its facades, PowerShell Standard and the four .NET Framework builds above. The facades include `netstandard.dll`, through which the shell uses the netstandard2.0 `Pwrs.Runtime.dll` beside it; the runtime and the bootstrap compile against netstandard 2.0's set whatever a module names. So a module that names `references` needs .NET Framework 4.7.2 or later under Windows PowerShell. Windows 10 from version 1803 and Windows Server 2019 ship it or a later one, and it installs on Windows 7 SP1, Windows 8.1, earlier Windows 10 and Windows Server 2008 R2 SP1 through 2016. A module that names none compiles that half against netstandard 2.0's set. The build fetches both packages the first time a module names any and checks them against the toolchain's lock like its other packages. They are references only, so nothing is shipped: the host carries the assemblies, and in the hello example pwsh 7.6.6 bound `System.Windows.Forms` 10.0.0.0 and Windows PowerShell 5.1 bound 4.0.0.0.

The build prints which half took each name. A name found for one half only compiles into that half. `System.Drawing.Common` is one: pwsh keeps `Bitmap`, `Font` and `Icon` there, and .NET Framework has no assembly of that name, since it keeps them in `System.Drawing`, which the set the Windows PowerShell half compiles against already holds. So naming `System.Drawing.Common` is what lets code using `Bitmap` compile into both halves, with no `#if`. A name neither set carries stops the build, naming the key and the name.

In pwsh 7.6.6 and Windows PowerShell 5.1, `Screen.PrimaryScreen.Bounds` and `SystemInformation` through `System.Windows.Forms`, and `SystemParameters` and `Colors` through `PresentationFramework`, `PresentationCore` and `WindowsBase`, compiled into both halves and ran.

The packages are NuGet downloads, so every build host compiles the same shell, and a module built on Linux carries the bytes one built on Windows does. No host off Windows carries the desktop assemblies: there the module imports, and a call into code using one throws, naming it. In pwsh 7.6.6 on Linux the error was `Could not load file or assembly 'System.Windows.Forms, Version=8.0.0.0, ...'`.

The hello example's `Get-RustHybridDesktop` names `System.Windows.Forms` and `System.Drawing.Common`, and answers `SystemInformation.ComputerName`, the version of the Windows Forms assembly the host bound, and a pixel it set in a 4 by 3 `Bitmap` and read back, with the name of the assembly `Bitmap` came from, without showing a window. `Hybrid.Tests.ps1` checks each on Windows in both hosts, `System.Drawing.Common` in pwsh 7.6.6 and `System.Drawing` in Windows PowerShell 5.1, and elsewhere that the call fails naming Windows Forms. The bitmap is drawn in a method of its own, so a host without Windows Forms reports that assembly rather than whichever one the JIT compiler reaches first.

## Sharing the runtime

Hybrid code may use `Pwrs.Native`, `Pwrs.PsStr16` and the other public runtime types, since the shell references `Pwrs.Runtime.dll`. The methods that call into the Rust library are `internal` to `Pwrs.Runtime` and are not reachable from hybrid code; a hybrid cmdlet talks to Rust by invoking a Rust cmdlet, not by calling exports itself.
