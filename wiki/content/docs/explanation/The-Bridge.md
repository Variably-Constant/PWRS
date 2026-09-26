---
title: The Bridge
weight: 2
---

How native code enters the managed world and how it calls back. Source: `crates/pwrs-sys/src/lib.rs`, `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs`, `NativeModule.cs`, `RustCmdlet.cs`, `crates/pwrs/src/runtime.rs`, and the generated shell from `crates/cargo-pwrs/src/generate.rs`.

## Three parties

- **The generated shell** is a C# assembly with one sealed `PSCmdlet` subclass per Rust cmdlet, one class or enum per `#[psclass]` and `#[psenum]`, one `IArgumentCompleter` per completer, one `NavigationCmdletProvider` per provider, and a module class holding the loaded `NativeModule`. It is what PowerShell reflects over. It contains no logic beyond packing a parameter block and calling its base class.
- **`Pwrs.Runtime`** is the support assembly. `RustCmdlet` is the cmdlet base class: it owns the handle to itself, the pointer to its Rust instance, the pipeline-thread identity, the pending terminating error, and the one native call per phase. `HostVTable` builds the function table. `NativeModule` loads the native library for the current runtime identifier and binds its exports.
- **The Rust library** is the module author's crate plus the `pwrs` runtime it links: the exports every module provides, the tables `export_module!` builds, and the typed API over the vtable.

## Managed to native: the exports

Every PWRS library exports the same set of `extern "C"` functions, emitted by `export_module!`:

| Export | Role |
|---|---|
| `pwrs_module_descriptor` | returns the JSON the build tool reads to generate the shell; the only export called at build time |
| `pwrs_module_init` | receives the vtable pointer; records the module's cmdlet, class, completer, dynamic-parameter and provider tables |
| `pwrs_cmdlet_create` | makes the Rust instance a managed cmdlet will own, and reports which phases the type needs |
| `pwrs_cmdlet_invoke` | runs one phase against an instance with the parameter block |
| `pwrs_cmdlet_stop` | sets the stop flag, from the engine's thread |
| `pwrs_cmdlet_release` | drops the instance when the managed cmdlet is disposed |
| `pwrs_proxy_get`, `pwrs_proxy_call`, `pwrs_proxy_drop`, `pwrs_proxy_bytes` | read a field of a proxy object; run one of its `#[psmethods]` methods; free it; report the native bytes its value holds |
| `pwrs_completer_invoke` | run a completer on the completion thread |
| `pwrs_transform_invoke` | run a `#[transform]` over an argument before the binder coerces it |
| `pwrs_dynparams_invoke` | compute a cmdlet's dynamic parameters before binding |
| `pwrs_provider_invoke` | run one provider operation by op code against the instance serving the current drive |
| `pwrs_module_lifecycle` | run the module's import or removal hook |
| `pwrs_cpu_requirements`, `pwrs_cpuid`, `pwrs_xgetbv` | the extensions the library was compiled for, as data, and the two instructions the runtime checks them with before anything else runs |

On .NET the shell calls them through unmanaged function pointers (`delegate* unmanaged[Cdecl]`), a direct `calli` with the GC transition and no marshalling stub. On .NET Framework it calls them through delegates from `Marshal.GetDelegateForFunctionPointer`. Rust sees `extern "C"` either way.

## Native to managed: the vtable

At `pwrs_module_init` the runtime hands the library a pointer to a table it allocated once in unmanaged memory: an 8-byte header (`size`, `version`) then one function pointer per entry in declaration order. The entries are the engine surface Rust needs: handle lifecycle, the cmdlet streams and error records, `ShouldProcess` and `ShouldContinue`, parameter reads, primitive and string construction and reading, arrays and pinning, `PSObject` construction, session state, script block invocation, dynamic member access, type tags, generated-type factories, borrows of a proxy object's value, memory views, diagnostics, and direct scalar writes. On .NET each entry is an `UnmanagedCallersOnly` static; on .NET Framework it is a delegate rooted in a static array so the collector never frees it.

`pwrs_sys::HostVTable` is the same memory as a Rust `#[repr(C)]` struct. The two files are edited together, in one commit, and the table only ever grows at the end; a module checks `size` before touching an entry newer than the version it was compiled against. The full slot list is in [ABI Reference](ABI-Reference.md).

## What crosses

- **Handles.** A managed object crosses as `PsHandle`, the `IntPtr` of a `GCHandle` the runtime allocated. `PsObject` owns one and frees it on drop; cloning allocates another handle to the same object. Never a raw object pointer.
- **Strings.** UTF-16 `(ptr, len)` pairs, borrowed for the call. Readers size and retry: a read entry reports the full length, and the caller grows its buffer when the first attempt was too small.
- **Primitives.** Inline, in parameter blocks and through typed entries (`i64_new`, `u64_read`, and the rest). Dates, time spans, GUIDs and chars cross the same way, as ticks, sixteen bytes and one code unit, through their own entries.
- **Pinned views.** `array_pin` pins a managed primitive array and returns its address, length and element size; `array_unpin` releases it before the phase returns.
- **Parameter blocks.** A `#[repr(C)]` struct the generator emitted on both sides: a dirty word, a bound word, then one slot per parameter, on the managed stack for one phase.

## Two rules at the boundary

**Nothing unwinds across it.** Every Rust export wraps its body in `catch_unwind`; a panic becomes status 2 with the message in a managed string. Every managed entry wraps its body in `try/catch`; an exception becomes status 1 with the exception in the `err` handle, and a `PipelineStoppedException` becomes status 5 after marking the cmdlet stopped. A body error from a phase is written through `write_error` before `pwrs_cmdlet_invoke` returns; a terminating one is stored on the managed cmdlet and raised with `ThrowTerminatingError` after the native call is over, so no managed exception ever propagates through native frames.

**Nothing crosses as a raw managed pointer.** Only handles and scoped pins. A pin must not outlive its phase; the `Pinned<'a, T>` borrow enforces that in Rust.

## The pipeline thread

The engine allows `WriteObject`, the stream writers, `ShouldProcess`, session state and script block invocation only on the thread running the current phase. Every such vtable entry checks that the calling thread is the one that entered the phase and refuses with status 3 otherwise. On the Rust side the check is static: those calls take `&Pipeline<'_>`, a `!Send` token the runtime creates for one phase, so a cmdlet cannot hand it to another thread.
