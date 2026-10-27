---
title: The Bridge
weight: 2
---

How .NET calls into the Rust library, and how the library calls back. Source: `crates/pwrs-sys/src/lib.rs`, `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs`, `NativeModule.cs`, `RustCmdlet.cs`, `CpuCheck.cs`, `crates/pwrs/src/runtime.rs`, and the generated shell from `crates/cargo-pwrs/src/generate.rs`.

## Three parties

- **The generated shell** is a C# assembly, and it is what PowerShell reflects over. It has:
  - a sealed cmdlet class for each Rust cmdlet;
  - a class or enum for each `#[psclass]` and `#[psenum]`, except a class in `psobject` mode, which is passed as a `PSObject` and has no type of its own;
  - a completer class for each completer, an argument transformation attribute for each `#[transform]`, and a `NavigationCmdletProvider` for each provider;
  - a lifecycle class when the module declares a lifecycle hook;
  - a module class whose `Native` field is the loaded `NativeModule`.

  Its code packs parameters and arguments into blocks, builds objects from the blocks Rust fills, and passes every call to a base class in `Pwrs.Runtime`, or, for a proxy class with a .NET base of its own, to the `Pwrs.ProxyCore` in its field. The work itself happens in Rust.
- **`Pwrs.Runtime`** is the support assembly. `RustCmdlet` is the cmdlet base class: it has the handle to itself, the pointer to its Rust instance, the pipeline-thread identity and the pending terminating error, and it makes the one native call per phase. `ProxyCore` is what a proxy object has of its Rust value: the pointer, the gate its calls take turns at, the load generation check, the memory report and the finalizer. `ProxyBase`, the base of every other proxy class, has one. `HostVTable` builds the function table. `NativeModule` loads the native library for the current runtime identifier, checks it against the CPU, and binds and calls its exports.
- **The Rust library** is the module author's crate plus the `pwrs` runtime it links: the exports every module provides, the tables `export_module!` builds, and the typed API over the vtable.

## Managed to native: the exports

`export_module!` emits the same exports in every PWRS library:

| Export | Role |
|---|---|
| `pwrs_module_descriptor` | returns the JSON the build tool reads to generate the shell; the only export called at build time |
| `pwrs_module_init` | receives the vtable pointer; records the module's cmdlets, classes, completers, transforms, dynamic parameters, providers and lifecycle hooks |
| `pwrs_cmdlet_create` | makes the Rust instance a managed cmdlet uses for its lifetime, and reports which phases the type needs a native call for |
| `pwrs_cmdlet_invoke` | runs one phase against an instance with the parameter block |
| `pwrs_cmdlet_stop` | sets the stop flag and runs the waker the phase registered, from the engine's thread |
| `pwrs_cmdlet_release` | drops the instance when the managed cmdlet is disposed |
| `pwrs_proxy_get`, `pwrs_proxy_call`, `pwrs_proxy_drop`, `pwrs_proxy_bytes` | read a field of a proxy object; run one of its `#[psmethods]` methods; free it; report the native bytes its value uses |
| `pwrs_completer_invoke` | run a completer on the completion thread |
| `pwrs_transform_invoke` | run a `#[transform]` over an argument before the binder converts it |
| `pwrs_dynparams_invoke` | compute a cmdlet's dynamic parameters before binding |
| `pwrs_provider_invoke` | run one provider operation, by op code, against the current drive's provider instance |
| `pwrs_module_lifecycle` | run the module's import, removal, load or unload hook |
| `pwrs_cpu_requirements` | data rather than a function: the instruction-set extensions the library was compiled for |
| `pwrs_cpuid`, `pwrs_xgetbv` | on x86-64 only: the two instructions the runtime checks that list with, and nothing else |

`NativeModule` reads `pwrs_cpu_requirements` before it calls anything else, and refuses a library compiled for an extension this machine does not offer, naming the extensions; [How To Use Instruction Sets](How-To-Use-Instruction-Sets.md#what-a-native-cpu-build-does) describes the check. On .NET it calls the functions through unmanaged function pointers (`delegate* unmanaged[Cdecl]`), a direct `calli` with the GC transition and no marshaling stub. On .NET Framework it calls them through delegates from `Marshal.GetDelegateForFunctionPointer`. Rust sees `extern "C"` either way.

## Native to managed: the vtable

At `pwrs_module_init` the runtime gives the library a pointer to a table in unmanaged memory that is never freed: an 8-byte header (`size`, `version`) then one function pointer per entry in declaration order. The entries are the parts of PowerShell that Rust needs:

- handle lifecycle;
- output, the streams, error records, progress and host writes, and direct scalar writes;
- `ShouldProcess` and `ShouldContinue`;
- parameter reads, session state and path resolution;
- making and reading primitives and strings, arrays and pinning, and making a `PSObject`;
- script block and command invocation, and dynamic member access;
- type tags, generated-type factories, and borrows of a proxy object's value;
- memory views, task completion and engine events;
- the staged paths of helpers and native files, and diagnostics.

On .NET each entry is an `UnmanagedCallersOnly` static; on .NET Framework it is a delegate stored in a static array, so the garbage collector never frees it.

On .NET every module loads its own copy of `Pwrs.Runtime`, and with it a table of its own. On .NET Framework every module in the process uses one copy, so each module gets its own copy of the table, whose `factory_new`, `proxy_enter`, `proxy_enter_shared`, `helper_path` and `native_file_path` entries work for that module.

`pwrs_sys::HostVTable` is the same memory as a Rust `#[repr(C)]` struct, field for field, and entries are only ever added at the end. At init the library refuses, with status 4, a table whose `version` differs from the one it was built for or whose `size` is smaller than its own struct. So it never calls an entry the runtime lacks, and it still works with a runtime whose table is longer. The full slot list is in [ABI Reference](ABI-Reference.md).

## What passes between Rust and .NET

- **Handles.** A .NET object is passed as a `PsHandle`, the `IntPtr` of a `GCHandle` the runtime allocated. A `PsObject` owns one and frees it when it is dropped; cloning allocates another handle to the same object. Never a raw object pointer.
- **Strings.** UTF-16 `(ptr, len)` pairs, borrowed for the call. A read entry writes what fits in the caller's buffer and reports the full length. A staged path or an exception's text is then read again into a buffer of that length. `String::from_ps` reads a string longer than 256 units where it sits in .NET's memory instead: it pins the string through `string_pin` and decodes its UTF-16 units with no copy in between.
- **Primitives.** Inline, in parameter blocks and through typed entries (`i64_new`, `u64_read`, and the rest). Dates, time spans, GUIDs and chars are passed the same way, as ticks, sixteen bytes and one code unit, through their own entries.
- **Pinned views.** `array_pin` pins a managed primitive array and returns its address, length and element size with the pin's handle, and `array_unpin` releases the pin. In Rust the view is `Pinned<'a, T>`, which borrows the object it pins, reads as a slice, and releases the pin when it is dropped.
- **Parameter blocks.** A `#[repr(C)]` struct the generator emitted on both sides: the dirty words, the bound words, then one slot per parameter, on the managed stack for one phase. Each mask has one 64-bit word per 64 parameters, and at least one.

## Two rules at the boundary

**Nothing unwinds across it.** Every export that runs the module's code catches a panic. One that returns a status returns 2, with the panic's message as a managed string; one with no status to return, such as releasing a cmdlet or dropping a proxy object, writes the message to standard error. Every managed entry that returns a status catches what its body throws: an exception becomes status 1 with the exception in the `err` handle, and a `PipelineStoppedException` becomes status 5 after marking the cmdlet stopped. A body error from a phase is written through `write_error` before `pwrs_cmdlet_invoke` returns. A terminating one is stored on the managed cmdlet and raised with `ThrowTerminatingError` after the native call is over, so no managed exception ever passes through native frames.

**No raw .NET object pointer is passed.** Only handles and pins are, and the safe API reads a pinned address only through the `Pinned` view that owns the pin.

## The pipeline thread

The engine allows output, the streams, host writes, `ShouldProcess` and `ShouldContinue`, parameter reads, session state, path resolution, and script block and command invocation only on the thread running the current phase. Each of those vtable entries checks that the calling thread is the one that entered the phase, and refuses otherwise with status 3 and an `InvalidOperationException` reading `pwrs: cmdlet stream and session-state calls are only valid on the pipeline thread`. On the Rust side the check is static: those calls take `&Pipeline<'_>`, a token the runtime creates for one phase that is neither `Send` nor `Sync`, so a cmdlet cannot pass it to another thread.

A `PsObject` is `Send`, so its own calls are checked at run time instead. In a debug build, and under `cargo pwrs test`, a call from a thread the host did not call the module on returns an error with id `PwrsOffThread`. [How To Use Threads](How-To-Use-Threads.md) says which threads may make them.
