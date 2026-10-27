---
title: How To Write A Cmdlet
weight: 1
---

The shape of a cmdlet and what it does inside its phases. Source: `crates/pwrs-macros/src/params.rs` (the attribute grammar and the generated block), `crates/pwrs/src/cmdlet.rs` (the traits and the phase logic), `crates/pwrs/src/pipeline.rs` (the stream API). [Pipeline Reference](Pipeline-Reference.md) lists every method of the `ps` token with its signature.

## The struct

```rust
use pwrs::prelude::*;

/// One-line synopsis.
///
/// Longer description, which may run over several paragraphs.
///
/// # Examples
/// Set-Thing -Name a -Force
#[cmdlet(verb = "Set", noun = "Thing", supports_should_process, confirm_impact = "High", alias = ["st"], output = ["System.String"])]
#[derive(Default)]
pub struct SetThing {
    /// Which thing.
    #[param(mandatory, position = 0, value_from_pipeline, alias = ["n"], validate_not_null_or_empty)]
    pub name: String,
    /// Skip confirmation.
    #[param]
    pub force: bool,
    /// Retries, default 3.
    #[param(validate_range(0, 10))]
    pub retries: Option<i32>,
    attempts: i32,
}
```

Rules the macro enforces:

- Named fields only, and any number of them may have `#[param]`.
- Every `#[param]` field type must be one of the supported types in [Conversions Reference](Conversions-Reference.md); anything else is a compile error naming the field.
- Fields without `#[param]` (`attempts` above) are ordinary state; the struct must still implement `Default`.
- The PowerShell parameter name is the field name in PascalCase (`name` becomes `-Name`, `max_count` becomes `-MaxCount`).

The full key list for `#[cmdlet]` and `#[param]` is in [Attribute Reference](Attribute-Reference.md).

## The phases

```rust
impl Cmdlet for SetThing {
    fn begin(&mut self, ps: &Pipeline<'_>) -> PsResult<()> { Ok(()) }
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> { /* per record */ Ok(()) }
    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> { Ok(()) }
}
```

- One instance is created per invocation, with `Default::default()`, and lives until the engine disposes the cmdlet.
- Parameters are bound into the fields before the first phase and again before any phase in which a parameter was reassigned, which happens once per record for `value_from_pipeline` and `value_from_pipeline_by_property_name` parameters. Reading `self.name` inside `process` gives the current record's value. The field of a parameter the engine has not assigned yet has the value it started with: `None` for an `Option<T>`, otherwise whatever `Default::default()` left there, which is the value a piped parameter has during `begin`.
- `begin` and `end` default to doing nothing. Implement them when you need them; leaving them out is also what lets the runtime skip their native calls for your type after the first instance.
- The `Pipeline<'_>` token is valid for the duration of one phase and is `!Send`: it cannot be stored or moved to another thread, which is the rule the engine imposes on `WriteObject`.

## Writing output

```rust
ps.write("text")?;                 // String and &str: direct entry, no handle
ps.write(42i64)?;                  // i64, f64, bool: direct entries
ps.write(42i32)?;                  // a System.Int32, not a widened Int64
ps.write(vec![1, 2, 3])?;          // Vec<T> enumerates: three objects
ps.write(PsArray(vec![1, 2, 3]))?; // one int[] object
ps.write(person)?;                 // any #[psclass] or #[psenum] value
ps.write(map)?;                    // HashMap<String, V> as a Hashtable
ps.write_object(&obj)?;            // a PsObject as is
ps.write_enumerated(&obj)?;        // a PsObject collection, unrolled
```

`ps.write` accepts anything implementing `IntoPs`; `Option<T>` writes `$null` for `None`.

## Streams and progress

```rust
ps.verbose("detail")?;
ps.debug("more detail")?;
ps.warning("careful")?;
ps.information("fyi")?;
ps.progress(1, "Copying", "3 of 10", 30)?;   // activity id, activity, status, percent
ps.progress(1, "Copying", "done", -1)?;      // a negative percent completes the bar
ps.write_progress(&Progress::new(1, "Copying", "3 of 10").with_percent(30).with_seconds_remaining(12))?;
ps.write_host("done", Some(ConsoleColor::Green), None, false)?;  // text, color, background, stay on the line
```

`write_progress` takes a `Progress` with every field a progress record has: a parent activity, the current operation and the seconds left as well. `write_host` writes to the host as `Write-Host` does, through the information stream, so it reaches the person and not the pipeline; a `None` color is the host's own.

Those four take a `&str`, so a formatted message is built even when
nothing will read it: PowerShell checks whether the record goes
somewhere only after the call reaches it. Use the macros for any
message that is not a literal; they build the text only when the record
will go somewhere.

```rust
pwrs::verbose!(ps, "copying {} to {}", src, dst)?;
pwrs::debug!(ps, "{} bytes buffered", n)?;
pwrs::warning!(ps, "{} was skipped", name)?;
pwrs::information!(ps, "finished {} files", count)?;
```

`ps.verbose_enabled()` and its three siblings return whether that
stream's records will go somewhere, so a cmdlet can skip more work than
building the message.

## Errors

```rust
fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
    if self.name.is_empty() {
        return Err(PsError::new(ErrorCategory::InvalidArgument, "EmptyName", "name is empty"));
    }
    let bytes = std::fs::read(&self.name)?;   // std::io::Error converts to PsError
    Ok(())
}
```

- `Err` from a phase becomes a non-terminating error record: `WriteError` with the category, the id as the `FullyQualifiedErrorId` prefix, and the message. The phase ends there and the cmdlet's later records still run; `-ErrorAction Stop` turns it terminating, as for any cmdlet.
- `ps.write_error(&error)` writes the record and returns, so the phase goes on: the way to report one input that failed among several and go on with the rest.
- `.terminating()` on the error raises it through `ThrowTerminatingError` after the phase returns. `.with_target(obj)` sets the record's target object.
- `.with_details(text)` adds text beyond the one-line message, such as a cause chain or a backtrace, in the record's `Exception.InnerException`: the default error view still shows the message alone, and `$err.Exception.InnerException.Message` has the details. A proxy method's or a static method's error has them the same way, one level further in, inside the `MethodInvocationException` PowerShell wraps it in. The example module's `Get-Greeting -Fail` and `-Terminate`, and `[Hello.Counter]`'s `Advance` and `Parse`, raise errors with details; `examples/hello/tests/ErrorDetails.Tests.ps1` reads them.
- `?` on a `std::io::Error` maps its kind to a category (`NotFound` to `ObjectNotFound`, `PermissionDenied` to `PermissionDenied`, and so on) with id `IoError`.
- A panic anywhere in a phase is caught at the boundary and reported as a terminating error with the panic message; the process survives and the module still works.
- Running out of memory is not a panic. When the allocator refuses an allocation, Rust ends the process, and nothing can catch that. So reserve any memory whose size comes from input: `buf.try_reserve(n)?` turns a refusal into an error record with id `PwrsOutOfMemory` and category `ResourceUnavailable`, and the session goes on. [How To Fix A Failure](How-To-Fix-A-Failure.md#the-shell-exits-with-a-memory-allocation-message) says what stays fatal.

## Confirmation

With `supports_should_process` on the cmdlet:

```rust
if ps.should_process(&self.name, "Set")? {
    // -WhatIf printed the message and returned false; -Confirm prompted the person
}
if self.force || ps.should_continue("Overwrite?", "Set-Thing")? {
    // ...
}
```

`confirm_impact = "High"` (or `"Medium"`, `"Low"`) sets `ConfirmImpact` on the `[Cmdlet]` attribute.

## Prompting the person

```rust
let ui = ps.host_ui()?;
let which = ui.prompt_for_choice("Overwrite", "The file exists.", &[("&Overwrite", "Replace it"), ("&Keep", "Leave it")], 1)?;
let name = ui.read_line()?;
let secret = ui.read_line_as_secure_string()?;
```

`should_process` and `should_continue` show PowerShell's own prompts,
and a caller can choose their outcome in advance with `-Confirm`,
`-WhatIf` and `-Force`. For a question PowerShell has no parameter for,
`host_ui` uses the host's own interface: a choice among labels, a line,
or a line typed without echo. `write_line` puts text on the host's
output, which is not the pipeline, so nothing downstream receives it.

A host that cannot prompt, such as one started with `-NonInteractive`,
refuses with PowerShell's own error, which is the `Err`. A cmdlet that
prompts should handle that error and, where it can, also take a
parameter that gives the value instead. With no person at the console, the
console host reads the replies to `read_line` and `prompt_for_choice`
from its standard input, which is how a script drives a prompting
command. `read_line_as_secure_string` does not, because it reads keys
from the console device and redirected input never reaches it. A
command that must run unattended takes the secret as a
`PsSecureString` parameter instead of prompting for it.

## Cancellation

Two things stop a cmdlet early. Ctrl+C and runspace stops make the engine call `StopProcessing` on its own thread; PWRS sets a flag the phase can poll:

```rust
for item in big_list {
    if ps.stopping() {
        break;
    }
    ps.write(item)?;
}
```

A downstream command that stops the pipeline (`Select-Object -First 2`) does not call `StopProcessing`; instead the next write throws `PipelineStoppedException` on the managed side, which reaches Rust as a terminating `OperationStopped` error from `ps.write`. Propagating it with `?` ends the phase, and the managed side rethrows the stop to the engine. A loop that writes and checks `?` therefore ends promptly on both paths; a long loop that does not write should poll `ps.stopping()`.

A phase that blocks on something of its own, such as a channel, a ring another process writes, or a parked thread, registers a waker instead of polling. `ps.on_stop(waker)` registers a closure that `StopProcessing` runs on PowerShell's thread when the stop comes, or at once if it already came, and returns a guard that unregisters it when dropped:

```rust
let me = std::thread::current();
let waker = ps.on_stop(move || me.unpark())?;
ps.write("started")?;
while !ps.stopping() {
    std::thread::park();
}
drop(waker);
```

The waker runs at most once. It must be short and must use no PowerShell object, since it runs on the thread that is stopping the pipeline. A phase can have one waker at a time: registering a second while the first guard is alive returns an error with id `PwrsStopWakerHeld`, so a phase that needs to wake two things does both in one closure.

## Reading session state

```rust
let pwd = String::from_ps(&ps.variable("PWD")?)?;
ps.set_variable("LastThing", &self.name.as_str().into_ps()?)?;
for path in ps.resolve_path("*.txt", false)? { /* provider paths */ }
```

`ps.parameter(name)` and `ps.parameter_is_bound(name)` read the engine's bound-parameter table; they exist for dynamic parameters, which are not struct fields.

## Aliases, sets, and pipeline binding

- `alias = ["st"]` on `#[cmdlet]` and `alias = ["n"]` on `#[param]` become `[Alias]` attributes and the manifest exports the cmdlet aliases.
- `set = "ByName"` on a parameter and `default_parameter_set = "ByName"` on the cmdlet give parameter sets. `set = ["Path", "LiteralPath"]` puts a parameter in those sets and no others, so the binder refuses it beside a parameter of any other set, and a parameter without `set` is in every set.
- `value_from_pipeline`, `value_from_pipeline_by_property_name` and `value_from_remaining` map to the matching `[Parameter]` properties.
- `dont_show` hides a parameter from completion; `help = "..."` overrides the doc comment as the help message.
