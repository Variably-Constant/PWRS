---
title: How To Raise Engine Events
weight: 50
---

Raising PowerShell engine events from Rust threads, which script receives through `Wait-Event`, `Get-Event` and `Register-EngineEvent -Action`. Source: `crates/pwrs/src/events.rs` (`PsEvents`), `crates/pwrs/src/pipeline.rs` (`events`), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs` (`EventRaise`, host-table entry 82).

## Raising one

`ps.events()` reads the event manager of the runspace the cmdlet runs in and answers a `PsEvents`. It is `Clone`, `Send` and `Sync`, so it moves to whatever thread does the work, which raises through it:

```rust
fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
    let events = ps.events()?;
    let files = self.files.clone();
    ps.stream_from_worker(move |w: Worker<String>| {
        for file in files {
            match index(&file) {
                Ok(entries) => {
                    if let Err(e) = events.raise("MyModule.Indexed", entries) {
                        w.write_error(e);
                        return;
                    }
                }
                Err(e) => {
                    w.write_error(PsError::new(ErrorCategory::ReadError, "IndexFailed", e.to_string()));
                }
            }
        }
    })
}
```

| Method | Raises |
|---|---|
| `raise(source_identifier, message_data)` | the event with `message_data` as its `MessageData`, and no sender or arguments |
| `raise_with(source_identifier, sender, args, message_data)` | the event as `New-Event -Sender -EventArguments -MessageData` raises one; `()` is none of any of them, and an `args` that converts to an array gives the event its elements as arguments |

Each value is any `IntoPs`, the module's own `#[psclass]` types included, and converts on the thread that raises. A value that does not convert raises nothing and is the `Err`. Converting and raising are crossings the thread makes on purpose, so `raise` holds [`attach_current_thread()`](How-To-Use-Threads.md#a-thread-that-drives-powershell-itself) while it makes them, and the thread check does not refuse them on a worker.

## Receiving them in script

An event with an action subscribed to its source identifier is handed to that action; any other event is queued in the session, whether or not anything registered for it, and `Wait-Event` and `Get-Event` read it there.

```powershell
Send-RustEvent Hello.Tick -Text hi -Count 3
Get-Event -SourceIdentifier Hello.Tick | ForEach-Object { $_.MessageData.Number }   # 1, 2, 3

Send-RustEvent Hello.Tick -Text again
$e = Wait-Event -SourceIdentifier Hello.Tick -Timeout 10

$null = Register-EngineEvent -SourceIdentifier Hello.Tick -Action {
    $global:LastTick = $Event.MessageData
}
```

An action reads the event as `$Event`, its `MessageData` as `$Event.MessageData`, its sender as `$Sender` and its arguments as `$Event.SourceArgs`; what it writes goes to its job, which `Register-EngineEvent` returns, and an event an action took is not queued. `Register-EngineEvent` without `-Action` is refused in pwsh 7.6.6 and Windows PowerShell 5.1 with "Action must be specified for non-forwarded events.", and with `-Forward` instead, which sends events on to a remote session's client, a local session's events are taken and never queued: `Wait-Event` timed out on events from `New-Event` and from `Send-RustEvent` alike, in both hosts. So a script that reads events through `Wait-Event` registers nothing.

## Order and threads

The runtime raises an event the way `New-Event` does, through the `PSEventManager.GenerateEvent` overload that processes it on the calling thread without waiting for its actions. The event is queued, or handed to its actions, before `raise` returns, so the events one thread raises arrive in the order it raised them, and once a cmdlet has joined its worker every event the worker raised is already in the queue. In the hello example, `Send-RustEvent Hello.Test.Order -Count 200` raises 200 events from a `stream_from_worker` worker, and `Events.Tests.ps1` reads all 200 back with `Get-Event` straight after the command returns, in order, in both hosts.

An action runs on the thread the runspace runs its pipelines on, never on the thread that raised the event, and not during the raise. On a Ryzen 9 7900X, in pwsh 7.6.6 and Windows PowerShell 5.1, STA and MTA alike, the actions of three events raised from another thread had not run at the statement after the raise, and had all run by the time a `Start-Sleep -Milliseconds 600` that followed returned, each on the pipeline thread and in the order raised. `Events.Tests.ps1` records the thread each action ran on, waiting with `Start-Sleep -Milliseconds 20` in a loop, and checks it is the pipeline thread.

## After the call returns

A `PsEvents` keeps the event manager alive while it lives, so a thread the module keeps raises through it after the cmdlet returned, which is how work that finishes later can say so:

```rust
let events = ps.events()?;
std::thread::spawn(move || {
    let report = run_job(job);
    if let Err(e) = events.raise("MyModule.JobDone", report) {
        eprintln!("MyModule: the end of the job could not be raised: {}", e.message);
    }
});
```

No pipeline is left to take an error then, so such a thread reports a failed raise wherever the module reports its own failures. The hello example's `Send-RustEvent -DelayMs 500` raises from a thread it leaves running, and `Events.Tests.ps1` checks that `Wait-Event` receives the event and that it was generated after the command returned.

A raise after the runspace was closed does not fail. In both hosts, a thread that raised after its runspace had been closed and disposed had its event added to that runspace's queue, which nothing reads, and the raise returned no error.

## Apartments

On Windows the pipeline thread is STA unless the host was started with `-MTA`, in pwsh 7.6.6 and Windows PowerShell 5.1. `Events.Tests.ps1` raises in a child host started with `-MTA` on Windows and with its default elsewhere, and `Wait-Event` receives the event in each. On Linux, pwsh reports the thread's apartment as `Unknown` and refuses `-MTA` with exit code 64.
