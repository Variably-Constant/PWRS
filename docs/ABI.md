# PWRS native ABI

Version 1. The Rust definition in `crates/pwrs-sys/src/lib.rs` and the
C# table builder in `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs` describe the
same memory.

## Rules

- Calling convention `extern "C"` everywhere. x64 and arm64 only.
- No unwinding across the boundary. Rust exports that run the module's
  code wrap `catch_unwind`; managed entries that return a status wrap
  `try/catch` and return status 1 with a `GCHandle` to the exception
  in the `err` out-pointer.
- Managed objects pass between the two sides only as `PsHandle` (a
  `GCHandle` `IntPtr`) or as `PsPinned` views released before the phase
  returns.
- Strings pass as UTF-16 `(ptr, len)`. An entry that reads a string
  writes what fits and reports the full length, so a caller can make a
  buffer of that length and call again, or read a long string in place
  through `string_pin`.
- The table is append-only. The 8-byte header is `size: u32,
  version: u32`. At `pwrs_module_init` a module refuses, with status
  4, a table whose `size` is smaller than the one it was compiled
  against or whose `version` differs, so it never calls through an
  entry the runtime lacks. `version` changes only when an existing
  entry changes meaning.

## Status codes

| Code | Meaning |
|---|---|
| 0 | ok |
| 1 | managed exception, `err` is set to it |
| 2 | native panic; `err` is set to a managed string with the message, not an exception |
| 3 | pipeline-thread-only entry called from another thread |
| 4 | ABI version mismatch |
| 5 | pipeline stopped; return promptly |

## Native exports

| Export | Signature |
|---|---|
| `pwrs_module_init` | `(vtable: *const HostVTable) -> PsStatus` |
| `pwrs_module_descriptor` | `() -> ModuleDescriptor` |
| `pwrs_cmdlet_create` | `(cmdlet_id: u32, out: *mut *mut c_void, phases: *mut u32, err: *mut PsHandle) -> PsStatus`; makes the Rust instance the managed cmdlet uses and reports the type's phase mask |
| `pwrs_cmdlet_invoke` | `(instance: *mut c_void, phase: u32, cmdlet: PsHandle, params: *const c_void, err: *mut PsHandle) -> PsStatus` |
| `pwrs_cmdlet_stop` | `(instance: *mut c_void)`; called from another thread, sets the stop flag |
| `pwrs_cmdlet_release` | `(instance: *mut c_void)`; drops the Rust instance when the managed cmdlet is disposed |
| `pwrs_proxy_drop` | `(class_id: u32, instance: *mut c_void)` |
| `pwrs_proxy_get` | `(class_id: u32, field_id: u32, instance: *mut c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus` |
| `pwrs_proxy_call` | `(class_id: u32, method_id: u32, instance: *mut c_void, args: *const c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Proxy methods below |
| `pwrs_proxy_bytes` | `(class_id: u32, instance: *mut c_void) -> u64`; the bytes of native memory a proxy value reports through its class's `native_bytes` function, 0 for a class without one, an unknown class id, or a function that panics. Bound optionally |
| `pwrs_completer_invoke` | `(completer_id: u32, word: PsStr16, command_text: PsStr16, fake_bound: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Argument completers below |
| `pwrs_dynparams_invoke` | `(cmdlet_id: u32, cmdlet: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Argument completers below |
| `pwrs_transform_invoke` | `(transform_id: u32, value: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Argument transformations below. Bound optionally |
| `pwrs_provider_invoke` | `(provider_id: u32, op: u32, instance: *mut c_void, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Providers below |
| `pwrs_module_lifecycle` | `(op: u32, err: *mut PsHandle) -> PsStatus`; op 0 is the import hook, op 1 the removal hook, op 2 the load hook and op 3 the unload hook, each doing nothing when the module declared no hook for it. An error comes back through `err` as `[message, id, category, details, target]`, as a provider operation's does. Bound optionally |
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

The managed cmdlet stores the instance pointer for its whole life, so an
invocation does no lookup and takes no lock: the native side reads the
pointer it is given.

The phase mask has bit 1 for Begin, 2 for Process, and 4 for End, and
starts at 7 for every cmdlet type. The trait's default `begin` and
`end` bodies mark the pipeline token; when a phase returns with the
mark set, the runtime clears that phase's bit in the type's static
mask, and every later `pwrs_cmdlet_create` for the type reports
the reduced mask. The generated `BeginProcessing` and `EndProcessing`
call native only when their bit is set, so a cmdlet that implements
only `process` costs one native call per command-line invocation
after its first. An implemented `begin` or `end` never sets the mark
and is always called. A `BeginProcessing` that does not call native
clears the block's dirty words, which the arguments set before `begin`,
so at the first `ProcessRecord` the dirty words have only what was
assigned after it; that call binds anyway, since its instance has not
been bound.

## Host vtable entries (version 1)

Slot order is the declaration order in `HostVTable`. An entry marked
"pipeline" returns status 3 when it is called from another thread.

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
| 76 | write_error_details | pipeline |
| 77 | exception_new | any |
| 78 | write_progress_record | pipeline |
| 79 | string_pin | any |
| 80 | memory_view_borrow | any |
| 81 | task_settle | any |
| 82 | event_raise | any |
| 83 | write_host | pipeline |
| 84 | native_file_path | any |

## Engine events

`event_raise(events, source_identifier, sender, args, message_data,
err)` raises an engine event through `events`, a handle to a
`PSEventManager`, with the arguments `New-Event` passes to
`GenerateEvent`: `sender` and `args` as their base objects, `args` as
an `object[]` of an array's elements (empty for null, the one value for
anything else), `message_data` as a `PSObject` (null for none), and
`processInCurrentThread` true with `waitForCompletionInCurrentThread`
false. The event is queued, or passed to the actions subscribed to it,
on the calling thread before the entry returns, so the events one
thread raises arrive in the order raised; an action runs later on the runspace's
pipeline thread. The entry does not free the handles; the caller does.
`Pipeline::events` reads the manager off the cmdlet's `Events` through
`dyn_get`, so getting the manager needs no entry of its own.

## Host writes

`write_host(cmdlet, text, foreground, background, no_new_line, err)`
writes `text` to the host as `Write-Host` does: it builds a
`HostInformationMessage` with `Message` set to `text` and `NoNewLine` to
whether `no_new_line` is nonzero, and writes it with
`WriteInformation(message, new[] { "PSHOST" })`. `foreground` and
`background` are `System.ConsoleColor` values 0 to 15, or
`PS_HOST_COLOR_CURRENT` (-1) for the color `Host.UI.RawUI` reports,
which is what `Write-Host` uses for a color it is not given. On a host
with no `RawUI` that read throws `HostException`, and the entry leaves
both colors unset, as `Write-Host` does; the engine then shows the text
in the host's colors, or with a given foreground over `Black`. A color
outside that range is a managed exception. The engine shows the record
whatever the information preference is, unless it is Ignore or a
redirection takes it, adds it to `-InformationVariable`, and a
transcript records it as one line, with its trailing white space
removed.

## Tasks

A `#[psmethods]` argument of slot `task` is the task the method
finishes. Its slot is a `PsTaskSlot { source: PsHandle, cancelled:
*const u8 }`: the generated method makes a `Pwrs.TaskSource<T>`, which
has a `TaskCompletionSource<T>` created with
`RunContinuationsAsynchronously` and registers a callback on the
caller's `CancellationToken`, and packs a handle to it with the byte
that callback sets nonzero. The Rust side clones the handle for itself
and reads the byte with an atomic load until it finishes the task.
`task_settle(source, outcome, value, err)` finishes it: outcome `0`
completes it with `value` as the result (null for a task with no
value), `1` faults it with `value` in the form a failing export returns
through `err` (an exception, or a string `[id] message` thrown as a
`PwrsException`), and `2` cancels it with the caller's token. Only the
first call has an effect; it disposes the token's registration, waiting
for a callback another thread is running, and only then frees the byte.
When the generated method throws, the task faults with the same
exception unless Rust has already finished it. The method's descriptor
lists the task among its `params`, with the element type as its `clr`
(`void` for `()`).

## Memory views with an owner

`memory_view_borrow(tag, ptr, len, read_only, refused, context,
release, out, err)` wraps `len` elements of the primitive `tag` at
`ptr` as a `Memory<T>`, or a `ReadOnlyMemory<T>` when `read_only` is
nonzero, over that memory on .NET, and copies them into a `T[]` on
.NET Framework. On success the host stores `context` and calls
`release(context)` exactly once: when the view's manager is disposed
or finalized on .NET, and before the entry returns on .NET Framework.
On failure it has not called `release` and never will, so the module
must still free `context` itself.

`refused`, when not null, points at a byte the module sets nonzero to
refuse the view, and must stay readable until `release` runs. The host
reads it when the view is made, refusing one already refused, and on
.NET before every `GetSpan` and `Pin`. A refusal is an
`ObjectDisposedException`, and so is `GetSpan` or `Pin` on a manager
already disposed. `memory_view_new(tag, ptr, len, drop_fn, out, err)`
is this entry with `ptr` as the context, writable and never refused.
`PsMemory<T>` and `PsMemoryView<T>` are both passed through
`memory_view_borrow`; the context is a box containing the owner and the
`Arc` whose `AtomicBool` is the refusal byte.

## Strings read in place

`string_pin(h, out, err)` converts `h` to a string as `string_read`
does and pins that string in place: `out` is set to the address of its
first UTF-16 unit, its length in units, an element size of 2 and the
pin. The units are read, never written, and the pin is released with
`array_unpin` before the phase that pinned it returns.
`String::from_ps` reads a string of up to 256 units through
`string_read` into a buffer of that size, and a longer one through
`string_pin`, converting the pinned units to UTF-8 with no copy in
between.

## Progress records with every field

`write_progress_record(cmdlet, activity_id, parent_activity_id,
activity, status, current_operation, percent, seconds_remaining,
completed, err)` writes a `ProgressRecord` with every field set. It is
the completing record when `completed` is nonzero and a processing one
otherwise, so a processing record can have a negative percent, which
shows no percentage. A `percent` above 100 is 100, a negative
`parent_activity_id` leaves the activity unnested, a negative
`seconds_remaining` shows no time left, and an empty
`current_operation` sets none. PowerShell refuses an empty activity or
status, a negative activity id, and a parent activity id equal to the
activity id, and that error comes back through `err`. In
`write_progress`, a negative `percent` writes the completing record.

## Errors with details

`write_error_details(cmdlet, message, error_id, category, target,
terminating, details, err)` is `write_error` for a `PsError` with
details: the record's exception has `message` as its Message and an
inner exception whose Message is `details`, so the default error view
shows the one line and `$err.Exception.InnerException.Message` has the
rest. `exception_new(message, details)` returns a handle to such an
exception. A failing export returns it through `err` in place of the
`[id] message` string, and every call site throws an exception it finds
there unchanged, so the errors of proxy methods, static calls, field
reads, providers, completers, transforms and lifecycle hooks reach
PowerShell with their details too; PowerShell wraps a method's exception in a
`MethodInvocationException`, so for a method the details are one level
further in. An error without details goes through `write_error` and the
string.

## Helper executables

`helper_path(name, buf, cap, out_len, err)` returns the path to start
a helper executable from, for a helper the module ships in
`runtimes/<rid>/native/`, `name` being its file name without `.exe`.
The first call for a helper copies it into the folder where the
process stages the module's library, at `native/<file>.<mark>/<file>`,
where the mark is the shipped file's write time and length in hex;
every later call returns the same path. The copy is written under a
temporary name and moved into place, so no caller gets the path of a
file another thread is still writing. The path is copied into `buf` as
`string_read` copies a string. A name that is not a bare file name, or
a helper the module does not ship, fails with status 1. On .NET
Framework each module's own copy of the table provides this entry, as
it provides `factory_new`.

`native_file_path(name, buf, cap, out_len, err)` returns the path to
load a file from, for a file `[package.metadata.pwrs.native-files]`
ships in `runtimes/<rid>/native/`, `name` being its own file name,
extension included. It stages the file as `helper_path` stages a
helper, at `native/<file>.<mark>/<file>`, without changing its mode,
so a library loaded from the copy has no file in the module folder
open. A name that is not a bare file name, or a file the module does
not ship, fails with status 1. On .NET Framework each module's own copy
of the table provides it too.

## Error flow through a phase

A body `Err` is reported by the native side through `write_error`
before `pwrs_cmdlet_invoke` returns. Non-terminating records go to
`WriteError` immediately. A terminating record is stored on the
managed cmdlet and `write_error` returns 0; the managed `Invoke`
raises it with `ThrowTerminatingError` after the native call returns,
so no managed exception ever unwinds through native frames. A
`PipelineStoppedException` inside any entry marks the cmdlet stopped
and returns status 5; `Invoke` rethrows it after the native call.

Dynamic static calls (`dyn_call_static`, `dyn_new`) try the .NET
default binder first. When no overload takes the arguments as they
are, each overload with as many parameters is tried, every argument
converted with `LanguagePrimitives.TryConvertTo`, and the one needing
the fewest conversions is called, the first declared among equals; a
static method's name then matches without regard to case. Instance
calls (`dyn_call`) use PowerShell's own member binder.

## Parameter block

Per cmdlet the generator emits a `#[repr(C)]` struct on both sides:
the dirty words, the bound words, then one slot per parameter in
declaration order. Each mask is one 64-bit word per 64 parameters,
and never fewer than one, so a cmdlet of 64 parameters or fewer has a
`dirty: u64` and a `bound: u64`. In Rust the masks are
`dirty: [u64; N]` and `bound: [u64; N]`. In C# each word is a `ulong`
field, `Dirty`, `Dirty1` and so on, then `Bound`, `Bound1` and so on.
Parameter `i` has bit `i % 64` of word `i / 64`. Its bound bit is set
when it was ever assigned, and its dirty bit when it was assigned
since the previous phase. Both are recorded by the generated property
setters, which the binder calls only for supplied parameters, so the
call path does no dictionary lookup. The native side binds before the
first phase and again only when a dirty word is non-zero, which is how
pipeline input is bound again for each record while a command-line
invocation is bound once across its three phases. The dirty words come
first, and `CmdletMeta::MASK_WORDS` gives how many there are, so the
runtime reads them from the start of any block without casting the
block to the cmdlet's own type. The block lives on the managed stack
for exactly one phase.

| Rust parameter type | CLR property | Slot |
|---|---|---|
| `bool` | `SwitchParameter` | `u8` / `byte` |
| `i8`..`i64`, `u8`..`u64` | `sbyte`..`long`, `byte`..`ulong` | same width |
| `f32`, `f64` | `float`, `double` | same |
| `String`, `PathBuf` | `string?` | `PsStr16` (pinned `fixed` for the call) |
| `Vec<T>` | `T[]?` | `PsHandle` (a `GCHandle` allocated for the call, freed after) |
| `PsArrayMut<T>`, `T` a pinnable primitive or `PsDecimalBits` | `T[]?`, `decimal[]?` for `PsDecimalBits` | `PsHandle` to the caller's own array, which `pin` pins in place |
| `PsObject` | `object?` | `PsHandle` |
| `PsScriptBlock`, `PsHashtable` | `ScriptBlock?`, `Hashtable?` | `PsHandle` |
| a `#[psenum]` type | the generated enum, boxed | `PsHandle` |
| a `#[psclass]` type | the generated class (`PSObject?` for psobject mode) | `PsHandle` |
| `PsDateTime`, `PsTimeSpan`, `PsGuid`, `char` | `DateTime`, `TimeSpan`, `Guid`, `char`, boxed | `PsHandle` |
| `PsSecureString`, `PsCredential` | `SecureString?`, `PSCredential?` | `PsHandle` |
| `Option<T>` | as `T`; unbound is `None`, by its bound bit | as `T` |

A parameter whose bound bit is clear is not read: the field's value
does not change, so a handle type's `FromPs` is never given a null for an
unbound parameter (a piped parameter during Begin, for instance).

A type implementing `PsTyped` (a `#[psenum]` or `#[psclass]` type,
`PsDateTime`, `PsTimeSpan`, `PsGuid`, `char`, `PsSecureString`,
`PsCredential`) reaches the descriptor through the trait, which names
its CLR type and says whether it is a value type. The shell boxes a value type without
a null check and reads a class field of it back with an unboxing
cast. A class field of an array type is read through
`LanguagePrimitives.ConvertTo`, so the array `IntoPs` built, whose
element type follows the Rust element's type tag, converts to the
declared element type.

The managed side frees the handles in the block after the phase; the
Rust reader clones them before storing them.

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

A `Vec<T>` of a pinnable primitive is written as a typed array through
one `array_pin` and a memcpy. It is read the same way when
`array_element_tag` returns `T`'s tag, and otherwise through
`array_get` and a read entry per element.

## Output classes

`#[psclass]` has three modes. Every mode registers a `ClassEntry` in
the module table; the index is the class id the generated C# and the
Rust `IntoPs` both use. A type's id is found through the entry's
`type_id`, its Rust type, so a type the table does not list has none
even when a listed type has its name, and writing it is refused with
`PwrsClassNameTaken`. Each type's id lookup also contains a record for
the build: `PWRS-CLASS/1`, a tab, its name, a tab, its Rust path, then
NUL, so a library has one for every class whose values it can write,
listed or not. `cargo pwrs build` reads the records out of the file and
refuses a type under a name another type has, naming both.

| Mode | Managed shape | How a value is passed |
|---|---|---|
| `copied` (default) | generated sealed class with one property per field | `IntoPs` packs a field block (one slot per field by the parameter rules, then a mask of one 64-bit word per 64 fields, never fewer than one, with bit `i % 64` of word `i / 64` set when `Option` field `i` is `Some`) and calls `factory_new(class_id, &block)` once; the registered factory copies values out |
| `proxy` | generated sealed class deriving `Pwrs.ProxyBase`, or deriving the .NET class the descriptor's `base` names and having a `Pwrs.ProxyCore` field | `IntoPs` boxes the value and passes the pointer as the block; each property getter calls `pwrs_proxy_get(class_id, field_id, ptr)`; `Dispose` or the finalizer calls `pwrs_proxy_drop(class_id, ptr)` exactly once |
| `psobject` | none | `IntoPs` builds a `PSObject`, inserts the `PSTypeName`, adds one note property per field |
| `enum` (`#[psenum]`) | generated `enum` with `long` underneath | `IntoPs` passes the discriminant as an `i64` block to `factory_new`; the registered factory casts it to the enum. `FromPs` reads the value back through `i64_read` |

A class id names a class only within its own module, so each module
has its own table of factories. On PowerShell 7 every module has its
own copy of `Pwrs.Runtime`, and `factory_new` in the one host table
reads that copy's table. On Windows PowerShell every module in the
process uses one copy, so each module's library gets its own copy of
the host table, identical except for `factory_new`, `proxy_enter`,
`proxy_enter_shared`, `helper_path` and `native_file_path`, which work
for that module; in the shared table those entries fail, since no
module is given it.
`Pwrs.NativeModule.FactoriesPerModule` is `true` on a runtime that
works this way and absent on one that does not.

Proxy reads after `Dispose` throw `ObjectDisposedException` from the
getter; PowerShell's property adapter turns that into a `$null` read
with no error record, so scripts see `$null` and `IsDisposed` is the
reliable check. A proxy
that stores a `PsObject` referring back to itself forms a cycle between
Rust and .NET that is never freed, and nothing detects it.

Every call into a proxy's value takes the object's gate: a property
read, a method call, and a native borrow, which `PsProxy::with` and
`with_mut` make through `proxy_enter_shared`, `proxy_enter` and
`proxy_exit`. An entry is shared or exclusive. A property read, a
method taking `&self` and a `with` borrow enter as shared, and shared
entries nest on the thread inside one, so a method may read the object
it runs on, as it does when the receiver is passed back as a by-value
argument. A method taking `&mut self` and a `with_mut` borrow enter
exclusively: each is refused while anything is inside, and while it
runs every other entry is refused, with the message "in use by a call
already running on this thread". The generated shell passes each
method's kind, which the descriptor records as `mutable`. A call from another thread waits for the gate
whatever its kind. `proxy_enter` and `proxy_enter_shared` check that
the handle is a live proxy of the calling module's class `class_id`
from the current load and write the value's pointer; `proxy_exit`
releases the entry, with `changed` nonzero after `with_mut`. `Dispose`
on the thread that is inside the gate frees the value when its last
entry ends; on another thread it waits for the gate.

A class declared with `native_bytes` passes `true` as the last
argument of its `Pwrs.ProxyCore`'s constructor, which a class with no
`base` gives as the fourth of `Pwrs.ProxyBase`'s. The core then calls
`pwrs_proxy_bytes` once when it is made and reports the figure with
`GC.AddMemoryPressure`,
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
bound words, one 64-bit word per 64 arguments and never fewer than
one, then one slot per argument in declaration order, with the slot
rules of a parameter block and bit `i % 64` of word `i / 64` set when
argument `i` was supplied, and calls `pwrs_proxy_call(class_id,
method_id, instance, &block)`. `out` receives the result as a handle
for the managed side to free, null for `()`; an `Err` from the method
is status 1 with the message in `err`, thrown to the script as a
`PwrsException`. The object's `Pwrs.ProxyCore` makes `Get` and `Call`
on one object take turns at its gate, since a method may take
`&mut self`.

A method marked `#[member]` also has `member`, the name of the member
of the class's `base` or interfaces that it backs. `new` may have
`base_args`, the indexes of the arguments `#[base(...)]` passes to the
base class's constructor. A method marked `#[base]` has `base: true`
and `base_ret`, the type of each value it returns. Its call returns
them as an `object[]`, and the generated class declares no method for
it.

An argument of slot `peer` is another object of the same class, a
`&Self` argument in Rust. Its slot is a pointer: the generated method
refuses a null object, enters that object's gate as shared as well as
the receiver's, the two in the order the objects were made, and writes the
other object's value pointer into the slot before the call. A method
has at most one, and only a method with a receiver has one.

A proxy class's descriptor may name, by PowerShell name, the methods
behind the .NET interfaces the generated class implements: `count` and
`item` (`IReadOnlyList<T>`, with `set_item` also `IList<T>`), `next`
(`IEnumerable<T>`), `compare` (`IComparable`, `IComparable<T>`), and
`equals` with `hash` (`IEquatable<T>`, and the `Equals` and
`GetHashCode` overrides). Each interface member calls the generated
method its key names, so it makes the same native call as that method.

A proxy class's descriptor may also name `base`, the .NET class the
generated class derives from, and `implements`, .NET interfaces it
implements beside the ones those keys make, each by its C# name.
`cargo pwrs build` reads their members from the reference assemblies
of both target frameworks. It declares each member a method's `member`
names with the member's own signature, making the native call that
method makes. Every proxy class implements `Pwrs.IProxy`, itself or
through `Pwrs.ProxyBase`, and the runtime reaches the object's
`Pwrs.ProxyCore` through it. A value Rust makes reaches the class
through `factory_new`. The class then calls the `#[base]` method on
the value's pointer through `Pwrs.ProxyCore.CallUnowned`, before the
object exists, and passes what it returns to the base class's
constructor. A class with no `#[base]` method calls the base
constructor that takes no arguments.

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
engine converts it to the parameter's declared type and before
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
err)` runs one provider operation on the Rust instance for the
current drive. `op` is a code in `pwrs::provider::op` (item
exists, get item, get child items, new item, remove item, rename,
get/set content, new drive, remove drive, drop drive, init default
drives, make path, get parent/child, expand path, ...). `args` is an
`object[]` that starts with the call's five values: the filter, the
include patterns, the exclude patterns, whether `-Force` was given, and
the credential, each null when the call has none. The operation's own
inputs follow (paths as strings, flags as bools, a value where
relevant). The native side makes the five current for the length of the
operation, which `pwrs::provider::call()` returns, and refuses fewer
with `PwrsProviderArgs`. An error the operation returns comes back
through `err` as the `object[]` `[message, id, category, details,
target]`, details and target null when the error has none, which the
managed provider throws as a `PwrsException` whose record has them, the
operation's path with the drive standing in for a missing target. The result is an `object[]` of rows. An item row is
`[path, value, is_container]`; a drive row is `[name, root,
instance]`, the pointer as an `i64`.

Each drive has its own instance. `INIT_DEFAULT_DRIVES` and `NEW_DRIVE`
take a null `instance` and return drive rows whose pointers the
managed `PwrsDriveInfo` (a `PSDriveInfo` subclass) stores; every item,
container and content operation passes the pointer of the drive
PowerShell set for it, or null when there is none, and for null the
native side returns `PwrsProviderNoDrive`. The path operations
(`IS_VALID_PATH`, `MAKE_PATH`, `GET_PARENT_PATH`, `GET_CHILD_NAME`,
`NORMALIZE_RELATIVE_PATH`) ignore the pointer. `REMOVE_DRIVE` runs
`remove_drive` and frees the instance when that returns `Ok`;
`DROP_DRIVE` frees it without running `remove_drive` and is what the
drive object's finalizer sends for a drive that was never removed. The
managed drive object runs the operations on one drive one at a time. The
generated `NavigationCmdletProvider` subclass does the engine-facing
work (`WriteItemObject` with the row's path, `ShouldProcess`, content
reader/writer) and calls this export per operation, so no per-item
callback ABI is needed.

## Generated shell

Per cmdlet the C# shell is one sealed class deriving `Pwrs.RustCmdlet`
with `[Cmdlet]`, `[Alias]`, `[OutputType]`, one property per parameter
with `[Parameter]` and validation attributes whose setter records
the bound and dirty bits, a nested `Block` struct mirroring the Rust
block, and a `Run(phase)` that packs the block and makes one `Invoke`
call. A module-level static stores the `NativeModule`. There is no
reflection in that path and `MyInvocation` is never touched by it; the
vtable's `get_parameter` and `parameter_is_bound` entries use
`MyInvocation.BoundParameters` and are for dynamic parameters only.

Scalar outputs (`String`, `&str`, `i64`, `f64`, `bool`) are written
through the `write_string`, `write_i64`, `write_f64` and `write_bool`
entries: one call and no `GCHandle`, against `string_new`, `write_object`
and `free_handle` for the generic object path.
