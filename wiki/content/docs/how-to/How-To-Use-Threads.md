---
title: How To Use Threads
weight: 6
---

Doing work off the pipeline thread and getting results back onto it. Source: `crates/pwrs/src/worker.rs` (`stream_from_thread`, `stream_from_worker`, `Worker`, `par_map`, `par_for_each`, `workers`, `Order`, `StopSignal`), `crates/pwrs/src/pipeline.rs` (`stopping`), `crates/pwrs/src/progress.rs` (`Progress`), `crates/pwrs/src/object.rs` (`PsObject` is `Send`), `crates/pwrs/src/host.rs` (the thread check and `attach_current_thread`), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs` (the off-thread check in every stream entry).

## The rule

PowerShell's `WriteObject`, `WriteError`, the stream writers, `ShouldProcess`, session state and script block invocation are valid only on the thread running the cmdlet's current phase. PWRS encodes that in the type system: every such call takes `&Pipeline<'_>`, the token is `!Send`, and it is created by the runtime for the length of one phase. A `Pipeline` cannot be moved into a spawned thread, so the compiler rejects the mistake. If a stream entry is ever reached from another thread through unsafe code, the managed side refuses it with status 3.

A `PsObject` is `Send` and `Sync`, because the `GCHandle` it owns may be released from any thread. Its methods are another matter: `get`, `set`, `call`, `pin`, `type_name`, `type_tag` and `PsType`'s calls reach the host, and belong on a thread the host called the module on. A worker the module started is not one: a call from it attaches that thread to the .NET runtime and runs PowerShell's member binder, which can run script, a script property for one, on a thread no runspace belongs to. So a worker builds plain Rust values (numbers, strings, vectors, the module's own types) and the pipeline thread converts and writes them.

The compiler cannot see this one, since `PsObject` has to be `Send`, so it is checked at run time instead: under `cargo pwrs test`, which sets `PWRS_THREAD_CHECK=1` for its hosts, and in a debug build, such a call returns an error with id `PwrsOffThread` rather than running. A release build does not check.

## A thread that drives PowerShell itself

Some threads are meant to reach the host: a driver the module starts to run scripts in a runspace it manages, calling `[powershell]::Create()`, `AddScript` and `Invoke` itself. `pwrs::attach_current_thread()` declares such a thread for as long as the guard it returns lives, and the check lets its calls run:

```rust
std::thread::spawn(move || -> PsResult<()> {
    let _attached = pwrs::attach_current_thread();
    let shell = PsType::from_name("System.Management.Automation.PowerShell").call_static("Create", &[])?;
    shell.set("Runspace", &runspace)?;
    shell.call("AddScript", &[script.into_ps()?])?;
    let results = shell.call("Invoke", &[])?;
    shell.call("Dispose", &[])?;
    // ...
    Ok(())
});
```

The module answers for what those calls run and in which runspace. A worker that only computes takes no guard, so the check still refuses one that reaches for a `PsObject` by mistake, and a thread that drops its guard is checked again. The guard cannot be sent to another thread. With the check off it changes nothing. The hello example's `Test-RustOffThread -Attached` reads a type name once while its worker holds the guard and once after, and `Parallel.Tests.ps1` checks both answers.

## A thread for a window or a COM object

A window belongs to the thread that made it, and a COM object made in a single-threaded apartment to that apartment's thread. `pwrs::thread::enter_sta()` puts the calling thread in a single-threaded apartment for as long as the guard it returns lives: `CoInitializeEx` with `COINIT_APARTMENTTHREADED` on Windows, and `CoUninitialize` on the same thread when the guard drops. Elsewhere the guard does nothing. `pwrs::thread::apartment()` says which apartment the calling thread is in, and `None` where there is no COM.

```rust
std::thread::spawn(move || -> PsResult<()> {
    let _sta = pwrs::thread::enter_sta()?;
    // make the window or the COM object here, serve it here, release it here
    Ok(())
});
```

A module never borrows the host's thread for this. The pipeline thread is single-threaded by default in pwsh 7.6.6 and Windows PowerShell 5.1 on Windows, and multithreaded in a host started with `-MTA`, so code that needs one kind of apartment starts a thread of its own and enters it there. A thread already in the multithreaded apartment is refused with `PwrsApartmentChanged`, since COM cannot move a thread between apartments. The guard stays on the thread that took it, and releasing what the thread made is that thread's job too: a proxy's value can be dropped on any thread, the finalizer's among them, as [Two Hosts](../explanation/Two-Hosts.md#process-wide-state-across-a-reload-and-a-drop) explains.

The hello example's `Get-RustApartment` runs a `stream_from_worker` worker that reads its apartment before, while and after it holds the guard. In pwsh 7.6.6 and Windows PowerShell 5.1 it read `ImplicitMta`, `Sta`, `ImplicitMta`, and in each started with `-MTA` it read `ImplicitMta`, `MainSta`, `ImplicitMta`: the worker was the process's first thread in a single-threaded apartment, which makes that apartment the process's main one. `Apartments.Tests.ps1` checks it in both hosts, and off Windows that all three read `NoCom`, the cmdlet's name for `None`.

## stream_from_thread

```rust
fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
    let n = self.count;
    ps.stream_from_thread(move |tx| {
        for i in 0..n {
            if tx.send(format!("item {i}")).is_err() {
                break;   // the pipeline stopped draining
            }
        }
    })
}
```

`stream_from_thread(work)` spawns a std thread running `work` with the sending half of an `mpsc` channel, and on the calling (pipeline) thread forwards every received item to the output stream in order with `ps.write`. `T` is any `IntoPs + Send + 'static`. Draining stops when the pipeline is stopping or a write fails; the receiver is dropped so the worker's next `send` returns `Err` and it can exit; the worker is always joined before the call returns. A worker panic becomes a terminating `PwrsWorkerPanic` error.

Because every item still crosses the boundary on the pipeline thread, this parallelizes the work, not the writes.

## A worker that writes every stream

`stream_from_worker(work)` runs `work` on a new thread with a `Worker`, the thread's way to the pipeline that started it. Through it the thread writes output and every other stream, and the pipeline thread writes each record on its stream in the order it was sent:

```rust
ps.stream_from_worker(move |w: Worker<String>| {
    for (n, file) in files.iter().enumerate() {
        let step = Progress::new(1, "Hashing", format!("{} of {}", n + 1, files.len())).with_current_operation(file.display().to_string());
        if !w.write_progress(step) {
            return; // the pipeline has stopped taking records
        }
        match hash(file) {
            Ok(sum) => w.write(sum),
            Err(e) => w.write_error(PsError::new(ErrorCategory::ReadError, "HashFailed", e.to_string())),
        };
    }
})
```

| Method | What it sends |
|---|---|
| `write(value)` | `value` to the output stream; only the `Worker<T>` that `stream_from_worker` hands out has output |
| `write_error(error)` | `error` to the error stream; a terminating error ends the call with that error once the pipeline thread reaches it |
| `warning(text)`, `verbose(text)`, `debug(text)`, `information(text)` | `text` to that stream |
| `write_progress(record)` | a [`Progress`](../reference/Pipeline-Reference.md#progress-progressrs) record |
| `stopping()` | nothing; true once the worker should return |

Each send answers whether the pipeline took the record. It is false once `stopping()` is true: the pipeline is stopping, a write failed, a terminating error was written, a worker of the same call panicked, or the call has returned. A `Worker` is `Clone`, `Send` and `Sync`, so a thread can hand clones to threads of its own; a clone kept after the work returns does not hold the call open.

The pipeline thread writes the records waiting when it wakes together, and of the progress records among them for one activity only the last, so a worker that reports progress faster than the host draws it adds no writes. The hello example's `Invoke-RustWorker -ProgressRecords 10000` sends ten thousand records for one activity as fast as it can, and `Workers.Tests.ps1` checks that fewer than that reach the host and that the last one sent is the last one written.

## par_map and par_for_each

```rust
fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
    ps.par_map(self.paths.clone(), Order::Input, |p| checksum(&p))
}
```

`par_map(items, order, f)` runs `f` over `items` on a pool as wide as `available_parallelism`, and writes every result from the pipeline thread. `Order::Input` writes them in the order the items were given; `Order::AsReady` writes each as its worker finishes. `T` is any `Send + 'static`, `U` any `IntoPs + Send + 'static`.

The `!Send` rule is what makes this safe: `f` cannot capture `&Pipeline<'_>`, so no worker can reach the engine, invoke a script block or write a record, and the compiler is what says so rather than a convention. The parallel half runs over owned Rust data; the writes happen on the one thread allowed to make them.

Workers claim their next item with one `fetch_add` on a shared cursor, so a free worker takes the next slot and nothing holds a lock.

`par_for_each(items, f)` is the same pool for work whose results are not written. Reach a total through an `Arc<AtomicI64>` or similar and write it once from the phase. The pipeline thread waits for the work without spinning.

`par_map_with(items, order, f)` and `par_for_each_with(items, f)` hand `f` a `&Worker` as well, for the other streams and for the stop, so a long item can report on itself and give up once the pipeline stops:

```rust
ps.par_for_each_with(self.files.clone(), |file, w| {
    for block in blocks(&file) {
        if w.stopping() {
            return;
        }
        copy(block);
    }
    w.verbose(format!("copied {}", file.display()));
})
```

The results of `par_map_with` keep the order asked for; what `f` sends through its worker is written as it arrives. The worker these helpers hand out has no output of its own: output is what `par_map_with`'s `f` returns.

Once the pipeline is stopping, neither helper starts another item, under either pool, and an item running under a `_with` form sees its worker's `stopping()` turn true. Both join every worker before returning, so an item that runs long and never checks its worker holds the stop until it ends. A worker panic becomes a terminating `PwrsWorkerPanic` error, and no item starts after it.

`ps.workers(n)?` gives the same four helpers with at most `n` items running at once, for work that waits on disks or the network more than it computes, or that must not take the whole machine:

```rust
ps.workers(self.threads as usize)?.par_for_each_with(self.files.clone(), |file, w| copy(&file, w))
```

Any count from one up is taken, and zero is refused with `PwrsWorkerCount`. The default pool starts that many threads for the call, or one per item when there are fewer items. Under the `parallel` feature they are tasks on Flynnel's pool, which also runs no more at once than it has workers. The hello example's `Invoke-RustParallelWork -Workers 2` records the most items that ran at once, and `Workers.Tests.ps1` checks that it is two.

The pool is std threads by default. The `parallel` feature swaps in the Flynnel work-stealing scheduler for both helpers; it is a Rust dependency of the cdylib and adds nothing to the module's PowerShell surface.

## Your own pool

A module that wants a different scheduler adds it as a dependency, produces `Send` values (or `PsObject`s) on it, and writes them from the phase. `stream_from_thread` is the pattern for a single producer; for several, send into one channel from all of them and drain it the same way.

## Events instead of records

A worker's records reach the pipeline that started it, while that call lasts. A thread that has to reach script after the call returns, or reach whatever subscribed rather than the pipeline, raises an engine event through `ps.events()`; see [How To Raise Engine Events](How-To-Raise-Engine-Events.md).

## Cancellation

Ctrl+C and runspace stops make the engine call `StopProcessing` on its own thread, which sets an atomic flag on the Rust instance that `ps.stopping()` reads. A downstream stop (`Select-Object -First`) surfaces instead as a failed write: a terminating `OperationStopped` error from `ps.write`. `stream_from_thread` handles both. It waits on the worker's channel for at most 50 ms at a time and reads the flag between waits, so a stop is seen while the worker sends nothing; it then stops draining, drops the receiver so the worker's next `send` returns `Err`, and joins the worker.

The join waits for the worker, so a worker that computes for a long time without sending holds a stopped pipeline until it returns. `stream_from_thread_until` hands it a `StopSignal` as well, set the moment the pipeline thread stops draining, for it to check between steps:

```rust
ps.stream_from_thread_until(move |tx, stop| {
    for chunk in input.chunks(4096) {
        if stop.is_set() {
            return;
        }
        let answer = crunch(chunk); // long, and sends nothing
        if tx.send(answer).is_err() {
            return;
        }
    }
})
```

The hello example's `Wait-RustSilence` runs a worker that sends one value and then nothing for the seconds it is given, checking its signal every 10 ms. On a Ryzen 9 7900X, `PowerShell.Stop()` on a pipeline running `Wait-RustSilence 60` returned in 30 to 56 ms in pwsh 7.6.6 and in 35 to 55 ms in Windows PowerShell 5.1, over five runs each.

`stream_from_worker`, `par_map` and `par_for_each` wait on their workers the same way, 50 ms at a time, so a stop is seen while no result arrives, and every `Worker` of the call reports `stopping()`. `Workers.Tests.ps1` stops `Invoke-RustParallelWork 32 -Workers 2 -SleepMs 10000` once an item has started, through `par_for_each_with` and through `par_map_with`, and checks that the stop returns within 5 seconds and that no more than the two items taken before it ever started.

A module's own workers see a stop through whatever flag or channel it shares with them. A phase that itself blocks on such a channel, on a ring another process writes, or on a parked thread, wakes without polling through `ps.on_stop(waker)`: `StopProcessing` runs the waker on the engine's thread, and the waker closes, sends on or unparks whatever the phase waits on. [How To Write A Cmdlet](How-To-Write-A-Cmdlet.md#cancellation) has the rules, and the hello example's `Wait-RustStop` parks with no timeout and is woken that way.
