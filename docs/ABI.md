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
| `pwrs_validate_invoke` | `(cmdlet_id: u32, parameter: u32, value: PsHandle, err: *mut PsHandle) -> PsStatus`; see Argument validation below. Bound optionally |
| `pwrs_provider_invoke` | `(provider_id: u32, op: u32, instance: *mut c_void, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; see Providers below |
| `pwrs_module_lifecycle` | `(op: u32, err: *mut PsHandle) -> PsStatus`; op 0 is the import hook, op 1 the removal hook, op 2 the load hook and op 3 the unload hook, each doing nothing when the module declared no hook for it. While op 1 runs, `lifecycle_removal` says why and for which runspace; see Removal hooks below. An error comes back through `err` as `[message, id, category, details, target]`, as a provider operation's does. Bound optionally |
| `pwrs_cpu_requirements` | Data: NUL-terminated ASCII. On x86-64, `PWRS-CPU/1` then ` name,level,leaf,subleaf,register,bit,xcr0` in decimal for each extension the library was compiled to require. On AArch64, `PWRS-ARM/1`, a space and the operating system (`linux`, `windows`, `macos`, or `other` with no entries), then ` name,tier,report` for each feature: the tier above which `PWRS_CPU_MAX` withholds it, 1 for one the target compiles every crate with, which the import check does not read, and how that system reports it (Linux `hwcap:hwcap2:half`, Windows PF constants joined by `+`, macOS terms joined by `+` of sysctl names joined by `\|`, empty for none). Read optionally, before any other export runs |
| `pwrs_cpuid` | `(leaf: u32, subleaf: u32, out: *mut u32)`; x86-64 only, naked: CPUID, EAX to EDX into `out[0..4]` |
| `pwrs_xgetbv` | `(xcr: u32) -> u64`; x86-64 only, naked: XGETBV, called only when CPUID leaf 1 reports OSXSAVE |
| `pwrs_cpu_query` | `(question: u32, name: *const u8, name_len: usize, out: *mut u8, out_len: usize) -> i32`; returns `Pwrs.Cpu`'s values from the library's own `pwrs::cpu`. For `question` 0, 1 and 2 it writes the x86-64 level, the AArch64 tier and the cap into `out` as UTF-8 and returns the length; for 3 and 4 it returns 1 or 0 for whether the extension `name` names is usable under the cap or offered by the CPU. -1 means there is no value, -2 an unknown `question` number, -3 an `out` too short. `pwrs_sys::cpu_query` has the numbers. Bound optionally |

Eight exports are bound optionally, with `TryExport` rather than
`Export`, so a library built before one of them existed still loads:

- one built before `pwrs_module_lifecycle` or `pwrs_transform_invoke`
  is treated as declaring no hooks and no transforms
- one built before `pwrs_validate_invoke` has no `validate_with`
  parameter for its shell to declare
- one built before `pwrs_proxy_bytes` reports no native bytes
- one built before the CPU-check exports is loaded unchecked
- one built before `pwrs_cpu_query` leaves `Pwrs.Cpu` to the next
  library loaded that has it

Every other export is required, and a library missing one
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
| 85 | lifecycle_removal | the thread running a removal hook |
| 86 | object_event_raise | any |
| 87 | object_event_subscribe | any |
| 88 | object_event_unsubscribe | any |
| 89 | proxy_exit_raising | the thread that entered |
| 90 | should_process_form | pipeline |
| 91 | should_continue_all | pipeline |
| 92 | paging | pipeline |
| 93 | write_total_count | pipeline |
| 94 | job_new | pipeline |
| 95 | job_write | any |
| 96 | job_write_stream | any |
| 97 | job_write_error | any |
| 98 | job_write_progress | any |
| 99 | job_finish | any |
| 100 | transaction_current | pipeline |
| 101 | transaction_enlist | any |
| 102 | steppable_begin | pipeline |
| 103 | steppable_process | pipeline |
| 104 | steppable_end | pipeline |
| 105 | steppable_dispose | pipeline |
| 106 | wrapped_arguments | pipeline |
| 107 | wrapped_input | pipeline |
| 108 | wrapped_begin | pipeline |
| 109 | wrapped_process | pipeline |
| 110 | wrapped_end | pipeline |
| 111 | exception_named | any |
| 112 | write_error_exception | pipeline |
| 113 | job_write_error_exception | any |
| 114 | job_finish_exception | any |
| 115 | library_folder | any |
| 116 | ordered_table_new | any |

## Engine events

`event_raise(events, source_identifier, sender, args, message_data,
err)` raises an engine event through `events`, a handle to a
`PSEventManager`. It passes `GenerateEvent` the arguments `New-Event`
passes:

- `sender` and `args` as their base objects
- `args` as an `object[]` of an array's elements, empty for null and the
  one value for anything else
- `message_data` as a `PSObject`, null for none
- `processInCurrentThread` true, with `waitForCompletionInCurrentThread`
  false

The event is queued, or passed to the actions subscribed to it,
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
`task_settle(source, outcome, value, err)` finishes it, by its outcome:

- `0` completes it with `value` as the result, null for a task with no
  value
- `1` faults it with `value` in the form a failing export returns
  through `err`: an exception, or a string `[id] message` thrown as a
  `PwrsException`
- `2` cancels it with the caller's token

Only the first call has an effect; it disposes the token's registration, waiting
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
between. `PsString::units` pins a string the same way and gives the
module the units to read itself, as a `PsStringUnits` view that cannot
write and releases the pin when it drops. A string is pinned where it
is, so a `PsString` parameter's units are the caller's own string's.

## Ordered tables

`ordered_table_new(out, err)` (entry 116) makes an empty `Hashtable`
that keeps its keys in the order they were added. The PowerShell 7 half
makes `System.Management.Automation.OrderedHashtable`; the Windows
PowerShell 5.1 half, whose engine has no such type, makes the runtime's
`Pwrs.OrderedHashtable`. That class keeps its entries in one
`OrderedDictionary`, as PowerShell 7's does, so keys compare by case, a
key removed and added again comes last, and `Keys`, `Values`, the
enumerator, `CopyTo` and `Clone` follow the order. `PsHashtable::ordered`
calls it.

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
there unchanged. So the errors of proxy methods, static calls, field
reads, providers, completers, transforms and lifecycle hooks reach
PowerShell with their details too. PowerShell wraps a method's exception
in a `MethodInvocationException`, so for a method the details are one
level further in. An error without details goes through `write_error` and the
string.

## Errors that name their exception

`exception_named(type_name, message, error_id, category, details)`
(entry 111) returns a handle to an exception of the .NET type
`type_name` names, resolved as PowerShell resolves a type literal. The
exception has `message` as its Message, an inner exception whose Message
is `details` when they are not empty, and `PwrsErrorId` and
`PwrsErrorCategory` (the category's name) in its `Data`. The runtime
makes it with the type's (string, Exception) constructor, or its
(string) one for an error without details. A name that gives no
exception type, or a type with no such constructor, gives a
`PwrsException` whose Message is `message` with the type and the reason
after it. The name `Pwrs.PwrsException` gives the runtime's own
`PwrsException`, made without resolving the name. A failing export
returns the handle through `err` with the `[id] message` text as its
Message, and the call site throws it unchanged, as it throws
`exception_new`'s. `pwrs_transform_invoke` and `pwrs_validate_invoke`
return one for every error, named `Pwrs.PwrsException` for an error that
names no type, with the error's message alone as its Message.

Each record path takes the handle, made with the plain message, and
leaves it the caller's to free.

- `write_error_exception(cmdlet, exception, error_id, category, target,
  terminating, err)` (entry 112) is `write_error` with that exception as
  the record's.
- `job_write_error_exception(job, exception, error_id, category, target,
  err)` (entry 113) adds such a record to a job's error stream.
- `job_finish_exception(job, exception, err)` (entry 114) ends a job as
  `job_finish` does for a failure, with the exception as its reason.
- A provider operation or a lifecycle hook returns the handle as a sixth
  cell after `[message, id, category, details, target]`. The managed side
  throws the `PwrsException` it throws for the five, whose record has the
  named exception as its exception.

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

`library_folder(name, buf, cap, out_len, err)` returns the path of a
copy of the folder `[package.metadata.pwrs.libraries]` ships at
`runtimes/<rid>/<name>/`, staged whole at
`libraries/<name>.<mark>/<name>`, the mark being the latest write time
among its files and their total length in hex. A name that is not a bare
folder name, or a folder the module does not ship for this platform,
fails with status 1. On .NET Framework each module's own copy of the
table provides it too.

## Removal hooks

`lifecycle_removal(cause, runspace, has_runspace, err)` reports the
removal the calling thread is running the module's removal hook for.
It sets `cause` to `REMOVAL_REMOVE_MODULE` (0) when `Remove-Module`
removed the module, and to `REMOVAL_RUNSPACE_CLOSED` (1) when the
runspace that imported it closed or broke with the module still
imported. For an import made in a runspace, it writes that runspace's
`InstanceId` to the 16 bytes at `runspace`, as `Guid.ToByteArray`
gives them, and sets `has_runspace` to 1; for an import made on a
thread with no default runspace it sets `has_runspace` to 0 and writes
nothing to `runspace`. The runtime records the removal on the thread
around each `pwrs_module_lifecycle` call with op 1 and restores the one
before it afterwards. On .NET Framework every module's copy of the
table has the same entry, reading the record of the one runtime the
process has, so a hook still reads its own removal after another
module's hook has run inside it on that thread. On a thread running no
removal hook the entry fails with status 1. `Removal::current` reads
it for the code `#[on_remove]` generates around a hook that takes a
`&Removal`.

## Object events

`object_event_raise(obj, name, args, err)` raises the event `name` of
`obj`, an object of a generated proxy class. A generated class stores
each event's handlers in a field `__pwrsEvent_` followed by the event's
raise name, its own name or `Interface.Name` for one implemented
explicitly, with each character other than an ASCII letter, digit or
`_` written as `_`. The entry reads that field by reflection, cached per
type and name. `args` gives the arguments as `event_raise` gives them.
For a delegate `(object sender, T e)` the object is the sender and the
one argument is `e`, `EventArgs.Empty` for an `EventHandler` given none.
Every argument converts to its parameter's type as
`LanguagePrimitives.ConvertTo` converts it, at the raise. The handlers
run through an invoker compiled per delegate type, so a handler's
exception arrives unwrapped. A raise made by the thread
inside the object's gate is queued on its `ProxyCore` and delivered
when that thread's last entry leaves, outside the gate; any other runs
the handlers before the entry returns. A handler's exception comes back
through `err`; delivered after a call, it is the call's exception,
unless the call failed, whose error is thrown and the handler's written
to standard error.

`object_event_subscribe(source, name, callback, context, release, out,
err)` builds a handler of the event's delegate type with expression
trees, which calls `callback(context, sender, args, err)` on the
raising thread. For a delegate `(object sender, T e)`, `sender` is the
delegate's first argument and `args` an `object[]` of `e`. For any
other, `sender` is null and `args` has every argument. When `callback`
returns a status other than 0, the error it wrote to `err` is thrown to
the raiser. `source` is an object, or a `Type` for its static event. On
success `out` is the subscription's handle and the subscription owns
`context`, calling `release(context)` once, after
the handler is removed and the last call into `callback` has returned;
on failure the caller still owns it. `object_event_unsubscribe
(subscription, err)` removes the handler without waiting; the caller
frees the handle. A delegate that returns a value or takes a `ref`,
`out` or `in` argument is refused. On a COM object whose type declares
no event of that name, the event is a method of the default source
interface the object's type information names. The handler takes the
method's arguments, the callback gets a null sender and all of them,
and `ComEventsHelper` attaches the handler to that interface. A method
that returns a value or takes an out argument is refused. The runtime
detaches such a handler when the runspace the call was made in closes,
and every one a module still has when the module unloads; on .NET
Framework each module's copy of the table has an
`object_event_subscribe` of its own, so one module's handlers stay
separate from another's.

`proxy_exit_raising(obj, changed, err)` is `proxy_exit` that, when the
borrow was the thread's last entry, delivers the raises the thread
queued during its entries, with a handler's exception through `err`.
`proxy_exit` delivers none of them; `PsProxy::with` and `with_mut` end
through it only when a panic unwinds them.

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
| `PsScriptBlock`, `PsHashtable` | `ScriptBlock?`, `IDictionary?` | `PsHandle` |
| `PsString` | `System.String?` | `PsHandle` to the caller's own string, which `units` pins in place |
| a `#[psenum]` type | the generated enum, boxed | `PsHandle` |
| a `#[psclass]` type | the generated class (`PSObject?` for psobject mode) | `PsHandle` |
| `PsDateTime`, `PsTimeSpan`, `PsGuid`, `char` | `DateTime`, `TimeSpan`, `Guid`, `char`, boxed | `PsHandle` |
| `PsSecureString`, `PsCredential` | `SecureString?`, `PSCredential?` | `PsHandle` |
| `Option<T>` | as `T`; unbound, or bound to a null string or handle, is `None` | as `T` |

A parameter whose bound bit is clear is not read: the field's value
does not change, so a handle type's `FromPs` is never given a null for an
unbound parameter (a piped parameter during Begin, for instance). An
optional parameter bound to `$null` is not read either, since its slot
holds a null pointer or handle: its field is `None`.

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
the host table. It differs in `factory_new`, `proxy_enter`,
`proxy_enter_shared`, `helper_path`, `native_file_path` and
`library_folder`, which work for that module; in the shared table those
entries fail, since no module is given it.
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
one C# method per entry. The method packs a `#[repr(C)]` block and calls
`pwrs_proxy_call(class_id, method_id, instance, &block)`. The block
starts with bound words, one 64-bit word per 64 arguments and never
fewer than one, with bit `i % 64` of word `i / 64` set when argument `i`
was supplied. One slot per argument follows in declaration order, with
the slot rules of a parameter block. `out` receives the result as a handle
for the managed side to free, null for `()`; an `Err` from the method
is status 1 with the message in `err`, thrown to the script as a
`PwrsException`, or the exception `exception_named` made for an error
that names its type. The object's `Pwrs.ProxyCore` makes `Get` and `Call`
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
`Pipeline`. An error comes back through `err` as the exception
`exception_named` makes for it, of the type the error names or
`Pwrs.PwrsException`, with the error's message alone as its Message.
The attribute throws an `ArgumentTransformationMetadataException` with
that Message and that exception inside, which the engine reports as a
binding failure naming the parameter, so the cmdlet body never runs. A
panic becomes one whose Message is the panic's message.

## Argument validation

`pwrs_validate_invoke(cmdlet_id: u32, parameter: u32, value: PsHandle,
err: *mut PsHandle) -> PsStatus` runs during binding, from
`ValidateArgumentsAttribute.Validate` on the module's
`PwrsValidateWith` attribute, which the shell declares with the cmdlet
id and the parameter's index on each parameter whose field names a
`validate_with` function. `value` is the argument after the engine
converted it to the parameter's declared type. The cmdlet's generated
`CmdletBind::validate` runs the named function. Its error comes back
through `err` as a transform's does, and the attribute throws a
`ValidationMetadataException` with the same Message and that exception
inside, which the engine reports as a validation failure naming the
parameter. As for a transformation, no cmdlet instance exists yet.

## Paging

`paging` (entry 92) reads the cmdlet's `PagingParameters`, which the
engine gives only a cmdlet whose `[Cmdlet]` declares `SupportsPaging`;
for any other the entry fails with an error naming the cmdlet.
`write_total_count` (entry 93) writes `NewTotalCount(total, accuracy)`
as an output write does, so an exception passing through it ends the
command.

## Jobs

`job_new(cmdlet, name, stop, context, release, out, err)` (entry 94)
makes a `Pwrs.RustJob`, a `Job2` whose command is the cmdlet's name and
whose name is `name`, or the engine's `Job<n>` when it is empty. It is
registered through the cmdlet's `JobManager` with the job source
adapter the module's shell declares, `<Module>RustJobSourceAdapter`, which
`Import-Module` registered from the shell, and it is Running. On
success the job owns `context`: Stop-Job, or Remove-Job -Force while it
runs, calls `stop(context)` once and waits for the job to end, and
`job_finish` calls `release(context)` once, after which `stop` is never
called. On failure the caller still owns it.

`job_write`, `job_write_stream`, `job_write_error` and
`job_write_progress` (entries 95 to 98) add to the job's own output,
message, error and progress streams from any thread, as `write_object`,
`write_stream`, `write_error_details` and `write_progress_record` do
for a cmdlet. `job_finish(job, failed, message, details, err)` (entry 99)
ends the job: Stopped when `stop` has been called, else Failed with an
exception of `message`, `details` inside it when not empty, when
`failed` is nonzero, else Completed. For an error that names its
exception type, `job_write_error_exception` and `job_finish_exception`
(entries 113 and 114) take the place of `job_write_error` and
`job_finish`, as [Errors that name their
exception](#errors-that-name-their-exception) says.

## Transactions

`transaction_current(cmdlet, out, err)` (entry 100) writes to `out` the
`System.Transactions.Transaction` of the PowerShell transaction the
cmdlet runs in, when `TransactionAvailable()` says it was given
`-UseTransaction` inside `Start-Transaction`, and null otherwise. Only
Windows PowerShell 5.1 gives a cmdlet `-UseTransaction`: PowerShell 7's
`CmdletAttribute` reads `SupportsTransactions` false whatever it was
given. So the entry reads whether the cmdlet's `[Cmdlet]` declares
`SupportsTransactions` from its metadata, and fails for one that does
not with an error naming it.

`transaction_enlist(transaction, prepare, end, context, err)` (entry
101) enlists a `Pwrs.RustEnlistment`, a volatile enlistment, in the
transaction the handle `transaction` refers to, which `transaction_current`
or a provider call gave. On success the enlistment owns `context`. When the
transaction is completed it calls `prepare(context, err)`, and a status
other than OK rolls the transaction back with the error written to `err`. It calls
`end(context, committed)` once, with `committed` 1 when the transaction
commits and 0 when it rolls back, for a refused `prepare` too, which
System.Transactions follows with no rollback of its own. On failure,
such as for a transaction that has ended, the caller still owns
`context`. Both callbacks run on the thread that completes or rolls back
the transaction, which for a timeout is a timer's.

## Steppable pipelines

`steppable_begin(cmdlet, name, parameters, out, out_delegates, err)`
(entry 102) begins the command `name` as a `SteppablePipeline` whose
output and streams are `cmdlet`'s, as a C# cmdlet wrapping a command
does. It calls `ScriptBlock.GetSteppablePipeline(CommandOrigin, ...)`
over `param($c, $a) & $c @a`, with the name passed as a string so the engine
resolves it as a typed command, then `Begin(cmdlet)`. `parameters` is a
hashtable handle or null. Each of `WhatIf`, `Confirm`, `Verbose`,
`Debug`, `ErrorAction`, `WarningAction` and `InformationAction` the
caller bound on `cmdlet` is added to the splat when `parameters` does not
name it and the command takes it; the common parameters naming a
variable are not, since the wrapper and the command would both assign
them. A handle to the pipeline goes to `out`, and `out_delegates` is 1
when the command takes `WhatIf`.

`steppable_process(cmdlet, steppable, input, has_input, err)` (entry 103)
calls `Process(input)` with the handle's object, passed as a `PSObject`
so its note properties are not lost, or `Process()` when `has_input` is 0.
`steppable_end(cmdlet, steppable, err)` (entry 104) calls `End()`.
`steppable_dispose(cmdlet, steppable, err)` (entry 105) calls `Clean()`
where the engine has it, PowerShell 7.3 and later, then `Dispose()`; the
handle stays the caller's to free.

An exception from any of the four is handled as one passing through a
write. A `PipelineStoppedException` marks the cmdlet stopped. Anything
else is stored for the phase and returned as the status, every later write,
confirmation and steppable call of the phase is refused with it, and the
managed `Invoke` throws it once the native call returns. So an inner
`Select-Object -First` stopping the records ends the wrapper quietly, and
an inner throw ends it with the inner command's own exception.

## Wrapped commands

A cmdlet declared with `wraps` derives from `Pwrs.WrappedCmdlet`, which
declares the command's copied parameters and stores the command's
`SteppablePipeline` from begin to end. Each of these is valid only on
such a cmdlet.

`wrapped_arguments(cmdlet, out, err)` (entry 106) writes to `out` a new
hashtable of the arguments the caller bound to the parameters copied from
the command, by parameter name: those of the cmdlet's properties and
those it declared at bind time. `wrapped_input(cmdlet, out, out_has,
err)` (entry 107) writes the record the pipeline passes to the cmdlet in this
phase, as it was piped and before any binding, with `out_has` 1, or sets
`out_has` to 0 when the cmdlet takes no pipeline input.

`wrapped_begin(cmdlet, arguments, hook, hook_ctx, out_delegates, err)`
(entry 108) begins the command with `arguments` splatted and the
caller's `WhatIf`, `Confirm`, `Verbose`, `Debug`, `ErrorAction`,
`WarningAction`, `InformationAction`, `ErrorVariable`, `WarningVariable`
and `InformationVariable` added where the table does not name them and
the command takes them. The command's errors, warnings and information
go straight to the caller's streams, so the command is the one to record
them. The pipeline is `param($c, $a, $h, $o) & $c @a | & $h -Owner $o`,
whose last stage passes each record the command writes to `hook`, a
`PsWrappedHook`, with `hook_ctx`; what the hook writes is the cmdlet's
output. `out_delegates` is as `steppable_begin`'s, and a cmdlet that runs
no command in this PowerShell is refused with its reason.
`wrapped_process(cmdlet, input, has_input, hook, hook_ctx, err)` (entry
109) calls `Process(input)`, or `Process()` when `has_input` is 0, and
`wrapped_end(cmdlet, hook, hook_ctx, err)` (entry 110) calls `End()`,
then `Clean()` where the engine has it and `Dispose()`. `hook` is called
only during the call that passes it.

`PsWrappedHook` is `fn(context, record, err) -> PsStatus`; the record's
handle is the caller's and lives for the call. A status of 5 throws a
`PipelineStoppedException` into the command; 2 throws an exception with
the panic's message; any other failure aborts the command, and the entry
then returns status 1 with no error, the native side reporting the error
its hook stored. Any other exception from the three entries is handled as
one passing through a write, as the steppable entries handle it.

## Providers

`pwrs_provider_invoke(provider_id: u32, op: u32, instance: *mut
c_void, args: PsHandle (object[]), out: *mut PsHandle (object[]),
err)` runs one provider operation on the Rust instance for the
current drive. `op` is a code in `pwrs::provider::op` (item
exists, get item, get child items, new item, remove item, rename,
get/set content, new drive, remove drive, drop drive, init default
drives, make path, get parent/child, expand path, ...). `args` is an
`object[]` that starts with the call's six values: the filter, the
include patterns, the exclude patterns, whether `-Force` was given, the
credential, and the `System.Transactions.Transaction` of the transaction
the call runs in, for a provider that declares `Transactions`, each null
when the call has none. The operation's own inputs follow (paths as
strings, flags as bools, a value where relevant). The native side makes
the six current for the length of the operation, which `pwrs::provider::call()` returns, and refuses fewer
with `PwrsProviderArgs`. An error the operation returns comes back
through `err` as the `object[]` `[message, id, category, details,
target]`, details and target null when the error has none, which the
managed provider throws as a `PwrsException` whose record has them, the
operation's path with the drive standing in for a missing target. An
error that names its exception type has the exception as a sixth cell,
and the record's exception is that one. The result is an `object[]` of rows. An item row is
`[path, value, is_container]`; a drive row is `[name, root,
instance]`, the pointer as an `i64`.

Each drive has its own instance. `INIT_DEFAULT_DRIVES` and `NEW_DRIVE`
take a null `instance` and return drive rows whose pointers the
managed `PwrsDriveInfo` (a `PSDriveInfo` subclass) stores. A default
drive uses the instance `INIT_DEFAULT_DRIVES` made: the managed provider
sends `NEW_DRIVE` for `New-PSDrive` alone, with the root as the user gave
it. Every item, container and content operation passes the pointer of the drive
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

Per cmdlet the C# shell is one sealed class deriving `Pwrs.RustCmdlet`,
with:

- `[Cmdlet]`, `[Alias]` and `[OutputType]`
- one property per parameter, with `[Parameter]` and validation
  attributes, whose setter records the bound and dirty bits
- a nested `Block` struct mirroring the Rust block
- a `Run(phase)` that packs the block and makes one `Invoke` call

A module-level static stores the `NativeModule`. There is no
reflection in that path and `MyInvocation` is never touched by it; the
vtable's `get_parameter` and `parameter_is_bound` entries use
`MyInvocation.BoundParameters` and are for dynamic parameters only.

Scalar outputs (`String`, `&str`, `i64`, `f64`, `bool`) are written
through the `write_string`, `write_i64`, `write_f64` and `write_bool`
entries: one call and no `GCHandle`, against `string_new`, `write_object`
and `free_handle` for the generic object path.
