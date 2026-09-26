# PWRS native ABI

Version 1. The Rust definition in `crates/pwrs-sys/src/lib.rs` and the
C# table builder in `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs` describe the
same memory; both change in the same commit.

## Rules

- Calling convention `extern "C"` everywhere. x64 and arm64 only.
- No unwinding across the boundary. Rust exports wrap `catch_unwind`;
  managed entries wrap `try/catch` and return status 1 with a
  `GCHandle` to the exception in the `err` out-pointer.
- Managed objects cross only as `PsHandle` (a `GCHandle` `IntPtr`) or
  as `PsPinned` views released before the phase returns.
- Strings cross as UTF-16 `(ptr, len)`; readers size and retry.
- The table is append-only. The 8-byte header is `size: u32,
  version: u32`. A module reads `size` before using any entry newer
  than the version it was compiled against. `version` changes only
  when an existing entry changes meaning; then the export names gain
  the major (`pwrs2_*`).

## Status codes

| Code | Meaning |
|---|---|
| 0 | ok |
| 1 | managed exception, `err` holds it |
| 2 | native panic; `err` holds a managed string with the message, not an exception |
| 3 | pipeline-thread-only entry called from another thread |
| 4 | ABI version mismatch |
| 5 | pipeline stopped; return promptly |

## Native exports

| Export | Signature |
|---|---|
| `pwrs_module_init` | `(vtable: *const HostVTable) -> PsStatus` |
| `pwrs_module_descriptor` | `() -> ModuleDescriptor` |
| `pwrs_cmdlet_create` | `(cmdlet_id: u32, out: *mut *mut c_void, phases: *mut u32, err: *mut PsHandle) -> PsStatus`; makes the Rust instance the managed cmdlet owns and reports the type's phase mask |
| `pwrs_cmdlet_invoke` | `(instance: *mut c_void, phase: u32, cmdlet: PsHandle, params: *const c_void, err: *mut PsHandle) -> PsStatus` |
| `pwrs_cmdlet_stop` | `(instance: *mut c_void)`; foreign thread, sets the stop flag |
| `pwrs_cmdlet_release` | `(instance: *mut c_void)`; drops the Rust instance when the managed cmdlet is disposed |
| `pwrs_proxy_drop` | `(class_id: u32, instance: *mut c_void)` |
| `pwrs_proxy_get` | `(class_id: u32, field_id: u32, instance: *mut c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus` |
| `pwrs_proxy_call` | `(class_id: u32, method_id: u32, instance: *mut c_void, args: *const c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Proxy methods below |
| `pwrs_proxy_bytes` | `(class_id: u32, instance: *mut c_void) -> u64`; the bytes of native memory a proxy value reports through its class's `native_bytes` function, 0 for a class without one, an unknown class id, or a function that panics. Bound optionally |
| `pwrs_completer_invoke` | `(completer_id: u32, word: PsStr16, command_text: PsStr16, fake_bound: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Argument completers below |
| `pwrs_dynparams_invoke` | `(cmdlet_id: u32, cmdlet: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Argument completers below |
| `pwrs_transform_invoke` | `(transform_id: u32, value: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Argument transformations below. Bound optionally |
| `pwrs_provider_invoke` | `(provider_id: u32, op: u32, instance: *mut c_void, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Providers below |
| `pwrs_module_lifecycle` | `(op: u32, err: *mut PsHandle) -> PsStatus`; op 0 is the import hook and op 1 the removal hook, each doing nothing when the module declared no hook for it. Bound optionally |
| `pwrs_cpu_requirements` | Data: NUL-terminated ASCII, `PWRS-CPU/1` then ` name,level,leaf,subleaf,register,bit,xcr0` in decimal for each x86-64 extension the library was compiled to require. Read optionally, before any other export runs |
| `pwrs_cpuid` | `(leaf: u32, subleaf: u32, out: *mut u32)`; x86-64 only, naked: CPUID, EAX to EDX into `out[0..4]` |
| `pwrs_xgetbv` | `(xcr: u32) -> u64`; x86-64 only, naked: XGETBV, called only when CPUID leaf 1 reports OSXSAVE |

Six exports are bound optionally, with `TryExport` rather than
`Export`: a library built before `pwrs_module_lifecycle` or
`pwrs_transform_invoke` existed loads and is treated as declaring no
hooks and no transforms, one built before `pwrs_proxy_bytes` reports no
native bytes, and one built before the CPU-check exports is loaded
unchecked. Every other export is required, and a library missing one
fails to load.

The managed cmdlet holds the instance pointer for its lifetime, so an
invocation carries no lookup and takes no lock: the native side reads
the pointer it is handed.

The phase mask has bit 1 for Begin, 2 for Process, and 4 for End, and
starts at 7 for every cmdlet type. The trait's default `begin` and
`end` bodies mark the pipeline token; when the runtime sees a phase
return with the mark set it clears that phase's bit in the type's
static mask, and every later `pwrs_cmdlet_create` for the type reports
the reduced mask. The generated `BeginProcessing` and `EndProcessing`
call native only when their bit is set, so a cmdlet that implements
only `process` costs one native call per command-line invocation
after its first. An implemented `begin` or `end` never sets the mark
and is always called.

## Host vtable entries (version 1)

Slot order is the declaration order in `HostVTable`. Entries marked
"pipeline thread" return status 3 elsewhere.

| Slot | Entry | Thread |
|---|---|---|
| 0 | free_handle | any |
| 1 | clone_handle | any |
| 2 | write_object | pipeline |
| 3 | write_error | pipeline |
| 4 | write_stream | pipeline |
| 5 | write_progress | pipeline |
| 6 | should_process | pipeline |
| 7 | should_continue | pipeline |
| 8 | get_parameter | pipeline |
| 9 | parameter_is_bound | pipeline |
| 10 | string_new | any |
| 11 | string_read | any |
| 12 | i64_new | any |
| 13 | i64_read | any |
| 14 | f64_new | any |
| 15 | f64_read | any |
| 16 | bool_new | any |
| 17 | bool_read | any |
| 18 | array_len | any |
| 19 | array_get | any |
| 20 | array_new | any |
| 21 | array_set | any |
| 22 | array_pin | any |
| 23 | array_unpin | any |
| 24 | psobject_new | any |
| 25 | psobject_add_note | any |
| 26 | psobject_get_property | any |
| 27 | get_variable | pipeline |
| 28 | set_variable | pipeline |
| 29 | resolve_path | pipeline |
| 30 | invoke_scriptblock | pipeline |
| 31 | dyn_get | any |
| 32 | dyn_set | any |
| 33 | dyn_call | any |
| 34 | dyn_call_static | any |
| 35 | dyn_new | any |
| 36 | factory_new | any |
| 37 | memory_view_new | any |
| 38 | exception_describe | any |
| 39 | write_string | pipeline |
| 40 | write_i64 | pipeline |
| 41 | write_f64 | pipeline |
| 42 | write_bool | pipeline |
| 43 | u64_new | any |
| 44 | u64_read | any |
| 45 | datetime_new | any |
| 46 | datetime_read | any |
| 47 | timespan_new | any |
| 48 | timespan_read | any |
| 49 | guid_new | any |
| 50 | guid_read | any |
| 51 | char_new | any |
| 52 | char_read | any |
| 53 | securestring_new | any |
| 54 | securestring_read | any |
| 55 | array_element_tag | any |
| 56 | stream_enabled | pipeline |
| 57 | readonly_table_new | any |
| 58 | object_type_tag | any |
| 59 | i8_new | any |
| 60 | i16_new | any |
| 61 | i32_new | any |
| 62 | u8_new | any |
| 63 | u16_new | any |
| 64 | u32_new | any |
| 65 | f32_new | any |
| 66 | decimal_new | any |
| 67 | decimal_read | any |
| 68 | datetimeoffset_new | any |
| 69 | datetimeoffset_read | any |
| 70 | invoke_command | pipeline |
| 71 | enum_new | any |
| 72 | proxy_enter | any; paired with proxy_exit on the same thread |
| 73 | proxy_exit | the thread that entered |
| 74 | helper_path | any |
| 75 | proxy_enter_shared | any; paired with proxy_exit on the same thread |

## Helper executables

`helper_path(name, buf, cap, out_len, err)` answers where to start a
helper executable the module ships in `runtimes/<rid>/native/`, `name`
being its file name without `.exe`. The first request for a helper's
bytes copies it into the folder the process stages the module's
library in, at `native/<file>.<mark>/<file>`, where the mark is the
shipped file's write time and length in hex; every later request
answers the same path. The copy is written under a temporary name and
moved into place, so no caller is answered a file another thread is
still writing. The path is copied into `buf` like `string_read`'s. A
name that is not a bare file name, and a helper the module does not
ship, are the status. On .NET Framework each module's own table
answers this entry, as it answers `factory_new`.

## Error flow through a phase

A body `Err` is reported by the native side through `write_error`
before `pwrs_cmdlet_invoke` returns. Non-terminating records go to
`WriteError` immediately. A terminating record is stored on the
managed cmdlet and `write_error` returns 0; the managed `Invoke`
raises it with `ThrowTerminatingError` after the native call returns,
so no managed exception ever unwinds through native frames. A
`PipelineStoppedException` inside any entry marks the cmdlet stopped
and returns status 5; `Invoke` rethrows it after the native call.

Dynamic static calls (`dyn_call_static`, `dyn_new`) use the .NET
default binder for overload resolution; instance calls (`dyn_call`)
use PowerShell's own member binder.

## Parameter block

Per cmdlet the generator emits a `#[repr(C)]` struct on both sides:
a `dirty: u64` word, a `bound: u64` word, then one slot per parameter
in declaration order. Bit `i` of `bound` is set when parameter `i`
was ever assigned; bit `i` of `dirty` is set when it was assigned
since the previous phase. Both are recorded by the generated property
setters, which the binder calls only for supplied parameters, so no
dictionary is consulted on the call path. The native side binds
before the first phase and again only when `dirty` is non-zero, which
is how pipeline input rebinds per record while a command-line
invocation binds once across its three phases. `dirty` is first so the
runtime can read it before the block's type is known. The block lives
on the managed stack for exactly one phase.

| Rust parameter type | CLR property | Slot |
|---|---|---|
| `bool` | `SwitchParameter` | `u8` / `byte` |
| `i8`..`i64`, `u8`..`u64` | `sbyte`..`long`, `byte`..`ulong` | same width |
| `f32`, `f64` | `float`, `double` | same |
| `String`, `PathBuf` | `string?` | `PsStr16` (pinned `fixed` for the call) |
| `Vec<T>` | `T[]?` | `PsHandle` (a `GCHandle` allocated for the call, freed after) |
| `PsObject` | `object?` | `PsHandle` |
| `PsScriptBlock`, `PsHashtable` | `ScriptBlock?`, `Hashtable?` | `PsHandle` |
| a `#[psenum]` type | the generated enum, boxed | `PsHandle` |
| a `#[psclass]` type | the generated class (`PSObject?` for psobject mode) | `PsHandle` |
| `PsDateTime`, `PsTimeSpan`, `PsGuid`, `char` | `DateTime`, `TimeSpan`, `Guid`, `char`, boxed | `PsHandle` |
| `PsSecureString`, `PsCredential` | `SecureString?`, `PSCredential?` | `PsHandle` |
| `Option<T>` | as `T`; unbound is `None` via the bitmask | as `T` |

A parameter whose `bound` bit is clear is not read: the field keeps
its value, so a handle type's `FromPs` is never handed a null for an
unbound parameter (a piped parameter during Begin, for instance).

A type implementing `PsTyped` (a `#[psenum]` or `#[psclass]` type,
`PsDateTime`, `PsTimeSpan`, `PsGuid`, `char`, `PsSecureString`,
`PsCredential`) reaches the descriptor through the trait, which names
its CLR type and says whether it is a value type. The shell boxes a value type without
a null check and reads a class field of it back with an unboxing
cast. A class field holding an array is read through
`LanguagePrimitives.ConvertTo`, so the array `IntoPs` built, whose
element type follows the Rust element's type tag, converts to the
declared element type.

Handles in the block are owned by the managed side for the duration
of the phase; the Rust reader clones them before keeping them.

## Typed values

| Entries | Representation |
|---|---|
| `datetime_new(ticks, kind, out, err)`, `datetime_read(h, out_ticks, out_kind, err)` | ticks of 100 ns from the start of year 1 and the `DateTimeKind` (0 unspecified, 1 UTC, 2 local); `datetime_new` fails outside the type's range; `datetime_read` converts through `LanguagePrimitives.ConvertTo<DateTime>` |
| `timespan_new(ticks)`, `timespan_read(h, out_ticks, err)` | ticks of 100 ns, negative allowed |
| `guid_new(bytes)`, `guid_read(h, out_bytes, err)` | the 16 bytes of `Guid.ToByteArray` |
| `char_new(unit)`, `char_read(h, out_unit, err)` | one UTF-16 code unit |
| `securestring_new(text, out, err)` | a read-only `SecureString`; fails past 65536 units |
| `securestring_read(h, buf, cap, out_len, err)` | decrypts into an unmanaged buffer, copies up to `cap` units, reports the full length, and zeroes the buffer; `cap` 0 reports the length only |
| `array_element_tag(h, out_tag, err)` | the element tag of a typed array (`PS_TYPE_U8` for a `byte[]`), `PS_TYPE_OBJECT` for any other array and any other object |

Type tags 14, 15 and 16 (`DateTime`, `TimeSpan`, `Guid`) join the
primitive tags `array_new` accepts, so a `Vec` of those types becomes
a typed array; such arrays are not pinnable.

A `Vec<T>` of a pinnable primitive uses `array_element_tag` to decide
its path: a matching typed array crosses through one `array_pin` and
a memcpy in each direction, anything else through `array_get` and a
read entry per element.

## Output classes

`#[psclass]` has three modes. Every mode registers a `ClassEntry` in
the module table; the index is the class id the generated C# and the
Rust `IntoPs` agree on.

| Mode | Managed shape | Crossing |
|---|---|---|
| `copied` (default) | generated sealed class with one property per field | `IntoPs` packs a field block (same slot rules as parameters plus a `mask: u64` for `Option` fields) and calls `factory_new(class_id, &block)` once; the registered factory copies values out |
| `proxy` | generated sealed class deriving `Pwrs.ProxyBase` | `IntoPs` boxes the value and passes the pointer as the block; each property getter calls `pwrs_proxy_get(class_id, field_id, ptr)`; `Dispose` or the finalizer calls `pwrs_proxy_drop(class_id, ptr)` exactly once |
| `psobject` | none | `IntoPs` builds a `PSObject`, inserts the `PSTypeName`, adds one note property per field |
| `enum` (`#[psenum]`) | generated `enum` with `long` underneath | `IntoPs` passes the discriminant as an `i64` block to `factory_new`; the registered factory casts it to the enum. `FromPs` reads the value back through `i64_read` |

A class id names a class only within its own module, so each module's
factories are a table of their own. On PowerShell 7 every module has a
copy of `Pwrs.Runtime` to itself, and `factory_new` in the one host
table reads that copy's table. On Windows PowerShell one copy serves
every module in the process, so each module's library is handed its
own copy of the host table, identical except for `factory_new` and
`proxy_enter`, which answer from that module's table; the shared
table's two entries refuse there, since no module is handed it.
`Pwrs.NativeModule.FactoriesPerModule` is `true` on a runtime that
works this way and absent on one that does not.

Proxy reads after `Dispose` throw `ObjectDisposedException` from the
getter; PowerShell's property adapter turns that into a `$null` read
with no error record, so scripts see `$null` and `IsDisposed` is the
reliable check. A proxy
that holds a `PsObject` referring back to itself is a cross-runtime
cycle and leaks; nothing detects that yet.

Every call into a proxy's value takes the object's gate: a property
read, a method call, and a native borrow, which `PsProxy::with` and
`with_mut` make through `proxy_enter_shared`, `proxy_enter` and
`proxy_exit`. An entry is shared or exclusive. A property read, a
method taking `&self` and a `with` borrow enter as shared, and shared
entries nest on the thread inside one, so a method may read the object
it runs on, as it does when the receiver is passed back as a by-value
argument. A method taking `&mut self` and a `with_mut` borrow enter
exclusively: refused while anything is inside, and refusing everything
while they run, with "in use by a call already running on this
thread". The generated shell passes each method's kind, carried by the
descriptor as `mutable`. A call from another thread waits for the gate
whatever its kind. `proxy_enter` and `proxy_enter_shared` check that
the handle is a live proxy of the calling module's class `class_id`
from the current load and write the value's pointer; `proxy_exit`
releases the entry, with `changed` nonzero after `with_mut`. `Dispose`
on the thread holding the gate frees the value when its last entry
ends; on another thread it waits for the gate.

A class declared with `native_bytes` passes `true` as the base
constructor's fourth argument. The wrapper then calls `pwrs_proxy_bytes`
once when it is made and reports the answer with `GC.AddMemoryPressure`,
calls it again after each method call and each `proxy_exit` with
`changed`, adding or removing the difference, and removes what is left
when the value is freed.

## Proxy methods

`#[psmethods]` adds a `methods` array to a proxy class's descriptor:
per method its PascalCase `name`, `rust` name, `index`, `help`,
`params` (each with `name`, `rust`, `index`, `clr`, `value_type`,
`slot`, `optional`) and `ret` (`clr`, `value_type`, `optional`, or
`null` for a method returning `()`). The generated class declares
one C# method per entry; the method packs a `#[repr(C)]` block of
`bound: u64` then one slot per argument in declaration order, with
the slot rules of a parameter block and bit `i` of `bound` set when
argument `i` was supplied, and calls `pwrs_proxy_call(class_id,
method_id, instance, &block)`. `out` receives the result as a handle
the managed side owns, null for `()`; an `Err` from the method is
status 1 with the message in `err`, thrown to the script as a
`PwrsException`. The managed base serializes `Get` and `Call` on one
object, since a method may take `&mut self`.

## Argument completers and dynamic parameters

`pwrs_completer_invoke(completer_id: u32, word: PsStr16,
command_text: PsStr16, fake_bound: PsHandle, out: *mut PsHandle,
err: *mut PsHandle) -> PsStatus` runs on the completion thread, so it
receives no cmdlet handle and the Rust side gets no `Pipeline`. `out`
is an `object[]` whose elements are `string[4]`: completion text,
list item text, result type name (`ParameterValue`, `ParameterName`,
`Text`, ...), tooltip. The generated shell attaches
`[ArgumentCompleter(typeof(<generated class>))]` to the parameter and
the class forwards to this export.

`pwrs_dynparams_invoke(cmdlet_id: u32, cmdlet: PsHandle, out: *mut
PsHandle, err: *mut PsHandle) -> PsStatus` runs on the pipeline
thread before binding, from `IDynamicParameters.GetDynamicParameters`.
`out` is one string, a line per parameter joined by `\u{3}`, each of
seven cells joined by `\u{1}`: name, CLR type name, mandatory
(`1`/`0`), position (`-1` for named), parameter set name, help,
validate-set values joined by `\u{2}`; null for no parameters. Bound
values are read back with `get_parameter`.

## Argument transformations

`pwrs_transform_invoke(transform_id: u32, value: PsHandle, out: *mut
PsHandle, err: *mut PsHandle) -> PsStatus` runs during binding, from
`ArgumentTransformationAttribute.Transform` on the generated
property. `value` is the argument as the caller wrote it, before the
engine coerces it to the parameter's declared type and before
validation; `out` is what to assign instead, and returning the value
unchanged is correct for anything the transform does not recognize.

No cmdlet instance exists at that point, so the export takes no
instance pointer and no cmdlet handle, and the Rust side gets no
`Pipeline`. A failed status becomes an
`ArgumentTransformationMetadataException`, which the engine reports
as a binding failure naming the parameter, so the cmdlet body never
runs.

## Providers

`pwrs_provider_invoke(provider_id: u32, op: u32, instance: *mut
c_void, args: PsHandle (object[]), out: *mut PsHandle (object[]),
err)` runs one provider operation against the Rust instance serving
the current drive. `op` is a code in `pwrs::provider::op` (item
exists, get item, get child items, new item, remove item, rename,
get/set content, new drive, remove drive, drop drive, init default
drives, make path, get parent/child, ...). `args` are the operation's
inputs as an `object[]` (paths as strings, flags as bools, a value
where relevant); the result is an `object[]` of rows. An item row is
`[path, value, is_container]`; a drive row is `[name, root,
instance]`, the pointer as an `i64`.

One instance serves one drive. `INIT_DEFAULT_DRIVES` and `NEW_DRIVE`
take a null `instance` and return drive rows whose pointers the
managed `PwrsDriveInfo` (a `PSDriveInfo` subclass) keeps; every item,
container and content operation passes the pointer of the drive the
engine resolved for it, and null when it resolved none, which the
native side answers with `PwrsProviderNoDrive`. The path operations
(`IS_VALID_PATH`, `MAKE_PATH`, `GET_PARENT_PATH`, `GET_CHILD_NAME`,
`NORMALIZE_RELATIVE_PATH`) ignore the pointer. `REMOVE_DRIVE` runs
`remove_drive` and frees the instance when that returns `Ok`;
`DROP_DRIVE` frees it without running `remove_drive` and is what the
drive object's finalizer sends for a drive that was never removed. The
managed drive object serializes the operations on one drive. The
generated `NavigationCmdletProvider` subclass does the engine-facing
work (`WriteItemObject` with the row's path, `ShouldProcess`, content
reader/writer) and calls this export per operation, so no per-item
callback ABI is needed.

## Generated shell

Per cmdlet the C# shell is one sealed class deriving `Pwrs.RustCmdlet`
with `[Cmdlet]`, `[Alias]`, `[OutputType]`, one property per parameter
carrying `[Parameter]` and validation attributes whose setter records
the bound and dirty bits, a nested `Block` struct mirroring the Rust
block, and a `Run(phase)` that packs the block and makes one `Invoke`
call. A module-level static holds the `NativeModule`. There is no
reflection in that path and `MyInvocation` is never touched by it; the
vtable's `get_parameter` and `parameter_is_bound` entries use
`MyInvocation.BoundParameters` and are for dynamic parameters only.

Scalar outputs (`String`, `&str`, `i64`, `f64`, `bool`) cross through
the `write_string`, `write_i64`, `write_f64` and `write_bool` entries:
one crossing and no `GCHandle`, against `string_new`, `write_object`
and `free_handle` for the generic object path.
