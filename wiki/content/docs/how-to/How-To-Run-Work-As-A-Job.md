---
title: How To Run Work As A Job
weight: 7
---

A cmdlet's Rust work can run as a PowerShell job: on a thread of its own, while the command returns at once, with what it writes stored for `Receive-Job`. Source: `crates/pwrs/src/job.rs` (the job API), `crates/pwrs/src/streams.rs` (the `Streams` trait), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/Jobs.cs` (the job type). [Pipeline Reference](../reference/Pipeline-Reference.md#jobcontext-jobbody-cmdletjob-and-streams-jobrs-streamsrs) lists every method.

## Starting a job

```rust
fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
    let count = self.count;
    let job = ps.start_job(Some("Count"), move |job| {
        for n in 1..=count {
            if job.stopping() {
                return Ok(());
            }
            job.write(n)?;
            pwrs::verbose!(job, "counted {n}")?;
        }
        Ok(())
    })?;
    ps.write_object(&job)
}
```

`ps.start_job(name, work)` makes the job, starts `work` on a thread of its own, and returns the job for the cmdlet to write, as `Start-Job` writes its job. PowerShell's job cmdlets see it as a `RustJob`:

- `Get-Job` lists it by name and by id. A `name` of `None` gives the engine's own `Job<n>`.
- `Wait-Job` waits for it, and `Receive-Job` reads what the work wrote in every stream: output, errors, verbose, debug, warning, information and progress. A job stores every record whatever the preferences; `Receive-Job` applies the receiving session's.
- `Stop-Job` requests a stop: `job.stopping()` turns true, a waker registered with `job.on_stop` runs, and `Stop-Job` waits for the work to return. `Remove-Job -Force` does the same for a running job.
- `Receive-Job -Wait -AutoRemoveJob` and `Remove-Job` remove it.

The work's `Ok` ends the job Completed. Its `Err` ends it Failed, with the error as `JobStateInfo.Reason`, which `Receive-Job` reports as an error, as it does for a `Start-Job` job whose script throws. An error that names its exception type with `with_exception` is that exception there, and `job.write_error` gives the record it writes that exception too; hello's `Start-RustCount -FailAfter 2 -Exception System.TimeoutException` does both. A job whose stop was requested ends Stopped, whatever the work returns, and a panic fails it with the panic's message.

The work runs on its own thread, so it is `Send` and owns what it uses: copy the parameters it needs out of `self` first, as `count` above. Its `JobContext` has the job's streams and the stop request. Session state, `invoke`, `should_process` and the host's prompts belong to the pipeline thread and are not there.

`Receive-Job -Wait` removes a job twice when the job ends after `Receive-Job` has started waiting on it but before it has written what the job has already stored. The second removal finds nothing and is reported as the error `ReceiveJobAutoRemovalError`, "The command cannot remove the job because it does not exist or because it is a child job." The job is still removed, and its output is complete. PowerShell does this with every kind of job: a minimal job derived directly from PowerShell's `Job2`, ending at once, got the error 29 times in 2000 in Windows PowerShell 5.1 and 20 times in 2000 in pwsh 7.6.9, on a Ryzen 9 7900X busy with other work. Work that ends almost as soon as it starts is the most exposed. A job that has already ended when `Receive-Job` sees it is removed once, so wait for it first:

```powershell
$job = 1..4 | Add-RustTotal -AsJob
$null = Wait-Job $job
$job | Receive-Job -Wait -AutoRemoveJob
```

## Work as a type

A type implementing `JobBody` is the same work with a name of its own, for `ps.start_job_body`:

```rust
struct SumWork {
    numbers: Vec<i64>,
}

impl JobBody for SumWork {
    fn run(self, job: &JobContext) -> PsResult<()> {
        job.write(self.numbers.iter().sum::<i64>())
    }
}

let job = ps.start_job_body(Some("Tally"), SumWork { numbers })?;
```

A closure is a `JobBody` too. `start_job` takes it as a closure so the compiler infers its argument.

## Work for both the pipeline and a job

`Streams` is what `Pipeline` and `JobContext` have in common: `write`, `write_object`, `write_enumerated`, `write_error`, the four message streams and their macros, `write_progress` and `stopping`. Work written against it runs in either place:

```rust
fn squares(count: i64, out: &impl Streams) -> PsResult<()> {
    for n in 1..=count {
        out.write(n * n)?;
    }
    pwrs::verbose!(out, "wrote {count} squares")
}
```

## -AsJob

`#[cmdlet(as_job)]` gives the cmdlet an `-AsJob` switch, and `ps.as_job()?` returns whether the caller gave it. The cmdlet starts the job itself, so you choose what runs there:

```rust
#[cmdlet(verb = "Get", noun = "RustSquares", as_job)]
#[derive(Default)]
pub struct GetRustSquares {
    #[param(mandatory, position = 0)]
    pub count: i64,
}

impl Cmdlet for GetRustSquares {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let count = self.count;
        if ps.as_job()? {
            let job = ps.start_job(Some("Squares"), move |job| squares(count, job))?;
            ps.write_object(&job)
        } else {
            squares(count, ps)
        }
    }
}
```

`#[cmdlet(as_job = "runtime")]` has the runtime run `-AsJob` instead, for a cmdlet that is `Clone` and implements `CmdletJob` beside `Cmdlet`. With `-AsJob`, one job runs the whole invocation on its thread, and the job is the command's output: `begin_job` once, then `process_job` for each record in order, each with that record's parameters and the instance's state so far, then `end_job` once input ends. Without `-AsJob`, the `Cmdlet` phases run as usual.

```rust
#[cmdlet(verb = "Add", noun = "RustTotal", as_job = "runtime")]
#[derive(Default, Clone)]
pub struct AddRustTotal {
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub number: i64,
    total: i64,
}

impl Cmdlet for AddRustTotal {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        self.add(ps)
    }
    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        self.report(ps)
    }
}

impl CmdletJob for AddRustTotal {
    fn process_job(&mut self, job: &JobContext) -> PsResult<()> {
        self.add(job)
    }
    fn end_job(&mut self, job: &JobContext) -> PsResult<()> {
        self.report(job)
    }
}
```

`1..4 | Add-RustTotal -AsJob | Receive-Job -Wait -AutoRemoveJob` gives 10, as `1..4 | Add-RustTotal` does. A phase's `Err` is an error record of the job and the next record goes on, as without `-AsJob`; a terminating one ends the job Failed. Each record crosses to the job's thread as a clone of the cmdlet, whose parameters are swapped into the job's one instance. A `PsProxy<T>` parameter is `Clone` for any class, another handle to the same object, so each record's object reaches the job, and `with` and `with_mut` run on the job's thread under the object's gate.

The hello example's `Start-RustCount`, `Start-RustTally`, `Get-RustSquares`, `Add-RustTotal` and `Add-RustLedgerEntry` do each of these, and `examples/hello/tests/Jobs.Tests.ps1` checks them with every job cmdlet in both hosts.

## How the job cmdlets find a job

PowerShell's job manager finds jobs through job source adapters, which `Import-Module` registers from the assembly it imports. Each module's shell declares its own, named after the module's class, so each module's jobs are listed once and removed by `-AutoRemoveJob`, and two modules' adapters never share a name.
