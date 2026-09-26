---
title: Glossary
weight: 8
---

Terms specific to PWRS and the PowerShell vocabulary it relies on.

**ABI.** The C contract between a PWRS native library and `Pwrs.Runtime`: the `HostVTable`, the exports, the status codes and the block layouts. Version 1. See [ABI Reference](ABI-Reference.md).

**AssemblyLoadContext (ALC).** The .NET mechanism `Pwrs.Bootstrap` uses on pwsh to load each module's `Pwrs.Runtime.dll` and shell into its own context, so two modules built against different PWRS versions coexist in one session.

**Binder.** PowerShell's parameter binder: the engine component that coerces command-line and pipeline values to the CLR types a cmdlet declares, applies validation attributes, and tracks which parameters were bound. PWRS declares types and attributes and lets the binder do the rest.

**Block.** The `#[repr(C)]` struct a cmdlet's parameters, a copied class's fields, or a proxy method's arguments are packed into for one crossing; emitted on both sides by the generator.

**Bootstrap.** `Pwrs.Bootstrap.dll`, the small assembly with a fixed identity that the module's `.psm1` loads first, and the `.psm1` itself.

**cdylib.** The Rust crate type a PWRS module builds as: a shared library with the PWRS exports.

**Cmdlet.** A PowerShell command implemented as a class deriving from `PSCmdlet`. In PWRS, a Rust struct with `#[cmdlet]` and an `impl Cmdlet`.

**Copied class.** A `#[psclass]` mode in which writing a value builds a generated CLR object with the field values copied out; the default.

**Descriptor.** The JSON a built library returns from `pwrs_module_descriptor`, describing every cmdlet, parameter, class, enum, completer and provider; the generator's input.

**Dirty word.** The first `u64` of a parameter block: a bit per parameter set when the generated setter ran since the previous phase. Non-zero means the native side binds.

**Dynamic parameter.** A parameter added at bind time based on the values already bound, through `IDynamicParameters`; in PWRS a `#[dynamic_params]` function.

**Entry.** One function pointer in the host vtable, implemented in `Pwrs.Runtime`, called from Rust.

**Export.** One `extern "C"` function a PWRS library provides, implemented in the `pwrs` runtime, called from `Pwrs.Runtime`.

**Handle.** `PsHandle`, the `IntPtr` of a `GCHandle`; how a managed object is referred to across the boundary. `PsObject` owns one.

**Host.** One of the two PowerShell editions a module runs in: PowerShell 7 (Core) from 7.4 on, or Windows PowerShell 5.1 on .NET Framework. The Core assembly is compiled against .NET 8's reference pack and the `System.Management.Automation` 7.4 reference whichever pwsh builds it, so it references the same assemblies from every machine.

**Hybrid C#.** Hand-written `.cs` files under `src/csharp/` compiled into the shell assembly beside the generated code; the cmdlets they declare are exported with the Rust ones.

**Lifecycle hook.** A `fn() -> PsResult<()>` marked `#[on_import]` or `#[on_remove]` and named in `export_module!`, run by the engine when the module is imported or removed through `IModuleAssemblyInitializer` and `IModuleAssemblyCleanup` on the generated shell. A removal unloads nothing, so the import hook runs on every import and the removal hook is where an import's resources are released. See [Attribute Reference](Attribute-Reference.md).

**MAML.** The XML help format `Get-Help` reads for binary modules; PWRS generates it from doc comments.

**Phase.** One of `begin`, `process`, `end`, corresponding to `BeginProcessing`, `ProcessRecord`, `EndProcessing`.

**Phase mask.** The bits `pwrs_cmdlet_create` reports saying which phases a cmdlet type needs a native call for; learned once per type by observing the default `begin` and `end` bodies.

**Pinned view.** A managed primitive array pinned by a `GCHandle` and borrowed as a Rust slice; `Pinned<'a, T>`.

**Pipeline thread.** The thread running a cmdlet's current phase, the only one on which the engine allows stream writes and session-state access. `Pipeline<'ps>` is the token that proves you are on it.

**Provider.** A PowerShell namespace provider (`NavigationCmdletProvider`); in PWRS a struct implementing the `Provider` trait with `#[provider]`, one instance of it per drive.

**Proxy class.** A `#[psclass]` mode in which the Rust value stays in Rust and the generated CLR object reads fields, and runs `#[psmethods]` methods, through native calls until it is disposed.

**PSObject class.** A `#[psclass]` mode with no CLR type: a `PSObject` with a `PSTypeName` and note properties.

**Runtime identifier (RID).** `win-x64`, `linux-x64`, `osx-arm64`, `freebsd-x64` and the like; the folder under `runtimes/` that holds the native library for a platform.

**Shell.** The generated C# assembly `<Module>.Shell.<stamp>.dll`: the CLR types PowerShell reflects over. Logic-free. The stamp is a hash of the managed source it was compiled from, which is what lets a rebuilt surface take the cmdlets over in a live session.

**Slot.** A position in the vtable, or the representation of one value in a block (`u8`, `i64`, `PsStr16`, `PsHandle`, and so on).

**Scale.** The number of digits a `System.Decimal` carries after the point, 0 to 28, held in bits 16 to 23 of its `flags` word. Part of the value's identity and not of its magnitude: `1.10` and `1.1` are equal and not identical, so PWRS carries the scale rather than normalizing it. A string cast does not agree across hosts; see [Conversions Reference](Conversions-Reference.md).

**Terminating error.** An error raised with `ThrowTerminatingError`, which ends the cmdlet; `PsError::terminating()` opts into it. The default `Err` is non-terminating (`WriteError`).

**Toolchain.** The fetched `csc` and the reference packages both compiles use, under `~/.pwrs/toolchain/`, and their lock file.

**Type tag.** A `u32` naming one of the CLR types PWRS knows, `PS_TYPE_OBJECT` (0) for anything else. `array_element_tag` answers for an array's elements and decides whether a `Vec<T>` takes one pin or the element-by-element path; `object_type_tag`, behind `PsObject::type_tag()`, answers for a single object and is how a caller dispatches on a type it does not know at compile time without reading `GetType().FullName`.

**Vtable.** The `HostVTable`: the append-only table of entries the runtime hands a module at `pwrs_module_init`.
