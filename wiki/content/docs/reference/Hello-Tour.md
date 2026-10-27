---
title: Hello Tour
weight: 9
---

`examples/hello/src/lib.rs` is one module that uses every mechanism PWRS offers, and `examples/hello/tests/*.Tests.ps1` test each behavior in both hosts. This page goes through it cmdlet by cmdlet. Build and run the tests yourself with:

```text
cargo run --profile test-fast -p cargo-pwrs -- pwrs test --release --manifest-dir examples/hello
```

Each section names the test file that covers it. All 472 tests pass in pwsh 7.6 on Windows and 468 in Windows PowerShell 5.1, and the 467 that are not Windows' alone pass in pwsh 7.4, 7.5 and 7.6 on Linux from one build. In pwsh 7.5 on FreeBSD 466 of them pass, since the pwsh built for FreeBSD has no `Start-ThreadJob` for the thread job's test. [`docs/PLATFORMS.md`](https://github.com/Variably-Constant/PWRS/blob/main/docs/PLATFORMS.md) records each.

## Get-Greeting: parameters, streams, errors, panics, cancellation

```rust
#[cmdlet(verb = "Get", noun = "Greeting", output = ["System.String"])]
#[derive(Default)]
pub struct GetGreeting {
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub name: String,
    #[param(validate_range(1, 1000000000))]
    pub count: Option<i64>,
    #[param]
    pub fail: bool,
    #[param]
    pub terminate: bool,
    #[param]
    pub panic: bool,
}
```

- `count: Option<i64>` is an optional parameter; `None` when `-Count` was not given. `validate_range` becomes `[ValidateRange(1L, 1000000000L)]`, so the engine rejects `-Count 0` before Rust runs.
- `fail`, `terminate` and `panic` are `SwitchParameter`s.
- `process` panics under `-Panic`; under `-Fail` or `-Terminate` it returns `Err(PsError::new(ErrorCategory::InvalidData, "GreetingRefused", ...))` with details, made terminating for `-Terminate`; otherwise it writes to the verbose stream and loops writing greetings while `ps.stopping()` is false.

What the tests in `Hello.Tests.ps1` show:

- A non-terminating error is written and the cmdlet continues: `-ErrorAction SilentlyContinue -ErrorVariable err` leaves one record whose `FullyQualifiedErrorId` matches `GreetingRefused` and whose category is `InvalidData`.
- `-ErrorAction Stop` turns the same error terminating.
- `ErrorDetails.Tests.ps1` reads the details from the record's `Exception.InnerException.Message`, for the record `-Fail` writes and the one `-Terminate` throws.
- A Rust panic is reported as a terminating error and the module still works afterward.
- `Get-Greeting -Name x -Count 1000000000 | Select-Object -First 2` returns in well under five seconds. The downstream `Select-Object` stops the pipeline, and the next `WriteObject` throws `PipelineStoppedException` on the managed side. The vtable entry reports status 5, the Rust `write` returns a terminating `OperationStopped` error that `?` passes up out of `process`, and the managed side rethrows the stop to the engine. `ps.stopping()` covers the other path, `StopProcessing`, which the engine calls for Ctrl+C and runspace stops.
- `Get-Help Get-Greeting` has the synopsis from the doc comment and the parameter description from the field's doc comment.

## Measure-RustLine: a blank line piped in

```rust
#[cmdlet(verb = "Measure", noun = "RustLine", output = ["System.String"])]
#[derive(Default)]
pub struct MeasureRustLine {
    #[param(mandatory, position = 0, value_from_pipeline, allow_empty_string)]
    pub input_object: String,
    lines: u64,
}
```

`Measure-RustLine` writes each line piped in as its number and its length, `3: 0` for a blank third line. The engine refuses an empty string for a mandatory string parameter before the cmdlet runs, so `Get-Content` output with a blank line in it would stop at that line with `ParameterArgumentValidationErrorEmptyStringNotAllowed`; `allow_empty_string` declares `[AllowEmptyString]`, and the blank line binds. `lines` has no `#[param]`, so it is a plain field that lives for the length of the pipeline.

`Binding.Tests.ps1` checks the attribute on `-InputObject`, pipes `'a', '', 'bc'` through and gets all three lines back, and pipes `''` into `Get-Greeting`, whose `-Name` has no such flag, to show the refusal on the error stream.

## Get-Person: a copied output class

```rust
#[psclass(name = "Hello.Person")]
#[derive(Default, Clone)]
pub struct Person {
    pub name: String,
    pub age: i64,
    pub tags: Vec<String>,
    pub score: Option<f64>,
    pub active: bool,
}
```

`Get-Person -Name Ada -Age 36 -Tag math, code -Score 9.5` returns a real CLR object of type `Hello.Person` with an `Int64` `Age`, a `string[]` `Tags`, and a nullable `Score`. `Classes.Tests.ps1` checks `GetType().FullName`, the property types, `Get-Member`, `Format-List`, and that an absent `Option` field reads as `$null`.

## New-Counter: a proxy class

```rust
#[psclass(name = "Hello.Counter", mode = proxy)]
#[derive(Default, Clone)]
pub struct Counter {
    pub label: String,
    pub value: i64,
    pub history: Vec<i64>,
}
```

The Rust value stays in Rust behind the object; each property read is one native call. `Dispose()` frees it exactly once, and `IsDisposed` reports the state. A property read after disposal returns `$null` to PowerShell: the getter throws `ObjectDisposedException`, and PowerShell's property adapter returns `$null` when a getter throws. Creating and disposing 2000 proxies in a loop is part of the test.

```rust
#[psmethods]
impl Counter {
    pub fn advance(&mut self, by: i64) -> PsResult<i64> { /* checked add, pushes to history */ }
    pub fn apply(&mut self, operator: String, by: i64) -> PsResult<i64> { /* + adds by, - takes it away */ }
    pub fn describe(&self, prefix: Option<String>) -> PsResult<String> { /* "prefix=value" */ }
    pub fn reset(&mut self) -> PsResult<()> { /* value 0, history cleared */ }
}
```

`Methods.Tests.ps1` calls them from script:

- `$c.Advance(5)` returns an `Int64`, and the `Value` and `History` properties show the change;
- `$c.Apply('+', 5)` takes an argument named `operator`, a word C# reserves, which the shell writes as `@operator`; `Get-Member` shows the name as `operator`;
- `$c.Describe()` and `$c.Describe('n')` show an optional argument omitted and supplied;
- `$c.Reset()` returns nothing;
- an overflowing `Advance` throws the Rust error, and a call after `Dispose()` throws;
- `Get-Member` lists the methods;
- `$c.Split('part')` returns a second `Hello.Counter` proxy with half the value;
- `$a.Absorb($b)` takes another counter as an argument;
- `$c.SameAs($c)`, a `&self` method, takes a copy of its own receiver as the argument, since reading the argument's properties nests inside the shared call;
- `$c.Absorb($c)`, whose `absorb` takes `&mut self`, is refused as in use, since no read of the object is allowed during an exclusive call.

Three of the block's functions take no receiver. `new` is the
constructor, so `[Hello.Counter]::new('made', 5)` makes a counter
without a cmdlet, and refuses a label containing `=`, which `Parse`
could not read back; `parse` is a static that returns the class, so
`[Hello.Counter]::Parse('label=7')` makes one from the text
`Describe` writes; and `limit` is a static returning a number. A
static runs against no object, and one that returns the class returns
a new proxy with the value it made. `Statics.Tests.ps1` makes,
calls, disposes and parses through the type on both hosts, checks
that a static is on the type and not on an instance, and makes a
counter whose constructor fails in a child host, then runs the garbage
collector and the finalizers and checks that the host is still running.

## Hello.Stretch: a constructor on a copied class

```rust
#[psclass(name = "Hello.Stretch")]
pub struct Stretch {
    pub start: i64,
    pub length: i64,
}

#[psmethods]
impl Stretch {
    pub fn new(start: Option<i64>, length: Option<i64>) -> PsResult<Self> { /* from Default, refusing a negative length */ }
    pub fn parse(text: String) -> PsResult<Self> { /* "start+length" */ }
}
```

A copied object is its fields, with no Rust value behind it, so
reading its properties never calls into Rust, which suits a value type.
For the same reason a copied class can have statics but not methods
with a receiver: there is no Rust value to run one against. `new` builds the
stretch in Rust and the new object is made from it field by field.

Two details are the point of this class. Its `Default` has a length of
60, not zero, and every argument of `new` is optional, so
`[Hello.Stretch]::new()` gives length 60: it starts from `Default`,
where a class without `new` would give CLR zeros. And because it
declares a `new`, that is the only public constructor it has, so a
script that makes a stretch goes through it, and a negative length is
refused. Rust has no overloading, so one `new` with `Option` arguments
handles `::new()`, `::new(5)` and `::new(10, 30)` alike.

`CopiedConstructors.Tests.ps1` checks each arity, the refusal of a
negative length as an exception, a static building through the
factory, that `Hello.Stretch` exposes exactly one public constructor,
and that `Hello.Person`, which declares none, still has its public
parameterless one.

## New-RustSlots: methods named like a collection's

`Hello.Slots` declares `Get`, `Set`, `Contains` and `Call`, the names a slot map would use. The generated class reaches the runtime through `Pwrs.ProxyBase`'s `PwrsGet` and `PwrsCall`, so a method of one of these names never replaces the runtime's own. `Methods.Tests.ps1` reads `Origin` and `Capacity` (the property getters, which call the base class as `base.PwrsGet`), runs each method, disposes the object, and checks that a Rust error still reaches the script.

## New-RustTicker: a proxy with Rust-only state

`Hello.Ticker` has `ticks: u64` under `#[psfield(skip)]`: `Get-Member` shows `Label`, `Width`, `Step` and `Limit` but no `Ticks`, and `$t.Tick()` counts in Rust across calls. The three other fields are the narrow numbers, a `u32`, an `f32` and an `Option<u16>`, whose property getters convert the value rather than unboxing it. So a value boxed as `long` or `double` reads as well as one boxed at the field's own width. `Classes.Tests.ps1` checks each.

## Get-Note: a psobject class

```rust
#[psclass(name = "Hello.Note", mode = psobject)]
pub struct Note {
    pub text: String,
    pub priority: i32,
}
```

No CLR type is generated. The output is a `PSObject` whose `PSTypeNames[0]` is `Hello.Note` with one note property per field.

## Invoke-RustBlock: script blocks

`#[param(mandatory, position = 0)] pub script: PsScriptBlock` and `#[param] pub arg: Vec<PsObject>`. `self.script.call(ps, &self.arg)` invokes the block with `$args` bound and returns every output object, which the cmdlet writes back. `Conversions.Tests.ps1`: `Invoke-RustBlock -Script { param($a, $b) $a * $b } -Arg 6, 7` is `42`.

## Get-RustTableInfo, Get-RustTableEntry and New-RustTable: hashtables

`PsHashtable` as a parameter exposes `len`, `keys`, `get`, `set`, `contains`. A `HashMap<String, String>` written with `ps.write(map)` arrives as a `[hashtable]` whose values are strings.

A `PsHashtable` parameter is declared `IDictionary`, so a `Hashtable`, an `[ordered]` dictionary or any other dictionary arrives as it was passed, its keys in their order. `Get-RustTableEntry` takes its table as a `PsObject` and wraps it with `PsHashtable::from_ps`, which reaches the same `contains` and `get`. `Conversions.Tests.ps1` reads a present key from each kind of table, and a missing key as `$null`, which is what `get` returns for any table.

## Get-RustChecksum and Get-RustBytes: zero-copy arrays

`self.bytes.pin::<u8>()` borrows a `byte[]` parameter as a `&[u8]` without copying; the pin is released when the borrow drops. `PsObject::from_slice(&data)` creates a `byte[]` and fills it through one pin. The tests checksum `[byte[]](1..255)` and read back `Get-RustBytes -Count 5` as a `Byte[]` of length 5.

## Get-RustByteSum and Get-RustByteRange: byte arrays as Vec<u8>

A `Vec<u8>` parameter binds a `byte[]` (or any array of numbers), and `PsArray(Vec<u8>)` writes one `byte[]`. A typed array is copied through one pin in each direction; an untyped one is read element by element. `Bytes.Tests.ps1` sums typed and untyped arrays, writes 0, 300 and 4194304 bytes, and round-trips the four megabytes.

`Get-RustRawByteSum` is the same cmdlet with `#[param(raw)]`, which declares the parameter `object`, so PowerShell passes the array as it is instead of converting it. The tests check that it gives the same sum as the typed one on four megabytes, and show the difference: a string is read as its characters, where the typed parameter refuses it.

`Measure-RustInput` goes the other way. Its `-InputObject` is a `PsObject` declared `byte[]` with `#[param(clr = "byte[]")]`, beside a `-LiteralPath` in a set of its own, the shape a compressor takes input in. The binder converts the argument to `byte[]`, so a byte array piped with `,` arrives whole, an enumerated one arrives a byte at a time as arrays of one, and a piped `FileInfo` goes to `-LiteralPath` by its `PSPath`. The bytes are read through `pin`, and `-Invert` flips them in place. `-InputObject` also has `allow_empty_collection`, so an empty array is measured rather than refused by the binder. `Bytes.Tests.ps1` checks the declared type, each binding, that `-Invert` changes the caller's own array, which is what shows nothing was copied, and that an empty array binds here while `Get-RustByteSum`, without the flag, refuses one.

## Get-RustMemory: Rust memory as Memory<byte>

`PsMemory::<u8>::zeroed(count)` allocates once, the loop fills it through `&mut [u8]`, and `ps.write(m)` passes the allocation to PowerShell. `Memory.Tests.ps1` reads a `` Memory`1 `` with `.Length` and `.ToArray()` on pwsh and a `Byte[]` on Windows PowerShell, for 5, 0 and 4194304 bytes.

`New-RustRegionView` writes views of sixteen bytes the module stores in an `Arc`, standing in for a mapped file. Each view is a `PsMemoryView` whose owner is a clone of that `Arc`; `-ReadOnly` makes it a `ReadOnlyMemory<byte>`, and `-Revocable` ties it to the `PsRevocation` that `Revoke-RustRegionView` sets. `Get-RustRegion` reads the module's bytes back and `Get-RustRegionHolderCount` counts the views not yet released. `Memory.Tests.ps1` checks in both hosts:

- that a write through the view reaches the module's bytes on pwsh, and not through the copy on Windows PowerShell;
- the read-only type;
- that a view's bytes stay alive until the view is garbage collected;
- that disposing its manager releases them at once and refuses every later read with `ObjectDisposedException`;
- that a revoked view refuses the same way, while a later view and an untied one still read;
- that a view made already revoked is refused and releases its owner.

## Test-RustBigInt: BigInteger

`PsBigInt::from_ps` reads a `System.Numerics.BigInteger` as its little-endian two's-complement bytes (through `ToByteArray` and a pin), `doubled()` shifts them, and `IntoPs` constructs a new `BigInteger` from the bytes. Positive and negative values round-trip.

## Test-RustDynamic: dynamic .NET access

With `-Text` alone: `"shout".into_ps()?.call("ToUpper", &[])` runs an instance method through PowerShell's member binder, and `PsType::from_name("System.Math").call_static("Abs", ...)` runs a static method whose argument already has the parameter's type, so .NET binds it as it is. With `-Uri`: `PsType::from_name("System.Uri").new(&[uri])` constructs an object and `.get("Host")` reads a property.

`-Round` calls `call_static("round", &[value, "AwayFromZero"])`. No overload of `Math.Round` takes text there, so the text converts to `MidpointRounding` as it would in a script, and the name matches without regard to case. `-Like` makes a `WildcardPattern` from the pattern and the text `"IgnoreCase"` the same way, and writes whether `-Text` matches it.

## Resolve-RustPath: PSPath resolution

`ps.resolve_path(&self.path, false)` resolves a PSPath through the session's providers, expanding wildcards; `true` gives the literal unresolved provider path. The tests run against `$TestDrive`.

## Get-RustTypeName: untyped input

A `PsObject` parameter accepts anything. `self.value.type_name()` reads `GetType().FullName`: `System.Int32` for `5`, `System.DateTime` for `Get-Date`.

## Get-RustColor and complete_color: argument completion

```rust
#[completer(cmdlet = "Get-RustColor", parameter = "Name")]
fn complete_color(ctx: &CompletionContext) -> PsResult<Vec<Completion>> {
    let prefix = ctx.word.to_lowercase();
    Ok(COLORS.iter().filter(|c| c.starts_with(&prefix)).map(|c| Completion::value(*c)).collect())
}
```

`Completers.Tests.ps1` calls `TabExpansion2` on `Get-RustColor -Name cr` and expects `crimson` but not `blue`; an empty word offers all five.

## Get-RustReading and reading_dynamic_params: dynamic parameters

```rust
#[dynamic_params(cmdlet = GetRustReading)]
fn reading_dynamic_params(bound: &PsHashtable) -> PsResult<Vec<DynamicParam>> {
    let kind = if bound.contains("Kind")? { String::from_ps(&bound.get("Kind")?)? } else { String::new() };
    if kind == "temperature" {
        Ok(vec![DynamicParam::string("Unit").with_validate_set(["C".to_string(), "F".to_string()])])
    } else {
        Ok(Vec::new())
    }
}
```

`-Unit` exists only when `-Kind temperature` is bound, is validated against `C` and `F`, and is read inside `process` with `ps.parameter_is_bound("Unit")` and `ps.parameter("Unit")`, since dynamic parameters are not fields of the struct.

`Get-RustStaticReading` and `Get-RustBlindReading` are `Get-RustReading` with the same parameter and the same `process`, the first with no hook and the second with a hook that adds nothing and never reads what is bound. They are there to be timed against it: `benches/dynamic_params.ps1` measures what dynamic parameters add to a call, and how much of that is PowerShell's own pass and how much is PWRS's, and [Benchmarks](../reference/Benchmarks.md) has the results. `Completers.Tests.ps1` checks that both write what `Get-RustReading` writes, that the static one implements no `IDynamicParameters`, and that neither offers `-Unit`.

## Measure-RustTotal: begin, process, end

```rust
impl Cmdlet for MeasureRustTotal {
    fn begin(&mut self, ps: &Pipeline<'_>) -> PsResult<()> { self.total = 0; ps.verbose("begin") }
    fn process(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> { self.total += self.value; Ok(()) }
    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> { ps.write(self.total) }
}
```

`total: i64` has no `#[param]`, so it is plain state on the instance, which lives from `BeginProcessing` to `Dispose`. `Phases.Tests.ps1` runs `1..4 | Measure-RustTotal` five times and checks the verbose `begin` record and the total on every run. An implemented `begin` or `end` is called on every instance, while a cmdlet that implements only `process` (such as `Get-Greeting`) stops making native calls for the other two phases after its first instance. See [The Call Path](The-Call-Path.md).

## Get-RustSignal, ConvertTo-RustSignal, New-RustLight: enums

```rust
#[psenum(name = "Hello.Signal")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Signal {
    #[default]
    Red,
    Amber = 5,
    Green,
}
```

`Hello.Signal` is a CLR enum with `long` underneath and members `Red = 0`, `Amber = 5`, `Green = 6`. `Enums.Tests.ps1` shows the binder converting `-Signal Green` from its name, rejecting `Purple`, binding `Option<Signal>` and `Vec<Signal>`, completing members through `TabExpansion2`, and `ConvertTo-RustSignal -Value 5` returning a value whose type is `Hello.Signal`, whose string form is `Amber`, and whose `[long]` cast is `5`. `New-RustLight` writes the enum in a copied class field, including an `Option<Signal>` that reads as `$null` when absent, and its `Hello.Light` declares `show = "{Name}: {State}"`, so it prints as `corner: Green`.

## Get-RustUnsigned: unsigned 64-bit

`u64` parameters and outputs pass without narrowing: `Get-RustUnsigned -Value ([uint64]::MaxValue)` returns a `UInt64` equal to it, and a `Vec<u64>` parameter sums to a `UInt64`.

## New-RustStamp, Add-RustTime, Test-RustValues, Test-RustCredential: dates, spans, GUIDs, chars, secrets

```rust
#[psclass(name = "Hello.Stamp")]
#[derive(Default, Clone)]
pub struct Stamp {
    pub at: PsDateTime,
    pub at_utc: PsDateTime,
    pub took: PsTimeSpan,
    pub id: PsGuid,
    pub initial: char,
    pub history: Vec<PsGuid>,
    pub signals: Vec<Signal>,
}
```

`New-RustStamp -At $date -Took $span -Id $guid -Label rust -History $g1, $g2 -Signals Red, Green` binds each typed parameter through the engine, a GUID from its string form and a `char` from a one-character string. It writes a `Hello.Stamp` whose properties are a `DateTime` with the given kind, its `ToUniversalTime()` from `to_utc()`, a `TimeSpan`, a `Guid`, a `Char`, a `Guid[]` and a `Signal[]`. `Add-RustTime` shifts a `DateTime` by a `TimeSpan`, with its kind unchanged, and writes the negated span. `Test-RustValues` writes a GUID's text form, the GUID itself and a `char`. `Test-RustCredential` reveals a `PSCredential`'s password (`ada:7`), builds a new credential in Rust with `PsCredential::new`, and reveals a `SecureString` parameter. `Values.Tests.ps1` covers all four in both hosts.

## New-RustTeam, Get-RustTeamSummary, Get-RustCounterText: classes as parameters and fields

```rust
#[psclass(name = "Hello.Team")]
#[derive(Default, Clone)]
pub struct Team {
    pub lead: Person,
    pub members: Vec<Person>,
    pub note: Option<Note>,
}
```

`New-RustTeam -Lead $ada -Member $ada, $bob -Note $n` binds `Hello.Person` objects and a `Hello.Note` PSObject to class-typed parameters and writes a `Hello.Team` whose `Lead` is a `Hello.Person`, whose `Members` is a `Person[]`, and whose `Note` is the PSObject. `$t | Get-RustTeamSummary` reads the team back through a piped class parameter, nested classes included; `Get-RustCounterText -Counter $c` reads a proxy back into Rust through its property getters. `ClassParams.Tests.ps1` covers each in both hosts, including the binder rejecting a string where a `Hello.Person` is declared.

`Runspaces.Tests.ps1` takes these objects to other runspaces and to a background job, in both hosts. A runspace has the module's commands only once it imports the module, or when a pool's `InitialSessionState` imports it. A counter passed to another runspace is the same object, and four runspaces advancing one counter 2000 times each leave it at 8000. In pwsh, `ForEach-Object -Parallel` and `Start-ThreadJob` return live counters. A job returns copies of the objects' properties: its `Hello.Person` binds to `New-RustTeam -Lead`, and `Get-RustCounterText` refuses its `Hello.Counter`, which `[Hello.Counter]::new` makes again. See [How To Use A Module In Runspaces, Jobs And Remote Sessions](../how-to/How-To-Use-Runspaces-Jobs-And-Remote-Sessions.md).

## Get-RustStream and Test-RustTarget: the worker thread and a targeted error

`Get-RustStream` writes a progress record, gives a worker thread a channel through `ps.stream_from_thread`, and writes every value the worker sends, in order. `Test-RustTarget` reads its argument's display string with `pwrs::types::display_string` and returns an error with that object as the record's target.

`Surface.Tests.ps1` streams five values and two thousand, stops a hundred thousand early with `Select-Object -First 3` to show the worker shutting down, reads the target object off the error record, and checks the tooltip the color completer attaches to each completion.

## Get-RustParallel and Measure-RustParallel: the worker pool

```rust
impl Cmdlet for GetRustParallel {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let order = if self.as_ready { Order::AsReady } else { Order::Input };
        ps.par_map((1..=self.count).collect::<Vec<i64>>(), order, |n| n * n)
    }
}
```

`Get-RustParallel` squares its input on a pool and writes the results from the pipeline thread; `-AsReady` writes each as its worker finishes instead of in input order. `Measure-RustParallel` runs the same pool through `ps.par_for_each`, adds to a shared `AtomicI64` from the workers, and writes the total once. Neither closure can capture `ps`, because the pipeline token is `!Send`.

`Parallel.Tests.ps1` checks that input order is the order given, that as-ready writes the same set with nothing dropped or repeated, that one item is not a special case, and that the total is right at five widths. The empty input and a worker panic are covered by the Rust tests in `crates/pwrs/src/convert_tests.rs`. See [How To Use Threads](../how-to/How-To-Use-Threads.md).

## Write-RustProgress, Invoke-RustWorker and Invoke-RustParallelWork: progress, workers and the stop

```rust
impl Cmdlet for InvokeRustParallelWork {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let items: Vec<i64> = (1..=self.count).collect();
        let sleep = std::time::Duration::from_millis(u64::from(self.sleep_ms));
        match (self.workers, self.map) {
            (Some(count), true) => ps.workers(count as usize)?.par_map_with(items, Order::Input, move |n, w| parallel_item(n, sleep, w)),
            // ...
        }
    }
}
```

`Write-RustProgress` writes one `Progress` record with every field it is given: a parent activity, a current operation, a percentage or none, the seconds left, and `-Completed` to end the activity. `Invoke-RustWorker` gives a thread a `Worker` through `ps.stream_from_worker`. From that thread it writes an output value, a warning, verbose, debug and information text, an error and a progress record, then a second value. `-Terminating` makes the error end the command, and `-ProgressRecords n` sends `n` progress records for one activity first, as fast as the thread can. `Invoke-RustParallelWork` runs `1..Count` through `par_for_each_with`, or `par_map_with` with `-Map`, each item waiting `-SleepMs` in 5 ms steps and returning early once its worker's `stopping()` is true, at most `-Workers` at once. `Get-RustParallelStats` reads how many items started and the most that ran at once.

`Workers.Tests.ps1`, in both hosts:

- reads each progress field back;
- merges a worker's streams with `*>&1` to check they arrive in the order sent;
- reads `-WarningVariable` and `-InformationVariable`;
- checks that a terminating error ends the command after the output before it;
- checks that ten thousand progress records for one activity reach the host as fewer, ending on the last;
- checks that two workers run at most two items at once, and that zero workers is refused;
- checks that stopping a run of ten-second items returns within 5 seconds, with no item started after the stop. The example's `parallel` feature builds it on Flynnel's pool, and the gates run its suite that way as well as on its own threads.

## New-RustFrame: a class shown by the text it renders

`Hello.Frame` is a proxy class declared with `view = draw`, so its default view is the text its `Draw` method returns. `New-RustFrame 3 1` shows a three-by-one box drawn with `+`, `-` and `|`, and `-Color` draws it in green with ANSI escape sequences. `Format-Table` and `Format-List` still show its `Width`, `Height` and `Color`. `Views.Tests.ps1` checks the drawing, that it is exactly what `Draw()` returns, the table and list, and that the escape sequences arrive unchanged, in both hosts. `Hello.Point` is a copied class declared with `show = "({X}, {Y})"`, so its `ToString()` is that text. `New-RustSegment 1 2 3 4` writes a `Hello.Segment` whose `Start` and `End` print as `(1, 2)` and `(3, 4)` in `Format-List` instead of the type name; `Views.Tests.ps1` checks both. `Hello.Job` has seven properties and declares `columns = ["Name", "Done", "Failed"]`, so `New-RustJob nightly 10 -Failed 2` shows as a table of those three while `Format-List` shows all seven. `Views.Tests.ps1` checks the table, `Format-Table`, the list and both views by name, in both hosts. See [How To Return Objects](../how-to/How-To-Return-Objects.md#default-views).

## Hello.Steps, Hello.Cells, Hello.Countdown and Hello.Release: lists, streams and comparisons

Four proxy classes name the `#[psmethods]` methods behind .NET interfaces, and each is made from script with its constructor. `[Hello.Steps]::new(10, 5, 4)` is the sequence 10, 15, 20, 25 computed as it is read, through `count = element_count` and `item = at`, so it is an `IReadOnlyList[long]`: `$steps.Count`, `$steps[2]`, `foreach` and `Measure-Object -Sum` read it. `[Hello.Cells]::new(3)` adds `set_item = set_cell`, so `$cells[1] = 7` writes through and the class is an `IList[long]` of a fixed size. `[Hello.Countdown]::new(3)` names `next = take` and enumerates 3, 2, 1 once. `[Hello.Release]::new(1, 2, 3)` names `compare = order`, `equals = same_as` and `hash = hash_code`, whose `order` and `same_as` take the other release as `&Self`, so `-lt`, `Sort-Object`, `-eq`, a hashtable key, `Group-Object` and `Select-Object -Unique` all go by the release's value.

`Interfaces.Tests.ps1` checks each of those in both hosts, and checks:

- that a list refuses an index past its end with the error its Rust method raises;
- that a fixed-size list throws `NotSupportedException` from `Add`;
- that a second enumeration of a stream is empty;
- that `Order($null)` throws `ArgumentNullException`, and ordering against a string `ArgumentException`;
- that two runspaces ordering the same pair of releases the opposite way round 2000 times each both finish.

## Hello.Vector and Hello.Tally: operators

`Hello.Vector` is a copied class with an `X` and a `Y`. It names `add = plus`, `sub = minus`, `mul = times` and `div = over`, static methods that take the vector by value first, so `$a + $b`, `$a - $b`, `$a * 2` and `$a / 2` each make a new vector. `over` refuses a division by 0 with an error of its own. `Hello.Tally` is a proxy class with a `Total`. It names `add = add_to` and `rem = remainder`, methods on the object, so `$t + 5` is a new tally and `$t % 4` is a number. Neither changes the tally it is called on.

`Operators.Tests.ps1` checks each operator's result and type in both hosts. It checks that the operands are left as they were, that each class declares the `op_` methods for its keys and no others, and that `$t - 1` throws, since `Hello.Tally` declares no `-`. It also checks that dividing a vector by 0 throws the error `over` returns, and that `-$a` is refused, since PowerShell runs it as `0 - $a`.

## Hello.Timer: methods that return tasks

`Hello.Timer`'s methods take a `PsTask<T>`, so each returns a `Task` that a Rust thread completes. `$timer.WaitAsync(50)` returns at once with a `Task[long]` that a thread completes with 50 after 50 ms, checking the caller's token every 5 ms and canceling the task once cancellation is requested; `-1` faults it. `PingAsync` completes a `Task` of no value before it returns, `ForgetAsync` drops its task without finishing it, and the static `[Hello.Timer]::SumAsync(2, 3)` adds on a thread of its own and faults on overflow. `StartedAsync` takes a `PsTask<Option<i64>>`, so it returns a `Task[Nullable[long]]`, completed with `$null` before the timer's first wait and with the count of waits after it. `CopyAsync` takes a `PsTask<Option<Self>>` and returns a `Task[Hello.Timer]`, completed with `$null` or with a copy of the timer. `Tasks.Tests.ps1` checks in both hosts:

- the task's type and value, and that the method returns before the work is done;
- `GetAwaiter().GetResult()`;
- cancellation while running and before the call;
- both faults, with the module's error, and that a dropped task faults with `PwrsTaskDropped`;
- the `Task` of no value, and both nullable tasks, with a value and with `$null`;
- the declared return type and optional token, and that the method ran once.

## Hello.ByteStream, Hello.Fraction, Hello.Descending and Hello.Failure: a .NET base class and .NET interfaces

`Hello.ByteStream` derives from `System.IO.Stream`. Its methods marked `#[member]` back `CanRead`, `CanSeek`, `CanWrite`, `Length`, `Position`, `Flush`, `Read`, `Write`, `Seek` and `SetLength` over bytes Rust has. A `StreamReader` reads `[Hello.ByteStream]::new("one`ntwo")` to the end, and disposing the reader disposes the stream. `Read` fills the caller's own `byte[]`, which it takes as `PsArrayMut<u8>`, and `CopyTo` copies the stream into a `MemoryStream`. `New-RustByteStream` makes one from a cmdlet, through the base constructor that takes no arguments. The static `CopyBytes` takes two arrays in place, and refuses one array given as both.

`Hello.Fraction` implements `IFormattable`, so `'{0:F2}' -f [Hello.Fraction]::new(1, 4)` is `0.25`. Its `ToString()` overrides `System.Object`'s through a method marked `#[member = "System.Object.ToString"]`. `Hello.Descending` implements `IComparer`, which `[Array]::Sort` orders by, largest first. `Hello.Failure` derives from `System.Exception`. Its `new` passes the message to the base constructor through `#[base(message)]`, and `base_args`, marked `#[base]`, does so for a failure `New-RustFailure` makes. `Set-RustBytes` sets every byte of the caller's array in place.

`Bases.Tests.ps1` checks each of those in both hosts, and checks:

- that a window outside the buffer, a seek before the start and an unknown format are refused with their Rust errors;
- that `CopyBytes` given one array as both is refused, and leaves the array as it was;
- that a `Hello.Failure` thrown from script is caught with its message and its code;
- that a `new` refusing leaves nothing behind through a garbage collection.

## Send-RustEvent: engine events from a Rust thread

`Send-RustEvent Hello.Tick -Text hi -Count 3` takes `ps.events()` on the pipeline thread and moves it into a `stream_from_worker` worker, which raises three events under `Hello.Tick`, each with a `Hello.EventData` as its MessageData: the text, and which event it is, from 1. `-Sender hello` gives each event that sender and its number and the count as arguments, and `-DelayMs 500` raises from a thread the command leaves running, half a second after it returns. `Events.Tests.ps1` checks in both hosts:

- that `Wait-Event` reads an event with its MessageData and nothing else;
- that `Get-Event` reads 200 of them in the order raised the moment the command returns;
- the sender and arguments;
- that a `Register-EngineEvent -Action` runs on the pipeline thread, in order, and leaves nothing queued;
- that the delayed event was generated after the command returned;
- that a child host started with `-MTA` receives one.

See [How To Raise Engine Events](../how-to/How-To-Raise-Engine-Events.md).

## Get-RustApartment: a thread for a window or a COM object

`Get-RustApartment` runs a `stream_from_worker` worker that reads its COM apartment through `pwrs::thread::apartment()` before, while and after it has the guard `pwrs::thread::enter_sta()` returns, and writes the three as a `Hello.Apartments`. On Windows the worker is `ImplicitMta` outside the guard and single-threaded inside it: `Sta` where the pipeline thread is single-threaded, as by default, and `MainSta` in a host started with `-MTA`, where the worker is the process's first thread to enter one. Off Windows all three read `NoCom`. `Apartments.Tests.ps1` checks both. See [How To Use Threads](../how-to/How-To-Use-Threads.md#a-thread-for-a-window-or-a-com-object).

## Get-RustProperty: reading a property bag from Rust

`Get-RustProperty` takes a `PSObject` from the pipeline and reads one
property off it with `pwrs::object::property`, which is the read side
of `add_note`. It reads note properties, so it works on a
`[pscustomobject]` built in script and on an object a Rust cmdlet
wrote. A name the object does not have is an error rather than a null,
so a misspelling does not read as missing data.

`Hello.Tests.ps1` reads a string and a number off a `[pscustomobject]`,
reads a property off the object `Get-Note` writes, and checks that an
absent name throws.

## Get-RustWidths and Get-RustTypeTag: the type a value arrives as

`Get-RustWidths` writes one value of each CLR width, so a caller can
see the type each one arrives as: `sbyte`, `short`, `int`, `byte`,
`ushort`, `uint`, `float`, `long`, `double`. They arrive as those
types rather than widened to `Int64` and `Double`, which matters
because in PowerShell the type of an arithmetic result depends on the
types of its operands.

`Get-RustTypeTag` writes the tag PWRS gives one object's type, which
`PsObject::type_tag()` returns in a single call into .NET. A type PWRS
has no tag for gives `0`, and for a value wrapped in a `PSObject` the
tag is the value's, not the wrapper's.

`Widths.Tests.ps1` asserts the nine type names in order, that a
`Single` is not a widened `Double`, that an `Int32` plus one is still
an `Int32`, and that a `PSCustomObject` gives `0` rather than a
guess.

## Get-RustDecimalRoundTrip and Measure-RustDecimalBlock: two orders for four words

`Get-RustDecimalRoundTrip` reads a decimal into the four words
`Decimal.GetBits` returns and builds a new one from them, so the
round trip can be checked; `-Scale` writes the scale instead.
`Measure-RustDecimalBlock` sums a `Decimal[]` by pinning it, which
reads the whole array as one block, and refuses a block whose
elements do not share one scale rather than adding words that do not
line up.

The two types are the same four words in the two orders that exist:
`PsDecimal` is `GetBits` order and `PsDecimalBits` is memory order,
which is what a pinned element is. The first decimal pin in a process
checks that order against `GetBits` before any element is read.

`Widths.Tests.ps1` round-trips both extremes and a negative, checks
that a trailing zero survives as scale, and sums a pinned block. It
also records that a string cast does not agree across hosts: pwsh 7
makes `[decimal]'1.10'` scale 1 and Windows PowerShell 5.1 makes it
2, so the test reads the scale the host produced rather than expecting
either value.

## Get-RustOffset: an instant with its UTC offset

`Get-RustOffset` round-trips a `DateTimeOffset`, or writes its offset
in whole minutes, or the same instant as ticks on the UTC clock. A
`PsDateTime`'s kind says only which clock the value is on, UTC or
local, so the offset is what makes a value the same instant on another
host.

## Measure-RustPropertyReads: what a read costs from this side

`Measure-RustPropertyReads` reads one property from every object it is
given, a given number of passes over, and writes how many reads that
was. Nothing else happens in the loop, so the call's time divided by
that count is the cost of one read, since invoking the cmdlet is paid
only once. `benches/dynamic_reads.ps1` runs it against the two reads
PowerShell has for the same objects, and
[Benchmarks](../reference/Benchmarks.md) has the results.

`Measure-RustTypeReads` does the same for reading each object's type,
which code that handles input of unknown types does for every item: it
reads the type of every object, through `type_tag` or, with `-ByName`,
through `type_name`, and writes the count. The same benchmark script runs both against
`$o.GetType().FullName` in script.

## Get-RustReadOnlyTable: a table script can read but not write

`Get-RustReadOnlyTable` wraps a table in `PsReadOnlyTable::over`, which
stores a reference to the source rather than copying it. Both `$t.key`
and `$t['key']` read, and neither writes: the type implements the
non-generic `IDictionary` publicly, because PowerShell's adapter
exposes a key as a property only through that interface, and it throws
from every member that would change the table. A nested table comes
back wrapped too, from the indexer, from `Values` and from enumerating,
so no nested table can be changed either. `-Ordered` builds it over an
`OrderedDictionary`, which shows that the order is the source's and not
a copy's.

`ReadOnlyTable.Tests.ps1` checks the value of every read, not only
that it did not throw, since a wrapper that returns `$null` for one way
of reading is the failure to catch, and checks each refused write by
reading the value back afterward.

## Write-RustStreams: a message that is never built

`Write-RustStreams` writes one record to each of the four message
streams through `pwrs::verbose!`, `debug!`, `warning!` and
`information!`. Those check whether the record would go somewhere,
and format the text only if it would. The plain `ps.verbose(&str)` form
cannot: the text is already formatted and allocated when it is called,
and whether the record goes somewhere is known only after the call
into .NET.

`Hello.Tests.ps1` checks each stream twice, once with the stream off
and once on, and that `-Verbose:$false` overrides a `$VerbosePreference`
of `Continue`, as the common parameter does in PowerShell.

## Write-RustHost: writing as Write-Host writes

```rust
impl Cmdlet for WriteRustHost {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        ps.write_host(&self.object, self.foreground_color, self.background_color, self.no_newline)
    }
}
```

`Write-RustHost` takes Write-Host's own parameters: the text,
`-ForegroundColor` and `-BackgroundColor` as `pwrs::ConsoleColor`, which
the binder converts from a color's name and completes, and `-NoNewline`.
`ps.write_host` writes what Write-Host writes, an information record
tagged `PSHOST` whose message has the text, its colors and whether the
line stays open. The host shows it whatever `$InformationPreference`
is, unless the preference is Ignore or a `6>` redirection takes the
record, and a color left out is the host's current one.

`HostWrite.Tests.ps1` runs each case through `Write-Host` and through
`Write-RustHost` in a host that records every call made to it, with
its colors, and both must make the same calls. It then checks the record
`-InformationVariable` saves, what `6>&1` and `6>$null` do with it, and
that a transcript records each write as a line, as it records
`Write-Host`'s.

## Remove-RustThing, Get-RustRecord and Get-RustRoute: what a parameter can declare

`Remove-RustThing` declares `supports_should_process` and `confirm_impact`, so PowerShell gives it `-WhatIf` and `-Confirm`, and the change happens only when `ps.should_process` returns true. `Get-RustRecord` uses the rest of what a `#[param]` can declare: two parameter sets with `default_parameter_set`, a parameter bound from a piped object's property, one that takes the remaining arguments, a pattern and a not-null validator, and one hidden with `dont_show`. `Get-RustRoute` has three sets, `Path`, `LiteralPath` and `Text`, and a `-Destination` declared with `set = ["Path", "LiteralPath"]`, so it binds with either file parameter and is refused with `-Text`.

`Binding.Tests.ps1` tests each in both hosts, including that `-WhatIf` performs nothing, that naming both sets at once is refused, and that the pattern validator is case-insensitive because PowerShell applies it that way. For `Get-RustRoute` it checks the sets `Get-Command` reports for `-Destination`, the binding in each set, the `AmbiguousParameterSet` refusal with `-Text`, and that help writes one syntax line per set naming only that set's parameters.

## Format-RustLabel: lengths, counts, scripts, nulls and wildcards

`Format-RustLabel` declares the remaining checks a parameter can have. `-Label` takes 1 to 8 characters through `validate_length(1, 8)`, and `-Tag` takes 1 to 3 values through `validate_count(1, 3)`. `-Size` must be even, through `validate_script = "$_ % 2 -eq 0"`. `-Owner` refuses `$null` through `validate_not_null`, and `-Note` is mandatory with `allow_null`, so it must be given but may be `$null`. `-Pattern` is declared `supports_wildcards`, and the cmdlet matches the label against it through `System.Management.Automation.WildcardPattern`.

`Binding.Tests.ps1` checks in both hosts that each attribute is on its parameter and that a call every key accepts binds. It checks that each key refuses what it should: a nine-character label, four tags, an odd size with the message PowerShell's `ValidateScript` gives, and `$null` for `-Owner`. It also checks that `-Note` takes `$null` while the mandatory `-Label` refuses it, and that the help says `-Pattern` takes wildcards. For `Get-RustRecord` it checks that a piped object whose `Tag` is `$null` leaves `-Tag` absent, while an empty `Tag` binds an empty string.

## New-RustWide and Hello.Wide: more than 64 parameters, arguments and fields

```rust
macro_rules! declare_wide {
    ($($field:ident)*) => {
        #[psclass(name = "Hello.Wide")]
        #[derive(Default, Clone)]
        pub struct Wide {
            $(pub $field: Option<i64>,)*
        }

        #[psmethods]
        impl Wide {
            #[allow(clippy::too_many_arguments)]
            pub fn new($($field: Option<i64>),*) -> PsResult<Wide> {
                Ok(Wide { $($field),* })
            }
        }

        #[cmdlet(verb = "New", noun = "RustWide", output = ["Hello.Wide"])]
        #[derive(Default)]
        pub struct NewRustWide {
            $(
                #[param(value_from_pipeline_by_property_name)]
                pub $field: Option<i64>,
            )*
        }
        // impl Cmdlet for NewRustWide writes Wide { $($field: self.$field),* }
    };
}

declare_wide!(n0 n1 n2 /* ... */ n128 n129);
```

A block marks each parameter or argument that was given, and each optional field that is set, with a bit, in 64-bit words, one word for each 64 items. `declare_wide!` takes 130 names, so `Hello.Wide` is a copied class of 130 optional numbers, `N0` to `N129`. Its constructor takes 130 optional arguments, and `New-RustWide` takes 130 parameters, so each of the three blocks has three words. The macro writes the names once for all three, and `#[cmdlet]`, `#[psclass]` and `#[psmethods]` work on what a `macro_rules!` expands to as they work on code written out. The `allow` on `new` is for clippy, which warns about a function of more than seven arguments.

Every parameter of `New-RustWide` also binds by property name, so a `Hello.Wide` piped in is copied, with what the command line gives in place of its own: `[Hello.Wide]::new(1, 2) | New-RustWide -N64 3` has `N0` 1, `N1` 2 and `N64` 3, and every other field `$null`.

`Wide.Tests.ps1` checks in both hosts:

- that the cmdlet declares all 130 parameters, each bound by property name;
- that the first and last parameter of each word bind from the command line, and no other;
- that a record whose only change is to a parameter in the first, the second or the third word is bound again;
- that a `Hello.Wide` piped in is copied with the change the command line makes;
- that the constructor takes all 130 arguments in order, and leaves `$null` each field whose argument is `$null` or left out.

Every one of the 130 parameters takes a record in those tests, so the surface check of `cargo pwrs test` finds each of them exercised.

## Expand-RustText and Get-RustModuleName: help and aliases

`Expand-RustText`'s doc comment writes its synopsis across two source lines and follows it with a blank line and a description paragraph. `Hello.Tests.ps1` asserts `Get-Help` shows the whole first paragraph joined into one sentence as the synopsis, and the paragraph after it as the description.

`Get-RustModuleName` is a unit struct with no parameters, so the generated cmdlet has no parameter block at all, and `alias = ["grmn"]` puts an alias on it. `Hybrid.Tests.ps1` checks that `grmn` runs and that it appears in `(Get-Module Hello).ExportedAliases`, where it is because the root module's `Export-ModuleMember` exports it from the nested binary module.

## Get-RustHybrid: hand-written C#

`src/csharp/GetRustHybrid.cs` is a plain `PSCmdlet` compiled into the shell assembly. It declares the cmdlet alias `grhyb` and gives its `-Name` parameter the alias `n`. `GetRustHybridInfo.cs` beside it declares the same two attributes in the other forms the scanner accepts: the alias ahead of the cmdlet attribute, the cmdlet attribute fully qualified, and the alias names as an array. `Hybrid.Tests.ps1` runs both, and checks that the manifest and the module export each cmdlet and alias along with the Rust ones. It checks that a parameter's alias is not exported as a cmdlet alias, and that the two appear in the manifest in file-name order rather than in whatever order the directory was listed.

`GetRustHybridDesktop.cs` reads Windows Forms' `SystemInformation` and sets one pixel of a 4 by 3 `System.Drawing.Bitmap`; `references = ["System.Windows.Forms", "System.Drawing.Common"]` in hello's `Cargo.toml` makes both available to the two compiles. `Get-RustHybridDesktop` writes the computer name, the Windows Forms version the host bound (10.0.0.0 in pwsh 7.6.7, 4.0.0.0 in Windows PowerShell 5.1), the bitmap's size, the green of the pixel read back, and the assembly `Bitmap` came from (`System.Drawing.Common` in pwsh, `System.Drawing` in Windows PowerShell). In pwsh 7.6.7:

```text
ComputerName    : <computer name>
FormsVersion    : 10.0.0.0
BitmapSize      : 4x3
PixelGreen      : 20
DrawingAssembly : System.Drawing.Common
```

Off Windows the same module imports, and `Hybrid.Tests.ps1` checks there that the call fails naming `System.Windows.Forms`. See [How To Add Hybrid C#](../how-to/How-To-Add-Hybrid-CSharp.md#windows-forms-wpf-and-other-desktop-assemblies).

## Read-RustHost: prompting at the console

```rust
let ui = ps.host_ui()?;
match self.kind.as_str() {
    "Line" => ps.write(ui.read_line()?),
    "Secure" => ps.write(ui.read_line_as_secure_string()?.len()? as i64),
    "Choice" => ps.write(ui.prompt_for_choice("Choose", "Which one?", &choices, 0)? as i64),
    ...
}
```

`Read-RustHost` prompts through the host's own prompts and writes what
was entered: a line, the length of a line typed without echo, or the
index of the choice taken among `-Choices`, with `-Say` putting text on
the host's output first. That text goes to the host and not to the
pipeline, so nothing downstream receives it.

`HostUi.Tests.ps1` runs in a `-NonInteractive` host, which cannot take
input, so its prompting cases run the cmdlet in a runspace whose host
reads each reply from a queue and records what was written to it: a
`PSHost` of a few dozen lines, compiled with `Add-Type`, that the
cmdlet reaches through the same `$Host.UI` a console provides. A
console host cannot be used in its place: its `ReadLineAsSecureString`
reads keys from the console device, which redirected input does not
reach, so a child shell fed through standard input waits on that
case forever. The `-NonInteractive` host is a case of its own, and
fails with PowerShell's error rather than hanging.

## Get-RustComposed: running a command by name

```rust
let results = if let Some(numbers) = &self.sort {
    ps.invoke_with_input("Sort-Object", &[], Some(&numbers.clone().into_ps()?))?
} else {
    let mut parameters = Vec::new();
    if let Some(name) = &self.name {
        parameters.push(("Name", name.clone().into_ps()?));
    }
    ps.invoke(self.command.as_deref().unwrap_or("Get-Greeting"), &parameters)?
};
```

`Get-RustComposed` runs another command from Rust by name and writes
what it wrote: `Get-Greeting` from this module with `-Name` bound as
its parameter, any command named with `-Command`, or `Sort-Object`
over the numbers `-Sort` pipes in. The command is added to a nested
pipeline by name, which PowerShell resolves as it does a typed
command, so no script block is built, nothing is parsed, and a
module's function runs in its module's scope. An error the command
raises is handled as if the user had run it: a non-terminating error
reaches this cmdlet's error stream and the output still comes back, so
`-ErrorAction` on `Get-RustComposed` controls how it is handled; a
terminating one is returned as the `Err`.

`Invoke.Tests.ps1` checks a module cmdlet against its own output, an
engine cmdlet's `Get-Location` against the session's, a function of a
module `New-Module` builds whose `dynamicparam` block reads that
module's `$script:` state, as a CDXML function's does, `3, 1, 2`
coming back sorted, a name the session does not have throwing with
PowerShell's own "not recognized" message, and a `Get-Item` on an
absent path producing an error record here and no exception.

## Get-RustInvocation: where a cmdlet stands in its pipeline

```rust
fn begin(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
    let invocation = ps.invocation()?;
    let position = i64::from_ps(&invocation.get("PipelinePosition")?)?;
    let length = i64::from_ps(&invocation.get("PipelineLength")?)?;
    self.place = format!("{position}/{length}");
    Ok(())
}
```

`ps.invocation()` returns the cmdlet's own `MyInvocation`, PowerShell's
`InvocationInfo`, so a cmdlet can read its place in the pipeline before
any input reaches it. `Get-RustInvocation` reads its place in `begin`,
passes its input through, and writes the place at `end`, so
`Get-RustInvocation | Get-RustInvocation | Get-RustInvocation` reports
`1/3`, `2/3` and `3/3`. Cmdlets that work together can use this to
find out whether the commands next to them are theirs.

`PipelineLength` counts commands, so piping to anything, `Should`
included, counts that command too, and an expression at the head of a
pipeline is input rather than a command and is not counted. Begin
blocks run left to right, but PowerShell processes a command's queued
input right after that command's own begin, so when an upstream begin
writes output, a command can process it before a later command has
begun. Cmdlets that work together through their places should not act
in `process` until all of the places are known.

`PipelinePosition.Tests.ps1` checks a lone call, the command it is
piped into being counted, every position of a three-stage chain,
other commands being counted, and an expression at the head not being
counted.

## Get-RustSize: changing an argument before it is bound

```rust
#[transform(cmdlet = "Get-RustSize", parameter = "Size")]
fn as_bytes(value: &PsObject) -> PsResult<PsObject> {
    let text = String::from_ps(value)?;
    ...
    match digits.trim().parse::<i64>() {
        Ok(n) => n.checked_mul(scale)...,
        Err(_not_a_number) => Err(size_error(trimmed, "is not a number followed by KB, MB or GB")),
    }
}
```

`-Size` is a `long`, and `Get-RustSize -Size 2MB` binds because the
transform turned the string into a number first. The transform runs
before PowerShell converts the argument to the parameter's declared
type and before validation, so it can accept forms the type itself
cannot. A value it does not recognize is returned unchanged, and the
binder converts or refuses it as it would without the transform.

A refusal from the transform is a binding failure, not an error the
cmdlet wrote: `Get-RustSize -Size twelveMB` throws with the
parameter named, and the cmdlet body never runs. Converting in the
body would have produced an error record instead, after the call had
started.

## Get-RustInk: a CLR enum this module never declared

```rust
#[psenum(clr = "System.ConsoleColor")]
pub enum Ink {
    Black = 0,
    DarkBlue = 1,
    Red = 12,
    White = 15,
}
```

`Ink` declares nothing to PowerShell. `System.ConsoleColor` already
exists, so the mirror maps a Rust enum onto it: `-Color` is declared
as the real CLR type, and the binder converts its member names,
completes them, and refuses a name that is not one before the body
runs. `Get-RustInk -Color Black` writes the next color as a
`System.ConsoleColor`, not as a number.

The four variants are a subset of `ConsoleColor`'s. `Green` is a
`ConsoleColor`, so the binder accepts it and `FromPs` then fails,
because the Rust enum has no variant with that value: a mirror can
have fewer members than the type it mirrors, and a value it has no
variant for is an error rather than a guess.

`Transforms.Tests.ps1` covers both: the suffixes and the two kinds
of refusal for the transform, and for the mirror the member names,
the type written back, the narrower variant list, and
`(Get-Command Get-RustInk).Parameters['Color'].ParameterType`.

## Get-RustErrorInfo: reading an error that already happened

```rust
let record = PsErrorRecord::from_ps(&self.record)?;
let target = if record.target.is_null() { String::new() } else { String::from_ps(&record.target)? };
ps.write(format!("{:?}|{}|{}|{}", record.category, record.error_id, record.message, target))
```

`PsError` is the error a cmdlet raises. `PsErrorRecord` is the error
it reads: the record the engine built for something that already
failed, taken as a parameter, off the pipeline, out of
`-ErrorVariable`, out of a `catch`, or back from a command run
through `invoke`. `Get-RustErrorInfo` writes its four parts, and the
category comes back as the `ErrorCategory` enum, so code can match on
`ObjectNotFound`, for example to retry, rather than on the words in
the message.

`ErrorRecord.Tests.ps1` uses the engine's own record, from a
`Get-Item` on a path that is not there, so the category, the id and
the target are the ones every PowerShell user has seen. It checks
that record caught in script and the same record passed down the
pipeline, a record a Rust cmdlet raised, and an object that is not a
record at all, which fails rather than reading as an empty one.

## Get-RustLifecycle: what runs at load, import, removal and unload

```rust
#[on_load]
fn make_registration() -> PsResult<()> {
    run_hook("load")?;
    set_registration(Some("registered"))
}

#[on_import]
fn count_import() -> PsResult<()> {
    run_hook("import")
}

#[on_remove]
fn count_remove() -> PsResult<()> {
    run_hook("remove")
}

#[on_unload]
fn release_registration() -> PsResult<()> {
    set_registration(None)?;
    run_hook("unload")
}
```

Named under `on_load`, `on_import`, `on_remove` and `on_unload` in
`export_module!`, these run with no cmdlet and no pipeline. The import
hook runs at every import, in any runspace, and the removal hook once
for each import. The load hook runs when the module goes from imported
in no runspace of the process to imported in one, and the unload hook
when it goes back. `run_hook` counts each run, and `Get-RustLifecycle`
writes a hook's count: `Import` by default, or `Remove`, `Load` or
`Unload`. The counts live in the native library, which a removal does
not unload, so they last across a `Remove-Module` and the import that
follows it.

`set_registration` stands in for something a module makes for the
whole process, such as a window class: a variable of the process
environment, set through `[Environment]::SetEnvironmentVariable` and
named `PWRS_HELLO_LOAD_` and the address of a static in the loaded
copy of the library, which no copy loaded beside it shares. The load
hook makes it and the unload hook releases it, so it stays set while
any runspace has the module.

`Set-RustLifecycleFailure -Hook Remove` makes the next run of that hook
return an error with the id `HelloLifecycleFailure`, so a test can see
what a failing hook does. The unload hook releases the registration
before it fails, and the load hook fails before it makes one.

`Lifecycle.Tests.ps1` runs each case in a child process of the host,
so the imports other suites make do not count. It checks that a
removal in one of two runspaces leaves the registration, and that the
last removal releases it. A runspace, a pool, a thread job or a
parallel run that ends without `Remove-Module` counts as a removal. A reload while
another runspace has the module runs the old copy's unload hook and the
new copy's load hook once each. The rest of the suite checks what each
kind of failing hook does. The hot reload gate checks across three
rebuilt copies that each releases its own; see
[Two Hosts](../explanation/Two-Hosts.md#process-wide-state-across-a-reload-and-a-drop).

## New-RustReservation: an allocation that can fail

```rust
let mut buf: Vec<u8> = Vec::new();
buf.try_reserve_exact(self.bytes as usize)?;
ps.write(buf.capacity() as u64)
```

Rust ends the process when an infallible allocation fails, and no
boundary can catch that, so a size taken from input is reserved with
`try_reserve`. `?` turns the `TryReserveError` into an error record
with id `PwrsOutOfMemory` and category `ResourceUnavailable`.
`Memory.Tests.ps1` requests half the address space, which the
allocator refuses, and more than a vector can have, which is refused
before the allocator is called; each is one error record with no
output, and the next reservation in the session succeeds.

## Test-RustOffThread: the call a worker must not make

`Test-RustOffThread` clones its `PsObject` parameter into a thread it
starts and calls `type_name` there. `PsObject` is `Send`, so the
compiler allows it, but the call would attach the thread to the .NET
runtime and run PowerShell's member binder on a thread with no
runspace.
While the thread check is on, in a debug build and under `cargo pwrs
test`, which sets `PWRS_THREAD_CHECK=1`, the call returns
`PwrsOffThread` instead. `Parallel.Tests.ps1` checks the refusal under
the check and the name without it, and that the same call on the thread
the cmdlet runs on is untouched. With `-Attached` the worker first calls
`pwrs::attach_current_thread()`, as a thread that calls PowerShell on
purpose does: it reads the name while it has the guard and is refused again
once the guard is dropped, and `Parallel.Tests.ps1` checks both
results. See
[How To Use Threads](../how-to/How-To-Use-Threads.md).

## Get-RustCpu and Measure-RustTieredSum: instruction sets

`Get-RustCpu` writes one `Hello.CpuFeature` per x86-64 extension
`pwrs::cpu` can detect: whether the library was compiled for it, whether the
CPU and operating system offer it, and whether a kernel may use it once
`PWRS_CPU_MAX` is applied. `Measure-RustTieredSum` pins a `Double[]`,
sums it through the widest kernel `pwrs::cpu::has` allows, AVX-512, AVX2
or scalar, and writes the tier and the sum; every tier adds in the same
order, so all three give the same bits.

`Cpu.Tests.ps1` checks that everything compiled in is offered where the
module was imported, then starts a child process of the same host under
a cap: `x86-64` refuses an import of a library compiled for more and
names what the cap withholds, `x86-64-v3` limits what `Get-RustCpu`
reports, and an unknown level is refused. The sum is checked bit for bit against
the same eight-lane order computed in script. See
[How To Use Instruction Sets](../how-to/How-To-Use-Instruction-Sets.md).

## New-RustSeries and its stages: one object for a whole series

```rust
#[param(mandatory, position = 0, value_from_pipeline)]
pub series: PsProxy<Series>,
```

A `Hello.Series` is a proxy with a count and a plan, whose steps are
in a skipped field, so it cannot be read back by value.
`New-RustSeries` makes one. `Add-RustSeriesStep` takes it by type,
copies it out with `self.series.with(Series::clone)`, appends `-Scale`,
`-Shift` or `-Above`, and writes the copy as a new series, leaving its
input as it was. `Measure-RustSeries` counts and sums the numbers in one
pass inside the borrow, and `Expand-RustSeries` writes them as rows,
from a copy, so the series is not borrowed while commands downstream
run. No
number exists until one of those two reads the series.

`Handles.Tests.ps1` checks that the stages pass one object, that a step
applies in the order it was added, that a stage leaves its input alone,
that the object goes through `Where-Object`, `ForEach-Object`, a
variable, a named argument and an array unchanged, that a million
numbers total without one being written, that the binder refuses a
string and a `Hello.Counter`, and that a disposed series is refused. See
[How To Pass Native Data Between Cmdlets](../how-to/How-To-Pass-Native-Data.md).

## Test-RustSeriesHold: the object's gate

`Test-RustSeriesHold` borrows a series through `with`, or through
`with_mut` with `-Exclusive`, while a script block runs. Under the
shared borrow a property read on the same series from that script goes
through, since shared entries nest. Under the exclusive borrow a method
call from the script is refused with the in-use message, since it
would reach the value while the cmdlet has the only reference, and a
property read there gives `$null`, which is what PowerShell's property
adapter returns when a getter throws. A `Dispose` from the script
frees the value once either kind of borrow ends, and the series reads
normally afterward. `Handles.Tests.ps1` checks all five.

## Wait-RustSilence: a worker that sends nothing

`Wait-RustSilence` runs a worker through `stream_from_thread_until`
that sends `started` and then nothing for `-Seconds`, checking its
`StopSignal` every 10 ms; the cmdlet writes `finished` itself when the
time runs out. `Handles.Tests.ps1` stops it two ways, with
`Select-Object -First 1` downstream and with `PowerShell.Stop()` on a
child runspace, and checks that each returns in well under the minute
the worker would otherwise take, and that one second of silence ends
with both words.

## Wait-RustStop: a call woken by its stop waker

`Wait-RustStop` registers a waker with `ps.on_stop` that unparks its
own thread, writes `started`, and parks with no timeout until
`ps.stopping()` is true, so nothing polls the stop flag.
`Handles.Tests.ps1` stops it with `PowerShell.BeginStop()` on a child
runspace and checks that the stop completes within ten seconds and
leaves the pipeline `Stopped`.

## New-RustBallast, Set-RustBallast and Measure-RustPressure: native bytes reported to the garbage collector

`Hello.Ballast` declares `native_bytes = Ballast::claimed_bytes`, which
reports a figure it never allocates, and `Hello.QuietBallast` is the
same class without the report. `New-RustBallast` makes one, and
`Set-RustBallast` takes it by type, changes what it claims through
`with_mut`, and writes the same object on. `Measure-RustPressure` makes
`-Count` of one or the other, `-Interval` milliseconds apart, releases
each at once, and writes how many collections ran, of any generation and
full, and how many values were still alive after the finalizers had run.

`Handles.Tests.ps1` reads the figure the object has reported to the
garbage collector as it is made, changed and disposed, and runs 64 of each class claiming
256 MB, 20 ms apart: collections run for the reporting ones and leave
fewer of them alive than of the quiet ones.

## Invoke-HelloHelper, Start-HelloHelper and Stop-HelloHelper: a helper executable

`src/bin/hello-helper.rs` is a `[[bin]]` target that
`[package.metadata.pwrs] helpers` names, so `cargo pwrs build` builds it
with the library and ships it beside the library in
`runtimes/<rid>/native/`. `Invoke-HelloHelper` starts it from the path
`pwrs::helper_path("hello-helper")` returns and writes each line it
prints. `Start-HelloHelper` leaves one waiting on its input and writes
its process id, and `Stop-HelloHelper` closes that input, waits for the
helper to exit, and writes what it printed.

`Helper.Tests.ps1` checks that the helper ships beside the library, that
it runs from a copy staged for the session rather than from the module
folder, and that every run starts the same copy. It also checks that the
shipped file can be opened for writing with nothing shared while a
helper runs, and that a name the module does not ship, or one with a
folder in it, is refused. See
[How To Ship A Helper Executable](../how-to/How-To-Ship-A-Helper-Executable.md).

## Get-HelloNativeFile: a file shipped beside the library

`Get-HelloNativeFile -Name <file>` writes the path
`pwrs::native_file_path` returns for a file the module ships in
`runtimes/<rid>/native/`, the way `[package.metadata.pwrs] native-files`
puts one there: a copy staged for the session, named by its own file
name with its extension. Without `-Name` it gets the path of the
module's own library, the one file every build ships there.

`Helper.Tests.ps1` checks that the path is under the session's staging
folder and not the module folder, that the staged copy has the shipped
file's bytes, that a second call returns the same path, and that a file
the module does not ship, or a name with a folder in it, is refused.
See [How To Ship A Prebuilt Native File](../how-to/How-To-Ship-A-Prebuilt-Native-File.md).

## The export

```rust
pwrs::export_module! {
    name: "Hello",
    cmdlets: [GetGreeting, MeasureRustLine, GetPerson, /* ... */ SendRustEvent, GetRustApartment, NewRustByteStream, NewRustFailure, SetRustBytes, NewRustWide],
    classes: [Person, Counter, Note, /* ... */ Vector, Tally, ByteStream, Fraction, Descending, Failure, Wide],
    enums: [Signal],
    completers: [complete_color],
    transforms: [as_bytes],
    dynamic_params: [GetRustReading, GetRustBlindReading],
    on_load: make_registration,
    on_import: count_import,
    on_remove: count_remove,
    on_unload: release_registration,
}
```

Every cmdlet, class, enum, completer, transform and dynamic-parameter provider must be listed here; the position in each list is the id the generated C# and the Rust runtime both use. `Ink` is not listed: a `#[psenum(clr = ...)]` mirror declares no type, so it has no id.
