---
title: Two Hosts
weight: 4
---

How one `cargo pwrs build` serves PowerShell 7 from 7.4 on and Windows PowerShell 5.1 on .NET Framework. Source: `crates/cargo-pwrs/src/build.rs` (the two compiles), `generate.rs` (`bootstrap_psm1`, `manifest`), `crates/cargo-pwrs/dotnet/Pwrs.Bootstrap/Loader.cs`, and the `#if NET` branches in `crates/cargo-pwrs/dotnet/Pwrs.Runtime/*.cs`.

## Two compiles from one source

The runtime, the bootstrap and the generated shell are compiled twice:

| Folder | Framework | References | Defines |
|---|---|---|---|
| `net10.0/` | .NET 8 references, whichever pwsh builds it | `Microsoft.NETCore.App.Ref` 8.0.31 and the `System.Management.Automation` 7.4.0 reference from NuGet | the .NET SDK's net8.0 set: `NET`, `NETCOREAPP`, `NET8_0`, and the `OR_GREATER` symbols up to `NET8_0_OR_GREATER` and `NETCOREAPP3_1_OR_GREATER` |
| `netstandard2.0/` | .NET Framework through .NET Standard 2.0 | `NETStandard.Library` 2.0.3 and `PowerShellStandard.Library` 5.1.1 from NuGet, and the .NET Framework builds of `System.Numerics.Vectors` 4.6.1, `System.Memory` 4.6.3, `System.Buffers` 4.6.1 and `System.Runtime.CompilerServices.Unsafe` 6.1.2 | the .NET SDK's netstandard2.0 set: `NETSTANDARD`, `NETSTANDARD2_0`, and the `OR_GREATER` symbols up to `NETSTANDARD2_0_OR_GREATER` |

The Rust library is the same file for both; nothing in the ABI depends on the host.

`net10.0/` is a folder name, not a compile target. The Core compile is `/nostdlib+` against .NET 8's reference pack and the `System.Management.Automation` 7.4.0 reference, both fetched from NuGet into the toolchain as the `netstandard2.0` packages are, so every machine that builds a module emits the same references, `System.Runtime` 8.0.0.0 and `System.Management.Automation` 7.4.0.0 among them, whichever pwsh ran the build. A host binds an assembly only when its own copy is at least the version referenced. The `System.Management.Automation` reference is 7.4.0's because that package's assembly version is 7.4.0.0; the 7.4.20 package's is 7.4.6.500, the version PowerShell 7.4.20 itself carries. The runtime's sources branch on `NET` alone; the other symbols are there for hand-written C#.

That is what the folder name buys and what it does not. It tells the `.psm1` where to look for the Core assembly. What decides which .NET that assembly needs is the reference pack. On a PowerShell 7 whose .NET is older than 8 the loader cannot be reached, and the `.psm1` reports that as `<Module> needs PowerShell 7.4 or later: its PowerShell 7 half is built against .NET 8, and this is PowerShell <version> on .NET <version>.` The check sits where the loader call fails, so an import that succeeds pays nothing for it. The runtime and a module's hand-written C# can use only the API .NET 8 and PowerShell 7.4 have; a call to anything newer fails to compile on every machine, rather than building on one and not loading on another.

## What differs under `#if NET`

- **Managed-to-native calls.** Function pointers (`delegate* unmanaged`) on .NET; delegates from `Marshal.GetDelegateForFunctionPointer` on .NET Framework, which has no function-pointer syntax.
- **Native-to-managed entries.** `UnmanagedCallersOnly` statics on .NET; delegates rooted in a static array on .NET Framework, so the collector keeps them alive for the life of the table.
- **Library loading.** `NativeLibrary.Load` and `GetExport` on .NET; `LoadLibraryW` and `GetProcAddress` on .NET Framework, which is Windows-only anyway.
- **Assembly isolation.** A per-module `AssemblyLoadContext` on .NET, and a further one per build of the shell; `Assembly.LoadFrom` on .NET Framework, where one runtime version per process is the rule. A rebuilt shell takes its cmdlets over on both hosts regardless, because each build's assembly carries a name of its own, and distinct names give distinct types even in one load context.
- **Memory views.** `memory_view_new` returns a `Memory<T>` over the Rust buffer on .NET, with the drop callback run when the manager is collected; on .NET Framework the bytes are copied into a managed array and the drop runs before the entry returns.
- **Runtime identifier.** `linux`, `osx` and `freebsd` are recognized on .NET; .NET Framework is always `win`.

Everything else, including the whole hot path, is one source.

## Import

The bootstrap `.psm1` the build writes:

```powershell
$pwrsImportLock = New-Object System.Threading.Mutex($false, ('Local\pwrs-import-' + $PID))
try { $null = $pwrsImportLock.WaitOne() } catch [System.Threading.AbandonedMutexException] { }
try {
foreach ($bundled in @('Calc')) {
if (-not (Get-Module -Name $bundled)) {
try {
Import-Module ([System.IO.Path]::Combine($PSScriptRoot, $bundled, $bundled + '.psd1')) -Global -DisableNameChecking -ErrorAction Stop
}
catch {
if (@('Calc') -notcontains $bundled) { throw }
Write-Warning ('Hello' + ' imports without its bundled module ' + $bundled + ', whose import failed: ' + $_.Exception.Message)
}
}
}
$root = $PSScriptRoot
$tfm = if ($PSVersionTable.PSEdition -eq 'Core') { 'net10.0' } else { 'netstandard2.0' }
if (-not ('Pwrs.Bootstrap.Loader' -as [type])) {
$stage = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), 'pwrs-load', $PID)
$null = [System.IO.Directory]::CreateDirectory($stage)
$boot = [System.IO.Path]::Combine($stage, 'Pwrs.Bootstrap.dll')
[System.IO.File]::Copy([System.IO.Path]::Combine($root, $tfm, 'Pwrs.Bootstrap.dll'), $boot, $true)
$null = [System.Reflection.Assembly]::LoadFrom($boot)
}
try { $shell = [Pwrs.Bootstrap.Loader]::Load($root, 'Hello', $tfm) }
catch {
if ($tfm -eq 'net10.0' -and [Environment]::Version.Major -lt 8) {
throw ('Hello' + ' needs PowerShell 7.4 or later: its PowerShell 7 half is built against .NET 8, and this is PowerShell ' + $PSVersionTable.PSVersion + ' on .NET ' + [Environment]::Version + '.')
}
throw
}
$shell.GetType('Pwrs.Modules.Hello.PwrsModuleRoot', $true).GetField('Value').SetValue($null, $root)
if ($tfm -eq 'netstandard2.0') {
$staged = [System.IO.Path]::GetDirectoryName($shell.Location)
foreach ($dep in @('System.Numerics.Vectors.dll', 'System.Memory.dll', 'System.Buffers.dll', 'System.Runtime.CompilerServices.Unsafe.dll')) {
$to = [System.IO.Path]::Combine($staged, $dep)
if (-not [System.IO.File]::Exists($to)) {
try { [System.IO.File]::Copy([System.IO.Path]::Combine($root, $tfm, $dep), $to) }
catch [System.IO.IOException] { if (-not [System.IO.File]::Exists($to)) { throw } }
}
}
}
foreach ($m in Get-Module -All) {
if ($m.ModuleType -eq 'Binary' -and ($m.Path -eq $shell.Location -or $m.Name -like '*Hello.Shell*')) {
Remove-Module -ModuleInfo $m -Force -ErrorAction SilentlyContinue
}
}
try {
Import-Module -Assembly $shell -Force
$reload = $shell.GetTypes() | Where-Object { $_.Name -eq 'PwrsModule' } | Select-Object -First 1
if ($null -ne $reload) { $null = $reload::ReloadNative() }
}
catch {
$inner = $_.Exception
while ($null -ne $inner.InnerException) { $inner = $inner.InnerException }
throw $inner
}
Export-ModuleMember -Cmdlet 'Get-Greeting', ... -Alias 'nrtick', ...
} finally {
$pwrsImportLock.ReleaseMutex()
$pwrsImportLock.Dispose()
}
```

It takes a mutex named for the process and holds it to the end, so runspaces importing at the same instant take turns over the bootstrap copy, the staging and the engine's module table (a mutex a dead thread left behind is still acquired, which the caught exception reports), imports each bundled module the session does not already hold, which here writes a warning and goes on when `Calc` cannot be imported, because hello's entry for it says `on-import-failure = "warn"` (a module bundling none has no such step, and an entry that stops has no `try`; see [How To Bundle A Module](../how-to/How-To-Bundle-A-Module.md)), picks the folder by edition, copies `Pwrs.Bootstrap.dll` to a folder named after the process and loads it from there once per session, asks the loader for the shell assembly (and when the loader cannot be reached on a PowerShell 7 whose .NET is older than 8, says which PowerShell the module needs in place of the missing-type error; a load that succeeds never evaluates that check), hands the shell the folder it was imported from, on Windows PowerShell copies the .NET Framework dependencies a module with hand-written C# ships beside the staged shell (a module without any has no such step), removes a stale nested binary module from an earlier `Import-Module -Force`, imports the shell as a nested binary module, asks the module to pick up a rebuilt native library (the native library loads in the shell's type initializer, so a refusal there, such as the CPU check's, arrives wrapped in a `TypeInitializationException`, and the script rethrows the innermost exception, whose message is the reason), and exports the cmdlets and aliases by name. The copy is what leaves the module folder writable while a session holds it, and the glob matches the shell whatever stamp its name carries. The shell runs from that copy, so it cannot find its module folder from its own location; the folder handed to it here is what it loads its native library from, and it asks the loader only when nothing was handed over. The loader keeps one table from staging folder to module for the whole process, and a staging folder is named by a 31-bit hash of the module folder, so two modules whose folders hash alike would share an entry. The unindented body is deliberate: the file is generated, and indentation would be bytes with no reader. The cmdlets are the ones the descriptor listed followed by any declared by hand-written C# under `src/csharp/`; the aliases are those the descriptor declared, and they have to be named here because the engine creates them inside the nested binary module. A provider-only module exports `@()` for both.

## Isolation on pwsh

`Pwrs.Bootstrap` has a fixed identity for the life of the project, so two PWRS modules never conflict on it. On .NET its `Loader` keeps one `AssemblyLoadContext` per module folder; the context resolves `Pwrs.Runtime.dll` and the shell from that folder and falls through to the default context for everything else. Two modules built against different PWRS runtime versions coexist in one session, which is what `examples/calc` beside `examples/hello` demonstrates. This follows Microsoft's guidance on resolving module assembly dependency conflicts.

## The manifest

`CompatiblePSEditions = @('Desktop', 'Core')` and `PowerShellVersion = '5.1'` unless the crate names others, so both hosts accept a module that says nothing. `CmdletsToExport` lists the descriptor's cmdlets then the hybrid C# ones and `AliasesToExport` the declared aliases; `FormatsToProcess` names the format file when there is one; `Description`, `ProjectUri` and the tags after `pwrs` come from the crate's own description, repository and keywords; the `GUID` is derived from the module name so a rebuild keeps its identity. Every other manifest property comes from `[package.metadata.pwrs]`, and narrowing either of the first two there is how a module declares itself one-host.

## What 5.1 cannot do

Windows PowerShell has no `hostfxr`, so the in-process test host does not cover it; `cargo pwrs test` spawns `powershell.exe` for its Pester run instead. It also lacks `PipelineStopToken`; cancellation is the same cooperative flag on both hosts.
