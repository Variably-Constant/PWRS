---
title: How To Use Threads
weight: 6
---

Doing work off the pipeline thread and getting results back onto it. Source: `crates/pwrs/src/pipeline.rs` (`stream_from_thread`, `par_map`, `par_for_each`, `Order`, `stopping`), `crates/pwrs/src/object.rs` (`PsObject` is `Send`), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs` (the off-thread check in every stream entry).

## The rule

PowerShell's `WriteObject`, `WriteError`, the stream writers, `ShouldProcess`, session state and script block invocation are valid only on the thread running the cmdlet's current phase. PWRS encodes that in the type system: every such call takes `&Pipeline<'_>`, the token is `!Send`, and it is created by the runtime for the length of one phase. A `Pipeline` cannot be moved into a spawned thread, so the compiler rejects the mistake. If a stream entry is ever reached from another thread through unsafe code, the managed side refuses it with status 3.

A `PsObject` is `Send` and `Sync`, because the `GCHandle` it owns may be released from any thread. Its methods are another matter: `get`, `set`, `call`, `pin`, `type_name`, `type_tag` and `PsType`'s calls reach the host, and belong on a thread the host called the module on. A worker the module started is not one: a call from it attaches that thread to the .NET runtime and runs PowerShell's member binder, which can run script, a script property for one, on a thread no runspace belongs to. So a worker builds plain Rust values (numbers, strings, vectors, the module's own types) and the pipeline thread converts and writes them.

The compiler cannot see this one, since `PsObject` has to be `Send`, so it is checked at run time instead: under `cargo pwrs test`, which sets `PWRS_THREAD_CHECK=1` for its hosts, and in a debug build, such a call returns an error with id `PwrsOffThread` rather than running. A release build does not check.

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

## par_map and par_for_each

```rust
fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
    ps.par_map(self.paths.clone(), Order::Input, |p| checksum(&p))
}
```

`par_map(items, order, f)` runs `f` over `items` on a pool as wide as `available_parallelism`, and writes every result from the pipeline thread. `Order::Input` writes them in the order the items were given; `Order::AsReady` writes each as its worker finishes. `T` is any `Send + 'static`, `U` any `IntoPs + Send + 'static`.

The `!Send` rule is what makes this safe: `f` cannot capture `&Pipeline<'_>`, so no worker can reach the engine, invoke a script block or write a record, and the compiler is what says so rather than a convention. The parallel half runs over owned Rust data; the writes happen on the one thread allowed to make them.

Workers claim their next item with one `fetch_add` on a shared cursor, so a free worker takes the next slot and nothing holds a lock.

`par_for_each(items, f)` is the same pool for work whose results are not written. Reach a total through an `Arc<AtomicI64>` or similar and write it once from the phase.

Both stop claiming new items once the pipeline is stopping, and both join every worker before returning. A worker panic becomes a terminating `PwrsWorkerPanic` error.

The pool is std threads by default. The `parallel` feature swaps in the Flynnel work-stealing scheduler; it is a Rust dependency of the cdylib and adds nothing to the module's PowerShell surface.

## Your own pool

A module that wants a different scheduler adds it as a dependency, produces `Send` values (or `PsObject`s) on it, and writes them from the phase. `stream_from_thread` is the pattern for a single producer; for several, send into one channel from all of them and drain it the same way.

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

A module's own workers see a stop through whatever flag or channel it shares with them.
