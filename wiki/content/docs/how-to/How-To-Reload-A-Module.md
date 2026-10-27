---
title: How To Reload A Module
weight: 18
---

Picking up a rebuild without restarting the shell. Source: `crates/cargo-pwrs/dotnet/Pwrs.Bootstrap/Loader.cs` (the shell assembly and its load context), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/NativeModule.cs` (`ReloadIfChanged`, `Imported`, `Stage`), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/ProxyCore.cs` (the generation guard), `crates/cargo-pwrs/src/build.rs` (the assembly stamp), `tools/hot_reload_gate.ps1` (the gate that checks all of it).

## The loop

```powershell
Import-Module .\target\pwrs\Greeter\Greeter.psd1
Get-Greeting -Name World
# edit src/lib.rs, then, in another window:
#   cargo pwrs build --release
Import-Module .\target\pwrs\Greeter\Greeter.psd1 -Force
Get-Greeting -Name World          # the new build runs
```

`cargo pwrs build` succeeds while the session has the module loaded, and the second `Import-Module -Force` runs the code that was just built. Every host does this: PowerShell 7 on Windows, Linux, FreeBSD and macOS, and Windows PowerShell 5.1.

## What actually happens

A module has two halves and they reload on different rules.

**The Rust library.** At every import, the runtime compares the library's write time with that of the library already loaded. On a change it loads the rebuilt library and switches every export to it with one assignment, so a call already running finishes in the library it started in.

**The managed shell.** The shell assembly's file name has a stamp: `Greeter.Shell.<stamp>.dll`, where `<stamp>` is a hash of what it was compiled from: the generated `Greeter.Shell.cs`, anything under `src/csharp/`, the runtime, and the names listed under `references`. Change the surface, which means a parameter, a cmdlet, a class, an enum, a completer or a provider, and the stamp changes, the assembly takes a new identity, and PowerShell binds against the new cmdlet types. Change only a Rust body and the stamp is the same, the session goes on using the types it already has, and only the library underneath is swapped.

The stamp is in the assembly's name only. The type names stay the same from build to build, so a script that matches the `FullyQualifiedErrorId` of a binding failure, which names the type, still matches after a rebuild.

**Both halves run from copies.** A mapped file is locked on Windows, so the shell, the runtime and the library are each copied into `pwrs-load/<pid>/` under the temp folder (`%TEMP%` on Windows, `$TMPDIR` or `/tmp` elsewhere), in a folder for the module, and loaded from there. That is what leaves the module folder writable for the next build, and it also means nothing is written into an installed module, which may be in a folder the session cannot write to. The copies cannot be deleted by the session that mapped them, so each session sweeps the folders of sessions that have already ended, and clears its own before its first load in case the system reused the process identifier.

On PowerShell 7 each new shell identity also gets an `AssemblyLoadContext` of its own, which resolves the module's own files and falls through to the default context for everything else. Windows PowerShell has one load context; there the distinct assembly names alone are what separate the generations, and that is enough.

## Nothing is unloaded

This is the part to understand before relying on it.

A reload **never frees the previous library and never unloads the previous assembly.** The load contexts are created so that .NET cannot unload them, and the old image stays mapped for the life of the process. So a thread of the module still running in it, and a callback it registered with the host, go on running after the reload. Each reload leaves one library image, and one shell assembly per surface change, in memory until the process exits.

What a module sets up and uses across calls is therefore not released by the reload itself. That is what the unload hook is for. A module that opens files, maps memory or starts threads in its load hook (`#[on_load]`) closes them in its unload hook (`#[on_unload]`), and a reload leaves nothing of the old load behind but its mapped image.

On a reload the old copy of the library runs its unload hook, and then the new copy runs its load hook, before the import hook of the import that reloaded it. In one runspace, `Import-Module -Force` removes the module first. That removal runs the old copy's removal hook, and its unload hook when no other runspace has the module; the import then loads the new copy. When another runspace still has the module, the reload itself runs the old copy's unload hook and the new copy's load hook. Either way each runs once. A call running in another runspace at that moment can run in the old copy after its unload hook has started, or in the new copy before its load hook has returned.

If the new copy's load hook fails, the import fails, and the next import runs the hook again. Until one succeeds, the module runs the new copy without what that hook makes. See [Attribute Reference](../reference/Attribute-Reference.md).

## The consequences of misusing it

**Memory grows with every reload and never comes back.** Reloading a few dozen times across a working session is unremarkable. Reloading in a loop, on a file watcher, or on a timer is not: nothing is reclaimed, so the process grows without bound. Restart the shell instead.

**Values made by an earlier load stop working.** A proxy class (`#[psclass(proxy)]`) is a handle onto a Rust value living in the image that made it. Each proxy records which load it came from and checks before every property read and method call. From a stale one the call is refused on the managed side, with a message naming both loads, rather than run against a library that has been replaced. Disposing a stale proxy abandons the Rust value instead of freeing it, because the previous image's allocator made it, so that value is leaked. Discard variables that refer to proxies across a reload.

**Types from an earlier load are different types.** After a surface change the old cmdlet, class and enum types and the new ones are different types to .NET, even though they have the same names. Anything that still refers to an old one, a variable, a closure, a registered event handler, is not accepted by a parameter of the new one. The binder reports it as a cast that cannot be made.

**Rust process state starts over.** The new image starts with its own statics, its own `OnceLock` and `lazy_static` values, its own caches and its own thread pools. Nothing is migrated. State a module stores in a `static` is gone as far as the new build is concerned, and still alive as far as the old image is concerned.

**Threads from the old image go on running.** A worker started by `stream_from_thread`, `par_map` or a module's own pool is not stopped by a reload. If it outlives the phase that started it, it goes on running inside the previous image.

## When to use it

Use it in the edit, build, test loop, with a shell open, while writing a module.

Do not use it as a production update mechanism. A long-lived host, a service, or anything that reloads automatically should restart the process instead, because only a restart reclaims what a reload leaves behind.

## What it is not

A reload is not an unload, and PowerShell does not unload a binary module either: `Remove-Module` removes its commands from the session, and its assembly stays loaded.
