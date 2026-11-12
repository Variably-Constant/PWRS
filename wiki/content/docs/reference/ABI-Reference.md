---
title: ABI Reference
weight: 6
---

The native contract, version 1. `crates/pwrs-sys/src/lib.rs` and `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs` describe the same memory; `docs/ABI.md` in the repository is the authoritative copy of this page.

## Rules

- Calling convention `extern "C"` everywhere; x64 and arm64.
- No unwinding across the boundary in either direction.
- Managed objects pass between the two sides only as `PsHandle` (the `IntPtr` of a `GCHandle`) or as `PsPinned` views released before the phase returns.
- Strings pass as UTF-16 `(ptr, len)`. An entry that reads a string writes what fits and reports the full length, so a caller can make a buffer of that length and call again, or read a long string in place through `string_pin`.
- The table is append-only. The 8-byte header is `size: u32, version: u32`. At `pwrs_module_init` a module refuses, with status 4, a table whose `size` is smaller than the one it was compiled against or whose `version` differs, so it never calls through an entry the runtime lacks. `version` changes only when an existing entry changes meaning.

## Status codes

| Code | Meaning |
|---|---|
| 0 | ok |
| 1 | managed exception; `err` is set to it |
| 2 | native panic; `err` is set to a managed string with the message |
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
| `pwrs_proxy_bytes` | `(class_id: u32, instance: *mut c_void) -> u64`; the bytes of native memory a proxy value reports through its class's `native_bytes` function, 0 for a class without one, an unknown class id, or a function that panics. The runtime does not require this export, so a library built before it existed still loads and reports no native bytes |
| `pwrs_completer_invoke` | `(completer_id: u32, word: PsStr16, command_text: PsStr16, fake_bound: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus` |
| `pwrs_transform_invoke` | `(transform_id: u32, value: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; runs a `#[transform]` over the value the binder is about to assign and returns what to assign instead. Called during binding, before the cmdlet instance exists. The runtime does not require this export, so a library built before it existed still loads and has no transforms |
| `pwrs_validate_invoke` | `(cmdlet_id: u32, parameter: u32, value: PsHandle, err: *mut PsHandle) -> PsStatus`; runs the function a parameter's `validate_with` names over the argument the binder has converted to the parameter's type, from the `ValidateArgumentsAttribute` the shell declares on that parameter with the cmdlet id and the parameter's index. An error comes back through `err` as the exception `exception_named` makes for it, its Message the error's message alone, and the engine reports it as a validation failure naming the parameter. Called during binding, before the cmdlet instance exists. The runtime does not require this export, so a library built before it existed still loads, and has no `validate_with` parameter for its shell to declare |
| `pwrs_dynparams_invoke` | `(cmdlet_id: u32, cmdlet: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`; runs on the pipeline thread before binding, from `GetDynamicParameters`. `out` is one string: a line per parameter joined by U+0003, each of seven cells joined by U+0001 (name, CLR type name, mandatory as `1` or `0`, position with `-1` for named, parameter set name, help, validate-set values joined by U+0002), and null for no parameters |
| `pwrs_provider_invoke` | `(provider_id: u32, op: u32, instance: *mut c_void, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus` |
| `pwrs_module_lifecycle` | `(op: u32, err: *mut PsHandle) -> PsStatus`; op 0 runs the import hook, op 1 the removal hook, op 2 the load hook and op 3 the unload hook, and each does nothing when the module declared no hook for it. While op 1 runs, `lifecycle_removal` says why and for which runspace; see [Removal hooks](#removal-hooks). An error comes back through `err` as `[message, id, category, details, target]`, as a provider operation's does. The runtime does not require this export, so a library built before it existed still loads and is treated as declaring none |
| `pwrs_cpu_requirements` | Data, not a function: NUL-terminated ASCII. On x86-64, `PWRS-CPU/1` then ` name,level,leaf,subleaf,register,bit,xcr0` in decimal for each extension the library was compiled to require. On AArch64, `PWRS-ARM/1`, a space and `linux`, `windows`, `macos` or `other`, which lists nothing, then ` name,tier,report` for each feature: `tier` is the tier's number, 1 for `neon` to 5 for `sve2` and 6 for none, above which `PWRS_CPU_MAX` withholds the feature, and 1 for one the target compiles every crate with, which the runtime does not read; `report` is how that system says the CPU has it, on Linux `hwcap:hwcap2:half`, the `AT_HWCAP` and `AT_HWCAP2` bits that must all be set and 1 when `AT_HWCAP` bit 9 requires bit 10, on Windows the `IsProcessorFeaturePresent` constants that must all be present joined by `+`, on macOS terms joined by `+` that must all be true, each `sysctlbyname` names joined by `\|` any one of which reading nonzero meets it, and empty for a feature that system cannot report. The runtime reads it before calling any other export and refuses the import when the CPU lacks one beyond the target's baseline, when the system cannot report one, or when `PWRS_CPU_MAX` caps below it; a library without it is not checked |
| `pwrs_cpuid` | `(leaf: u32, subleaf: u32, out: *mut u32)`; x86-64 only. A naked function, so its body is only CPUID and the moves around it, whatever the library was compiled for: EAX to EDX into `out[0..4]` |
| `pwrs_xgetbv` | `(xcr: u32) -> u64`; x86-64 only, naked: XGETBV, which the runtime calls only when CPUID leaf 1 reports OSXSAVE |
| `pwrs_cpu_query` | `(question: u32, name: *const u8, name_len: usize, out: *mut u8, out_len: usize) -> i32`; returns `Pwrs.Cpu`'s values from the library's own `pwrs::cpu`, under the cap the library read. For `question` 0 it writes the x86-64 level, `x86-64` to `x86-64-v4`, into `out` as UTF-8 and returns its length; for 1, the AArch64 feature tier, `neon` to `sve2`; and for 2, the cap as `PWRS_CPU_MAX` names it, `native` for none. For 3 it returns 1 when a module may use the extension `name` names and 0 when not, as `pwrs::cpu::has` or `arm::has` returns, and for 4 whether the CPU and the operating system offer it, as `detected` does. `name` is UTF-8, spelled as Rust's `target_feature` spells it, and only 3 and 4 read it. -1 means there is no value: a level on AArch64, a tier on x86-64, or a name that is not UTF-8 or not an extension `pwrs::cpu` tracks there. -2 means the `question` number has no meaning, and -3 that `out` is too short for the name. `pwrs_sys::cpu_query` has the numbers. The runtime does not require this export: `Pwrs.Cpu` reads from the first library loaded in the process that has it |

Phases: 0 Begin, 1 Process, 2 End. The phase mask reported by `pwrs_cmdlet_create` has bit 1 for Begin, 2 for Process, 4 for End; it starts at 7, and a phase's bit is cleared once that phase has run the trait's default body for the type.

## Host vtable

Slot order is declaration order. An entry marked "pipeline" returns status 3 when it is called from another thread.

The output and confirmation entries store an exception the engine raises in them, other than a stopped pipeline, on the cmdlet. They are `write_object`, `write_error`, `write_error_details`, `write_error_exception`, `write_stream`, `write_host`, `should_process`, `should_continue`, the four direct scalar writes and the four `steppable_` entries. After it, every write, confirmation or steppable call of the phase, the two progress entries included, returns status 1 with that exception without reaching the engine, and the runtime throws it once `pwrs_cmdlet_invoke` returns.

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
|  |  |  | 76 | `write_error_details` | pipeline |
|  |  |  | 77 | `exception_new` | any |
|  |  |  | 78 | `write_progress_record` | pipeline |
|  |  |  | 79 | `string_pin` | any |
|  |  |  | 80 | `memory_view_borrow` | any |
|  |  |  | 81 | `task_settle` | any |
|  |  |  | 82 | `event_raise` | any |
|  |  |  | 83 | `write_host` | pipeline |
|  |  |  | 84 | `native_file_path` | any |
|  |  |  | 85 | `lifecycle_removal` | the thread running a removal hook |
|  |  |  | 86 | `object_event_raise` | any |
|  |  |  | 87 | `object_event_subscribe` | any |
|  |  |  | 88 | `object_event_unsubscribe` | any |
|  |  |  | 89 | `proxy_exit_raising` | the thread that entered |
|  |  |  | 90 | `should_process_form` | pipeline |
|  |  |  | 91 | `should_continue_all` | pipeline |
|  |  |  | 92 | `paging` | pipeline |
|  |  |  | 93 | `write_total_count` | pipeline |
|  |  |  | 94 | `job_new` | pipeline |
|  |  |  | 95 | `job_write` | any |
|  |  |  | 96 | `job_write_stream` | any |
|  |  |  | 97 | `job_write_error` | any |
|  |  |  | 98 | `job_write_progress` | any |
|  |  |  | 99 | `job_finish` | any |
|  |  |  | 100 | `transaction_current` | pipeline |
|  |  |  | 101 | `transaction_enlist` | any |
|  |  |  | 102 | `steppable_begin` | pipeline |
|  |  |  | 103 | `steppable_process` | pipeline |
|  |  |  | 104 | `steppable_end` | pipeline |
|  |  |  | 105 | `steppable_dispose` | pipeline |
|  |  |  | 106 | `wrapped_arguments` | pipeline |
|  |  |  | 107 | `wrapped_input` | pipeline |
|  |  |  | 108 | `wrapped_begin` | pipeline |
|  |  |  | 109 | `wrapped_process` | pipeline |
|  |  |  | 110 | `wrapped_end` | pipeline |
|  |  |  | 111 | `exception_named` | any |
|  |  |  | 112 | `write_error_exception` | pipeline |
|  |  |  | 113 | `job_write_error_exception` | any |
|  |  |  | 114 | `job_finish_exception` | any |
|  |  |  | 115 | `library_folder` | any |
|  |  |  | 116 | `ordered_table_new` | any |

`paging` reads the cmdlet's `PagingParameters`, its `First`, `Skip` and `IncludeTotalCount`, which the engine gives only a cmdlet whose `[Cmdlet]` declares `SupportsPaging`; for any other the entry fails with an error naming the cmdlet. `write_total_count(cmdlet, total, accuracy, err)` writes `PagingParameters.NewTotalCount(total, accuracy)` as an output write does, so an exception the engine raises there ends the command as it ends a compiled cmdlet.

`job_new(cmdlet, name, stop, context, release, out, err)` makes a `Pwrs.RustJob`, a `Job2` named `name`, or the engine's `Job<n>` when it is empty, registered through the cmdlet's `JobManager` with the job source adapter the module's shell declares and Import-Module registered, and Running. On success the job owns `context`. Stop-Job, or Remove-Job -Force while it runs, calls `stop(context)` once and waits for the job to end, and `job_finish` calls `release(context)` once, after which `stop` is never called. On failure the caller still owns it. `job_write`, `job_write_stream`, `job_write_error` and `job_write_progress` add to the job's own streams from any thread. `job_finish(job, failed, message, details, err)` ends it Stopped when `stop` has been called, else Failed with an exception of `message` (and `details` inside it) when `failed` is nonzero, else Completed. See [How To Run Work As A Job](../how-to/How-To-Run-Work-As-A-Job.md).

`transaction_current(cmdlet, out, err)` writes to `out` the `System.Transactions.Transaction` of the PowerShell transaction the cmdlet runs in, when `TransactionAvailable()` says it was given `-UseTransaction` inside `Start-Transaction`, and null otherwise. A cmdlet whose `[Cmdlet]` does not declare `SupportsTransactions`, read from its metadata since PowerShell 7's `CmdletAttribute` reads it false, fails with an error naming it. A provider declaring `Transactions` gets the call's transaction as the sixth of the context arguments `pwrs_provider_invoke` is given before an operation's own, null without one. `transaction_enlist(transaction, prepare, end, context, err)` enlists a `Pwrs.RustEnlistment`, a volatile enlistment, in that transaction. On success it owns `context`: when the transaction is completed it calls `prepare(context, err)`, whose status other than OK rolls the transaction back with the error, and it calls `end(context, committed)` once, with `committed` 1 for a commit and 0 for a rollback, a refused `prepare` included. On failure, such as for a transaction that has ended, the caller still owns it. Both callbacks run on the thread that completes or rolls back the transaction, a timer's for a timeout. See [How To Write A Cmdlet](../how-to/How-To-Write-A-Cmdlet.md#transactions).

`steppable_begin(cmdlet, name, parameters, out, out_delegates, err)` begins the command `name`, resolved as a typed command is, as a `SteppablePipeline` whose output and streams are the cmdlet's: `GetSteppablePipeline(CommandOrigin, ...)` over `& $name @parameters`, then `Begin(cmdlet)`. `parameters` is a hashtable handle or null. Each of `-WhatIf`, `-Confirm`, `-Verbose`, `-Debug`, `-ErrorAction`, `-WarningAction` and `-InformationAction` the caller gave the cmdlet is added when `parameters` does not name it and the command takes it. The pipeline's handle goes to `out`, and `out_delegates` is 1 when the command takes `-WhatIf`. `steppable_process(cmdlet, steppable, input, has_input, err)` calls `Process(input)`, or `Process()` when `has_input` is 0. `steppable_end(cmdlet, steppable, err)` calls `End()`. `steppable_dispose(cmdlet, steppable, err)` calls `Clean()` where the engine has it, then `Dispose()`, and leaves the handle for the caller to free. See [How To Write A Cmdlet](../how-to/How-To-Write-A-Cmdlet.md#wrap-another-command).

The five `wrapped_` entries serve a cmdlet declared with `wraps`, whose class derives from `Pwrs.WrappedCmdlet`. `wrapped_arguments(cmdlet, out, err)` writes a new hashtable of the arguments the caller bound to the parameters copied from the command. `wrapped_input(cmdlet, out, out_has, err)` writes the record the pipeline passes to the cmdlet in this phase, as it was piped, with `out_has` 1, or `out_has` 0 when it takes no pipeline input. `wrapped_begin(cmdlet, arguments, hook, hook_ctx, out_delegates, err)` begins the command with the arguments splatted, adding the caller's preference common parameters, `-ErrorVariable`, `-WarningVariable` and `-InformationVariable` where the command takes them. The pipeline is `param($c, $a, $h, $o) & $c @a | & $h -Owner $o`, whose last stage passes each record the command writes to `hook` with `hook_ctx`. `wrapped_process(cmdlet, input, has_input, hook, hook_ctx, err)` calls `Process(input)`, or `Process()` when `has_input` is 0, and `wrapped_end(cmdlet, hook, hook_ctx, err)` calls `End()`, then `Clean()` where the engine has it and `Dispose()`. A `PsWrappedHook` is `fn(context, record, err) -> PsStatus`: status 5 stops the command, 2 throws the panic's message, and any other failure aborts the command, the entry then returning status 1 with no error, so the native side reports the error its hook stored. See [How To Write A Cmdlet](../how-to/How-To-Write-A-Cmdlet.md#declare-a-proxy-of-a-command).

## Memory views with an owner

`memory_view_borrow(tag, ptr, len, read_only, refused, context, release, out, err)` wraps `len` elements of the primitive `tag` at `ptr` as a `Memory<T>`, or a `ReadOnlyMemory<T>` when `read_only` is nonzero, over that memory on .NET, and copies them into a `T[]` on .NET Framework. On success the host stores `context` and calls `release(context)` exactly once: when the view's manager is disposed or finalized on .NET, and before the entry returns on .NET Framework. On failure it has not called `release` and never will. `refused`, when not null, points at a byte the module sets nonzero to refuse the view, readable until `release` runs. The host reads it when the view is made, and on .NET before every `GetSpan` and `Pin`. A refusal is an `ObjectDisposedException`, as is `GetSpan` or `Pin` on a disposed manager. `memory_view_new(tag, ptr, len, drop_fn, out, err)` is this entry with `ptr` as the context, writable and never refused. `PsMemory<T>` and `PsMemoryView<T>` are passed through it; see the [Pipeline Reference](Pipeline-Reference.md#pinneda-t-psmemoryt-psmemoryviewt-and-psrevocation-pinnedrs).

## Tasks

A `#[psmethods]` argument of slot `task` is a `PsTaskSlot { source: PsHandle, cancelled: *const u8 }`. `source` is a handle to the `Pwrs.TaskSource<T>` the generated method made, which has a `TaskCompletionSource<T>` created with `RunContinuationsAsynchronously` and registers a callback on the caller's `CancellationToken`; `cancelled` points at a byte that callback sets nonzero. `task_settle(source, outcome, value, err)` finishes the task:

- outcome `0` completes it with `value` as the result, null for a task with no value;
- `1` faults it with `value` in the form a failing export returns through `err`: an exception, or a string `[id] message` thrown as a `PwrsException`;
- `2` cancels it with the caller's token.

Only the first call has an effect; it disposes the token's registration, waiting for a callback another thread is running, and only then frees the byte. When the generated method throws, the task faults with the same exception unless Rust has already finished it.

## Engine events

`event_raise(events, source_identifier, sender, args, message_data, err)` raises an engine event through `events`, a handle to a `PSEventManager`. It calls `GenerateEvent` with the arguments `New-Event` passes it:

- `sender` and `args` as their base objects, with `args` as an `object[]` of an array's elements, empty for null, and the one value for anything else;
- `message_data` as a `PSObject`, null for none;
- the event processed on the calling thread, without waiting for its actions.

The event is queued, or passed to the actions subscribed to it, before the entry returns, so the events one thread raises arrive in the order raised; an action runs later on the runspace's pipeline thread. The entry does not free the handles; the caller does. `Pipeline::events` reads the manager off the cmdlet's `Events` through `dyn_get`. See [How To Raise Engine Events](../how-to/How-To-Raise-Engine-Events.md).

## Strings read in place

`string_pin(h, out, err)` converts `h` to a string as `string_read` does and pins that string in place: `out` is set to the address of its first UTF-16 unit, its length in units, an element size of 2 and the pin. The units are read, never written, and the pin is released with `array_unpin` before the phase that pinned it returns. `String::from_ps` reads a string of up to 256 units through `string_read` into a buffer of that size, and a longer one through `string_pin`, converting the pinned units to UTF-8 with no copy in between. `PsString::units` pins a string the same way and gives the module the units to read itself, as a `PsStringUnits` view that cannot write and releases the pin when it drops. A string is pinned where it is, so a `PsString` parameter's units are the caller's own string's.

## Ordered tables

`ordered_table_new(out, err)` (entry 116) makes an empty `Hashtable` that keeps its keys in the order they were added. The PowerShell 7 half makes `System.Management.Automation.OrderedHashtable`; the Windows PowerShell 5.1 half, whose engine has no such type, makes the runtime's `Pwrs.OrderedHashtable`. That class keeps its entries in one `OrderedDictionary`, as PowerShell 7's does, so keys compare by case, a key removed and added again comes last, and `Keys`, `Values`, the enumerator, `CopyTo` and `Clone` follow the order. `PsHashtable::ordered` calls it.

## Progress records with every field

`write_progress_record(cmdlet, activity_id, parent_activity_id, activity, status, current_operation, percent, seconds_remaining, completed, err)` writes a `ProgressRecord` with every field set. It is the completing record when `completed` is nonzero and a processing one otherwise, so a processing record can have a negative percent, which shows no percentage. A `percent` above 100 is 100, a negative `parent_activity_id` leaves the activity unnested, a negative `seconds_remaining` shows no time left, and an empty `current_operation` sets none. PowerShell refuses an empty activity or status, a negative activity id, and a parent activity id equal to the activity id, and that error comes back through `err`. In `write_progress`, a negative `percent` writes the completing record. `Pipeline::write_progress` and `Worker::write_progress` are the Rust surface over it; see the [Pipeline Reference](Pipeline-Reference.md).

## Errors with details

`write_error_details(cmdlet, message, error_id, category, target, terminating, details, err)` is `write_error` for a `PsError` with details. The record's exception has `message` as its Message and an inner exception whose Message is `details`, so the default error view shows the one line and `$err.Exception.InnerException.Message` has the rest. `exception_new(message, details)` returns a handle to such an exception. A failing export returns it through `err` in place of the `[id] message` string, and every call site throws an exception it finds there unchanged. So the errors of proxy methods, static calls, field reads, providers, completers, transforms and lifecycle hooks reach PowerShell with their details too. PowerShell wraps a method's exception in a `MethodInvocationException`, so for a method the details are one level further in. An error without details goes through `write_error` and the string.

## Errors that name their exception

`exception_named(type_name, message, error_id, category, details)` returns a handle to an exception of the .NET type `type_name` names, resolved as PowerShell resolves a type literal. Its Message is `message`, its inner exception has `details` when they are not empty, and its `Data` holds `PwrsErrorId` and `PwrsErrorCategory`, the category's name. The runtime makes it with the type's (string, Exception) constructor, or its (string) one for an error without details. A name that gives no exception type, or a type with no such constructor, gives a `PwrsException` whose Message is `message` with the type and the reason after it. The name `Pwrs.PwrsException` gives the runtime's own `PwrsException`, made without resolving the name. A failing export returns the handle through `err`, with the `[id] message` text as its Message, and the call site throws it unchanged. `pwrs_transform_invoke` and `pwrs_validate_invoke` return one for every error, named `Pwrs.PwrsException` for an error that names no type, with the error's message alone as its Message.

The record paths take the handle, made with the plain message, and leave it the caller's to free:

- `write_error_exception(cmdlet, exception, error_id, category, target, terminating, err)` is `write_error` with that exception as the record's.
- `job_write_error_exception(job, exception, error_id, category, target, err)` adds such a record to a job's error stream.
- `job_finish_exception(job, exception, err)` ends a job as `job_finish` does for a failure, with the exception as its reason.
- A provider operation or a lifecycle hook returns the handle as a sixth cell after its five, and the record of the `PwrsException` the managed side throws has it as its exception.

## Helper executables

`helper_path(name, buf, cap, out_len, err)` returns the path to start a helper executable from, for a helper the module ships in `runtimes/<rid>/native/`, `name` being its file name without `.exe`. The first call for a helper copies it into the folder where the process stages the module's library, at `native/<file>.<mark>/<file>`, where the mark is the shipped file's write time and length in hex; every later call returns the same path. The copy is written under a temporary name and moved into place, so no caller gets the path of a file another thread is still writing. The path is copied into `buf` as `string_read` copies a string. A name that is not a bare file name, or a helper the module does not ship, fails with status 1. `native_file_path(name, buf, cap, out_len, err)` does the same for a file `[package.metadata.pwrs] native-files` ships there, `name` being its file name with its extension, and stages it as a plain file. `library_folder(name, buf, cap, out_len, err)` does the same for a folder `[package.metadata.pwrs.libraries]` ships at `runtimes/<rid>/<name>/`, staged whole at `libraries/<name>.<mark>/<name>`, where the mark is the latest write time among its files and their total length in hex. On Windows PowerShell each module's own copy of the table provides all three entries, as it provides `factory_new`. `pwrs::helper_path`, `pwrs::native_file_path` and `pwrs::library_folder` are the Rust surface over them; see [How To Ship A Helper Executable](../how-to/How-To-Ship-A-Helper-Executable.md), [How To Ship A Prebuilt Native File](../how-to/How-To-Ship-A-Prebuilt-Native-File.md) and [How To Ship A Second Library](../how-to/How-To-Ship-A-Second-Library.md).

## Removal hooks

`lifecycle_removal(cause, runspace, has_runspace, err)` reports the removal the calling thread is running the module's removal hook for. It sets `cause` to `REMOVAL_REMOVE_MODULE` (0) when `Remove-Module` removed the module, and to `REMOVAL_RUNSPACE_CLOSED` (1) when the runspace that imported it closed or broke with the module still imported. For an import made in a runspace, it writes that runspace's `InstanceId` to the 16 bytes at `runspace`, as `Guid.ToByteArray` gives them, and sets `has_runspace` to 1; for an import made on a thread with no default runspace it sets `has_runspace` to 0 and writes nothing to `runspace`. The runtime records the removal on the thread around each `pwrs_module_lifecycle` call with op 1, and restores the one before it afterwards. On Windows PowerShell every module's table reads the record of the one runtime the process has, so this lets a hook still read its own removal after another module's hook has run inside it on that thread. On a thread running no removal hook the entry fails with status 1. The code `#[on_remove]` generates for a hook that takes a `&Removal` reads it through this entry; see the [Attribute Reference](Attribute-Reference.md#on_load-on_import-on_remove-and-on_unload).

## Object events

`object_event_raise(obj, name, args, err)` raises the event `name` of `obj`, an object of a generated proxy class. A generated class stores each event's handlers in a field `__pwrsEvent_` followed by the event's raise name with each character other than an ASCII letter, digit or `_` written as `_`, its own name or `Interface.Name` for one implemented explicitly, and the entry reads that field. `args` gives the arguments as `event_raise` gives them: an array's elements, none for null, anything else as the one argument. For a delegate `(object sender, T e)` the object is the sender and the one argument is `e`, `EventArgs.Empty` for an `EventHandler` given none; each converts to its parameter's type as `LanguagePrimitives.ConvertTo` converts it, at the raise. A raise made by the thread inside the object's gate is queued on its `ProxyCore` and delivered when that thread's last entry leaves; any other runs the handlers before the entry returns, and a handler's exception comes back through `err`.

`object_event_subscribe(source, name, callback, context, release, out, err)` builds a handler of the event's delegate type, with expression trees, that calls `callback(context, sender, args, err)` on the raising thread. For a delegate `(object sender, T e)`, `sender` is the delegate's first argument and `args` an `object[]` of `e`; for any other, `sender` is null and `args` has every argument. When `callback` returns a status other than 0, the error it wrote to `err` is thrown to the raiser. `source` is an object, or a `Type` for its static event. On success `out` is the subscription's handle and the subscription owns `context`, calling `release(context)` once, after the handler is removed and no call into `callback` runs; on failure the caller still owns it. `object_event_unsubscribe(subscription, err)` removes the handler without waiting for a call running on another thread; the caller frees the handle. A delegate that returns a value or takes a `ref`, `out` or `in` argument is refused. On a COM object whose type declares no event of that name, the event is a method of the default source interface the object's type information names. The handler takes the method's arguments, the callback gets a null sender and all of them, and `ComEventsHelper` attaches the handler to that interface. A method that returns a value or takes an out argument is refused. The runtime detaches such a handler when the runspace the call was made in closes, and detaches every one a module still has when the module unloads. On .NET Framework each module's copy of the table has an `object_event_subscribe` of its own, so one module's handlers stay separate from another's.

`proxy_exit_raising(obj, changed, err)` is `proxy_exit` that, when the borrow was the thread's last entry, delivers the raises the thread queued during its entries; a handler's exception comes back through `err`. `proxy_exit` delivers none of them, and `PsProxy::with` and `with_mut` reach it only when a panic unwinds them. See [How To Raise And Handle Object Events](../how-to/How-To-Raise-And-Handle-Object-Events.md).

## Typed values

| Entries | Representation |
|---|---|
| `datetime_new(ticks, kind, out, err)`, `datetime_read(h, out_ticks, out_kind, err)` | ticks of 100 ns from the start of year 1 and the `DateTimeKind` (0 unspecified, 1 UTC, 2 local); `datetime_new` fails outside the type's range; `datetime_read` converts through `LanguagePrimitives.ConvertTo<DateTime>` |
| `timespan_new(ticks)`, `timespan_read(h, out_ticks, err)` | ticks of 100 ns, negative allowed |
| `guid_new(bytes)`, `guid_read(h, out_bytes, err)` | the 16 bytes of `Guid.ToByteArray` |
| `char_new(unit)`, `char_read(h, out_unit, err)` | one UTF-16 code unit |
| `securestring_new(text, out, err)` | a read-only `SecureString`; fails past 65536 units |
| `securestring_read(h, buf, cap, out_len, err)` | decrypts into an unmanaged buffer, copies up to `cap` units, reports the full length, and zeroes the buffer; `cap` 0 reports the length only |
| `array_element_tag(h, out_tag, err)` | the element tag of a typed array, `PS_TYPE_OBJECT` for anything else; a `Vec<T>` of a pinnable primitive is copied through one pin when this tag is `T`'s, and read element by element otherwise |
| `stream_enabled(cmdlet, kind, out_enabled, err)` | whether a record written to that stream would go somewhere, by the preference the engine gives a compiled cmdlet: the stream's own common parameter where it is bound, then `-Debug`, under which PowerShell prompts the user after each verbose record and warning, and `-Verbose`, which shows warnings, then the session's preference variable; the runtime works it out once per cmdlet instance, and the Rust side stores the result on the instance, so a later phase or record does not call this entry again |
| `object_type_tag(h, out_tag, err)` | the tag of one object's own type, `PS_TYPE_OBJECT` for a type PWRS has no tag for; unwraps a `PSObject` first, so a wrapped `Int32` gives `PS_TYPE_I32` |
| `i8_new`, `i16_new`, `i32_new`, `u8_new`, `u16_new`, `u32_new`, `f32_new` | each builds the CLR type of its own name instead of widening to `Int64` or `Double` |
| `decimal_new(lo, mid, hi, flags, out, err)`, `decimal_read(h, out_bits, err)` | the four words in `Decimal.GetBits` order, which is not their order in memory; `decimal_new` rejects a scale above 28 or reserved bits set |
| `datetimeoffset_new(ticks, offset_minutes, out, err)`, `datetimeoffset_read(h, out_ticks, out_offset_minutes, err)` | ticks of 100 ns from the start of year 1, and the UTC offset in whole minutes |
| `enum_new(type_name, value, out, err)` | a value of the CLR enum `type_name` names, from its underlying number, through `Enum.ToObject`. The name is resolved with `LanguagePrimitives.ConvertTo<Type>`, the same resolver a type literal uses, and a name it cannot resolve, or a type that is not an enum, fails with status 1. A module's own `#[psenum]` types go through `factory_new` instead, which needs no name |
| `proxy_enter(obj, class_id, out_instance, err)`, `proxy_enter_shared(obj, class_id, out_instance, err)`, `proxy_exit(obj, changed)` | give native code a proxy's value inside the object's gate, which its property reads and method calls also enter. Both enter forms check that `obj` is a live proxy of the calling module's class `class_id` from the current load. `proxy_enter` is the exclusive entry `with_mut` makes: it is refused while anything is inside the object on the calling thread, and while it lasts every other entry is refused, as for a `&mut self` method; `proxy_enter_shared` is the shared entry `with` makes, which nests inside a `&self` method, a property read or another shared entry and is refused only inside an exclusive one. `proxy_exit` releases the entry, and with `changed` nonzero reads a `native_bytes` class's byte count again. On Windows PowerShell each module's own copy of the table provides both enter forms, as it provides `factory_new` |
| `invoke_command(cmdlet, name, parameters, input, out, err)` | runs `name` in a nested `PowerShell` in the current runspace, added by name, so the engine resolves it as it does a typed command and a module's function runs in its module's scope, `parameters` an `IDictionary` bound by name or null, `input` piped in or null (a collection unrolled, a string or other single value one record); `out` receives an `object[]` of everything it wrote. The command's non-terminating errors are written to the calling cmdlet's error stream; a terminating one, and an unknown name, fail with status 1. Pipeline thread |

Type tags 14, 15 and 16 (`DateTime`, `TimeSpan`, `Guid`) join the primitive tags `array_new` accepts, so a `Vec` of those types becomes a typed array; such arrays are not pinnable.

Tag 17 (`Decimal`) is accepted by `array_new` and, unlike those three, its arrays are pinnable: `Type.IsPrimitive` is false for `System.Decimal` but it is blittable and pins, so `array_pin` accepts it by name. A pinned element is `PsDecimalBits`, the four words in memory order, and the first such pin in a process checks that order against `Decimal.GetBits` before any element is read.

## Parameter block

Per cmdlet, a `#[repr(C)]` struct on both sides: the dirty words, the bound words, then one slot per parameter in declaration order. Each mask is one 64-bit word per 64 parameters, and at least one, so a cmdlet of 64 parameters or fewer has `dirty: u64` and `bound: u64`. In Rust the masks are `dirty: [u64; N]` and `bound: [u64; N]`; in C# each word is a `ulong` field, `Dirty`, `Dirty1` and so on, then `Bound`, `Bound1` and so on. Parameter `i` has bit `i % 64` of word `i / 64`. Its bound bit is set when it was ever assigned, and its dirty bit when it was assigned since the previous phase. The generated property setters maintain both. The native side binds before the first phase and again only when a dirty word is non-zero, and leaves a parameter whose bound bit is clear at its field's value. The slots:

- `u8` for switches;
- the numeric types at their width;
- `PsStr16` for strings, pinned `fixed` for the call;
- `PsHandle` for arrays, objects, class objects, script blocks, hashtables, secure strings, credentials and boxed value types (enums, `DateTime`, `TimeSpan`, `Guid`, `char`): a `GCHandle` allocated for the call and freed after.

The block lives on the managed stack for one phase, and the Rust reader clones handles before storing them.

## Class field block

Copied classes pack one slot per field in the same way, followed by a mask of one 64-bit word per 64 fields, and at least one: bit `i % 64` of word `i / 64` is set when `Option` field `i` is `Some`. `factory_new(class_id, &block)` passes it to the registered factory, which unboxes value-typed handles with a cast and converts array-typed ones with `LanguagePrimitives.ConvertTo` to the declared element type. Proxy classes pass the boxed instance pointer instead. Enums pass a single `i64`.

A class id is its class's position in its own module's `export_module!` list, so it names a class only within one module, and each module has its own table of factories. A type's id is found by its Rust type, through the entry's `type_id`, never by its name. A type the list does not include has no id even when a listed type has its name, and writing it is refused with `PwrsClassNameTaken`, naming both. Each type's id lookup also contains a record: `PWRS-CLASS/1`, a tab, its name, a tab, its Rust path, then NUL. So a library has one for every class whose values it can write, listed or not, and `cargo pwrs build` reads them out of the file to refuse a type under a name another type has. On PowerShell 7 every module has its own copy of `Pwrs.Runtime`, so `factory_new` in the one host table reads that copy's table. On Windows PowerShell every module in the process uses one copy, so each module's library gets its own copy of the host table, identical except for `factory_new`, `proxy_enter`, `proxy_enter_shared`, `helper_path` and `native_file_path`, which work for that module. In the shared table those entries fail, since no module is given it. `Pwrs.NativeModule.FactoriesPerModule` is `true` on a runtime that works this way and absent on one that does not.

## Method block

A `#[psmethods]` method on a proxy class receives its bound words, one 64-bit word per 64 arguments and at least one, then one slot per argument in declaration order, packed by the generated C# method with the slot rules of a parameter block. Bit `i % 64` of word `i / 64` is set when argument `i` was supplied. `pwrs_proxy_call` returns the result as a handle (null for `()`); an `Err` is status 1 with the message, thrown to the script as a `PwrsException`, or the exception `exception_named` made for an error that names its type. The object's `Pwrs.ProxyCore` runs the calls and property reads of one object one at a time.

An argument of slot `peer` is another object of the same class, a `&Self` argument in Rust, and its slot is a pointer. The generated method refuses a null object, and enters that object's gate as shared as well as the receiver's, the two in the order the objects were made. It then writes the other object's value pointer into the slot. A method has at most one, and only a method with a receiver has one. An argument of slot `task` is a `PsTaskSlot`; see [Tasks](#tasks). A class's descriptor may name the methods behind the .NET interfaces its generated class implements (`count`, `item`, `set_item`, `next`, `compare`, `equals`, `hash`), and each interface member calls the method its key names.

## Provider drives

A provider operation passes the pointer of the Rust instance for the current drive. Its `args` start with the call's five values: the filter, the include patterns, the exclude patterns, whether `-Force` was given, and the credential, each null when the call has none. The operation's own arguments follow, and the native side refuses fewer than five with `PwrsProviderArgs`. While the operation runs, `pwrs::provider::call()` returns the five. An error the operation returns comes back through `err` as `[message, id, category, details, target]`, details and target null when the error has none. The managed provider throws it as a `PwrsException` whose error record has the id, the category and the target, or the operation's path with the drive when the error has no target. An error that names its exception type has the exception as a sixth cell, and the record's exception is that one. The drive-making operations (`INIT_DEFAULT_DRIVES`, `NEW_DRIVE`) take null and return rows `[name, root, instance]` whose pointers the managed `PwrsDriveInfo` stores. The path operations ignore the pointer. Every other operation needs it, and fails with `PwrsProviderNoDrive` when PowerShell set no drive of the provider for the operation. `REMOVE_DRIVE` runs `remove_drive` and frees the instance on `Ok`; `DROP_DRIVE` frees it without running `remove_drive`, when a drive that was never removed is finalized. Operations on one drive are serialized by the managed drive object.

## Error flow through a phase

A body `Err` is written through `write_error` before `pwrs_cmdlet_invoke` returns: non-terminating records go to `WriteError` at once; a terminating record is stored on the managed cmdlet and raised with `ThrowTerminatingError` after the native call returns. A `PipelineStoppedException` inside any entry marks the cmdlet stopped and returns status 5; the managed `Invoke` rethrows it after the native call. A panic in a phase returns status 2 and the managed side raises a terminating `PwrsNativePanic` error with the message.
