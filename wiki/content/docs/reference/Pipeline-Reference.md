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
| `write` | `<T: IntoPs>(&self, value: T) -> PsResult<()>` | writes any convertible value; `Vec<T>` enumerates, `PsArray<T>` does not; `i64`, `f64`, `bool` and strings take a direct entry, and every other scalar goes through a handle, so it reaches PowerShell at its own CLR width |
| `write_object` | `(&self, obj: &PsObject) -> PsResult<()>` | `WriteObject(obj)` |
| `write_enumerated` | `(&self, obj: &PsObject) -> PsResult<()>` | `WriteObject(obj, true)` |
| `write_str`, `write_i64`, `write_f64`, `write_bool` | `(&self, value) -> PsResult<()>` | the direct scalar entries `write` uses |
| `write_error` | `(&self, e: &PsError) -> PsResult<()>` | `WriteError`, or a pending `ThrowTerminatingError` when `e.terminating` |
| `verbose`, `debug`, `warning`, `information` | `(&self, text: &str) -> PsResult<()>` | the four streams |
| `stream_enabled` | `(&self, kind: PsStreamKind) -> bool` | whether a record written to that stream would go somewhere, by the preference the engine gives a compiled cmdlet: the stream's own common parameter where it is bound, then `-Debug`, under which PowerShell prompts the user after each verbose record and warning, and `-Verbose`, which shows warnings, then the session's preference variable; read from the host once per cmdlet instance, since the managed cmdlet works each stream out once for its instance, so later phases and records use the stored result |
| `verbose_enabled`, `debug_enabled`, `warning_enabled`, `information_enabled` | `(&self) -> bool` | `stream_enabled` for one stream |
| `verbose_if`, `debug_if`, `warning_if`, `information_if` | `(&self, f: impl FnOnce() -> String) -> PsResult<()>` | writes what `f` returns, and calls `f` only when the record would go somewhere |
| `write_host` | `(&self, text: &str, foreground: Option<ConsoleColor>, background: Option<ConsoleColor>, no_new_line: bool) -> PsResult<()>` | writes `text` to the host as `Write-Host` does: a `HostInformationMessage` tagged `PSHOST` on the information stream, shown whatever the information preference says unless it is Ignore or a `6>` redirection takes the record, saved by `-InformationVariable`, and recorded by a transcript as one line per call. A `None` color is the host's current one, read from its `RawUI` as `Write-Host` reads it, and stays unset on a host with no `RawUI`. The text goes as it is, escape codes included |
| `progress` | `(&self, activity_id: i32, activity: &str, status: &str, percent: i32) -> PsResult<()>` | `WriteProgress` of a record with an activity, a status and a percentage; a negative percent writes the record that completes the activity |
| `write_progress` | `(&self, record: &Progress) -> PsResult<()>` | `WriteProgress` with every field of a [`Progress`](#progress-progressrs): the parent activity, the current operation, the seconds left, and a negative percent on a processing record, which shows none |
| `should_process` | `(&self, target: &str, action: &str) -> PsResult<bool>` | `ShouldProcess` |
| `should_continue` | `(&self, query: &str, caption: &str) -> PsResult<bool>` | `ShouldContinue` |
| `should_process_target` | `(&self, target: &str) -> PsResult<bool>` | `ShouldProcess(target)`, the command's name as the action |
| `should_process_described` | `(&self, verbose_description: &str, verbose_warning: &str, caption: &str) -> PsResult<bool>` | `ShouldProcess(verboseDescription, verboseWarning, caption)`: `-WhatIf` and `-Verbose` show the description, and `-Confirm` prompts with the warning under the caption |
| `should_process_reason` | the same arguments `-> PsResult<(bool, ShouldProcessReason)>` | the same with its `out ShouldProcessReason`: `WhatIf` when the result came from `-WhatIf`, else `None` |
| `should_continue_all` | `(&self, query: &str, caption: &str) -> PsResult<bool>` | `ShouldContinue(query, caption, ref yesToAll, ref noToAll)`: the prompt offers Yes to All and No to All, and that answer applies to the invocation's later records, for which the user is not prompted again |
| `should_continue_all_with_impact` | `(&self, query: &str, caption: &str, has_security_impact: bool) -> PsResult<bool>` | the same with `hasSecurityImpact`, which makes No the prompt's default |
| `paging` | `(&self) -> PsResult<PsPaging>` | the values `-First`, `-Skip` and `-IncludeTotalCount` gave a cmdlet declaring `supports_paging`, read from its `PagingParameters`: `first` is `u64::MAX` and `skip` 0 when not given, and `PsPaging::window(len)` is the range of positions to write out of `len` records. A cmdlet without `supports_paging` gets an error naming it |
| `write_total_count` | `(&self, total: u64, accuracy: f64) -> PsResult<()>` | `WriteObject(PagingParameters.NewTotalCount(total, accuracy))`: the total count the caller requests with `-IncludeTotalCount`, the number with an `Accuracy` property, whose text is the engine's own, `Total count: N` for 1, `Estimated total count: N` between 0 and 1 and `Unknown total count` for 0 |
| `transaction` | `(&self) -> PsResult<Option<PsTransaction>>` | the PowerShell transaction a cmdlet declaring `supports_transactions` runs in: `Some` when it was given `-UseTransaction` inside `Start-Transaction`, which only Windows PowerShell 5.1 allows, so in PowerShell 7 it is always `None`. A cmdlet without `supports_transactions` gets an error naming it. See [`PsTransaction`](#pstransaction-and-enlistment-transactionrs) |
| `start_job` | `(&self, name: Option<&str>, work: impl FnOnce(&JobContext) -> PsResult<()> + Send + 'static) -> PsResult<PsObject>` | a `RustJob` running `work` on a thread of its own, returned for the cmdlet to write; Get-Job, Wait-Job, Receive-Job, Stop-Job and Remove-Job see it, and `None` names it `Job<n>`. See [`JobContext`](#jobcontext-jobbody-cmdletjob-and-streams-jobrs-streamsrs) |
| `start_job_body` | `(&self, name: Option<&str>, body: impl JobBody) -> PsResult<PsObject>` | `start_job` with the work as a value of a type implementing `JobBody` |
| `as_job` | `(&self) -> PsResult<bool>` | whether the caller gave `-AsJob`, the switch `#[cmdlet(as_job)]` declares; false for a cmdlet without it |
| `host_ui` | `(&self) -> PsResult<HostUi>` | the cmdlet's `Host.UI`, for prompts and lines read from the person; see [`HostUi`](#hostui-host_uirs) |
| `events` | `(&self) -> PsResult<PsEvents>` | the engine events of the runspace the cmdlet runs in, the cmdlet's `Events` read through one dynamic member access; any thread raises through it, during the call and after it returns; see [`PsEvents`](#psevents-eventsrs) |
| `runspace_id` | `(&self) -> PsResult<PsGuid>` | the `InstanceId` of the runspace the cmdlet runs in, read from `Runspace.DefaultRunspace` through a static call and a member access, with no vtable entry; the id an `#[on_remove]` taking a [`&Removal`](#removal-and-removalcause-lifecyclers) is given when that runspace's import is removed, so a command records it to release later what that runspace made |
| `invocation` | `(&self) -> PsResult<PsObject>` | the cmdlet's `MyInvocation`, its `InvocationInfo`, through one dynamic member access with no vtable entry. `PipelinePosition` counts from 1 and `PipelineLength` counts commands, so an expression at the head of a pipeline is input and not counted. Set before `begin`, so any phase reads it. The engine runs begins left to right but drains a command's queued input right after its own begin, so when an upstream begin writes output, a command's `process` can run before a later command has begun |
| `parameter` | `(&self, name: &str) -> PsResult<PsObject>` | a bound parameter's value from `MyInvocation.BoundParameters`, or `$null` |
| `parameter_is_bound` | `(&self, name: &str) -> bool` | whether that table has the name |
| `variable` | `(&self, name: &str) -> PsResult<PsObject>` | `SessionState.PSVariable.GetValue` |
| `set_variable` | `(&self, name: &str, value: &PsObject) -> PsResult<()>` | `SessionState.PSVariable.Set` |
| `resolve_path` | `(&self, path: &str, literal: bool) -> PsResult<Vec<String>>` | `GetResolvedProviderPathFromPSPath`, or `GetUnresolvedProviderPathFromPSPath` when `literal` |
| `invoke` | `(&self, name: &str, parameters: &[(&str, PsObject)]) -> PsResult<Vec<PsObject>>` | a nested `PowerShell` in the current runspace with the command added by name, which the engine resolves as it does a typed command, so a module's function runs in its module's scope, and the parameters bound by name; no script text is built or parsed. The command's non-terminating errors are written to this cmdlet's error stream and its output is returned; a terminating one is the `Err`, and so is a name that matches no command, with the message of PowerShell's `CommandNotFoundException` |
| `invoke_with_input` | `(&self, name: &str, parameters: &[(&str, PsObject)], input: Option<&PsObject>) -> PsResult<Vec<PsObject>>` | as `invoke`, with `input` piped in: a collection is unrolled into records, a string or any other single value is one record |
| `steppable` | `(&self, command: &str, parameters: &[(&str, PsObject)]) -> PsResult<PsSteppable>` | begins `command`, resolved as a typed command is, as a [`PsSteppable`](#pssteppable-steppablers) whose output and streams are this cmdlet's, with `parameters` bound by name and the caller's `-WhatIf`, `-Confirm`, `-Verbose`, `-Debug`, `-ErrorAction`, `-WarningAction` and `-InformationAction` added where the command takes them and `parameters` does not name them. For `cargo pwrs test`'s check, a command taking `-WhatIf` counts as this cmdlet calling `should_process` |
| `stream_from_thread` | `<T: IntoPs + Send + 'static, F: FnOnce(Sender<T>) + Send + 'static>(&self, work: F) -> PsResult<()>` | runs `work` on a new thread and writes each item it sends, in order; waits on the channel 50 ms at a time and stops draining when the pipeline stops, sent or not; joins the worker |
| `stream_from_thread_until` | `<T: IntoPs + Send + 'static, F: FnOnce(Sender<T>, StopSignal) + Send + 'static>(&self, work: F) -> PsResult<()>` | `stream_from_thread`, with a `StopSignal` passed to the worker: `stop.is_set()` turns true when the pipeline thread stops draining, on a stop, a failed write, or the channel closing |
| `stream_from_worker` | `<T: IntoPs + Send + 'static, F: FnOnce(Worker<T>) + Send + 'static>(&self, work: F) -> PsResult<()>` | runs `work` on a new thread with a [`Worker<T>`](#workert-workerrs) through which it writes output and every other stream, and writes each record in the order sent; waits 50 ms at a time and stops draining when the pipeline stops; a terminating error the worker writes ends the call with that error; joins the worker |
| `par_map` | `<T: Send + 'static, U: IntoPs + Send + 'static, F: Fn(T) -> U + Send + Sync + 'static>(&self, items: Vec<T>, order: Order, f: F) -> PsResult<()>` | runs `f` over `items` on a pool as wide as `available_parallelism` and writes every result from this thread; `Order::Input` in the order given, `Order::AsReady` as each finishes; waits 50 ms at a time, so a stop is seen while no result arrives, and starts no item after it; joins every worker |
| `par_map_with` | `<T: Send + 'static, U: IntoPs + Send + 'static, F: Fn(T, &Worker) -> U + Send + Sync + 'static>(&self, items: Vec<T>, order: Order, f: F) -> PsResult<()>` | `par_map`, with a [`Worker`](#workert-workerrs) passed to `f` for the other streams and the stop; the results are written in `order`, and what `f` sends through the worker is written as it arrives |
| `par_for_each` | `<T: Send + 'static, F: Fn(T) + Send + Sync + 'static>(&self, items: Vec<T>, f: F) -> PsResult<()>` | the same pool for work whose results are not written; this thread waits without spinning |
| `par_for_each_with` | `<T: Send + 'static, F: Fn(T, &Worker) + Send + Sync + 'static>(&self, items: Vec<T>, f: F) -> PsResult<()>` | `par_for_each`, with a [`Worker`](#workert-workerrs) passed to `f` |
| `workers` | `(&self, count: usize) -> PsResult<Workers<'_, 'ps>>` | the four parallel helpers with at most `count` items running at once; see [`Workers`](#workers-workerrs); zero is refused with `PwrsWorkerCount` |

The four writers take a `&str`, so a caller that formats one spends the
time to format and allocate it whether or not anything reads the result:
PowerShell checks that on the .NET side, after the call. The
`verbose!`, `debug!`, `warning!` and `information!` macros wrap the
`_if` methods around a format, which is the short way to write it:

```rust
pwrs::verbose!(ps, "greeting {}", self.name)?;
```

A record goes somewhere when one written by PowerShell's own `Write-*`
cmdlets would be shown, saved in a variable or redirected, as measured
in pwsh 7.6.6 and Windows PowerShell 5.1.26100.9444. A verbose or debug
record goes somewhere only where it is shown. A warning under
SilentlyContinue still goes to `-WarningVariable`, and an information
record under SilentlyContinue, the default, reaches
`-InformationVariable` and a `6>` redirection.
Under Ignore a warning or information record is written only when its
`-WarningVariable` or `-InformationVariable` is bound: pwsh 7.6.6 fills
the variable and Windows PowerShell 5.1 does not.
`-Debug` shows verbose records and warnings and prompts after each one
whether to continue, as it does for any compiled cmdlet; Windows
PowerShell 5.1 prompts after each debug record too, where pwsh 7 shows
them without a prompt. `-Verbose` shows warnings whatever
`$WarningPreference` is set to.

`Order` is `AsReady` or `Input`. A worker panic in any of the parallel helpers becomes a terminating `PwrsWorkerPanic` error, and no item starts after it. The pool is std threads; the `parallel` feature swaps in Flynnel for all of them, which changes nothing about these signatures or the module's PowerShell surface.

Every method that reaches a stream or session state returns an error when the engine refuses (status 3 off-thread, status 5 when the pipeline stopped, status 1 for a managed exception); the error's message is the exception's `Type: Message`.

An exception that passes through an output write, `write_error`, a stream record, `write_host`, `should_process` or `should_continue` is not an error about the cmdlet's own record: a later command's throw coming back through the write, or a prompt that `-Debug` or `-Confirm` makes the engine show and the host cannot show. The call returns it, and every later write and prompt of the phase returns it again without reaching the engine. Once the phase returns, the command ends with that exception, as a compiled cmdlet's would, whatever `-ErrorAction` is set to. The engine's refusal of a progress record is the cmdlet's own error, which `-ErrorAction` applies to.

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

What a worker thread uses to write to the pipeline that started it: `Clone`, `Send` and `Sync`. `T` defaults to `Infallible`, a worker with no output, which is what `par_map_with` and `par_for_each_with` pass to `f`; `stream_from_worker` gives a `Worker<T>` whose output is `T`. Each record goes to the pipeline thread, which writes it on its stream in the order sent; of the progress records for one activity waiting together, only the last is written. Every method that sends returns whether the pipeline took the record, false once `stopping()` is true.

| Method | Signature | Sends |
|---|---|---|
| `write` | `(&self, value: T) -> bool` | `value` to the output stream |
| `write_error` | `(&self, error: PsError) -> bool` | `error` to the error stream; a terminating error ends the call with that error once the pipeline thread reaches it |
| `warning`, `verbose`, `debug`, `information` | `(&self, text: impl Into<String>) -> bool` | `text` to that stream |
| `write_progress` | `(&self, record: Progress) -> bool` | a progress record |
| `stopping` | `(&self) -> bool` | nothing; true once the pipeline is stopping, a write failed, a terminating error was written, a worker of the same call panicked, or the call has returned |

`StopSignal` (`worker.rs`) is `Clone` and `Send`: `is_set(&self) -> bool`.

## `StopWaker` (`stop.rs`)

From `Pipeline::on_stop`: a guard over the waker the phase registered, with no methods. Dropping it unregisters the waker and frees it if it has not run. The waker and the guard share one slot on the cmdlet's instance; whichever of the stop and the guard empties the slot first takes the waker, so it runs at most once and is never freed while it runs. A waker that panics is caught on the stopping thread and reported on standard error, since nothing may unwind across the native export the stop arrives through.

## `Workers` (`worker.rs`)

From `Pipeline::workers(count)`: `count(&self) -> usize`, and `par_map`, `par_map_with`, `par_for_each` and `par_for_each_with` with the `Pipeline` signatures, running at most `count` items at once. The default pool starts that many threads for the call, or one per item when there are fewer items; under the `parallel` feature they are tasks on Flynnel's pool, which also runs no more at once than it has workers.

## `JobContext`, `JobBody`, `CmdletJob` and `Streams` (`job.rs`, `streams.rs`)

`JobContext` is what a job's work gets on its thread, `!Send` as the pipeline token is. [How To Run Work As A Job](../how-to/How-To-Run-Work-As-A-Job.md) shows each of these in use.

| Method | Signature | Does |
|---|---|---|
| `job` | `(&self) -> &PsObject` | the job, as Get-Job gives it |
| `write`, `write_object`, `write_enumerated` | as `Pipeline`'s | add to the job's output, which Receive-Job reads; a `Vec<T>` enumerates |
| `write_error` | `(&self, e: &PsError) -> PsResult<()>` | adds an error record to the job's error stream, its details as the inner exception and the exception the error names as the record's; the work goes on |
| `verbose`, `debug`, `warning`, `information` | `(&self, text: &str) -> PsResult<()>` | add to the job's message streams, every record stored whatever the preferences; Receive-Job applies the receiving session's |
| `write_progress` | `(&self, record: &Progress) -> PsResult<()>` | adds a progress record with every field |
| `stopping` | `(&self) -> bool` | true once Stop-Job, or Remove-Job -Force while the job ran, has requested a stop |
| `on_stop` | `(&self, waker: impl FnOnce() + Send + 'static) -> PsResult<StopWaker<'_>>` | as `Pipeline::on_stop`, for the job's stop request |

`JobBody` is `fn run(self, job: &JobContext) -> PsResult<()>` on a `Send + 'static` type, implemented for every closure of that shape. `Ok` ends the job Completed, `Err` Failed with the error as its `JobStateInfo.Reason`, a panic Failed with its message, and a job whose stop was requested ends Stopped, whatever the work returns.

`CmdletJob: Cmdlet + Clone` has `begin_job`, `process_job` and `end_job`, each `(&mut self, job: &JobContext) -> PsResult<()>`, `begin_job` and `end_job` defaulting to nothing. For a cmdlet declaring `as_job = "runtime"`, `-AsJob` runs them in one job for the whole invocation: each record's parameters are swapped into the job's one instance before `process_job`, a phase's `Err` is an error record of the job, and a terminating one ends it Failed. A `PsProxy<T>` parameter is `Clone` for any class, another handle to the same object.

`Streams` is implemented by `Pipeline` and `JobContext`: `write`, `write_object`, `write_enumerated`, `write_error`, `verbose`, `debug`, `warning`, `information` and their `_if` forms, which the `verbose!` macros call, `write_progress` and `stopping`. Work taking `&impl Streams` runs inline and in a job.

## `PsTransaction` and `Enlistment` (`transaction.rs`)

`PsTransaction` is the PowerShell transaction a command runs in, from `Pipeline::transaction` or a provider call's `Call::transaction`; it is `Clone` and `Send`. [How To Write A Cmdlet](../how-to/How-To-Write-A-Cmdlet.md#transactions) and [How To Write A Provider](../how-to/How-To-Write-A-Provider.md#transactions) show it in use.

| Method | Signature | What it does |
|---|---|---|
| `enlist` | `(&self, enlistment: impl Enlistment) -> PsResult<()>` | enlists the work. When the transaction is completed every enlistment's `prepare` runs, in the order enlisted, and then exactly one of `commit` and `rollback`: `commit` when all of them prepared, and `rollback` otherwise, for the enlistment that refused too. A transaction rolled back by `Undo-Transaction` or a timeout runs `rollback` alone. A transaction that has ended refuses the enlistment, which is dropped |
| `on_complete` | `(&self, commit: impl FnOnce() + Send + 'static, rollback: impl FnOnce() + Send + 'static) -> PsResult<()>` | enlists two closures on `enlist`'s terms, with no `prepare` |
| `id` | `(&self) -> PsResult<String>` | the transaction's `TransactionInformation.LocalIdentifier`, the same for every command that runs in it |
| `object` | `(&self) -> &PsObject` | the `System.Transactions.Transaction` |

`Enlistment: Send + 'static` has `prepare(&mut self) -> PsResult<()>`, which defaults to `Ok`, `commit(self)` and `rollback(self)`. An `Err` from `prepare` rolls the whole transaction back, and `Complete-Transaction` fails with a `TransactionAbortedException`, `The transaction has aborted.`, whose inner exception has the error's message, written `[id] message`. Each of the three runs on the thread completing or rolling back the transaction: the pipeline thread of `Complete-Transaction` or `Undo-Transaction`, or a timer's for a `Start-Transaction -Timeout` that passes, so none of them can write to a pipeline. A panic in `prepare` refuses with the panic's message; one in `commit` or `rollback` is written to standard error.

## `PsSteppable` (`steppable.rs`)

A command begun by `Pipeline::steppable`, stored in a cmdlet's field from `begin` to `end`. It has a handle of its own to the cmdlet, so its calls need no `Pipeline`, and they run on the pipeline thread. [How To Write A Cmdlet](../how-to/How-To-Write-A-Cmdlet.md#wrap-another-command) shows one in use.

| Method | Signature | What it does |
|---|---|---|
| `process` | `(&self, input: &PsObject) -> PsResult<()>` | passes `input` to the command as one record, as `Process($input)`; a collection is one record |
| `process_empty` | `(&self) -> PsResult<()>` | runs the command's process blocks without a record, as `Process()` |
| `end` | `(self) -> PsResult<()>` | runs the command's end blocks, then disposes it; the end's error comes first, then the disposal's |

Dropping one that has not ended disposes it, calling `Clean()` in PowerShell 7.3 and later so its commands' clean blocks run, and writes a failure to standard error. A stop or terminating error coming back through any call ends the cmdlet as it ends a compiled cmdlet making the same call. The call returns it, every later write and step of the phase returns it again, and the command ends with the engine's own exception once the phase returns.

## `Wrapper` (`wrapper.rs`)

The steps a cmdlet declared with `#[cmdlet(wraps = "Verb-Noun")]` takes beside the command it wraps. `#[cmdlet]` implements `Cmdlet` for it through these, so `impl Wrapper for MyCmdlet {}` is a proxy that forwards everything as it is. [How To Write A Cmdlet](../how-to/How-To-Write-A-Cmdlet.md#declare-a-proxy-of-a-command) shows one in use.

| Method | Signature | Default |
|---|---|---|
| `arguments` | `(&mut self, arguments: &mut PsHashtable, ps: &Pipeline<'_>) -> PsResult<()>` | leaves the table as it is: the arguments the caller bound to the parameters copied from the command, by name, which the command begins with |
| `input` | `(&mut self, record: PsObject, ps: &Pipeline<'_>) -> PsResult<Option<PsObject>>` | passes each record piped to the cmdlet to the command, as it was piped; `None` drops it |
| `output` | `(&mut self, record: PsObject, ps: &Pipeline<'_>) -> PsResult<()>` | writes each record the command writes, during the cmdlet's own begin, process and end; a non-terminating error it returns is written and the command goes on, a terminating one ends both |

## `PsObject` (`object.rs`, `dynamic.rs`, `pinned.rs`)

A `GCHandle`, freed when the `PsObject` is dropped; `Send`, `Sync`, `Clone` (a second handle to the same object), `Default` (`$null`).

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
| `subscribe` | `<F: Fn(&PsObject, &[PsObject]) -> PsResult<()> + Send + Sync + 'static>(&self, event: &str, callback: F) -> PsResult<PsSubscription>` | gives `callback` to the object's event, or to the static event of that name when the object is a type, as C#'s `+=` does: it runs on the raising thread with the sender and the arguments, `[e]` for a delegate `(object sender, T e)` and every argument with a `$null` sender for any other, and its `Err` or panic is thrown to the raiser. A delegate returning a value or taking a `ref`, `out` or `in` argument is refused; see [`PsSubscription`](#psthis-pssubscription-and-the-raises-of-an-object-proxyrs-eventsrs). On a COM object made late-bound, which has no .NET events, the event is a method of the default source interface its class's type information names, and the callback gets a `$null` sender and the event's arguments; an event returning a value or taking an out argument is refused. Such an object made on the pipeline thread raises its events there while the thread waits on a .NET wait, and the runtime detaches a callback still on one when the runspace it was subscribed in closes and when the module unloads; see [How To Raise And Handle Object Events](../how-to/How-To-Raise-And-Handle-Object-Events.md#a-com-objects-events) |
| `type_name` | `(&self) -> PsResult<String>` | `GetType().FullName` |
| `type_tag` | `(&self) -> PsResult<PsTypeTag>` | the `PS_TYPE_*` tag of the object's own type, with one call into .NET; `PS_TYPE_OBJECT` for a type outside that set. See [Conversions Reference](Conversions-Reference.md#the-object-type-tag) |
| `pin` | `<T: Primitive>(&self) -> PsResult<Pinned<'_, T>>` | a pinned borrow of a primitive array |
| `from_slice` | `<T: Primitive + IntoPs>(&[T]) -> PsResult<PsObject>` | a new array filled through one pin |

Free functions in `pwrs::object`: `new_psobject(type_name: &str) -> PsObject`, `add_note(obj: &PsObject, name: &str, value: PsObject) -> PsResult<()>`, and `property(obj: &PsObject, name: &str) -> PsResult<PsObject>`, which reads one back by name, note properties included, and fails for a name the object does not have, instead of reading as `$null`. `PsObject::get` goes to the underlying .NET object instead, so it does not see a note.

## `Pinned<'a, T>`, `PsMemory<T>`, `PsMemoryView<T>` and `PsRevocation` (`pinned.rs`)

`Pinned<'a, T>` derefs to `[T]` and `[T]` mutably; releases the pin on drop. Must not outlive the phase.

`PsMemory<T: Primitive>`: `zeroed(len: usize) -> PsResult<PsMemory<T>>`, `from_slice(&[T]) -> PsResult<PsMemory<T>>`, `TryFrom<Vec<T>>`, `len()`, `is_empty()`, `read_only(self) -> PsMemory<T>`, `revocable(self, &PsRevocation) -> PsMemory<T>`; derefs to `[T]` and `[T]` mutably; `Send` and `Sync`; `IntoPs` gives the buffer to PowerShell as a `Memory<T>`, or a `ReadOnlyMemory<T>` after `read_only` (a `T[]` copy on .NET Framework).

`PsMemoryView<T: Primitive>`: `unsafe writable<O: Send + Sync + 'static>(ptr: *mut T, len: usize, owner: O) -> PsMemoryView<T>`, `unsafe read_only<O: Send + Sync + 'static>(ptr: *const T, len: usize, owner: O) -> PsMemoryView<T>`, `revocable(self, &PsRevocation) -> PsMemoryView<T>`, `len()`, `is_empty()`. It is `Send` and `Sync`. `IntoPs` gives PowerShell a `Memory<T>` or `ReadOnlyMemory<T>` over the memory (a `T[]` copy on .NET Framework), and the owner with it, which is dropped when the view is released.

`PsRevocation`: `new() -> PsRevocation`, `revoke(&self)`, `is_revoked(&self) -> bool`; `Clone` shares the flag, which is never cleared.

## `PsTask<T>` (`task.rs`)

The argument a `#[psmethods]` method takes to return a `Task<T>` (a `Task` for `PsTask<()>`); see [Attribute Reference](Attribute-Reference.md#psmethods). `complete(self, value: T)` where `T: IntoPs`, converting on the calling thread and faulting the task with the conversion's error if it fails; `fail(self, error: PsError)`; `cancel(self)`; `finish(self, result: PsResult<T>)`, which completes with the value or faults with the error; `is_cancelled(&self) -> bool`, a byte read that the caller's `CancellationToken` sets. `Send`, so it moves to whatever thread does the work. Finishing a task consumes it, so a task finishes once; one dropped without being finished faults with `PwrsTaskDropped`. Continuations run asynchronously, never on the thread that finishes the task.

## `PsEvents` (`events.rs`)

The engine events of a runspace, from `Pipeline::events`: `Clone`, `Send` and `Sync`. The runspace's event manager lives at least as long as it does. See [How To Raise Engine Events](../how-to/How-To-Raise-Engine-Events.md).

| Method | Signature | Does |
|---|---|---|
| `raise` | `(&self, source_identifier: &str, message_data: impl IntoPs) -> PsResult<()>` | raises the event with `message_data` as its `MessageData` and no sender or arguments |
| `raise_with` | `(&self, source_identifier: &str, sender: impl IntoPs, args: impl IntoPs, message_data: impl IntoPs) -> PsResult<()>` | raises it as `New-Event -Sender -EventArguments -MessageData` does; `()` is none of any of them, and `args` that converts to an array gives the event its elements as arguments |

Both convert their values on the calling thread while it has `attach_current_thread()`, and a value that does not convert raises nothing and is the `Err`. The event is queued, or passed to the actions subscribed to it, before the call returns, so the events one thread raises arrive in the order raised; an action runs later on the runspace's pipeline thread.

| Method | Signature | Does |
|---|---|---|
| `subscribe` | `(&self, source: &PsObject, event: &str, source_identifier: &str) -> PsResult<PsEventSubscription>` | queues each raise of `source`'s event, from the raising thread and so in the order one thread raised them, as a record under `source_identifier` with the shape `Register-ObjectEvent` gives one: `Sender` is `source`, `SourceArgs` the delegate's arguments. Runs on any thread |
| `take` | `(&self, source_identifier: &str) -> PsResult<Vec<PsEventRecord>>` | every record waiting under the identifier, oldest first, removed from the queue as `Get-Event` and `Remove-Event` do, with one call of each however many wait; empty when none waits. A phase reading a steady stream takes in a loop and waits only when nothing is queued |
| `wait` | `(&self, source_identifier: &str, timeout: Option<Duration>) -> PsResult<Option<PsEventRecord>>` | the oldest record, removed, waiting for one when none is queued: `None` when the pipeline stops or `timeout` passes first. The queue's own `PSEventReceived` wakes it, and it looks at the pipeline every 50 ms. Its wait is a .NET wait, as `Wait-Event`'s is, so the calls COM queues for the pipeline thread's single-threaded apartment run during it, and a COM object made on that thread raises its events |

`take` and `wait` run through the cmdlet whose `Pipeline::events` made the `PsEvents`, on its pipeline thread, and refuse with `PwrsNoPhase` for one no phase made. `PsEventSubscription`: `source_identifier(&self) -> &str` and `unsubscribe(self) -> PsResult<()>`. Dropping it unsubscribes, on any thread, and the records already queued stay. `PsEventRecord { source_identifier: String, event_identifier: i32, sender: PsObject, source_args: Vec<PsObject>, source_event_args: PsObject, message_data: PsObject, time_generated: PsDateTime }` is a queued `PSEventArgs` read by property; `source_event_args` is the `e` of a delegate `(object sender, T e)` whose `T` is an `EventArgs`, and `$null` otherwise. See [How To Raise And Handle Object Events](../how-to/How-To-Raise-And-Handle-Object-Events.md).

## `PsThis`, `PsSubscription` and the raises of an object (`proxy.rs`, `events.rs`)

`PsThis` is the object a `#[psmethods]` method runs on, given as an argument `this: &PsThis` the method's .NET signature does not have: `object(&self) -> &PsObject` and `raise(&self, event: &str, args: impl IntoPs) -> PsResult<()>`. It is `Clone`, `Send` and `Sync`, and the object stays alive as long as a clone does, so a thread the method starts can raise after the call returns. `PsProxy::raise` is the same raise for a cmdlet with the object.

A raise names an event the object's class declares, its own or an interface's or base's (`Interface.Name` for one implemented explicitly). For a delegate `(object sender, T e)` the object is the sender and `args` is `e`, `()` giving `EventArgs.Empty` to an `EventHandler`; for any other delegate `args` gives every argument, an array its elements. Each converts to its parameter's type as PowerShell converts it, at the raise. A raise made by the thread inside a call into the object, such as a method, a property read or a `PsProxy::with` or `with_mut` borrow, is delivered once that call has released the object. A handler's exception is then the call's, or the borrow's `Err`. A call that returned an `Err` returns that one, and the handler's exception goes to standard error. Any other raise runs the handlers on its thread before it returns, and a handler's exception is its `Err`. A script block handler runs only on a runspace's thread: raised from another thread it fails with "There is no Runspace available to run scripts in this thread".

`PsSubscription`, from `PsObject::subscribe`: `unsubscribe(self) -> PsResult<()>`. Dropping it removes the callback on any thread and does not wait: a call already running finishes with its own share of the closure, which is dropped once the last call returns. A drop writes a failure to standard error; `unsubscribe` returns it.

## `pwrs::thread` (`thread.rs`)

For a thread the module starts to own a window, a COM object or anything else that belongs to one thread. See [How To Use Threads](../how-to/How-To-Use-Threads.md#a-thread-for-a-window-or-a-com-object).

| Item | Signature | Does |
|---|---|---|
| `enter_sta` | `() -> PsResult<StaGuard>` | on Windows, `CoInitializeEx` with `COINIT_APARTMENTTHREADED` on the calling thread, and `CoUninitialize` when the guard drops; elsewhere nothing. A thread already in a single-threaded apartment stays in it; one in the multithreaded apartment is refused with `PwrsApartmentChanged`. The guard is `!Send`, so it stays on the thread that entered |
| `apartment` | `() -> Option<Apartment>` | the calling thread's apartment, as `CoGetApartmentType` reports it; `None` where there is no COM, which is everywhere but Windows |
| `Apartment` | enum | `Sta`, `MainSta`, `Mta`, `ImplicitMta` (a thread that never initialized COM in a process where another thread joined the multithreaded apartment), `Neutral`, `NotInitialized` |

## `PsType` (`dynamic.rs`)

| Method | Signature | Does |
|---|---|---|
| `from_name` | `(name: impl Into<String>) -> PsType` | a type by the name the engine's type resolver accepts |
| `name` | `(&self) -> &str` | |
| `call_static` | `(&self, method: &str, args: &[PsObject]) -> PsResult<PsObject>` | a public static method; when no overload takes the arguments as they are, each converts as PowerShell converts a value, and the name matches without regard to case |
| `new` | `(&self, args: &[PsObject]) -> PsResult<PsObject>` | a public constructor, chosen as `call_static` chooses a method |

`pwrs::dynamic::args_array(&[PsObject]) -> PsResult<PsObject>` builds the `object[]` those calls pass.

## `PsHashtable`, `PsReadOnlyTable`, `PsScriptBlock`, `PsBigInt`, `PsSecureString`, `PsCredential` (`types.rs`)

`PsHashtable(pub PsObject)`: `new() -> PsResult<PsHashtable>`, `get(key) -> PsResult<PsObject>`, `set(key, value) -> PsResult<()>`, `contains(key) -> PsResult<bool>`, `len() -> PsResult<usize>`, `is_empty() -> PsResult<bool>`, `keys() -> PsResult<Vec<String>>`.

`PsReadOnlyTable(pub PsObject)`: `over(source: &PsObject) -> PsResult<PsReadOnlyTable>`, a `Pwrs.ReadOnlyTable` over an existing `IDictionary`, read through and never copied; `get(key)`, `contains(key)`, `len()`, `is_empty()` and `keys()` as `PsHashtable`'s, and no `set`. A write through it from script is a terminating error.

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
| `prompt_for_choice` | `(&self, caption: &str, message: &str, choices: &[(&str, &str)], default: usize) -> PsResult<usize>` | `PromptForChoice` over `ChoiceDescription`s built from each label and help text; the engine's binder takes the array as the `Collection<ChoiceDescription>` the method declares. A `&` in a label marks its hot key. Returns the index chosen; `default` is taken when the person enters nothing |

## `PsDateTime`, `PsTimeSpan`, `PsGuid` (`values.rs`)

`PsDateTime { pub ticks: i64, pub kind: DateTimeKind }`: `new(ticks, kind)`, `utc(ticks)`, `to_utc() -> PsResult<PsDateTime>`; `TryFrom<SystemTime>` and `TryFrom<PsDateTime> for SystemTime`. `DateTimeKind` is `Unspecified`, `Utc` or `Local`. Constants `TICKS_PER_SECOND`, `UNIX_EPOCH_TICKS`, `MAX_DATETIME_TICKS`.

`PsTimeSpan { pub ticks: i64 }`: `from_ticks(ticks)`, `is_negative()`; `TryFrom<Duration>` and `TryFrom<PsTimeSpan> for Duration`; ordered.

`PsGuid { pub bytes: [u8; 16] }`: `NIL`, `to_rfc4122() -> [u8; 16]`, `from_rfc4122(bytes)`; `Display` and `FromStr`.

## `PsError` and `ErrorCategory` (`error.rs`)

`PsError { message, error_id, category, target: Option<PsObject>, terminating, details: Option<String>, exception: Option<String> }` with `new(category, error_id, message)`, `terminating(self)`, `with_target(self, PsObject)`, `with_details(self, text)`, `with_exception(self, type_name)`; `From<std::io::Error>`; `Display` as `[id] message`. Details reach the record's `Exception.InnerException` on every path an error leaves by: a cmdlet's record, written or terminating, and the exception a proxy method, static call, field read, provider, completer, transform or lifecycle hook throws. The default error view still shows `message` alone. `ErrorCategory` has the 32 values of `System.Management.Automation.ErrorCategory` with the engine's numeric values, and `ErrorCategory::from_code(u32) -> Option<ErrorCategory>` maps one back. `PsResult<T>` is `Result<T, PsError>`.

`with_exception` names the .NET exception type the error reaches .NET as, resolved as PowerShell resolves a type literal, so a catch of that type catches it. The exception has the message, the details as its InnerException, and `PwrsErrorId` and `PwrsErrorCategory` in its `Data`. Where it goes depends on the path:

- A proxy method, a static call, a property, a `#[member]` and an event callback throw it, and a task faulted with it has it as its exception, its message `[id] message`.
- A record has it as its exception and keeps its id, category and target: a cmdlet's, written or terminating, a job's, a provider's and a lifecycle hook's.
- A failed job has it as its reason.
- A transform's or `validate_with` function's refusal is still the binder's exception, with this one inside it.

A name that gives no exception type, or a type with no public constructor taking a message (and an inner exception, for an error with details), gives `Pwrs.PwrsException`, its message saying which type could not be made and why.

`PsError` is what a cmdlet raises. `PsErrorRecord { category: ErrorCategory, error_id: String, message: String, target: PsObject }` in `values.rs` is what a cmdlet reads: `FromPs` for a `System.Management.Automation.ErrorRecord` taken as a parameter or off the pipeline, from `-ErrorVariable`, from a `catch`, or from another command run through `invoke`. It is `FromPs` only; `target` is `$null` when the record has none.

## `Cmdlet`, `CmdletMeta`, `CmdletBind` (`cmdlet.rs`)

`Cmdlet` is the one you implement: `process(&mut self, &Pipeline<'_>) -> PsResult<()>` is required, `begin` and `end` have default bodies that mark the phase empty so the runtime stops calling them for that type.

`CmdletMeta` (the `Verb-Noun` name, the descriptor, the phase mask, and `job_runner` for `as_job = "runtime"`) and `CmdletBind` (`bind`, which reads the parameter block, `validate`, which runs the function a parameter's `validate_with` names, and `take_params`, which moves a record's parameters into a job's instance) are implemented by `#[cmdlet]` and `#[param]`. They are public because the generated code in your crate names them, not because a module writes them; writing either by hand is not supported.

`TransformFn` (`transform.rs`) and `CompleterFn` (`completer.rs`) are the same shape for `#[transform]` and `#[completer]`: the macro turns the function into a unit struct implementing the trait, and `export_module!` lists that struct.

## `Removal` and `RemovalCause` (`lifecycle.rs`)

`Removal { pub cause: RemovalCause, pub runspace: Option<PsGuid> }` is what an `#[on_remove]` that takes a `&Removal` is given; it is `Copy`, `Eq` and `Hash`. `RemovalCause` is `RemoveModule` when `Remove-Module` removed the module, and `RunspaceClosed` when the runspace that imported it closed or broke with the module still imported: it was disposed, a thread job or a `ForEach-Object -Parallel` run ended, its pool was disposed, or PowerShell exited. `runspace` is that runspace's `InstanceId`, the value [`Pipeline::runspace_id`](#pipelineps-cratespwrssrcpipeliners-workerrs) gives a command it runs, and `None` only for an import made on a thread with no default runspace. `OnLoad`, `OnImport`, `OnRemove` and `OnUnload` are the traits the four lifecycle attributes implement; see the [Attribute Reference](Attribute-Reference.md#on_load-on_import-on_remove-and-on_unload).

## `Completion`, `CompletionContext`, `DynamicParam` (`completer.rs`)

`CompletionContext { word, command, bound: PsHashtable }`. `Completion { text, list_item, kind: CompletionKind, tooltip }` with `value(text)` and `with_tooltip(text)`. `DynamicParam { name, clr_type, mandatory, position, set, help, validate_set }` with `string(name)`, `mandatory()`, `with_validate_set(values)`.

## `Item`, `Drive`, `Provider` (`provider.rs`)

`Item { path, value, is_container }`, with `leaf(path, value)` and `container(path, value)`, and `Drive { name, root }`. The `Provider` trait's methods are listed in [How To Write A Provider](How-To-Write-A-Provider.md). Each drive has its own value of the type, made by `default_drives() -> PsResult<Vec<(Drive, Self)>>` or `new_drive(name, root) -> PsResult<(Drive, Self)>`, and dropped after `remove_drive(&mut self)`. `pwrs::provider::child_name(path)` is the last segment of a `\`- or `/`-separated path.

## `pwrs::testing` and `pwrs::trace`

The fake host for unit tests and the counters are described in [How To Test A Module](How-To-Test-A-Module.md) and [How To Trace A Module](How-To-Trace-A-Module.md).
