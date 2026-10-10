---
title: Pipeline Reference
weight: 3
---

Every method on the runtime types a module's own code calls, with the file each lives in. The handful the generated shell and the macros use, such as `Pipeline::new`, are public for them and are not listed.

## `Pipeline<'ps>` (`crates/pwrs/src/pipeline.rs`, `worker.rs`)

The pipeline-thread token, `!Send` and `!Sync`, created by the runtime for one phase and passed to `begin`, `process` and `end` as `&Pipeline<'_>`.

| Method | Signature | Does |
|---|---|---|
| `stopping` | `(&self) -> bool` | true once the engine called `StopProcessing` |
| `on_stop` | `(&self, waker: impl FnOnce() + Send + 'static) -> PsResult<StopWaker<'ps>>` | registers `waker` for `StopProcessing` to run on the engine's thread when the stop comes, or runs it at once when the stop already came, so a phase blocked on something of its own wakes with no poll; at most once; the [`StopWaker`](#stopwaker-stoprs) guard unregisters it when dropped; a second registration while a guard lives is refused with `PwrsStopWakerHeld` |
| `cmdlet_handle` | `(&self) -> PsHandle` | the managed cmdlet's handle, for vtable entries that need it |
| `write` | `<T: IntoPs>(&self, value: T) -> PsResult<()>` | writes any convertible value; `Vec<T>` enumerates, `PsArray<T>` does not; `i64`, `f64`, `bool` and strings take a direct entry, and every other scalar takes a handle to keep its CLR width |
| `write_object` | `(&self, obj: &PsObject) -> PsResult<()>` | `WriteObject(obj)` |
| `write_enumerated` | `(&self, obj: &PsObject) -> PsResult<()>` | `WriteObject(obj, true)` |
| `write_str`, `write_i64`, `write_f64`, `write_bool` | `(&self, value) -> PsResult<()>` | the direct scalar entries `write` uses |
| `write_error` | `(&self, e: &PsError) -> PsResult<()>` | `WriteError`, or a pending `ThrowTerminatingError` when `e.terminating` |
| `verbose`, `debug`, `warning`, `information` | `(&self, text: &str) -> PsResult<()>` | the four streams |
| `stream_enabled` | `(&self, kind: PsStreamKind) -> bool` | whether the engine would keep a record on that stream, from the stream's common parameter where it is bound and from the session's preference variable otherwise; asked once per phase and kept |
| `verbose_enabled`, `debug_enabled`, `warning_enabled`, `information_enabled` | `(&self) -> bool` | `stream_enabled` for one stream |
| `verbose_if`, `debug_if`, `warning_if`, `information_if` | `(&self, f: impl FnOnce() -> String) -> PsResult<()>` | writes what `f` returns, and calls `f` only when the record is kept |
| `write_host` | `(&self, text: &str, foreground: Option<ConsoleColor>, background: Option<ConsoleColor>, no_new_line: bool) -> PsResult<()>` | writes `text` to the host as `Write-Host` does: a `HostInformationMessage` tagged `PSHOST` on the information stream, shown whatever the information preference says unless it is Ignore or a `6>` redirection takes the record, kept by `-InformationVariable`, and recorded by a transcript as one line per call. A `None` color is the host's current one, read from its `RawUI` as `Write-Host` reads it, and stays unset on a host with no `RawUI`. The text goes as it is, escape codes included |
| `progress` | `(&self, activity_id: i32, activity: &str, status: &str, percent: i32) -> PsResult<()>` | `WriteProgress` of a record with an activity, a status and a percentage; a negative percent writes the record that completes the activity |
| `write_progress` | `(&self, record: &Progress) -> PsResult<()>` | `WriteProgress` with every field of a [`Progress`](#progress-progressrs): the parent activity, the current operation, the seconds left, and a negative percent on a processing record, which shows none |
| `should_process` | `(&self, target: &str, action: &str) -> PsResult<bool>` | `ShouldProcess` |
| `should_continue` | `(&self, query: &str, caption: &str) -> PsResult<bool>` | `ShouldContinue` |
| `host_ui` | `(&self) -> PsResult<HostUi>` | the cmdlet's `Host.UI`, for prompts and lines read from the person; see [`HostUi`](#hostui-host_uirs) |
| `events` | `(&self) -> PsResult<PsEvents>` | the engine events of the runspace the cmdlet runs in, the cmdlet's `Events` read through one dynamic member access; any thread raises through it, during the call and after it returns; see [`PsEvents`](#psevents-eventsrs) |
| `invocation` | `(&self) -> PsResult<PsObject>` | the cmdlet's `MyInvocation`, its `InvocationInfo`, through one dynamic member access with no vtable entry. `PipelinePosition` counts from 1 and `PipelineLength` counts commands, so an expression at the head of a pipeline is input and not counted. Set before `begin`, so any phase reads it. The engine runs begins left to right but drains a command's queued input right after its own begin, so when an upstream begin writes output, a command's `process` can run before a later command has begun |
| `parameter` | `(&self, name: &str) -> PsResult<PsObject>` | a bound parameter's value from `MyInvocation.BoundParameters`, or `$null` |
| `parameter_is_bound` | `(&self, name: &str) -> bool` | whether that table has the name |
| `variable` | `(&self, name: &str) -> PsResult<PsObject>` | `SessionState.PSVariable.GetValue` |
| `set_variable` | `(&self, name: &str, value: &PsObject) -> PsResult<()>` | `SessionState.PSVariable.Set` |
| `resolve_path` | `(&self, path: &str, literal: bool) -> PsResult<Vec<String>>` | `GetResolvedProviderPathFromPSPath`, or `GetUnresolvedProviderPathFromPSPath` when `literal` |
| `invoke` | `(&self, name: &str, parameters: &[(&str, PsObject)]) -> PsResult<Vec<PsObject>>` | a nested `PowerShell` in the current runspace with the command added by name, which the engine resolves as it does a typed command, so a module's function runs in its module's scope, and the parameters bound by name; no script text is built or parsed. The command's non-terminating errors are written to this cmdlet's error stream and its output is returned; a terminating one is the `Err`, and so is a name no command answers to, with the message of PowerShell's `CommandNotFoundException` |
| `invoke_with_input` | `(&self, name: &str, parameters: &[(&str, PsObject)], input: Option<&PsObject>) -> PsResult<Vec<PsObject>>` | as `invoke`, with `input` piped in: a collection is unrolled into records, a string or any other single value is one record |
| `stream_from_thread` | `<T: IntoPs + Send + 'static, F: FnOnce(Sender<T>) + Send + 'static>(&self, work: F) -> PsResult<()>` | runs `work` on a new thread and writes each item it sends, in order; waits on the channel 50 ms at a time and stops draining when the pipeline stops, sent or not; joins the worker |
| `stream_from_thread_until` | `<T: IntoPs + Send + 'static, F: FnOnce(Sender<T>, StopSignal) + Send + 'static>(&self, work: F) -> PsResult<()>` | `stream_from_thread`, with a `StopSignal` the worker keeps: `stop.is_set()` turns true when the pipeline thread stops draining, on a stop, a failed write, or the channel closing |
| `stream_from_worker` | `<T: IntoPs + Send + 'static, F: FnOnce(Worker<T>) + Send + 'static>(&self, work: F) -> PsResult<()>` | runs `work` on a new thread with a [`Worker<T>`](#workert-workerrs) through which it writes output and every other stream, and writes each record in the order sent; waits 50 ms at a time and stops draining when the pipeline stops; a terminating error the worker writes ends the call with that error; joins the worker |
| `par_map` | `<T: Send + 'static, U: IntoPs + Send + 'static, F: Fn(T) -> U + Send + Sync + 'static>(&self, items: Vec<T>, order: Order, f: F) -> PsResult<()>` | runs `f` over `items` on a pool as wide as `available_parallelism` and writes every result from this thread; `Order::Input` in the order given, `Order::AsReady` as each finishes; waits 50 ms at a time, so a stop is seen while no result arrives, and starts no item after it; joins every worker |
| `par_map_with` | `<T: Send + 'static, U: IntoPs + Send + 'static, F: Fn(T, &Worker) -> U + Send + Sync + 'static>(&self, items: Vec<T>, order: Order, f: F) -> PsResult<()>` | `par_map`, with a [`Worker`](#workert-workerrs) handed to `f` for the other streams and the stop; the results keep `order`, and what `f` sends through the worker is written as it arrives |
| `par_for_each` | `<T: Send + 'static, F: Fn(T) + Send + Sync + 'static>(&self, items: Vec<T>, f: F) -> PsResult<()>` | the same pool for work whose results are not written; this thread waits without spinning |
| `par_for_each_with` | `<T: Send + 'static, F: Fn(T, &Worker) + Send + Sync + 'static>(&self, items: Vec<T>, f: F) -> PsResult<()>` | `par_for_each`, with a [`Worker`](#workert-workerrs) handed to `f` |
| `workers` | `(&self, count: usize) -> PsResult<Workers<'_, 'ps>>` | the four parallel helpers with at most `count` items running at once; see [`Workers`](#workers-workerrs); zero is refused with `PwrsWorkerCount` |

The four writers take a `&str`, so a caller that formats one pays the
format and the allocation whether or not anything reads the result: the
engine decides that on the managed side, after the crossing. The
`verbose!`, `debug!`, `warning!` and `information!` macros wrap the
`_if` methods around a format, which is the short way to write it:

```rust
pwrs::verbose!(ps, "greeting {}", self.name)?;
```

A record counts as kept where the engine keeps one written by its own
`Write-*` cmdlets, measured in pwsh 7.6.6 and Windows PowerShell
5.1.26100.9444. A verbose or debug record is kept only where it is
shown. A warning is also kept under SilentlyContinue, in
`-WarningVariable`, and an information record under SilentlyContinue,
the default, reaches `-InformationVariable` and a `6>` redirection.
Under Ignore a warning or information record is written only when its
`-WarningVariable` or `-InformationVariable` is bound: pwsh 7.6.6 fills
the variable and Windows PowerShell 5.1 does not.

`Order` is `AsReady` or `Input`. A worker panic in any of the parallel helpers becomes a terminating `PwrsWorkerPanic` error, and no item starts after it. The pool is std threads; the `parallel` feature swaps in Flynnel for all of them, which changes nothing about these signatures or the module's PowerShell surface.

Every method that reaches a stream or session state returns an error when the engine refuses (status 3 off-thread, status 5 when the pipeline stopped, status 1 for a managed exception); the error's message is the exception's `Type: Message`.

## `Progress` (`progress.rs`)

`Progress { activity_id: i32, parent_activity_id: i32, activity: String, status: String, current_operation: String, percent_complete: i32, seconds_remaining: i32, completed: bool }`, a record for `Pipeline::write_progress` and `Worker::write_progress` with every field a `ProgressRecord` has; `Clone`, `Debug`, `PartialEq`, `Eq`.

| Method | Signature | Does |
|---|---|---|
| `new` | `(activity_id: i32, activity: impl Into<String>, status: impl Into<String>) -> Progress` | a processing record with no parent (-1), no current operation, no percentage (-1) and no time left (-1) |
| `with_parent` | `(self, parent: i32) -> Progress` | nests the activity under `parent`; negative for none |
| `with_current_operation` | `(self, operation: impl Into<String>) -> Progress` | the step under way, shown below the bar; empty for none |
| `with_percent` | `(self, percent: i32) -> Progress` | 0 to 100; negative shows no percentage, and above 100 is written as 100 |
| `with_seconds_remaining` | `(self, seconds: i32) -> Progress` | negative shows none |
| `completed` | `(self) -> Progress` | makes it the record that ends the activity and removes its bar; without it the record is processing whatever its percentage |

The engine refuses a record whose activity or status is empty, whose activity id is negative, or whose parent activity id is its own activity id, and the write returns its error.

## `Worker<T>` (`worker.rs`)

A worker thread's way to the pipeline that started it: `Clone`, `Send` and `Sync`. `T` defaults to `Infallible`, a worker with no output, which is what `par_map_with` and `par_for_each_with` hand out; `stream_from_worker` hands out a `Worker<T>` whose output is `T`. Each record goes to the pipeline thread, which writes it on its stream in the order sent; of the progress records for one activity waiting together, only the last is written. Every method that sends answers whether the pipeline took the record, false once `stopping()` is true.

| Method | Signature | Sends |
|---|---|---|
| `write` | `(&self, value: T) -> bool` | `value` to the output stream |
| `write_error` | `(&self, error: PsError) -> bool` | `error` to the error stream; a terminating error ends the call with that error once the pipeline thread reaches it |
| `warning`, `verbose`, `debug`, `information` | `(&self, text: impl Into<String>) -> bool` | `text` to that stream |
| `write_progress` | `(&self, record: Progress) -> bool` | a progress record |
| `stopping` | `(&self) -> bool` | nothing; true once the pipeline is stopping, a write failed, a terminating error was written, a worker of the same call panicked, or the call has returned |

`StopSignal` (`worker.rs`) is `Clone` and `Send`: `is_set(&self) -> bool`.

## `StopWaker` (`stop.rs`)

From `Pipeline::on_stop`: a guard over the waker the phase registered, with no methods. Dropping it unregisters the waker and frees it if it has not run. The waker and the guard share one slot on the cmdlet's instance; whichever of the stop and the guard empties the slot first owns the waker, so it runs at most once and is never freed while it runs. A waker that panics is caught on the stopping thread and reported on standard error, since nothing may unwind across the native export the stop arrives through.

## `Workers` (`worker.rs`)

From `Pipeline::workers(count)`: `count(&self) -> usize`, and `par_map`, `par_map_with`, `par_for_each` and `par_for_each_with` with the `Pipeline` signatures, running at most `count` items at once. The default pool starts that many threads for the call, or one per item when there are fewer items; under the `parallel` feature they are tasks on Flynnel's pool, which also runs no more at once than it has workers.

## `PsObject` (`object.rs`, `dynamic.rs`, `pinned.rs`)

An owned `GCHandle`; `Send`, `Sync`, `Clone` (a second handle to the same object), `Default` (`$null`).

| Method | Signature | Does |
|---|---|---|
| `null` | `() -> PsObject` | `$null` |
| `is_null` | `(&self) -> bool` | |
| `from_raw` | `unsafe (PsHandle) -> PsObject` | takes ownership of a handle |
| `as_raw` | `(&self) -> PsHandle` | |
| `into_raw` | `(self) -> PsHandle` | gives up ownership |
| `get` | `(&self, name: &str) -> PsResult<PsObject>` | a property, field, ETS member, or dictionary entry by name |
| `set` | `(&self, name: &str, value: &PsObject) -> PsResult<()>` | a settable property |
| `call` | `(&self, name: &str, args: &[PsObject]) -> PsResult<PsObject>` | an instance method; PowerShell's binder, then the CLR's |
| `type_name` | `(&self) -> PsResult<String>` | `GetType().FullName` |
| `pin` | `<T: Primitive>(&self) -> PsResult<Pinned<'_, T>>` | a pinned borrow of a primitive array |
| `from_slice` | `<T: Primitive + IntoPs>(&[T]) -> PsResult<PsObject>` | a new array filled through one pin |

Free functions in `pwrs::object`: `new_psobject(type_name: &str) -> PsObject`, `add_note(obj: &PsObject, name: &str, value: PsObject) -> PsResult<()>`, and `property(obj: &PsObject, name: &str) -> PsResult<PsObject>`, which reads one back by name, note properties included, and fails for a name the object does not carry rather than reading as `$null`. `PsObject::get` goes to the underlying .NET object instead, so it does not see a note.

## `Pinned<'a, T>`, `PsMemory<T>`, `PsMemoryView<T>` and `PsRevocation` (`pinned.rs`)

`Pinned<'a, T>` derefs to `[T]` and `[T]` mutably; releases the pin on drop. Must not outlive the phase.

`PsMemory<T: Primitive>`: `zeroed(len: usize) -> PsResult<PsMemory<T>>`, `from_slice(&[T]) -> PsResult<PsMemory<T>>`, `TryFrom<Vec<T>>`, `len()`, `is_empty()`, `read_only(self) -> PsMemory<T>`, `revocable(self, &PsRevocation) -> PsMemory<T>`; derefs to `[T]` and `[T]` mutably; `Send` and `Sync`; `IntoPs` hands the buffer to the engine as a `Memory<T>`, or a `ReadOnlyMemory<T>` after `read_only` (a `T[]` copy on .NET Framework).

`PsMemoryView<T: Primitive>`: `unsafe writable<O: Send + Sync + 'static>(ptr: *mut T, len: usize, owner: O) -> PsMemoryView<T>`, `unsafe read_only<O: Send + Sync + 'static>(ptr: *const T, len: usize, owner: O) -> PsMemoryView<T>`, `revocable(self, &PsRevocation) -> PsMemoryView<T>`, `len()`, `is_empty()`; `Send` and `Sync`; `IntoPs` hands the memory to the engine as a `Memory<T>` or `ReadOnlyMemory<T>` over it (a `T[]` copy on .NET Framework) and the owner with it, dropped when the view is released.

`PsRevocation`: `new() -> PsRevocation`, `revoke(&self)`, `is_revoked(&self) -> bool`; `Clone` shares the flag, which is never cleared.

## `PsTask<T>` (`task.rs`)

The argument a `#[psmethods]` method takes to return a `Task<T>` (a `Task` for `PsTask<()>`); see [Attribute Reference](Attribute-Reference.md#psmethods). `complete(self, value: T)` where `T: IntoPs`, converting on the calling thread and faulting the task with the conversion's error if it fails; `fail(self, error: PsError)`; `cancel(self)`; `finish(self, result: PsResult<T>)`, which completes with the value or faults with the error; `is_cancelled(&self) -> bool`, a byte read that the caller's `CancellationToken` sets. `Send`, so it moves to whatever thread does the work. Settling consumes it, so a task settles once; one dropped unsettled faults with `PwrsTaskDropped`. Continuations run asynchronously, never on the settling thread.

## `PsEvents` (`events.rs`)

The engine events of a runspace, from `Pipeline::events`: `Clone`, `Send` and `Sync`, and it keeps the runspace's event manager alive while it lives. See [How To Raise Engine Events](../how-to/How-To-Raise-Engine-Events.md).

| Method | Signature | Does |
|---|---|---|
| `raise` | `(&self, source_identifier: &str, message_data: impl IntoPs) -> PsResult<()>` | raises the event with `message_data` as its `MessageData` and no sender or arguments |
| `raise_with` | `(&self, source_identifier: &str, sender: impl IntoPs, args: impl IntoPs, message_data: impl IntoPs) -> PsResult<()>` | raises it as `New-Event -Sender -EventArguments -MessageData` does; `()` is none of any of them, and `args` that converts to an array gives the event its elements as arguments |

Both convert their values on the calling thread while holding `attach_current_thread()`, and a value that does not convert raises nothing and is the `Err`. The event is queued, or handed to the actions subscribed to it, before the call returns, so the events one thread raises arrive in the order raised; an action runs later on the runspace's pipeline thread.

## `pwrs::thread` (`thread.rs`)

For a thread the module starts to own a window, a COM object or anything else that belongs to one thread. See [How To Use Threads](../how-to/How-To-Use-Threads.md#a-thread-for-a-window-or-a-com-object).

| Item | Signature | Does |
|---|---|---|
| `enter_sta` | `() -> PsResult<StaGuard>` | on Windows, `CoInitializeEx` with `COINIT_APARTMENTTHREADED` on the calling thread, and `CoUninitialize` when the guard drops; elsewhere nothing. A thread already single-threaded keeps its apartment; one in the multithreaded apartment is refused with `PwrsApartmentChanged`. The guard is `!Send`, so it stays on the thread that entered |
| `apartment` | `() -> Option<Apartment>` | the calling thread's apartment, as `CoGetApartmentType` reports it; `None` where there is no COM, which is everywhere but Windows |
| `Apartment` | enum | `Sta`, `MainSta`, `Mta`, `ImplicitMta` (a thread that never initialized COM in a process where another thread joined the multithreaded apartment), `Neutral`, `NotInitialized` |

## `PsType` (`dynamic.rs`)

| Method | Signature | Does |
|---|---|---|
| `from_name` | `(name: impl Into<String>) -> PsType` | a type by the name the engine's type resolver accepts |
| `name` | `(&self) -> &str` | |
| `call_static` | `(&self, method: &str, args: &[PsObject]) -> PsResult<PsObject>` | a public static method |
| `new` | `(&self, args: &[PsObject]) -> PsResult<PsObject>` | a public constructor |

`pwrs::dynamic::args_array(&[PsObject]) -> PsResult<PsObject>` builds the `object[]` those calls pass.

## `PsHashtable`, `PsScriptBlock`, `PsBigInt`, `PsSecureString`, `PsCredential` (`types.rs`)

`PsHashtable(pub PsObject)`: `new() -> PsResult<PsHashtable>`, `get(key) -> PsResult<PsObject>`, `set(key, value) -> PsResult<()>`, `contains(key) -> PsResult<bool>`, `len() -> PsResult<usize>`, `is_empty() -> PsResult<bool>`, `keys() -> PsResult<Vec<String>>`.

`PsScriptBlock(pub PsObject)`: `call(&self, ps: &Pipeline<'_>, args: &[PsObject]) -> PsResult<Vec<PsObject>>`.

`PsBigInt { pub bytes: Vec<u8> }`: little-endian two's-complement; `is_negative()`, `doubled()`.

`PsSecureString(pub PsObject)`: `new(text: &str) -> PsResult<PsSecureString>`, `len() -> PsResult<usize>`, `is_empty() -> PsResult<bool>`, `reveal() -> PsResult<String>`.

`PsCredential { pub user_name: String, pub password: PsSecureString }`: `new(user_name: &str, password: &str) -> PsResult<PsCredential>`.

`pwrs::types::display_string(&PsObject) -> PsResult<String>` is `String::from_ps`.

## `HostUi` (`host_ui.rs`)

`$Host.UI` of the cmdlet's host, from `Pipeline::host_ui`. Every method is a dynamic member access on the pipeline thread, which is what a prompt costs next to the wait for a person. A host that cannot prompt, such as one run with `-NonInteractive`, refuses with the engine's own error, and that is the `Err`.

| Method | Signature | Engine call |
|---|---|---|
| `read_line` | `(&self) -> PsResult<String>` | `ReadLine`; the line without its ending |
| `read_line_as_secure_string` | `(&self) -> PsResult<PsSecureString>` | `ReadLineAsSecureString`; typed without echo. The console host reads it from the console device, not from its standard input, so a redirected shell waits on it with nothing to read |
| `write_line` | `(&self, text: &str) -> PsResult<()>` | `WriteLine`; the host's own output, which is not the pipeline and reaches no downstream command |
| `prompt_for_choice` | `(&self, caption: &str, message: &str, choices: &[(&str, &str)], default: usize) -> PsResult<usize>` | `PromptForChoice` over `ChoiceDescription`s built from each label and help text; the engine's binder takes the array as the `Collection<ChoiceDescription>` the method declares. A `&` in a label marks its hot key. Returns the index chosen; `default` is taken on an empty answer |

## `PsDateTime`, `PsTimeSpan`, `PsGuid` (`values.rs`)

`PsDateTime { pub ticks: i64, pub kind: DateTimeKind }`: `new(ticks, kind)`, `utc(ticks)`, `to_utc() -> PsResult<PsDateTime>`; `TryFrom<SystemTime>` and `TryFrom<PsDateTime> for SystemTime`. `DateTimeKind` is `Unspecified`, `Utc` or `Local`. Constants `TICKS_PER_SECOND`, `UNIX_EPOCH_TICKS`, `MAX_DATETIME_TICKS`.

`PsTimeSpan { pub ticks: i64 }`: `from_ticks(ticks)`, `is_negative()`; `TryFrom<Duration>` and `TryFrom<PsTimeSpan> for Duration`; ordered.

`PsGuid { pub bytes: [u8; 16] }`: `NIL`, `to_rfc4122() -> [u8; 16]`, `from_rfc4122(bytes)`; `Display` and `FromStr`.

## `PsError` and `ErrorCategory` (`error.rs`)

`PsError { message, error_id, category, target: Option<PsObject>, terminating, details: Option<String> }` with `new(category, error_id, message)`, `terminating(self)`, `with_target(self, PsObject)`, `with_details(self, text)`; `From<std::io::Error>`; `Display` as `[id] message`. Details reach the record's `Exception.InnerException` on every path an error leaves by, a cmdlet's record written or terminating and the exception a proxy method, static call, field read, provider, completer, transform or lifecycle hook throws, and the default error view still shows `message` alone. `ErrorCategory` has the 32 values of `System.Management.Automation.ErrorCategory` with the engine's numeric values, and `ErrorCategory::from_code(u32) -> Option<ErrorCategory>` maps one back. `PsResult<T>` is `Result<T, PsError>`.

`PsError` is what a cmdlet raises. `PsErrorRecord { category: ErrorCategory, error_id: String, message: String, target: PsObject }` in `values.rs` is what a cmdlet reads: `FromPs` for a `System.Management.Automation.ErrorRecord` taken as a parameter or off the pipeline, from `-ErrorVariable`, from a `catch`, or from another command run through `invoke`. It is `FromPs` only; `target` is `$null` when the record carries none.

## `Cmdlet`, `CmdletMeta`, `CmdletBind` (`cmdlet.rs`)

`Cmdlet` is the one you implement: `process(&mut self, &Pipeline<'_>) -> PsResult<()>` is required, `begin` and `end` have default bodies that mark the phase empty so the runtime stops calling them for that type.

`CmdletMeta` (the `Verb-Noun` name, the descriptor, the phase mask) and `CmdletBind` (`bind`, which reads the parameter block) are implemented by `#[cmdlet]` and `#[param]`. They are public because the generated code in your crate names them, not because a module writes them; writing either by hand is not supported.

`TransformFn` (`transform.rs`) and `CompleterFn` (`completer.rs`) are the same shape for `#[transform]` and `#[completer]`: the macro turns the function into a unit struct implementing the trait, and `export_module!` lists that struct.

## `Completion`, `CompletionContext`, `DynamicParam` (`completer.rs`)

`CompletionContext { word, command, bound: PsHashtable }`. `Completion { text, list_item, kind: CompletionKind, tooltip }` with `value(text)` and `with_tooltip(text)`. `DynamicParam { name, clr_type, mandatory, position, set, help, validate_set }` with `string(name)`, `mandatory()`, `with_validate_set(values)`.

## `Item`, `Drive`, `Provider` (`provider.rs`)

`Item { path, value, is_container }` with `leaf(path, value)` and `container(path, value)`; `Drive { name, root }`; the `Provider` trait's methods are listed in [How To Write A Provider](How-To-Write-A-Provider.md): one value of the type serves one drive, made by `default_drives() -> PsResult<Vec<(Drive, Self)>>` or `new_drive(name, root) -> PsResult<(Drive, Self)>`, dropped after `remove_drive(&mut self)`. `pwrs::provider::child_name(path)` is the last segment of a `\`- or `/`-separated path.

## `pwrs::testing` and `pwrs::trace`

The fake host for unit tests and the counters are described in [How To Test A Module](How-To-Test-A-Module.md) and [How To Trace A Module](How-To-Trace-A-Module.md).
