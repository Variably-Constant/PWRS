---
title: ABI Reference
weight: 6
---

The native contract, version 1. `crates/pwrs-sys/src/lib.rs` and `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs` describe the same memory and change in the same commit; `docs/ABI.md` in the repository is the normative copy of this page.

## Rules

- Calling convention `extern "C"` everywhere; x64 and arm64.
- No unwinding across the boundary in either direction.
- Managed objects cross only as `PsHandle` (the `IntPtr` of a `GCHandle`) or as `PsPinned` views released before the phase returns.
- Strings cross as UTF-16 `(ptr, len)`; readers size and retry.
- The table is append-only. The 8-byte header is `size: u32, version: u32`. A module checks `size` before using an entry newer than the version it was compiled against. `version` changes only when an existing entry changes meaning, and then the export names gain the major (`pwrs2_*`).

## Status codes

| Code | Meaning |
|---|---|
| 0 | ok |
| 1 | managed exception; `err` holds it |
| 2 | native panic; `err` holds a managed string with the message |
| 3 | pipeline-thread-only entry called from another thread |
| 4 | ABI version mismatch |
| 5 | pipeline stopped; return promptly |

## Native exports

| Export | Signature |
|---|---|
| `pwrs_module_init` | `(vtable: *const HostVTable) -> PsStatus` |
| `pwrs_module_descriptor` | `() -> ModuleDescriptor` (`json_utf8: *const u8, json_len: usize`) |
| `pwrs_cmdlet_create` | `(cmdlet_id: u32, out: *mut *mut c_void, phases: *mut u32, err: *mut PsHandle) -> PsStatus` |
| `pwrs_cmdlet_invoke` | `(instance: *mut c_void, phase: u32, cmdlet: PsHandle, params: *const c_void, err: *mut PsHandle) -> PsStatus` |
| `pwrs_cmdlet_stop` | `(instance: *mut c_void)` |
| `pwrs_cmdlet_release` | `(instance: *mut c_void)` |
| `pwrs_proxy_get` | `(class_id: u32, field_id: u32, instance: *mut c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus` |
| `pwrs_proxy_call` | `(class_id: u32, method_id: u32, instance: *mut c_void, args: *const c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus` |
| `pwrs_proxy_drop` | `(class_id: u32, instance: *mut c_void)` |
| `pwrs_proxy_bytes` | `(class_id: u32, instance: *mut c_void) -> u64`; the bytes of native memory a proxy value reports through its class's `native_bytes` function, 0 for a class without one, an unknown class id, or a function that panics. The runtime binds this export optionally, so a library built before it existed loads and reports no native bytes |
| `pwrs_completer_invoke` | `(completer_id: u32, word: PsStr16, command_text: PsStr16, fake_bound: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus` |
| `pwrs_transform_invoke` | `(transform_id: u32, value: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; runs a `#[transform]` over the value the binder is about to assign and returns what to assign instead. Called during binding, before the cmdlet instance exists. The runtime binds this export optionally, so a library built before it existed loads and has no transforms |
| `pwrs_dynparams_invoke` | `(cmdlet_id: u32, cmdlet: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; runs on the pipeline thread before binding, from `GetDynamicParameters`. `out` is one string: a line per parameter joined by U+0003, each of seven cells joined by U+0001 (name, CLR type name, mandatory as `1` or `0`, position with `-1` for named, parameter set name, help, validate-set values joined by U+0002), and null for no parameters |
| `pwrs_provider_invoke` | `(provider_id: u32, op: u32, instance: *mut c_void, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus` |
| `pwrs_module_lifecycle` | `(op: u32, err: *mut PsHandle) -> PsStatus`; op 0 runs the import hook and op 1 the removal hook, and either does nothing when the module declared no hook for it. The runtime binds this export optionally, so a library built before it existed loads and is treated as declaring neither |
| `pwrs_cpu_requirements` | Data, not a function: NUL-terminated ASCII, `PWRS-CPU/1` then ` name,level,leaf,subleaf,register,bit,xcr0` in decimal for each x86-64 extension the library was compiled to require. The runtime reads it before calling any other export and refuses the import when the CPU lacks one, or when `PWRS_CPU_MAX` caps below it; a library without it is not checked |
| `pwrs_cpuid` | `(leaf: u32, subleaf: u32, out: *mut u32)`; x86-64 only. Naked, so it runs only CPUID and the moves around it, whatever the library was compiled for: EAX to EDX into `out[0..4]` |
| `pwrs_xgetbv` | `(xcr: u32) -> u64`; x86-64 only, naked: XGETBV, which the runtime calls only when CPUID leaf 1 reports OSXSAVE |

Phases: 0 Begin, 1 Process, 2 End. The phase mask reported by `pwrs_cmdlet_create` has bit 1 for Begin, 2 for Process, 4 for End; it starts at 7 and loses a bit once the runtime has seen that phase run the trait's default body for the type.

## Host vtable

Slot order is declaration order. "pipeline" entries return status 3 off the pipeline thread.

| Slot | Entry | Thread | Slot | Entry | Thread |
|---|---|---|---|---|---|
| 0 | `free_handle` | any | 28 | `set_variable` | pipeline |
| 1 | `clone_handle` | any | 29 | `resolve_path` | pipeline |
| 2 | `write_object` | pipeline | 30 | `invoke_scriptblock` | pipeline |
| 3 | `write_error` | pipeline | 31 | `dyn_get` | any |
| 4 | `write_stream` | pipeline | 32 | `dyn_set` | any |
| 5 | `write_progress` | pipeline | 33 | `dyn_call` | any |
| 6 | `should_process` | pipeline | 34 | `dyn_call_static` | any |
| 7 | `should_continue` | pipeline | 35 | `dyn_new` | any |
| 8 | `get_parameter` | pipeline | 36 | `factory_new` | any |
| 9 | `parameter_is_bound` | pipeline | 37 | `memory_view_new` | any |
| 10 | `string_new` | any | 38 | `exception_describe` | any |
| 11 | `string_read` | any | 39 | `write_string` | pipeline |
| 12 | `i64_new` | any | 40 | `write_i64` | pipeline |
| 13 | `i64_read` | any | 41 | `write_f64` | pipeline |
| 14 | `f64_new` | any | 42 | `write_bool` | pipeline |
| 15 | `f64_read` | any | 43 | `u64_new` | any |
| 16 | `bool_new` | any | 44 | `u64_read` | any |
| 17 | `bool_read` | any | 45 | `datetime_new` | any |
| 18 | `array_len` | any | 46 | `datetime_read` | any |
| 19 | `array_get` | any | 47 | `timespan_new` | any |
| 20 | `array_new` | any | 48 | `timespan_read` | any |
| 21 | `array_set` | any | 49 | `guid_new` | any |
| 22 | `array_pin` | any | 50 | `guid_read` | any |
| 23 | `array_unpin` | any | 51 | `char_new` | any |
| 24 | `psobject_new` | any | 52 | `char_read` | any |
| 25 | `psobject_add_note` | any | 53 | `securestring_new` | any |
| 26 | `psobject_get_property` | any | 54 | `securestring_read` | any |
| 27 | `get_variable` | pipeline | 55 | `array_element_tag` | any |
|  |  |  | 56 | `stream_enabled` | pipeline |
|  |  |  | 57 | `readonly_table_new` | any |
|  |  |  | 58 | `object_type_tag` | any |
|  |  |  | 59 | `i8_new` | any |
|  |  |  | 60 | `i16_new` | any |
|  |  |  | 61 | `i32_new` | any |
|  |  |  | 62 | `u8_new` | any |
|  |  |  | 63 | `u16_new` | any |
|  |  |  | 64 | `u32_new` | any |
|  |  |  | 65 | `f32_new` | any |
|  |  |  | 66 | `decimal_new` | any |
|  |  |  | 67 | `decimal_read` | any |
|  |  |  | 68 | `datetimeoffset_new` | any |
|  |  |  | 69 | `datetimeoffset_read` | any |
|  |  |  | 70 | `invoke_command` | pipeline |
|  |  |  | 71 | `enum_new` | any |
|  |  |  | 72 | `proxy_enter` | any; paired with `proxy_exit` on the same thread |
|  |  |  | 73 | `proxy_exit` | the thread that entered |
|  |  |  | 74 | `helper_path` | any |
|  |  |  | 75 | `proxy_enter_shared` | any; paired with `proxy_exit` on the same thread |

## Helper executables

`helper_path(name, buf, cap, out_len, err)` answers where to start a helper executable the module ships in `runtimes/<rid>/native/`, `name` being its file name without `.exe`. The first request for a helper's bytes copies it into the folder the process stages the module's library in, at `native/<file>.<mark>/<file>`, where the mark is the shipped file's write time and length in hex; every later request answers the same path. The copy is written under a temporary name and moved into place, so no caller is answered a file another thread is still writing. The path is copied into `buf` like `string_read`'s. A name that is not a bare file name, and a helper the module does not ship, are the status. On Windows PowerShell each module's own table answers this entry, as it answers `factory_new`. `pwrs::helper_path` is the Rust surface over it; see [How To Ship A Helper Executable](../how-to/How-To-Ship-A-Helper-Executable.md).

## Typed values

| Entries | Representation |
|---|---|
| `datetime_new(ticks, kind, out, err)`, `datetime_read(h, out_ticks, out_kind, err)` | ticks of 100 ns from the start of year 1 and the `DateTimeKind` (0 unspecified, 1 UTC, 2 local); `datetime_new` fails outside the type's range; `datetime_read` converts through `LanguagePrimitives.ConvertTo<DateTime>` |
| `timespan_new(ticks)`, `timespan_read(h, out_ticks, err)` | ticks of 100 ns, negative allowed |
| `guid_new(bytes)`, `guid_read(h, out_bytes, err)` | the 16 bytes of `Guid.ToByteArray` |
| `char_new(unit)`, `char_read(h, out_unit, err)` | one UTF-16 code unit |
| `securestring_new(text, out, err)` | a read-only `SecureString`; fails past 65536 units |
| `securestring_read(h, buf, cap, out_len, err)` | decrypts into an unmanaged buffer, copies up to `cap` units, reports the full length, and zeroes the buffer; `cap` 0 reports the length only |
| `array_element_tag(h, out_tag, err)` | the element tag of a typed array, `PS_TYPE_OBJECT` for anything else; a `Vec<T>` of a pinnable primitive uses it to choose between one pin and the element-by-element path |
| `stream_enabled(cmdlet, kind, out_enabled, err)` | whether the engine would keep a record written to that stream, from the stream's common parameter where it is bound and from the session's preference variable otherwise; the runtime answers once per cmdlet instance and the `Pipeline` keeps the answer for the phase |

| `object_type_tag(h, out_tag, err)` | the tag of one object's own type, `PS_TYPE_OBJECT` for a type outside the vocabulary; unwraps a `PSObject` first, so a wrapped `Int32` answers `PS_TYPE_I32` |
| `i8_new`, `i16_new`, `i32_new`, `u8_new`, `u16_new`, `u32_new`, `f32_new` | each builds the CLR type of its own name instead of widening to `Int64` or `Double` |
| `decimal_new(lo, mid, hi, flags, out, err)`, `decimal_read(h, out_bits, err)` | the four words in `Decimal.GetBits` order, which is not their order in memory; `decimal_new` rejects a scale above 28 or reserved bits set |
| `datetimeoffset_new(ticks, offset_minutes, out, err)`, `datetimeoffset_read(h, out_ticks, out_offset_minutes, err)` | ticks of 100 ns from the start of year 1, and the UTC offset in whole minutes |
| `enum_new(type_name, value, out, err)` | a value of the CLR enum `type_name` names, from its underlying number, through `Enum.ToObject`. The name is resolved with `LanguagePrimitives.ConvertTo<Type>`, the same resolver a type literal uses, and a name it cannot reach or a type that is not an enum is the status. A module's own `#[psenum]` types go through `factory_new` instead, which needs no name |
| `proxy_enter(obj, class_id, out_instance, err)`, `proxy_enter_shared(obj, class_id, out_instance, err)`, `proxy_exit(obj, changed)` | lend a proxy's value to native code under the object's gate, the one its property reads and method calls take. Both enter forms check that `obj` is a live proxy of the calling module's class `class_id` from the current load. `proxy_enter` is the exclusive entry `with_mut` makes, refused while anything is inside the object on the calling thread and refusing everything while it lasts, as a `&mut self` method is; `proxy_enter_shared` is the shared entry `with` makes, which nests inside a `&self` method, a property read or another shared entry and is refused only inside an exclusive one. `proxy_exit` releases the entry, and with `changed` nonzero asks a `native_bytes` class for its bytes again. On Windows PowerShell each module's own table answers both enter forms, as it answers `factory_new` |
| `invoke_command(cmdlet, name, parameters, input, out, err)` | resolves `name` to a `CommandInfo` through the cmdlet's `SessionState.InvokeCommand` and runs it in a nested `PowerShell` in the current runspace, `parameters` an `IDictionary` bound by name or null, `input` piped in or null (a collection unrolled, a string or other single value one record); `out` receives an `object[]` of everything it wrote. The command's non-terminating errors are written to the calling cmdlet's error stream; a terminating one, and an unknown name, are the status. Pipeline thread |

Type tags 14, 15 and 16 (`DateTime`, `TimeSpan`, `Guid`) join the primitive tags `array_new` accepts, so a `Vec` of those types becomes a typed array; such arrays are not pinnable.

Tag 17 (`Decimal`) is accepted by `array_new` and, unlike those three, its arrays ARE pinnable: `Type.IsPrimitive` is false for `System.Decimal` but it is blittable and pins, so `array_pin` admits it by name. A pinned element is `PsDecimalBits`, the four words in memory order, and the first such pin in a process checks that order against `Decimal.GetBits` before any element is read.

## Parameter block

Per cmdlet, a `#[repr(C)]` struct on both sides: `dirty: u64`, `bound: u64`, then one slot per parameter in declaration order. Bit `i` of `bound` is set when parameter `i` was ever assigned; bit `i` of `dirty` when it was assigned since the previous phase. The generated property setters maintain both. The native side binds before the first phase and again only when `dirty` is non-zero, and leaves a parameter whose `bound` bit is clear at its field's value. Slots: `u8` for switches, the numeric types at their width, `PsStr16` for strings (pinned `fixed` for the call), `PsHandle` for arrays, objects, class objects, script blocks, hashtables, secure strings, credentials and boxed value types (enums, `DateTime`, `TimeSpan`, `Guid`, `char`), a `GCHandle` allocated for the call and freed after. The block lives on the managed stack for one phase; the Rust reader clones handles before keeping them.

## Class field block

Copied classes pack one slot per field in the same way, followed by `mask: u64` with a bit per `Option` field that is `Some`; `factory_new(class_id, &block)` hands it to the registered factory, which unboxes value-typed handles with a cast and converts array-typed ones with `LanguagePrimitives.ConvertTo` to the declared element type. Proxy classes pass the boxed instance pointer instead. Enums pass a single `i64`.

A class id is its class's position in its own module's `export_module!` list, so it names a class only within one module, and each module's factories are a table of their own. On PowerShell 7 every module has a copy of `Pwrs.Runtime` to itself, so `factory_new` in the one host table reads that copy's table. On Windows PowerShell one copy serves every module in the process, so each module's library is handed its own copy of the host table, identical except for `factory_new`, which answers from that module's table; the shared table's `factory_new` refuses there, since no module is handed it. `Pwrs.NativeModule.FactoriesPerModule` is `true` on a runtime that works this way and absent on one that does not.

## Method block

A `#[psmethods]` method on a proxy class receives `bound: u64` then one slot per argument in declaration order, packed by the generated C# method with the slot rules of a parameter block; bit `i` of `bound` is set when argument `i` was supplied. `pwrs_proxy_call` returns the result as a handle (null for `()`); an `Err` is status 1 with the message, thrown to the script as a `PwrsException`. The managed base serializes calls and property reads on one object.

## Provider drives

A provider operation carries the pointer of the Rust instance serving the current drive. The drive-making operations (`INIT_DEFAULT_DRIVES`, `NEW_DRIVE`) take null and return rows `[name, root, instance]` whose pointers the managed `PwrsDriveInfo` keeps; the path operations ignore the pointer; every other operation needs it and fails with `PwrsProviderNoDrive` when the engine resolved no drive of the provider. `REMOVE_DRIVE` runs `remove_drive` and frees the instance on `Ok`; `DROP_DRIVE` frees it without running `remove_drive`, for a drive the engine collected without removing. Operations on one drive are serialized by the managed drive object.

## Error flow through a phase

A body `Err` is written through `write_error` before `pwrs_cmdlet_invoke` returns: non-terminating records go to `WriteError` at once; a terminating record is stored on the managed cmdlet and raised with `ThrowTerminatingError` after the native call returns. A `PipelineStoppedException` inside any entry marks the cmdlet stopped and returns status 5; the managed `Invoke` rethrows it after the native call. A panic in a phase returns status 2 and the managed side raises a terminating `PwrsNativePanic` error with the message.
