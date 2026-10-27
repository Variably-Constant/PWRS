---
title: Glossary
weight: 8
---

Terms specific to PWRS and the PowerShell vocabulary it relies on.

**ABI.** The C contract between a PWRS native library and `Pwrs.Runtime`: the `HostVTable`, the exports, the status codes and the block layouts. Version 1. See [ABI Reference](ABI-Reference.md).

**AssemblyLoadContext (ALC).** The .NET mechanism `Pwrs.Bootstrap` uses on pwsh to load each module's `Pwrs.Runtime.dll` and shell into its own context, so two modules built against different PWRS versions coexist in one session.

**Binder.** PowerShell's parameter binder: the part of PowerShell that converts command-line and pipeline values to the .NET types a cmdlet declares, applies validation attributes, and tracks which parameters were bound. PWRS declares types and attributes and lets the binder do the rest.

**Block.** The `#[repr(C)]` struct a cmdlet's parameters, a copied class's fields, or a proxy method's arguments are packed into for one call between .NET and Rust; emitted on both sides by the generator.

**Bootstrap.** `Pwrs.Bootstrap.dll`, the small assembly with a fixed identity that the module's `.psm1` loads first, and the `.psm1` itself.

**cdylib.** The Rust crate type a PWRS module builds as: a shared library with the PWRS exports.

**Cmdlet.** A PowerShell command implemented as a class deriving from `PSCmdlet`. In PWRS, a Rust struct with `#[cmdlet]` and an `impl Cmdlet`.

**Copied class.** A `#[psclass]` mode in which writing a value builds a generated CLR object with the field values copied out; the default.

**Descriptor.** The JSON a built library returns from `pwrs_module_descriptor`, describing every cmdlet, parameter, class, enum, completer, transform and provider, and whether the module declares an import or removal hook; the generator's input.

**Dirty word.** One of the 64-bit words a parameter block starts with, one per 64 parameters and at least one. Bit `i % 64` of word `i / 64` is set when parameter `i`'s generated setter ran since the previous phase, and any word that is not zero means the native side binds.

**Dynamic parameter.** A parameter added at bind time based on the values already bound, through `IDynamicParameters`; in PWRS a `#[dynamic_params]` function.

**Entry.** One function pointer in the host vtable, implemented in `Pwrs.Runtime`, called from Rust.

**Export.** One symbol a PWRS library provides: an `extern "C"` function implemented in the `pwrs` runtime and called from `Pwrs.Runtime`, or `pwrs_cpu_requirements`, which is data.

**Handle.** `PsHandle`, the `IntPtr` of a `GCHandle`; how Rust refers to a .NET object. A `PsObject` owns one and frees it when dropped.

**Host.** One of the two PowerShell editions a module runs in: PowerShell 7 (Core) from 7.4 on, or Windows PowerShell 5.1 on .NET Framework. The Core assembly is compiled against .NET 8's reference pack and the `System.Management.Automation` 7.4 reference whichever pwsh builds it, so it references the same assemblies from every machine.

**Hybrid C#.** Hand-written `.cs` files under `src/csharp/` compiled into the shell assembly beside the generated code; the cmdlets they declare are exported with the Rust ones.

**Lifecycle hook.** A `fn() -> PsResult<()>` marked `#[on_load]`, `#[on_import]`, `#[on_remove]` or `#[on_unload]` and named in `export_module!`. The import hook runs at every import, in any runspace, and the removal hook once for each import, at `Remove-Module` or when the runspace closes. The load hook runs when the module goes from imported nowhere in the process to imported, and the unload hook when it goes back. A removal unloads nothing, so the unload hook is where a module releases what its load hook made. See [Attribute Reference](Attribute-Reference.md).

**MAML.** The XML help format `Get-Help` reads for binary modules; PWRS generates it from doc comments.

**Phase.** One of `begin`, `process`, `end`, corresponding to `BeginProcessing`, `ProcessRecord`, `EndProcessing`.

**Phase mask.** The bits `pwrs_cmdlet_create` reports saying which phases a cmdlet type needs a native call for; recorded once per type, when the default `begin` and `end` bodies run.

**Pinned view.** A managed primitive array pinned by a `GCHandle` and borrowed as a Rust slice; `Pinned<'a, T>`.

**Pipeline thread.** The thread running a cmdlet's current phase, the only one on which PowerShell allows stream writes and session-state access. Code that has a `Pipeline<'ps>` is running on it.

**Provider.** A PowerShell namespace provider (`NavigationCmdletProvider`); in PWRS a struct implementing the `Provider` trait with `#[provider]`, one instance of it per drive.

**Proxy class.** A `#[psclass]` mode in which the Rust value stays in Rust and the generated CLR object reads fields, and runs `#[psmethods]` methods, through native calls until it is disposed.

**PSObject class.** A `#[psclass]` mode with no CLR type: a `PSObject` with a `PSTypeName` and note properties.

**Runtime identifier (RID).** `win-x64`, `linux-x64`, `osx-arm64`, `freebsd-x64` and the like; the folder under `runtimes/` with the native library for a platform.

**Scale.** The number of digits a `System.Decimal` has after the point, 0 to 28, stored in bits 16 to 23 of its `flags` word. It is part of the value's identity, not its size: `1.10` and `1.1` are equal and not identical, so PWRS leaves the scale as it is instead of normalizing it. The string cast `[decimal]'1.10'` gives scale 2 on Windows PowerShell and scale 1 on PowerShell 7; see [Conversions Reference](Conversions-Reference.md).

**Shell.** The generated C# assembly `<Module>.Shell.<stamp>.dll`: the CLR types PowerShell reflects over. Its code packs blocks and passes each call to `Pwrs.Runtime`; the work happens in Rust. The stamp is a hash of the source the shell was compiled from, so a rebuild that changes the surface gets a new assembly name, and its cmdlets replace the old ones in a live session.

**Slot.** A position in the vtable, or the representation of one value in a block (`u8`, `i64`, `PsStr16`, `PsHandle`, and so on).

**Surface.** What a module declares to PowerShell: its cmdlets and their parameters, and its classes, enums, completers, transforms and providers. A change to the surface gives the shell a new stamp, and the surface check of `cargo pwrs test` reports declarations no test exercised.

**Terminating error.** An error raised with `ThrowTerminatingError`, which ends the cmdlet; `PsError::terminating()` opts into it. The default `Err` is non-terminating (`WriteError`).

**Toolchain.** The fetched `csc`, the reference packages both compiles use and the Pester the suites run under, stored under `~/.pwrs/toolchain/`, and their lock file.

**Type tag.** A `u32` naming one of a fixed set of CLR types, `PS_TYPE_OBJECT` (0) for any other. `array_element_tag` returns the tag of an array's elements: a `Vec<T>` is copied through one pin when that tag is `T`'s, and read element by element otherwise. `object_type_tag`, behind `PsObject::type_tag()`, returns the tag of a single object, so code can find the type of an object it receives without reading `GetType().FullName`.

**Vtable.** The `HostVTable`: the append-only table of entries the runtime gives a module at `pwrs_module_init`.
