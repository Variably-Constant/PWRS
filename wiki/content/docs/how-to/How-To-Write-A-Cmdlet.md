---
title: How To Write A Cmdlet
weight: 1
---

The shape of a cmdlet and everything it can do inside its phases. Source: `crates/pwrs-macros/src/params.rs` (the attribute grammar and the generated block), `crates/pwrs/src/cmdlet.rs` (the traits and the phase logic), `crates/pwrs/src/pipeline.rs` (the stream API).

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

- Named fields only, at most 64 with `#[param]`.
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
- Parameters are bound into the fields before the first phase and again before any phase in which a parameter was reassigned, which happens once per record for `value_from_pipeline` and `value_from_pipeline_by_property_name` parameters. Reading `self.name` inside `process` gives the current record's value. A parameter the engine has not assigned yet keeps its field's value: `None` for an `Option<T>`, otherwise whatever `Default::default()` left there, which is what a piped parameter holds during `begin`.
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
```

Those four take a `&str`, so a formatted message is built whether or not
anything reads it: the engine decides after the crossing. Use the macros
for anything that is not a literal, and the text is built only when the
record is kept.

```rust
pwrs::verbose!(ps, "copying {} to {}", src, dst)?;
pwrs::debug!(ps, "{} bytes buffered", n)?;
pwrs::warning!(ps, "{} was skipped", name)?;
pwrs::information!(ps, "finished {} files", count)?;
```

`ps.verbose_enabled()` and its three siblings answer the same question
directly, for a caller that wants to skip more than the message.

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

- `Err` from a phase becomes a non-terminating error record: `WriteError` with the category, the id as the `FullyQualifiedErrorId` prefix, and the message. The cmdlet's later records still run; `-ErrorAction Stop` turns it terminating, as for any cmdlet.
- `.terminating()` on the error raises it through `ThrowTerminatingError` after the phase returns. `.with_target(obj)` sets the record's target object.
- `?` on a `std::io::Error` maps its kind to a category (`NotFound` to `ObjectNotFound`, `PermissionDenied` to `PermissionDenied`, and so on) with id `IoError`.
- A panic anywhere in a phase is caught at the boundary and reported as a terminating error with the panic message; the process survives and the module keeps working.
- Running out of memory is not a panic. Rust answers an allocation the allocator refuses by ending the process, which no boundary can catch, so reserve any memory whose size comes from input: `buf.try_reserve(n)?` turns a refusal into an error record with id `PwrsOutOfMemory` and category `ResourceUnavailable`, and the session carries on. [How To Fix A Failure](How-To-Fix-A-Failure.md#the-shell-exits-with-a-memory-allocation-message) says what stays fatal.

## Confirmation

With `supports_should_process` on the cmdlet:

```rust
if ps.should_process(&self.name, "Set")? {
    // -WhatIf printed the message and returned false; -Confirm asked
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

`should_process` and `should_continue` are the prompts the engine
owns, and `-Confirm`, `-WhatIf` and `-Force` answer them for the
person. For a question the engine has no parameter for, `host_ui`
reaches the host's own interface: a choice among labels, a line, or a
line typed without echo. `write_line` puts text on the host's output,
which is not the pipeline, so nothing downstream sees it.

A host that cannot ask, such as one run with `-NonInteractive`,
refuses with the engine's own error and that is the `Err`; a cmdlet
that prompts should expect it and, where it can, take a parameter
that answers the question instead. With no person attached, the
console host answers `read_line` and `prompt_for_choice` from its
standard input, which is how a script drives a prompting command;
`read_line_as_secure_string` is not among them, because it reads keys
from the console device and redirected input never reaches it. A
command that must run unattended takes the secret as a
`PsSecureString` parameter rather than asking for it.

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
