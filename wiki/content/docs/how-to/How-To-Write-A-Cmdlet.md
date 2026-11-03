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

## Paging

`supports_paging` on the cmdlet gives it the parameters PowerShell adds for paging: `-First`, `-Skip` and `-IncludeTotalCount`. The cmdlet reads them and applies them itself, so it can skip at the source rather than make records nobody reads:

```rust
#[cmdlet(verb = "Get", noun = "Page", supports_paging)]
#[derive(Default)]
pub struct GetPage {
    #[param(mandatory, position = 0)]
    pub count: i64,
}

impl Cmdlet for GetPage {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let paging = ps.paging()?;
        let total = self.count.max(0) as u64;
        if paging.include_total_count {
            ps.write_total_count(total, 1.0)?;
        }
        for n in paging.window(total) {
            ps.write(n as i64 + 1)?;
        }
        Ok(())
    }
}
```

`ps.paging()` returns a `PsPaging`: `first` is `u64::MAX` and `skip` 0 when the caller gave neither, and `window(len)` is the range of positions to write out of `len` records. `ps.write_total_count(total, accuracy)` writes the count as PowerShell's own `PagingParameters.NewTotalCount` makes it, with the number as its value and the accuracy in an `Accuracy` property. It shows in the session's language as `Total count: 10` for an accuracy of 1, `Estimated total count: 10` for one between 0 and 1, and `Unknown total count` for 0. A cmdlet without `supports_paging` that calls `ps.paging()` gets an error naming it. `Get-RustPage` in the hello example pages this way, and `examples/hello/tests/Paging.Tests.ps1` checks it against a compiled cmdlet doing the same.

## Transactions

`supports_transactions` on the cmdlet gives it `-UseTransaction` in Windows PowerShell 5.1. Called with it inside `Start-Transaction`, the cmdlet runs in that transaction, and `ps.transaction()` returns it. The cmdlet enlists its change, which lands when `Complete-Transaction` commits the transaction and is dropped when `Undo-Transaction` rolls it back. hello's `Set-RustSetting` enlists a change whose `prepare` refuses when the setting changed after the transaction set it, so the transaction rolls back rather than overwrite that change:

```rust
struct SettingChange {
    name: String,
    value: Option<String>,
    seen: u64,
}

impl Enlistment for SettingChange {
    fn prepare(&mut self) -> PsResult<()> {
        if setting_changes(&self.name) != self.seen {
            return Err(PsError::new(ErrorCategory::InvalidOperation, "HelloSettingChanged", format!("the setting {} changed after this transaction set it", self.name)));
        }
        Ok(())
    }

    fn commit(self) {
        put_setting(&self.name, self.value);
    }

    fn rollback(self) {}
}

#[cmdlet(verb = "Set", noun = "RustSetting", supports_transactions)]
#[derive(Default)]
pub struct SetRustSetting {
    #[param(mandatory, position = 0)]
    pub name: String,
    #[param(mandatory, position = 1)]
    pub value: String,
}

impl Cmdlet for SetRustSetting {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        match ps.transaction()? {
            Some(transaction) => transaction.enlist(SettingChange { name: self.name.clone(), value: Some(self.value.clone()), seen: setting_changes(&self.name) }),
            None => {
                put_setting(&self.name, Some(self.value.clone()));
                Ok(())
            }
        }
    }
}
```

```powershell
Start-Transaction
Set-RustSetting color red -UseTransaction
Get-RustSetting color          # what it was: the change has not landed
Complete-Transaction
Get-RustSetting color          # red
```

When the transaction is completed, every enlistment's `prepare` runs, and then each one's `commit`, or each one's `rollback` once any `prepare` refuses. The refusing enlistment's `rollback` runs too. A refusal fails `Complete-Transaction` with `The transaction has aborted.`, whose inner exception has the error's message. `Undo-Transaction` runs `rollback` alone. `transaction.on_complete(commit, rollback)` enlists two closures with no `prepare`, as `Remove-RustSetting` does. The callbacks run on the thread that completes or rolls back the transaction, which for a `Start-Transaction -Timeout` that passes is a timer's, so they are `Send` and write nothing to a pipeline. Store what they change in a value they own or share, as hello stores its settings in a `static`.

PowerShell 7 gives no compiled cmdlet `-UseTransaction`: its `CmdletAttribute` reads `SupportsTransactions` false whatever it was given. It has `Start-Transaction` and the other transaction cmdlets on Windows, and none of them in pwsh 7.6.6 on Linux or 7.5.5 on FreeBSD. So there `ps.transaction()` is always `None`, and the cmdlet makes its change at once, as it does in Windows PowerShell without `-UseTransaction`. A cmdlet without `supports_transactions` that calls `ps.transaction()` gets an error naming it. `examples/hello/tests/Transactions.Tests.ps1` checks hello's settings cmdlets against a compiled cmdlet declaring `SupportsTransactions` in both hosts.

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
building the message. They follow the engine's rules for a compiled
cmdlet, so `-Debug` turns the verbose stream on as well.

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
- An exception that passes through a write or a prompt is not the cmdlet's error, and `-ErrorAction` does not apply to it: a later command's throw coming back through an output write, or a prompt that `-Debug` or `-Confirm` makes the engine show and the host cannot show. The call returns it, so `?` ends the phase; later writes and prompts return it again, and the command ends with it, as a compiled cmdlet's does.
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
`-WhatIf` and `-Force`. Each other form the engine has is a method too:
`should_process_target`, `should_process_described`,
`should_process_reason`, which also returns whether the result came
from `-WhatIf`, and `should_continue_all`, whose Yes to All or No to All
applies to the rest of the invocation's records, with or without a
security impact. For any other question to the user, `host_ui` uses the
host's own interface: a choice among labels, a line,
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

`credential` on a `PsCredential` parameter declares `[Credential()]`,
so the engine does this prompting for the cmdlet. A user name given in
place of a credential, as in `-Credential ada`, binds by prompting for
the password through the host, as `Get-Credential -Credential ada` does.
A host that cannot prompt refuses the argument. Windows PowerShell's
console host prompts in a credential dialog, even under `-NonInteractive`,
so there a script that must run unattended passes a `PSCredential`.

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

## Wrap another command

`ps.steppable(command, parameters)` begins another command as a `PsSteppable`, as a C# cmdlet wrapping a command does with a `SteppablePipeline`: the cmdlet passes it records from its own `process` and ends it from its own `end`, and what the command writes is the cmdlet's output and streams. Hello's `Select-RustFirst` writes the first records, through `Select-Object`:

```rust
#[cmdlet(verb = "Select", noun = "RustFirst", output = ["System.Object"])]
#[derive(Default)]
pub struct SelectRustFirst {
    /// How many records to write.
    #[param(mandatory, position = 0)]
    pub count: i64,
    /// A record to pass on.
    #[param(value_from_pipeline)]
    pub input_object: Option<PsObject>,
    step: Option<PsSteppable>,
}

impl Cmdlet for SelectRustFirst {
    fn begin(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        self.step = Some(ps.steppable("Select-Object", &[("First", self.count.into_ps()?)])?);
        Ok(())
    }
    fn process(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> {
        hand_on(&self.step, &self.input_object)
    }
    fn end(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> {
        end_step(&mut self.step)
    }
}
```

`hand_on` calls `step.process(record)` for a piped record and `step.process_empty()` when nothing was piped; `end_step` calls `step.end()`, which runs the command's end blocks and disposes it. A `PsSteppable` dropped without ending, such as when a later command stopped the pipeline, is disposed, and the clean blocks of its commands run in PowerShell 7.3 and later.

- The command is resolved as a typed command is, so a module's function runs in its module's scope. Parameters go by name.
- Each of `-WhatIf`, `-Confirm`, `-Verbose`, `-Debug`, `-ErrorAction`, `-WarningAction` and `-InformationAction` the caller gave the wrapper goes to the command too, unless `parameters` names it or the command does not take it. Hello's `New-RustFile`, declared `supports_should_process`, steps `New-Item`, so `New-RustFile x -WhatIf` makes nothing. For `cargo pwrs test`'s check that a `supports_should_process` cmdlet calls `should_process`, a command that takes `-WhatIf` counts as the wrapper calling it.
- A stop or a terminating error coming back through any step ends the wrapper as it ends a compiled cmdlet: the step returns it as the `Err`, and the command ends with the engine's own exception once the phase returns. `1..5 | Select-RustFirst 2` writes 1 and 2 and ends quietly, and a throw in `Invoke-RustEach`'s block ends it with that throw's own exception and error id.

### Declare a proxy of a command

`wraps = "Verb-Noun"` on `#[cmdlet]` makes the cmdlet a proxy of a command, as `ProxyCommand.Create` makes a proxy function: of one of PowerShell's own cmdlets, as below, or of [another module's command](#another-modules-command). The cmdlet declares every parameter the command has in the PowerShell running it, and the command's `[Cmdlet]` settings and output types. It runs the command as a steppable pipeline and passes it each record as it was piped. `impl Wrapper` gives the cmdlet steps of its own around the command, each defaulting to changing nothing: `arguments` before the command begins, `input` for each piped record, and `output` for each record the command writes. Hello's `Get-RustChildItem` adds `-Upper`:

```rust
#[cmdlet(verb = "Get", noun = "RustChildItem", wraps = "Get-ChildItem")]
#[derive(Default)]
pub struct GetRustChildItem {
    /// Writes each item's name in upper case instead of the item.
    #[param]
    pub upper: bool,
}

impl Wrapper for GetRustChildItem {
    fn output(&mut self, record: PsObject, ps: &Pipeline<'_>) -> PsResult<()> {
        if !self.upper {
            return ps.write_object(&record);
        }
        let name = String::from_ps(&record.get("Name")?)?;
        ps.write_object(&name.to_uppercase().into_ps()?)
    }
}
```

`impl Wrapper for GetRustProcess {}` makes a proxy that forwards everything as it is.

- The declarations are copied when the module is built, from the command's own assemblies and never from a PowerShell installed on the building machine. PowerShell 7's come from its NuGet packages at the newest release of each minor version from 7.4: 7.4.22, 7.5.13 and 7.6.9, each for Windows and for Unix. Windows PowerShell's come from its own assemblies on Windows.
- A parameter every release declares alike is a property of the cmdlet. One a release adds, drops or declares otherwise is declared at bind time for the running release and platform, so `Join-RustPath -ChildPath b, c` works in PowerShell 7.6 and is refused in 7.5 and in Windows PowerShell, as `Join-Path`'s is. Such a parameter shows in `Get-Command -Syntax` and in completion but not in the module's help file.
- The command's own dynamic parameters, such as the FileSystem provider's `-File` and `-Directory`, are taken from the command at bind time and forwarded.
- `-WhatIf`, `-Confirm`, the preference common parameters, `-ErrorVariable`, `-WarningVariable` and `-InformationVariable` go to the command, so its errors, warnings and prompts reach the caller as they do from the command run directly, as they do from a `ProxyCommand.Create` proxy.
- `output` runs during the cmdlet's own begin, process and end, for each record as the command writes it. A non-terminating error it returns is written and the command goes on; a terminating one ends the command and the cmdlet.
- An attribute C# cannot name, such as an internal completer, is left off, and the build prints a warning naming the command, the parameter and the attribute.
- A wrapping cmdlet takes none of `supports_should_process`, `confirm_impact`, `default_parameter_set`, `supports_paging`, `supports_transactions`, `help_uri`, `remoting_capability`, `output` and `as_job`, which come from the command, and no `#[dynamic_params]`. A parameter of its own may not share a name or an alias with one of the command's.
- Only Windows has Windows PowerShell's declarations. A module that targets Windows PowerShell builds elsewhere with a warning, and its wrapping cmdlets refuse to run in Windows PowerShell, naming why; a merge into the Windows build's folder leaves that build's shells as they are. A module whose `compatible-ps-editions` is `["Core"]` never reads them, and its wrapping cmdlets refuse in Windows PowerShell.

#### Another module's command

`wraps` also names a command of another module, as `Verb-Noun` or as `Module\Verb-Noun`, which picks that module's command and is what the cmdlet runs. Hello's `Compress-RustArchive` wraps `Microsoft.PowerShell.Archive\Compress-Archive`, a function. No package contains such a command's declarations, so the first build reads them where its module is installed, in pwsh and, on Windows, in Windows PowerShell: a compiled cmdlet's from its assembly's file, a function's or a CDXML command's from its param block, each attribute with its constant arguments. The build writes them to `wrapped.lock.json` beside `Cargo.toml` and says to commit it.

- Every later build, on any machine, compiles from `wrapped.lock.json`, so the module need not be installed where it builds. A build warns when the installed module's version is not the one the file was read from; deleting the command's entry makes the next build read it again.
- An attribute argument that is no constant, such as `[ValidateScript]`'s script block, cannot be written in C#, so that attribute is left off with a warning.
- A parameter type the module's own assembly defines is declared at bind time, once the cmdlet has loaded the module by resolving the command.

## Aliases, sets, and pipeline binding

- `alias = ["st"]` on `#[cmdlet]` and `alias = ["n"]` on `#[param]` become `[Alias]` attributes and the manifest exports the cmdlet aliases.
- `set = "ByName"` on a parameter and `default_parameter_set = "ByName"` on the cmdlet give parameter sets. `set = ["Path", "LiteralPath"]` puts a parameter in those sets and no others, so the binder refuses it beside a parameter of any other set, and a parameter without `set` is in every set.
- `value_from_pipeline`, `value_from_pipeline_by_property_name` and `value_from_remaining` map to the matching `[Parameter]` properties.
- A parameter is positional only with `position`. A compiled cmdlet's parameters get no position they did not declare, which is what `PositionalBinding = $false` gives a script function, so there is no key for it.
- `dont_show` hides a parameter from completion; `help = "..."` overrides the doc comment as the help message.
- `help_uri = "https://..."` on the cmdlet is the page `Get-Help -Online` opens, and `remoting_capability = "None"` (or `SupportedByCommand`, `OwnedByCommand`, or the default `PowerShell`) is what `Get-Command` reports for running it remotely.

## Checking an argument in Rust

`validate_with` names a function that checks a parameter's argument while it is bound, a `[ValidateScript]` written in Rust:

```rust
fn even_only(value: &PsObject) -> PsResult<()> {
    let n = i64::from_ps(value)?;
    if n % 2 == 0 {
        Ok(())
    } else {
        Err(PsError::new(ErrorCategory::InvalidArgument, "OddNumber", format!("{n} is odd")))
    }
}

#[cmdlet(verb = "Split", noun = "Even")]
#[derive(Default)]
pub struct SplitEven {
    #[param(mandatory, value_from_pipeline, validate_with = even_only)]
    pub number: i64,
}
```

The function runs after the binder converts the argument to the parameter's type, so `-Number '8'` reaches it as the `long` 8, and before the cmdlet's phase runs. An `Err` refuses the argument as PowerShell refuses one its own validation attributes reject: a `ParameterArgumentValidationError` naming the parameter, with `[OddNumber] 3 is odd` in its message. A piped value it refuses fails that record alone, and the other records go on. `Split-RustEven` in the hello example works this way.

## .NET attributes

`attribute(...)` puts any .NET attribute on the generated cmdlet or one of its parameters, and PowerShell does with it what it does with the same attribute on a compiled cmdlet. Hello's `Get-RustOldGreeting` is obsolete, and so is its `-Times`:

```rust
#[cmdlet(verb = "Get", noun = "RustOldGreeting", output = ["System.String"], attribute(type = "System.ObsoleteAttribute", args = ["Use Get-Greeting."]))]
#[derive(Default)]
pub struct GetRustOldGreeting {
    #[param(mandatory, position = 0)]
    pub name: String,
    #[param(attribute(type = "System.ObsoleteAttribute", args = ["Pipe the name once for each greeting."]))]
    pub times: Option<i64>,
}
```

PowerShell warns of the command on every call, before any parameter binds, and of the parameter when it binds:

```text
PS> Get-RustOldGreeting World -Times 2
WARNING: The command 'Get-RustOldGreeting' is obsolete. Use Get-Greeting.
WARNING: Parameter 'Times' is obsolete. Pipe the name once for each greeting.
Hello, World!
Hello, World!
```

`type` is the attribute class's full name, its `Attribute` suffix included. `args` are the constructor's arguments, a list or a single value, and `named(Help = "...")` sets the attribute's properties and fields by name. Each value is a Rust literal: a string, an integer, a float, `true` or `false`, a char, `null`, or a list of values. Where the constructor takes an enum, a string names its member, `"Show"` for `ExperimentAction.Show`, or several joined by commas; where it takes a `System.Type`, a string names the type. A constructor taking a `params` array takes its elements as separate arguments:

```rust
#[param(mandatory, position = 0, attribute(type = "System.Management.Automation.ValidateDriveAttribute", args = ["Env", "Variable"]))]
pub path: String,
#[param(attribute(type = "System.Management.Automation.PSDefaultValueAttribute", named(Help = "Hello")))]
pub greeting: Option<String>,
```

Hello's `Get-RustSalute -Path Function:\prompt` is refused before the cmdlet runs, with `The path argument drive Function does not belong to the set of approved drives: Env,Variable.`, and the help's default value for a parameter is what its `[PSDefaultValue]` says, its `Help` and otherwise its `Value`:

```text
PS> Get-Help Get-RustSalute -Parameter Greeting

-Greeting <string>
    The word to salute with. Help gives its default from the parameter's [PSDefaultValue].

    Required?                    false
    Position?                    named
    Default value                Hello
```

`cargo pwrs build` reads each attribute from the reference assemblies of both shells and writes it through the public constructor its arguments fit. An attribute only one shell's references have is written for that shell alone, with a warning. `[Experimental]` is PowerShell 7's, so hello's `Get-RustPreview` is hidden in PowerShell 7 while the feature `Hello.Preview` is off and is an ordinary cmdlet in Windows PowerShell:

```text
pwrs: cmdlet Get-RustPreview: System.Management.Automation.ExperimentalAttribute is in PowerShell 7's references and not Windows PowerShell's, so the Windows PowerShell shell leaves the attribute off
```

The build refuses each of these, naming the cmdlet, parameter or member and the shell whose references refused it:

- a type that is in neither shell's references as a public attribute class
- one whose `[AttributeUsage]` does not allow that kind of member, or allows it once where it is given twice
- arguments no constructor fits, or two constructors fit equally
- a named member that is no settable property or field
- `[Obsolete]` with `true` for its error flag, since PowerShell ignores that flag and warns either way, and C# would refuse the shell's own uses of the member

A class takes the same key: `#[psclass(attribute(...))]` on the class, `#[psfield(attribute(...))]` on a property, and a `#[psmethods]` method's own `#[attribute(...)]` mark on the method, or on `new`, the constructor. Hello's `Hello.Badge` has one of each, which reflection reads, as `[Hello.Badge].GetMethod('Shout').GetCustomAttributes([ObsoleteAttribute], $false).Message`.
