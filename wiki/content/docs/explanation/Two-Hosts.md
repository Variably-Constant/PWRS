---
title: Two Hosts
weight: 4
---

How one `cargo pwrs build` runs in PowerShell 7 from 7.4 on and in Windows PowerShell 5.1 on .NET Framework. Source: `crates/cargo-pwrs/src/build.rs` (the two compiles), `generate.rs` (`bootstrap_psm1`, `manifest`), `crates/cargo-pwrs/dotnet/Pwrs.Bootstrap/Loader.cs`, and the `#if NET` branches in `crates/cargo-pwrs/dotnet/Pwrs.Runtime/*.cs`.

## Two compiles from one source

The runtime, the bootstrap and the generated shell are compiled twice:

| Folder | Framework | References | Defines |
|---|---|---|---|
| `net10.0/` | .NET 8 references, whichever pwsh builds it | `Microsoft.NETCore.App.Ref` 8.0.31 and the `System.Management.Automation` 7.4.0 reference from NuGet | the .NET SDK's net8.0 set: `NET`, `NETCOREAPP`, `NET8_0`, and the `OR_GREATER` symbols up to `NET8_0_OR_GREATER` and `NETCOREAPP3_1_OR_GREATER` |
| `netstandard2.0/` | .NET Framework through .NET Standard 2.0 | `NETStandard.Library` 2.0.3 and `PowerShellStandard.Library` 5.1.1 from NuGet, the .NET Framework builds of `System.Numerics.Vectors` 4.6.1, `System.Memory` 4.6.3, `System.Buffers` 4.6.1 and `System.Runtime.CompilerServices.Unsafe` 6.1.2, and the netstandard2.0 builds of `System.Security.AccessControl`, `System.Security.Principal.Windows`, `System.IO.FileSystem.AccessControl` and `Microsoft.Win32.Registry` 4.5.0 | the .NET SDK's netstandard2.0 set: `NETSTANDARD`, `NETSTANDARD2_0`, and the `OR_GREATER` symbols up to `NETSTANDARD2_0_OR_GREATER` |

.NET Standard 2.0 leaves out the access-control types a provider's security descriptors use: `ObjectSecurity`, the file, directory and registry descriptors, and the identities their rules name. The `netstandard2.0` compile takes them from the four 4.5.0 packages in the table, whose assembly versions are the ones PowerShell Standard's engine reference names. Windows PowerShell has these types in mscorlib. So every module ships each package's .NET Framework build in `netstandard2.0`, a file that only points each type to mscorlib's, and the `.psm1` copies them beside the staged shell. PowerShell 7 has the types in .NET itself, so the `net10.0` compile takes them from the reference pack.

A module that names `references` compiles its `netstandard2.0` shell, though not the runtime or the bootstrap, against the .NET Framework 4.7.2 reference assemblies in place of `NETStandard.Library`'s, with the same symbols, so that shell needs .NET Framework 4.7.2 or later. [How To Add Hybrid C#](../how-to/How-To-Add-Hybrid-CSharp.md#windows-forms-wpf-and-other-desktop-assemblies) has the set.

The Rust library is the same file for both; nothing in the ABI depends on the host.

`net10.0/` is a folder name, not a compile target. The Core compile is `/nostdlib+` against .NET 8's reference pack and the `System.Management.Automation` 7.4.0 reference, both fetched from NuGet into the toolchain as the `netstandard2.0` packages are. So every machine that builds a module emits the same references, `System.Runtime` 8.0.0.0 and `System.Management.Automation` 7.4.0.0 among them, whichever pwsh ran the build. A host binds an assembly only when its own copy is at least the version referenced. The `System.Management.Automation` reference is 7.4.0's because that package's assembly version is 7.4.0.0; the 7.4.20 package's is 7.4.6.500, the version PowerShell 7.4.20 itself has. The runtime's sources branch on `NET` alone; the other symbols are there for hand-written C#.

The folder name only tells the `.psm1` where to look for the Core assembly. Which .NET that assembly needs is set by the reference pack. On a PowerShell 7 whose .NET is older than 8, the loader cannot be reached. The `.psm1` then reports `<Module> needs PowerShell 7.4 or later: its PowerShell 7 half is built against .NET 8, and this is PowerShell <version> on .NET <version>.` The check runs only when the loader call fails, so an import that succeeds never runs it. The runtime and a module's hand-written C# can use only the API .NET 8 and PowerShell 7.4 have; a call to anything newer fails to compile on every machine, rather than building on one and not loading on another.

## What differs under `#if NET`

- **Managed-to-native calls.** Function pointers (`delegate* unmanaged`) on .NET; delegates from `Marshal.GetDelegateForFunctionPointer` on .NET Framework, which has no function-pointer syntax.
- **Native-to-managed entries.** `UnmanagedCallersOnly` statics on .NET; delegates stored in a static array on .NET Framework, so the garbage collector never frees them.
- **Library loading.** On Windows, `LoadLibraryExW` with `LOAD_WITH_ALTERED_SEARCH_PATH` on .NET, the call `NativeLibrary.Load` makes, so the DLLs a library imports are looked for first beside its staged copy, and `LoadLibraryW` on .NET Framework, which looks first beside the executable; elsewhere `NativeLibrary.Load`. Exports are bound with `NativeLibrary.GetExport` on .NET and `GetProcAddress` on .NET Framework. A load that fails on Windows names what the library lacks on both hosts, as [How To Fix A Failure](../how-to/How-To-Fix-A-Failure.md#import-fails) shows.
- **Assembly isolation.** A per-module `AssemblyLoadContext` on .NET, and a further one per build of the shell; `Assembly.LoadFrom` on .NET Framework, where one runtime version per process is the rule. There a module that uses what an earlier runtime lacks is refused after a module built by an earlier `cargo-pwrs`, as [How To Fix A Failure](../how-to/How-To-Fix-A-Failure.md#two-modules-in-one-windows-powershell-session) describes. A rebuilt shell takes its cmdlets over on both hosts regardless, because each build's assembly has a name of its own, and distinct names give distinct types even in one load context.
- **Memory views.** `memory_view_new` and `memory_view_borrow` return a `Memory<T>`, or a `ReadOnlyMemory<T>`, over the Rust memory on .NET, and its release runs when the manager is disposed or collected. On .NET Framework the elements are copied into a managed array, and the release runs before the entry returns.
- **Runtime identifier.** `linux`, `osx` and `freebsd` are recognized on .NET; .NET Framework is always `win`.

Everything else, including the whole hot path, is one source.

## Import

The bootstrap `.psm1` the build writes:

```powershell
foreach ($bundled in @('Calc')) {
if (-not (Get-Module -Name $bundled)) {
try {
Import-Module ([System.IO.Path]::Combine($PSScriptRoot, $bundled, $bundled + '.psd1')) -Global -DisableNameChecking -ErrorAction Stop
}
catch {
if (@('Calc') -notcontains $bundled) { throw }
Microsoft.PowerShell.Utility\Write-Warning ('Hello' + ' imports without its bundled module ' + $bundled + ', whose import failed: ' + $_.Exception.Message)
}
}
}
$root = $PSScriptRoot
$tfm = if ($PSVersionTable.PSEdition -eq 'Core') { 'net10.0' } else { 'netstandard2.0' }
if (-not ('Pwrs.Bootstrap.Loader' -as [type])) {
$stage = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), 'pwrs-load', [string]$PID, 'boot', '2a260e781c62dc01', $tfm)
$null = [System.IO.Directory]::CreateDirectory($stage)
$boot = [System.IO.Path]::Combine($stage, 'Pwrs.Bootstrap.dll')
if (-not [System.IO.File]::Exists($boot)) {
$pdb = [System.IO.Path]::Combine($root, $tfm, 'Pwrs.Bootstrap.pdb')
$bootPdb = [System.IO.Path]::Combine($stage, 'Pwrs.Bootstrap.pdb')
if ([System.IO.File]::Exists($pdb) -and -not [System.IO.File]::Exists($bootPdb)) {
$partial = $bootPdb + '.' + [System.Guid]::NewGuid().ToString('N') + '.partial'
[System.IO.File]::Copy($pdb, $partial)
try { [System.IO.File]::Move($partial, $bootPdb) }
catch [System.IO.IOException] {
[System.IO.File]::Delete($partial)
if (-not [System.IO.File]::Exists($bootPdb)) { throw }
}
}
$partial = $boot + '.' + [System.Guid]::NewGuid().ToString('N') + '.partial'
[System.IO.File]::Copy([System.IO.Path]::Combine($root, $tfm, 'Pwrs.Bootstrap.dll'), $partial)
try { [System.IO.File]::Move($partial, $boot) }
catch [System.IO.IOException] {
[System.IO.File]::Delete($partial)
if (-not [System.IO.File]::Exists($boot)) { throw }
}
}
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
foreach ($dep in @('System.Numerics.Vectors.dll', 'System.Memory.dll', 'System.Buffers.dll', 'System.Runtime.CompilerServices.Unsafe.dll', 'System.Security.AccessControl.dll', 'System.Security.Principal.Windows.dll', 'System.IO.FileSystem.AccessControl.dll', 'Microsoft.Win32.Registry.dll')) {
$to = [System.IO.Path]::Combine($staged, $dep)
if (-not [System.IO.File]::Exists($to)) {
$partial = $to + '.' + [System.Guid]::NewGuid().ToString('N') + '.partial'
[System.IO.File]::Copy([System.IO.Path]::Combine($root, $tfm, $dep), $partial)
try { [System.IO.File]::Move($partial, $to) }
catch [System.IO.IOException] {
[System.IO.File]::Delete($partial)
if (-not [System.IO.File]::Exists($to)) { throw }
}
}
}
}
foreach ($m in Get-Module -All) {
if ($m.ModuleType -eq 'Binary' -and ($m.Path -eq $shell.Location -or $m.Name -like '*Hello.Shell*')) {
Remove-Module -ModuleInfo $m -Force -ErrorAction SilentlyContinue
}
}
$lifecycle = $shell.GetType('Pwrs.Modules.Hello.PwrsModuleLifecycle', $true)
try {
Import-Module -Assembly $shell -Force
}
catch {
foreach ($e in $lifecycle::TakeImportErrors()) { Microsoft.PowerShell.Utility\Write-Error -ErrorRecord $e -ErrorAction Continue }
$inner = $_.Exception
while ($null -ne $inner.InnerException -and $inner.GetType().FullName -ne 'Pwrs.PwrsException') { $inner = $inner.InnerException }
if ($inner -is [System.Management.Automation.IContainsErrorRecord]) { throw $inner.ErrorRecord }
throw $inner
}
foreach ($e in $lifecycle::TakeImportErrors()) { Microsoft.PowerShell.Utility\Write-Error -ErrorRecord $e -ErrorAction Continue }
$ExecutionContext.SessionState.Module.OnRemove = { foreach ($e in $lifecycle::Removed()) { Microsoft.PowerShell.Utility\Write-Error -ErrorRecord $e -ErrorAction Continue } }
Export-ModuleMember -Cmdlet 'Get-Greeting', ... -Alias 'nrtick', ...
```

In order, the script:

1. Imports each bundled module the session does not already have. Here a failed import of `Calc` writes a warning and the script goes on, because hello's entry for it says `on-import-failure = "warn"`. An entry that stops has no `try`, and a module bundling none has no such step (see [How To Bundle A Module](../how-to/How-To-Bundle-A-Module.md)).
2. Picks the folder by edition.
3. Copies `Pwrs.Bootstrap.dll` to a folder named after the process, the stamp of the bootstrap's source and the edition, and loads it from there, once per session. When the build wrote `Pwrs.Bootstrap.pdb`, that goes in first, so the copy has its symbols beside it.
4. Calls the loader to get the shell assembly. When the loader cannot be reached on a PowerShell 7 whose .NET is older than 8, the script says which PowerShell the module needs, in place of the missing-type error; a load that succeeds never runs that check.
5. Gives the shell the folder the module was imported from.
6. On Windows PowerShell, copies the .NET Framework files the module ships beside the staged shell: the four access-control files every module has, and for a module with hand-written C#, the four dependencies that C# may use.
7. Removes a stale nested binary module left by an earlier `Import-Module -Force`.
8. Imports the shell as a nested binary module. hello declares lifecycle hooks, so its shell has a lifecycle class, whose import hook picks up a rebuilt native library and then runs the module's load and import hooks. The native library loads in the shell's type initializer, so a refusal there, such as the CPU check's, arrives wrapped in a `TypeInitializationException`. The script rethrows the first `PwrsException` inside what it caught, or else the innermost exception, whose message is the reason. It throws that exception's own record when it has one, so the record's id and category reach the user.
9. Writes the errors the import returned without failing it, such as one from the old copy's unload hook at a reload.
10. Sets the module's `OnRemove` block, which runs the removal and unload hooks at `Remove-Module` and writes their errors.
11. Exports the cmdlets and aliases by name.

A module that declares no lifecycle hook has neither step 9 nor step 10. Instead it calls `ReloadNative` after the shell import, so the module picks up a rebuilt native library.

Every command the script runs is in `Microsoft.PowerShell.Core`, which every session has. The warning and the error writes are called by their module-qualified names, `Microsoft.PowerShell.Utility\Write-Warning` and `Microsoft.PowerShell.Utility\Write-Error`, and run only when there is something to write. The writes pass `-ErrorAction Continue`: under an error preference of `Stop`, a write without it throws, and in the `OnRemove` block that stops `Remove-Module` with the module still imported. Because the module runs from a copy, the module folder stays writable while a session has the module loaded, and the `*Hello.Shell*` pattern in step 7 matches the shell whatever stamp its name has. The shell cannot find the module folder from its own location, so it loads its native library from the folder step 5 gives it, and calls the loader only when it was given none. The loader has one table from staging folder to module for the whole process, and a staging folder is named by a 31-bit hash of the module folder, so two modules whose folders hash alike would share an entry. The cmdlets are the ones the descriptor listed, followed by any declared by hand-written C# under `src/csharp/`. The aliases are those the descriptor declared, and they have to be exported by name because PowerShell creates them inside the nested binary module. A provider-only module exports `@()` for both.

Runspaces importing at the same instant run the script side by side and take no lock. Every file the script and the loader copy is written under a temporary name and renamed into place, so a file another import finds is whole. A second `LoadFrom` of the bootstrap returns the assembly already loaded, on both hosts. The loader's tables are maps that a change copies and swaps in with compare-exchange. When several imports of a new shell run at once, the first makes its load context while the others wait for it. hello's suite imports the module cold from eight runspaces at once, twice, in a child host of each edition. `Import-Module` binds the manifest's format file before the script runs. In a `RunspacePool`, whose runspaces share the list that binding adds to and walks, two imports at once can fail inside PowerShell whatever the module; [How To Fix A Failure](../how-to/How-To-Fix-A-Failure.md#import-fails) has the message and the way around it.

## Isolation on pwsh

`Pwrs.Bootstrap` has one fixed identity, so two PWRS modules never conflict on it. On .NET its `Loader` gives each module folder an `AssemblyLoadContext` of its own, which resolves `Pwrs.Runtime.dll` and the shell from that folder and falls through to the default context for everything else. Each module therefore runs the runtime its own folder ships, and modules built against different PWRS versions coexist in one session. hello imports `examples/calc` as a bundled module, and the two run side by side, each in a context of its own.

## Process-wide state across a reload and a drop

A rebuilt native library is loaded beside the copy it replaces, which stays loaded: the reload points the module at the new copy and unloads nothing, and the copies' statics are separate, one set per copy. So each copy in turn makes whatever the library makes for the whole process, such as a window class, a named kernel object, a variable of the process environment, or a singleton in a static that other code reaches by name. Two copies must not collide on it. Each copy names its own after something no other copy loaded beside it shares, such as the address of one of its own statics.

A module makes such a thing in `#[on_load]` and releases it in `#[on_unload]`. Those run once for the whole process, however many runspaces have the module, and on a reload the old copy's unload hook runs before the new copy's load hook; see [How To Reload A Module](../how-to/How-To-Reload-A-Module.md). The import and removal hooks run once for each runspace's import instead, so a removal in one runspace while another still has the module would release it too early.

hello does this with a variable of the process environment named `PWRS_HELLO_LOAD_` and the address of a static in the loaded copy, set through `[Environment]::SetEnvironmentVariable` in its load hook and cleared in its unload hook. The hot reload gate (`tools/hot_reload_gate.ps1`) checks after each of its three builds, imported in turn into one session with `Import-Module -Force`, that exactly one such variable exists and that the previous build's is gone. `Lifecycle.Tests.ps1` checks the same across removals and closes in two runspaces and a reload while another runspace has the module. Both pass in pwsh and Windows PowerShell 5.1 on Windows, and in pwsh on Linux and FreeBSD.

A proxy class's value is read, called and dropped on whatever thread uses the .NET object: the pipeline thread, a runspace's, a thread a script started, or the .NET finalizer thread when a script never disposes the object. So a value that owns something tied to one thread, a window or a COM object made in a single-threaded apartment, does not release it in `Drop`. `Drop` sends it back to the thread it belongs to, through that thread's own channel or message queue, and that thread releases it.

## The manifest

`CompatiblePSEditions = @('Desktop', 'Core')` and `PowerShellVersion = '5.1'` unless the crate names others, so both hosts accept a module that says nothing. `CmdletsToExport` lists the descriptor's cmdlets, then the hybrid C# ones, and `AliasesToExport` the declared aliases. `FormatsToProcess` names the format file when there is one. `ModuleVersion`, `Author`, `Description`, `ProjectUri` and the tags after `pwrs` come from the crate's own version, first author, description, repository and keywords. `[package.metadata.pwrs]`'s `description` and `tags` take the place of the last two when the crate sets them. The `GUID` is derived from the module name, so a rebuild has the same identity. Every other manifest property comes from `[package.metadata.pwrs]`, and narrowing either of the first two there is how a module declares itself one-host.

## What 5.1 cannot do

Windows PowerShell has no `hostfxr`, so `pwrs_host::Session`, the test host that runs pwsh inside a Rust test, covers PowerShell 7 only. `cargo pwrs test` runs each host's Pester suites in a child process of its own, pwsh and, on Windows, `powershell.exe`. Windows PowerShell also lacks `PipelineStopToken`; cancellation is the same cooperative flag on both hosts.
