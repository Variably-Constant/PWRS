---
title: How To Reload A Module
weight: 11
---

Picking up a rebuild without restarting the shell. Source: `crates/cargo-pwrs/dotnet/Pwrs.Bootstrap/Loader.cs` (the shell assembly and its load context), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/NativeModule.cs` (`ReloadIfChanged`, `Stage`), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/ProxyBase.cs` (the generation guard), `crates/cargo-pwrs/src/build.rs` (the assembly stamp), `tools/hot_reload_gate.ps1` (the gate that holds all of it).

## The loop

```powershell
Import-Module .\target\pwrs\Greeter\Greeter.psd1
Get-Greeting -Name World
# edit src/lib.rs, then, in another window:
#   cargo pwrs build --release
Import-Module .\target\pwrs\Greeter\Greeter.psd1 -Force
Get-Greeting -Name World          # the new build answers
```

`cargo pwrs build` succeeds while the session holds the module, and the second `Import-Module -Force` runs the code that was just built. Every host does this: PowerShell 7 on Windows and Linux, and Windows PowerShell 5.1.

## What actually happens

A module has two halves and they reload on different rules.

**The Rust library.** Every import asks `ReloadIfChanged`, which compares the library's write time against the load that is running. On a change it loads the rebuilt library and repoints all eleven exports at it in one store, so a call already in flight finishes against the image it started on.

**The managed shell.** The shell assembly's file name carries a stamp: `Greeter.Shell.<stamp>.dll`, where `<stamp>` is a hash of the managed source it was compiled from, which is the generated `Greeter.Shell.cs`, anything under `src/csharp/`, and the runtime. Change the surface, which means a parameter, a cmdlet, a class, an enum, a completer or a provider, and the stamp changes, the assembly takes a new identity, and PowerShell binds against the new cmdlet types. Change only a Rust body and the stamp is the same, the session keeps the types it already has, and only the library underneath is swapped.

The stamp is on the assembly name rather than on the namespace because a binding failure names the type in `FullyQualifiedErrorId` and scripts match on that string; an assembly name never appears there.

**Both halves run from copies.** A mapped file is locked on Windows, so the shell, the runtime and the library are each copied to `%TEMP%/pwrs-load/<pid>/` (`/tmp/pwrs-load/<pid>/` on Linux) and loaded from there. That is what leaves the module folder writable for the next build, and it also keeps the writing out of an installed module, which may sit where the session has no right to write. The copies cannot be deleted by the session that mapped them, so each session sweeps the folders of sessions that have already ended, and clears its own before its first load in case the system handed the same process identifier out again.

On PowerShell 7 each new shell identity also gets an `AssemblyLoadContext` of its own, which resolves the module's own files and falls through to the default context for everything else. Windows PowerShell has one load context; there the distinct assembly names alone are what keep the generations apart, and that is enough.

## Nothing is unloaded

This is the part to understand before relying on it.

A reload **never frees the previous library and never unloads the previous assembly.** The load contexts are created non-collectible on purpose. The old image stays mapped for the life of the process.

That is a deliberate trade. Freeing is what would make the swap unsafe, in four ways that no handler can catch:

- a thread from the module still executing inside the image,
- a callback registered with the host firing after the image is gone,
- a proxy object dereferenced from script after its code has been unmapped,
- a `FreeLibrary` that only decrements a reference count, so the unmap happens later and somewhere else.

Keeping the mapping turns every one of those from an access violation into a live address. The price is one library image, and one shell assembly per surface change, held until the process exits.

What a module holds across calls is therefore not released by the reload itself. That is what `#[on_remove]` is for: the bootstrap script removes the old binary module before it imports the new one, so on a reload the old library's removal hook runs, then the new library's import hook, in that order. A module that opens files, maps memory or starts threads at import closes them in its removal hook, and a reload leaves nothing of the old load behind but its mapped image. The import hook runs after the library has been pointed at the rebuilt body, so it sees what the import is importing. See [Attribute Reference](../reference/Attribute-Reference.md).

## The consequences of misusing it

**Memory grows with every reload and never comes back.** Reloading a few dozen times across a working session is unremarkable. Reloading in a loop, on a file watcher, or on a timer is not: nothing is reclaimed, so the process grows without bound. Restart the shell instead.

**Values made by an earlier load stop working.** A proxy class (`#[psclass(proxy)]`) is a handle onto a Rust value living in the image that made it. Each proxy records which load it came from and checks before every property read and method call; from a stale one the call is refused on the managed side, with a message naming both loads, rather than dereferenced against a body that has moved on. Disposing a stale proxy abandons the Rust value instead of freeing it, because the allocator that owns it belongs to the previous image, so that value is leaked. Discard variables holding proxies across a reload.

**Types from an earlier load are different types.** After a surface change the old cmdlet, class and enum types and the new ones are unrelated to the runtime even though they have the same names. Anything still holding an old one, a variable, a closure, a registered event handler, will not satisfy a parameter of the new one. The binder reports it as a cast that cannot be made.

**Rust process state does not carry over.** The new image starts with its own statics, its own `OnceLock` and `lazy_static` values, its own caches and its own thread pools. Nothing is migrated. State a module keeps in a `static` is gone as far as the new build is concerned, and still alive as far as the old image is concerned.

**Threads from the old image keep running.** A worker started by `stream_from_thread`, `par_map` or a module's own pool is not stopped by a reload. If it outlives the phase that started it, it carries on inside the previous image.

## When to use it

Use it in the edit, build, test loop, with a shell open, while writing a module.

Do not use it as a production update mechanism. A long-lived host, a service, or anything that reloads automatically wants a process restart, because that is the only thing that reclaims what a reload leaves behind.

## What it is not

PowerShell itself has never unloaded a binary module. `Remove-Module` removes the commands from the session; the assembly stays loaded. Without the stamped assembly name a rebuilt shell would be a second load of one identity, and the engine would resolve a cmdlet against whichever of the two types it cached first.
