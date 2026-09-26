---
title: Hello Tour
weight: 2
---

`examples/hello/src/lib.rs` is one module that uses every mechanism PWRS offers, and `examples/hello/tests/*.Tests.ps1` pin each behavior in both hosts. This page walks it cmdlet by cmdlet. Build and run the tests yourself with:

```text
cargo run --profile test-fast -p cargo-pwrs -- pwrs test --release --manifest-dir examples/hello
```

Each section names the test file that covers it. All 299 tests pass in pwsh 7.6 and in Windows PowerShell 5.1 on Windows, in pwsh 7.4, 7.5 and 7.6 on Linux from one build, and in pwsh 7.5 on FreeBSD; [`docs/PLATFORMS.md`](https://github.com/Variably-Constant/PWRS/blob/main/docs/PLATFORMS.md) records each, and macOS as of 0.2.0.

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
    pub panic: bool,
}
```

- `count: Option<i64>` is an optional parameter; `None` when `-Count` was not given. `validate_range` becomes `[ValidateRange(1L, 1000000000L)]`, so the engine rejects `-Count 0` before Rust runs.
- `fail: bool` and `panic: bool` are `SwitchParameter`s.
- `process` writes to the verbose stream, then either returns `Err(PsError::new(ErrorCategory::InvalidData, "GreetingRefused", ...))`, panics, or loops writing greetings while `ps.stopping()` is false.

What the tests in `Hello.Tests.ps1` show:

- A non-terminating error is written and the cmdlet continues: `-ErrorAction SilentlyContinue -ErrorVariable err` leaves one record whose `FullyQualifiedErrorId` matches `GreetingRefused` and whose category is `InvalidData`.
- `-ErrorAction Stop` turns the same error terminating.
- A Rust panic is reported as a terminating error and the module keeps working afterwards.
- `Get-Greeting -Name x -Count 1000000000 | Select-Object -First 2` returns in well under five seconds: the downstream `Select-Object` stops the pipeline, the next `WriteObject` throws `PipelineStoppedException` on the managed side, the vtable entry reports status 5, the Rust `write` returns a terminating `OperationStopped` error that `?` propagates out of `process`, and the managed side rethrows the stop to the engine. `ps.stopping()` covers the other path, `StopProcessing`, which the engine calls for Ctrl+C and runspace stops.
- `Get-Help Get-Greeting` has the synopsis from the doc comment and the parameter description from the field's doc comment.

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
#[derive(Clone)]
pub struct Counter {
    pub label: String,
    pub value: i64,
}
```

The Rust value stays in Rust behind the object; each property read is one native call. `Dispose()` frees it exactly once, `IsDisposed` reports the state, a property read after disposal returns `$null` to PowerShell (the getter throws `ObjectDisposedException`, which the adapter swallows), and creating and disposing 2000 proxies in a loop is part of the test.

```rust
#[psmethods]
impl Counter {
    pub fn advance(&mut self, by: i64) -> PsResult<i64> { /* checked add, pushes to history */ }
    pub fn describe(&self, prefix: Option<String>) -> PsResult<String> { /* "prefix=value" */ }
    pub fn reset(&mut self) -> PsResult<()> { /* value 0, history cleared */ }
}
```

`Methods.Tests.ps1` calls them from script: `$c.Advance(5)` returns an `Int64` and the `Value` and `History` properties show the change; `$c.Describe()` and `$c.Describe('n')` show an optional argument omitted and supplied; `$c.Reset()` returns nothing; an overflowing `Advance` throws the Rust error; a call after `Dispose()` throws; `Get-Member` lists the methods; `$c.Split('part')` returns a second `Hello.Counter` proxy holding half the value; `$a.Absorb($b)` takes another counter as an argument; `$c.SameAs($c)`, a `&self` method, takes its own receiver by value, since the argument's reads nest inside the shared call; and `$c.Absorb($c)`, whose `absorb` takes `&mut self`, is refused as in use, since an exclusive call admits no read of its object.

Three of the block's functions take no receiver. `new` is the
constructor, so `[Hello.Counter]::new('made', 5)` makes a counter
without a cmdlet, and refuses a label holding `=`, which `Parse`
could not read back; `parse` is a static that returns the class, so
`[Hello.Counter]::Parse('label=7')` makes one from the text
`Describe` writes; and `limit` is a static returning a number. A
static runs against no object, and one that returns the class returns
a new proxy holding the value it made. `Statics.Tests.ps1` makes,
calls, disposes and parses through the type on both hosts, checks
that a static is on the type and not on an instance, and makes a
counter whose constructor refuses in a child host, then collects it
and runs the finalizers, which leave the host running.

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

A copied object is its fields, with no Rust value behind it, so its
property reads never cross the boundary. That is why a value type is
copied rather than proxied, and it is also why a copied class takes
statics but not methods with a receiver: there is nothing to run one
against. `new` builds the stretch in Rust and the new object is made
from it field by field.

Two details are the point of this class. Its `Default` has a length of
60, not zero, and every argument of `new` is optional, so
`[Hello.Stretch]::new()` gives length 60: it starts from `Default`,
where a class without `new` would give CLR zeros. And because it
declares a `new`, that is the only public constructor it has, so no
script can make a stretch the class never meant to exist. Rust has no
overloading, which is why one `new` with `Option` arguments answers
`::new()`, `::new(5)` and `::new(10, 30)` alike.

`CopiedConstructors.Tests.ps1` checks each arity, the refusal of a
negative length as an exception, a static building through the
factory, that `Hello.Stretch` exposes exactly one public constructor,
and that `Hello.Person`, which declares none, still has its public
parameterless one.

## New-RustSlots: methods named like a collection's

`Hello.Slots` declares `Get`, `Set`, `Contains` and `Call`, the names a slot map wants and the names `Pwrs.ProxyBase` uses for its own protected members. `Methods.Tests.ps1` reads `Origin` and `Capacity` (the property getters, which reach the base class as `base.PwrsGet`), runs each method, disposes the object, and checks a Rust error still surfaces.

## New-RustTicker: a proxy with Rust-only state

`Hello.Ticker` keeps `ticks: u64` under `#[psfield(skip)]`: `Get-Member` shows `Label`, `Width`, `Step` and `Limit` but no `Ticks`, and `$t.Tick()` counts in Rust across calls. The three other fields are the narrow numbers, a `u32`, an `f32` and an `Option<u16>`, whose property getters convert out of the box the engine put them in rather than unboxing them as their own width. `Classes.Tests.ps1` checks each.

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

A `PsHashtable` parameter binds as `[hashtable]`, so the engine copies any other dictionary into one first. `Get-RustTableEntry` takes its table as a `PsObject` and wraps it with `PsHashtable::from_ps`, so a `Hashtable`, an ordered dictionary and a generic dictionary reach `contains` and `get` as they were passed; `Conversions.Tests.ps1` reads a held key from each and a missing key as `$null`, which is what `get` answers for any table.

## Get-RustChecksum and Get-RustBytes: zero-copy arrays

`self.bytes.pin::<u8>()` borrows a `byte[]` parameter as a `&[u8]` without copying; the pin is released when the borrow drops. `PsObject::from_slice(&data)` creates a `byte[]` and fills it through one pin. The tests checksum `[byte[]](1..255)` and read back `Get-RustBytes -Count 5` as a `Byte[]` of length 5.

## Get-RustByteSum and Get-RustByteRange: byte arrays as Vec<u8>

A `Vec<u8>` parameter binds a `byte[]` (or any array of numbers), and `PsArray(Vec<u8>)` writes one `byte[]`. A typed array crosses through one pin in each direction; an untyped one falls back to reading element by element. `Bytes.Tests.ps1` sums typed and untyped arrays, writes 0, 300 and 4194304 bytes, and round-trips the four megabytes.

`Get-RustRawByteSum` is the same cmdlet with `#[param(raw)]`, which declares the parameter `object` so the engine hands the array over instead of coercing it: the tests check it agrees with the typed one on four megabytes, and show what it gives up, a string enumerating as characters where the typed parameter refuses it.

`Measure-RustInput` goes the other way. Its `-InputObject` is a `PsObject` declared `byte[]` with `#[param(clr = "byte[]")]`, beside a `-LiteralPath` in a set of its own, the shape a compressor takes input in. The binder coerces to `byte[]`, so a byte array piped with `,` arrives whole, an enumerated one arrives a byte at a time as arrays of one, and a piped `FileInfo` goes to `-LiteralPath` by its `PSPath`. The bytes are read through `pin`, and `-Invert` flips them where they lie. `-InputObject` also carries `allow_empty_collection`, so an empty array is measured rather than refused by the binder. `Bytes.Tests.ps1` checks the declared type, each binding, that `-Invert` changes the caller's own array, which is what shows nothing was copied, and that an empty array binds here while `Get-RustByteSum`, without the flag, refuses one.

## Get-RustMemory: Rust memory as Memory<byte>

`PsMemory::<u8>::zeroed(count)` allocates once, the loop fills it through `&mut [u8]`, and `ps.write(m)` hands the allocation to the engine. `Memory.Tests.ps1` reads a `` Memory`1 `` with `.Length` and `.ToArray()` on pwsh and a `Byte[]` on Windows PowerShell, for 5, 0 and 4194304 bytes.

## Test-RustBigInt: BigInteger

`PsBigInt::from_ps` reads a `System.Numerics.BigInteger` as its little-endian two's-complement bytes (through `ToByteArray` and a pin), `doubled()` shifts them, and `IntoPs` constructs a new `BigInteger` from the bytes. Positive and negative values round-trip.

## Test-RustDynamic: dynamic .NET access

Without `-Uri`: `"shout".into_ps()?.call("ToUpper", &[])` runs an instance method through PowerShell's member binder, and `PsType::from_name("System.Math").call_static("Abs", ...)` runs a static through the CLR binder. With `-Uri`: `PsType::from_name("System.Uri").new(&[uri])` constructs an object and `.get("Host")` reads a property.

## Resolve-RustPath: PSPath resolution

`ps.resolve_path(&self.path, false)` resolves a PSPath through the session's providers, expanding wildcards; `true` asks for the literal unresolved provider path. The tests run against `$TestDrive`.

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

`Completers.Tests.ps1` drives `TabExpansion2` on `Get-RustColor -Name cr` and expects `crimson` but not `blue`; an empty word offers all five.

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

`Get-RustStaticReading` and `Get-RustBlindReading` are `Get-RustReading` with the same parameter and the same `process`, the first with no hook and the second with a hook that adds nothing and never reads what is bound. They exist to be timed against it: `benches/dynamic_params.ps1` reads what dynamic parameters cost a call, which part of that is PowerShell's own pass and which is PWRS's, and [Benchmarks](../reference/Benchmarks.md) carries what it measured. `Completers.Tests.ps1` checks that both write what `Get-RustReading` writes, that the static one implements no `IDynamicParameters`, and that neither offers `-Unit`.

## Measure-RustTotal: begin, process, end

```rust
impl Cmdlet for MeasureRustTotal {
    fn begin(&mut self, ps: &Pipeline<'_>) -> PsResult<()> { self.total = 0; ps.verbose("begin") }
    fn process(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> { self.total += self.value; Ok(()) }
    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> { ps.write(self.total) }
}
```

`total: i64` has no `#[param]`, so it is plain state on the instance, which lives from `BeginProcessing` to `Dispose`. `Phases.Tests.ps1` runs `1..4 | Measure-RustTotal` five times and checks the verbose `begin` record and the total on every run: an implemented `begin` or `end` is called on every instance, while a cmdlet that implements only `process` (such as `Get-Greeting`) stops paying for the other two phases after its first instance. See [The Call Path](The-Call-Path.md).

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

`Hello.Signal` is a CLR enum with `long` underneath and members `Red = 0`, `Amber = 5`, `Green = 6`. `Enums.Tests.ps1` shows the binder converting `-Signal Green` from its name, rejecting `Purple`, binding `Option<Signal>` and `Vec<Signal>`, completing members through `TabExpansion2`, and `ConvertTo-RustSignal -Value 5` returning a value whose type is `Hello.Signal`, whose string form is `Amber`, and whose `[long]` cast is `5`. `New-RustLight` carries the enum on a copied class field, including an `Option<Signal>` that reads as `$null` when absent.

## Get-RustUnsigned: unsigned 64-bit

`u64` parameters and outputs cross without narrowing: `Get-RustUnsigned -Value ([uint64]::MaxValue)` returns a `UInt64` equal to it, and a `Vec<u64>` parameter sums to a `UInt64`.

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

`New-RustStamp -At $date -Took $span -Id $guid -Label rust -History $g1, $g2 -Signals Red, Green` binds each typed parameter through the engine (a GUID from its string form, a `char` from a one-character string) and writes a `Hello.Stamp` whose properties are a `DateTime` with the given kind, its `ToUniversalTime()` from `to_utc()`, a `TimeSpan`, a `Guid`, a `Char`, a `Guid[]` and a `Signal[]`. `Add-RustTime` shifts a `DateTime` by a `TimeSpan` keeping its kind and writes the negated span. `Test-RustValues` writes a GUID's text form, the GUID itself and a `char`. `Test-RustCredential` reveals a `PSCredential`'s password (`ada:7`), builds a new credential in Rust with `PsCredential::new`, and reveals a `SecureString` parameter. `Values.Tests.ps1` covers all four in both hosts.

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

## Get-RustStream and Test-RustTarget: the worker thread and a targeted error

`Get-RustStream` reports a progress record, hands a worker thread a channel through `ps.stream_from_thread`, and writes every value the worker sends, in order. `Test-RustTarget` reads its argument's display string with `pwrs::types::display_string` and refuses it with an error carrying that object as the record's target.

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

`Get-RustParallel` squares its input on a pool and writes the results from the pipeline thread; `-AsReady` writes each as its worker finishes instead of in input order. `Measure-RustParallel` runs the same pool through `ps.par_for_each`, reaches a shared `AtomicI64` from the workers, and writes the total once. Neither closure can capture `ps`, because the pipeline token is `!Send`.

`Parallel.Tests.ps1` checks that input order is the order given, that as-ready writes the same set with nothing dropped or repeated, that one item is not a special case, and that the total is right at five widths. The empty input and a worker panic are covered by the Rust tests in `crates/pwrs/src/convert_tests.rs`. See [How To Use Threads](../how-to/How-To-Use-Threads.md).

## Get-RustProperty: reading a property bag from Rust

`Get-RustProperty` takes a `PSObject` from the pipeline and reads one
property off it with `pwrs::object::property`, which is the read side
of `add_note`. It sees note properties, so a `[pscustomobject]` built
in script and an object a Rust cmdlet wrote both answer. A name the
object does not carry is an error rather than a null, so a misspelling
does not read as missing data.

`Hello.Tests.ps1` reads a string and a number off a `[pscustomobject]`,
reads a property off the object `Get-Note` writes, and checks that an
absent name throws.

## Get-RustWidths and Get-RustTypeTag: the type a value arrives as

`Get-RustWidths` writes one value of each CLR width, so a caller can
read back what the engine gave each one: `sbyte`, `short`, `int`,
`byte`, `ushort`, `uint`, `float`, `long`, `double`. They arrive as
those types rather than widened to `Int64` and `Double`, which
matters because the engine types every operator's answer by its
operands' widths.

`Get-RustTypeTag` writes the tag PWRS gives one object's type, which
`PsObject::type_tag()` answers in a single crossing. A type outside
the vocabulary answers `0`, and the tag sees through the `PSObject`
the engine wraps a value in.

`Widths.Tests.ps1` asserts the nine type names in order, that a
`Single` is not a widened `Double`, that an `Int32` plus one is still
an `Int32`, and that a `PSCustomObject` answers `0` rather than a
guess.

## Get-RustDecimalRoundTrip and Measure-RustDecimalBlock: two orders for four words

`Get-RustDecimalRoundTrip` reads a decimal into the four words
`Decimal.GetBits` answers and builds a new one from them, so the
round trip is observable; `-Scale` writes the scale instead.
`Measure-RustDecimalBlock` sums a `Decimal[]` by pinning it, which
reads the whole array as one block, and refuses a block whose
elements do not share one scale rather than adding words that do not
line up.

The two types are the same four words in the two orders that exist:
`PsDecimal` is `GetBits` order and `PsDecimalBits` is memory order,
which is what a pinned element is. The first decimal pin in a process
proves that order against `GetBits` before any element is read.

`Widths.Tests.ps1` round-trips both extremes and a negative, checks
that a trailing zero survives as scale, and sums a pinned block. It
also records that a string cast does not agree across hosts: pwsh 7
makes `[decimal]'1.10'` scale 1 and Windows PowerShell 5.1 makes it
2, so the test asks the host what it produced rather than writing
either answer in.

## Get-RustOffset: an instant that keeps its meaning

`Get-RustOffset` round-trips a `DateTimeOffset`, or writes its offset
in whole minutes, or the same instant as ticks on the UTC clock.
`PsDateTime`'s kind says only which clock a value belongs to, so an
offset is what carries an instant away from the host that produced
it.

## Measure-RustPropertyReads: what a read costs from this side

`Measure-RustPropertyReads` reads one property from every object it is
given, a given number of passes over, and writes how many reads that
was. Nothing else happens in the loop, so the call's wall time over
that count is the cost of one read: the invocation is paid once and
divides away. `benches/dynamic_reads.ps1` drives it against the two
reads PowerShell has for the same objects, and
[Benchmarks](../reference/Benchmarks.md) carries what that measured.

`Measure-RustTypeReads` has the same shape for the other thing code
dispatching on unknown input does per item: it asks every object its
type, through `type_tag` or, with `-ByName`, through `type_name`, and
writes the count. The same harness drives both routes against
`$o.GetType().FullName` in script.

## Get-RustReadOnlyTable: a table script can read but not write

`Get-RustReadOnlyTable` wraps a table in `PsReadOnlyTable::over`, which
holds the source rather than copying it. Both `$t.key` and `$t['key']`
read, and neither writes: the type carries the non-generic
`IDictionary` publicly, because that is the interface the engine's
adapter needs before it will expose a key as a property, and it refuses
every mutating member instead. A nested table comes back wrapped too,
from the indexer, from `Values` and from enumerating, so the refusal
reaches all the way down. `-Ordered` builds it over an
`OrderedDictionary`, which shows that the order is the source's and not
a copy's.

`ReadOnlyTable.Tests.ps1` asserts a value for every read rather than
merely that it did not throw, since a wrapper that quietly answers
`$null` for one access form is the failure worth catching, and judges
each refused write by reading the value back afterwards.

## Write-RustStreams: a message that is never built

`Write-RustStreams` writes one record to each of the four message
streams through `pwrs::verbose!`, `debug!`, `warning!` and
`information!`. Those ask the engine whether it would keep the record
and build the text only if it would. The plain `ps.verbose(&str)` form
cannot: its caller has already run the format and the allocation by the
time the call is made, and the engine decides after the crossing.

`Hello.Tests.ps1` checks each stream twice, once with the stream off
and once on, and that `-Verbose:$false` beats a `$VerbosePreference`
of `Continue` the way the common parameter is supposed to.

## Remove-RustThing, Get-RustRecord and Get-RustRoute: what the binder is told

`Remove-RustThing` carries `supports_should_process` and `confirm_impact`, so the engine gives it `-WhatIf` and `-Confirm` and `ps.should_process` decides whether the change happens. `Get-RustRecord` carries the rest of what a `#[param]` can say: two parameter sets with `default_parameter_set`, a parameter bound from a piped object's property, one that sweeps up the remaining arguments, a pattern and a not-null validator, and one hidden with `dont_show`. `Get-RustRoute` has three sets, `Path`, `LiteralPath` and `Text`, and a `-Destination` declared with `set = ["Path", "LiteralPath"]`, so it binds beside either file parameter and is refused beside `-Text`.

`Binding.Tests.ps1` pins each in both hosts, including that `-WhatIf` performs nothing, that naming both sets at once is refused, and that the pattern validator is case-insensitive because the engine applies it that way. For `Get-RustRoute` it checks the sets `Get-Command` reports for `-Destination`, the binding in each set, the `AmbiguousParameterSet` refusal beside `-Text`, and that help writes one syntax line per set naming only that set's parameters.

## Expand-RustText and Get-RustModuleName: help and aliases

`Expand-RustText`'s doc comment writes its synopsis across two source lines and follows it with a blank line and a description paragraph. `Hello.Tests.ps1` asserts `Get-Help` shows the whole first paragraph joined into one sentence as the synopsis, and the paragraph after it as the description.

`Get-RustModuleName` is a unit struct with no parameters, so the generated cmdlet carries no parameter block at all, and `alias = ["grmn"]` puts an alias on it. `Hybrid.Tests.ps1` checks that `grmn` runs and that it appears in `(Get-Module Hello).ExportedAliases`, which the root module's `Export-ModuleMember` is what carries out of the nested binary module.

## Get-RustHybrid: hand-written C#

`src/csharp/GetRustHybrid.cs` is a plain `PSCmdlet` compiled into the shell assembly. It declares the cmdlet alias `grhyb` and gives its `-Name` parameter the alias `n`. `GetRustHybridInfo.cs` beside it declares the same two attributes in the other forms the scanner accepts: the alias ahead of the cmdlet attribute, the cmdlet attribute fully qualified, and the alias names as an array. `Hybrid.Tests.ps1` runs both, checks that the manifest and the module export each cmdlet and alias beside the Rust ones, that a parameter's alias is not exported as a cmdlet alias, and that the two appear in the manifest in file-name order rather than in whatever order the directory was listed.

## Read-RustHost: asking the person at the console

```rust
let ui = ps.host_ui()?;
match self.kind.as_str() {
    "Line" => ps.write(ui.read_line()?),
    "Secure" => ps.write(ui.read_line_as_secure_string()?.len()? as i64),
    "Choice" => ps.write(ui.prompt_for_choice("Choose", "Which one?", &choices, 0)? as i64),
    ...
}
```

`Read-RustHost` asks through the host's own prompts and writes the
answer: a line, the length of a line typed without echo, or the index
of the choice taken among `-Choices`, with `-Say` putting text on the
host's output first. That output is the host's and not the pipeline's,
so nothing downstream sees it.

`HostUi.Tests.ps1` runs in a host that is `-NonInteractive` and
cannot answer, so its prompting cases run the cmdlet in a runspace
whose host answers from a queue and keeps what was written to it: a
`PSHost` of a few dozen lines, compiled with `Add-Type`, that the
cmdlet reaches through the same `$Host.UI` a console hands it. A
console host cannot stand in for it: its `ReadLineAsSecureString`
reads keys from the console device, which redirected input does not
reach, so a child shell fed through standard input waits on that
case forever. The host that cannot answer is its own case, and
refuses with the engine's error rather than hanging.

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
over the numbers `-Sort` pipes in. The name is resolved to the
command and run in a nested pipeline, so no script block is built and
nothing is parsed. What the command raises is handled as if the user
had run it: a non-terminating error reaches this cmdlet's error
stream and the output still comes back, so `-ErrorAction` on
`Get-RustComposed` decides what it means; a terminating one is the
`Err`.

`Invoke.Tests.ps1` checks a module cmdlet against its own output, an
engine cmdlet's `Get-Location` against the session's, `3, 1, 2`
coming back sorted, a name the session cannot see throwing, and a
`Get-Item` on an absent path producing an error record here and no
exception.

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

`ps.invocation()` hands back the cmdlet's own `MyInvocation`, the
engine's `InvocationInfo`, so a cmdlet can see where it stands before
any input reaches it. `Get-RustInvocation` reads its place in `begin`,
passes its input through, and writes the place at `end`, so
`Get-RustInvocation | Get-RustInvocation | Get-RustInvocation` reports
`1/3`, `2/3` and `3/3`. That is how a set of cooperating cmdlets can
tell that their neighbors are their own.

`PipelineLength` counts commands, so piping to anything, `Should`
included, counts that command too, and an expression at the head of a
pipeline is input rather than a command and is not counted. Begin
blocks run left to right, but the engine processes a command's queued
input right after that command's own begin, so when an upstream begin
writes output, a command can process it before a later command has
begun. Cmdlets that coordinate through their places should not act in
`process` until all of them are known.

`PipelinePosition.Tests.ps1` checks a lone call, the command it is
piped into being counted, every position of a three-stage chain,
other commands being counted, and an expression at the head not being
counted.

## Get-RustSize: changing an argument before the binder sees it

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
transform turned the string into a number first. That is the whole
point of the attribute: it runs before the engine coerces the
argument to the parameter's declared type and before validation, so
it can accept shapes the type itself cannot. A value it does not
recognize is handed back untouched, and the binder coerces or
refuses it as it would have anyway.

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

The four variants are a deliberate subset. `Green` is a
`ConsoleColor`, so the binder accepts it and `FromPs` then fails,
because the Rust enum has no variant with that value: a mirror can
be narrower than what it mirrors, and says so at the boundary rather
than guessing.

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
category comes back as the `ErrorCategory` enum, so a cmdlet deciding
whether to retry matches on `ObjectNotFound` rather than on the words
in the message.

`ErrorRecord.Tests.ps1` uses the engine's own record, from a
`Get-Item` on a path that is not there, so the category, the id and
the target are the ones every PowerShell user has seen. It checks
that record caught in script and the same record handed down the
pipeline, a record a Rust cmdlet raised, and an object that is not a
record at all, which fails rather than reading as an empty one.

## Get-RustLifecycle: what runs at import and at removal

```rust
#[on_import]
fn count_import() -> PsResult<()> {
    IMPORTS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[on_remove]
fn count_remove() -> PsResult<()> {
    REMOVES.fetch_add(1, Ordering::Relaxed);
    Ok(())
}
```

Named under `on_import` and `on_remove` in `export_module!`, these
run when the module is imported and when it is removed, with no
cmdlet and no pipeline. `Get-RustLifecycle` writes the import count,
or with `-Removes` the removal count. The counts live in the native
library, which a removal does not unload, so they carry across a
`Remove-Module` and the import that follows it. That is what makes
the removal hook observable at all: once the module is removed its
cmdlets are gone, and only the next import can report what ran.

`Lifecycle.Tests.ps1` reads the counts, removes the module, imports
it again, and checks that each count rose by one; and that the import
count is always exactly one more than the removal count, since every
removal in a session was preceded by an import.

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
`Memory.Tests.ps1` asks for half the address space, which the
allocator refuses, and for more than a vector can hold, which is
refused before the allocator is asked; each is one error record with
no output, and the next reservation in the session succeeds.

## Test-RustOffThread: the call a worker must not make

`Test-RustOffThread` clones its `PsObject` parameter into a thread it
starts and calls `type_name` there. `PsObject` is `Send`, so the
compiler allows it, but the call would attach the thread to the .NET
runtime and run the engine's member binder where no runspace belongs.
While the thread check is on, in a debug build and under `cargo pwrs
test`, which sets `PWRS_THREAD_CHECK=1`, the call returns
`PwrsOffThread` instead. `Parallel.Tests.ps1` checks the refusal under
the check and the name without it, and that the same call on the thread
the cmdlet runs on is untouched. See
[How To Use Threads](../how-to/How-To-Use-Threads.md).

## Get-RustCpu and Measure-RustTieredSum: instruction sets

`Get-RustCpu` writes one `Hello.CpuFeature` per x86-64 extension
`pwrs::cpu` knows: whether the library was compiled for it, whether the
CPU and operating system offer it, and whether a kernel may use it once
`PWRS_CPU_MAX` is applied. `Measure-RustTieredSum` pins a `Double[]`,
sums it through the widest kernel `pwrs::cpu::has` allows, AVX-512, AVX2
or scalar, and writes the tier and the sum; every tier adds in the same
order, so all three give the same bits.

`Cpu.Tests.ps1` checks that everything compiled in is offered where the
module was imported, then starts a child process of the same host under
a cap: `x86-64` refuses an import of a library compiled for more and
names what the cap withholds, `x86-64-v3` caps `Get-RustCpu`'s answers,
and an unknown level is refused. The sum is checked bit for bit against
the same eight-lane order computed in script. See
[How To Use Instruction Sets](../how-to/How-To-Use-Instruction-Sets.md).

## New-RustSeries and its stages: one object for a whole series

```rust
#[param(mandatory, position = 0, value_from_pipeline)]
pub series: PsProxy<Series>,
```

A `Hello.Series` is a proxy holding a count and a plan, with the plan's
steps in a skipped field, so it cannot be read back by value.
`New-RustSeries` makes one. `Add-RustSeriesStep` takes it by type,
copies it out with `self.series.with(Series::clone)`, appends `-Scale`,
`-Shift` or `-Above`, and writes the copy as a new series, leaving its
input as it was. `Measure-RustSeries` counts and sums the numbers in one
pass inside the borrow, and `Expand-RustSeries` writes them as rows,
from a copy, so nothing is held while commands downstream run. No
number exists until one of those two reads the series.

`Handles.Tests.ps1` checks that the stages pass one object, that a step
applies in the order it was added, that a stage leaves its input alone,
that the object goes through `Where-Object`, `ForEach-Object`, a
variable, a named argument and an array unchanged, that a million
numbers total without one being written, that the binder refuses a
string and a `Hello.Counter`, and that a disposed series is refused. See
[How To Pass Native Data Between Cmdlets](../how-to/How-To-Pass-Native-Data.md).

## Test-RustSeriesHold: the object's gate

`Test-RustSeriesHold` holds a series through `with`, or through
`with_mut` with `-Exclusive`, while a script block runs. Under the
shared hold a property read on the same series from that script goes
through, since shared entries nest. Under the exclusive hold a method
call from the script is refused with the in-use message, since it
would reach the value while the cmdlet holds the only reference, and a
property read there gives `$null`, which is what PowerShell's property
adapter makes of any getter's exception. A `Dispose` from the script
frees the value once either kind of hold ends, and the series reads
normally afterwards. `Handles.Tests.ps1` checks all five.

## Wait-RustSilence: a worker that sends nothing

`Wait-RustSilence` runs a worker through `stream_from_thread_until`
that sends `started` and then nothing for `-Seconds`, checking its
`StopSignal` every 10 ms; the cmdlet writes `finished` itself when the
time runs out. `Handles.Tests.ps1` stops it two ways, with
`Select-Object -First 1` downstream and with `PowerShell.Stop()` on a
child runspace, and checks that each returns in well under the minute
the worker would otherwise take, and that one second of silence ends
with both words.

## New-RustBallast, Set-RustBallast and Measure-RustPressure: native bytes the collector is told about

`Hello.Ballast` declares `native_bytes = Ballast::claimed_bytes`, which
reports a figure it never allocates, and `Hello.QuietBallast` is the
same class without the report. `New-RustBallast` makes one, and
`Set-RustBallast` takes it by type, changes what it claims through
`with_mut`, and writes the same object on. `Measure-RustPressure` makes
`-Count` of one or the other, `-Interval` milliseconds apart, lets go of
each at once, and writes how many collections ran, of any generation and
full, and how many values were still alive after the finalizers had run.

`Handles.Tests.ps1` reads the figure the object holds with the collector
as it is made, changed and disposed, and runs 64 of each class claiming
256 MB, 20 ms apart: collections run for the reporting ones and leave
fewer of them alive than of the quiet ones.

## Invoke-HelloHelper, Start-HelloHelper and Stop-HelloHelper: a helper executable

`src/bin/hello-helper.rs` is a `[[bin]]` target that
`[package.metadata.pwrs] helpers` names, so `cargo pwrs build` builds it
with the library and ships it beside the library in
`runtimes/<rid>/native/`. `Invoke-HelloHelper` starts it from the path
`pwrs::helper_path("hello-helper")` answers and writes each line it
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

## The export

```rust
pwrs::export_module! {
    name: "Hello",
    cmdlets: [GetGreeting, GetPerson, /* ... */ NewRustBallast, SetRustBallast, InvokeHelloHelper, StartHelloHelper, StopHelloHelper],
    classes: [
        Person, Counter, Note, TableInfo, Light, Stamp, Team, Ticker, Slots, Stretch, CpuFeature,
        Series, SeriesTotal, Ballast, QuietBallast, PressureReport,
    ],
    enums: [Signal],
    completers: [complete_color],
    transforms: [as_bytes],
    dynamic_params: [GetRustReading, GetRustBlindReading],
    on_import: count_import,
    on_remove: count_remove,
}
```

Every cmdlet, class, enum, completer, transform and dynamic-parameter provider must be listed here; the position in each list is the id the generated C# and the Rust runtime agree on. `Ink` is absent on purpose: a `#[psenum(clr = ...)]` mirror declares no type, so there is nothing to give an id to.
