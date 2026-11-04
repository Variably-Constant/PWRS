---
title: How To Port A Crate
weight: 2
---

A Rust crate that already does the work, such as a parser, a codec or a client for a service, can become a PowerShell module without changing it. A small separate crate depends on it and turns what it does into cmdlets. The worked example is `examples/toml`, which ports the [toml](https://crates.io/crates/toml) crate as two cmdlets PowerShell does not have: `ConvertFrom-Toml` reads TOML text into objects, and `ConvertTo-Toml` writes objects as TOML text. Source: `examples/toml/src/lib.rs`, and its tests in `examples/toml/tests/Toml.Tests.ps1`.

## The module crate

The module is a separate library crate, made with `cargo pwrs new` as in [Getting Started](../tutorials/Getting-Started.md). It depends on `PoWerRuSt` and on the crate it ports. The ported crate does not change, so it still updates from crates.io and still serves other Rust code:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
pwrs = { package = "PoWerRuSt", version = "1.1.0" }
toml = { version = "1.1.8", features = ["preserve_order"] }
```

The toml crate sorts a table's keys unless `preserve_order` is on. With it on, a table's keys stay in the order of the text, which is the order PowerShell users expect for an object's properties.

## Cmdlets for what the crate does

Each task the crate performs that a script would use becomes a cmdlet, named with an approved verb. The toml crate reads text into values and writes values as text, as PowerShell's `ConvertFrom-Json` and `ConvertTo-Json` do for JSON. So the cmdlets take those names, and take their input the way those do.

The crate reads a whole document at once, while `Get-Content` writes a file one line at a time. So `process` collects each string the cmdlet is given, and `end` reads them as one document:

```rust
#[cmdlet(verb = "ConvertFrom", noun = "Toml", output = ["System.Management.Automation.PSCustomObject", "System.Collections.Specialized.OrderedDictionary"])]
#[derive(Default)]
pub struct ConvertFromToml {
    /// TOML text, or one line of it.
    #[param(mandatory, position = 0, value_from_pipeline, allow_empty_string)]
    pub input_object: String,
    /// Returns each table as an ordered hashtable rather than a
    /// PSCustomObject. ...
    #[param]
    pub as_hashtable: bool,
    /// The lines given so far, read as one document at the end.
    lines: Vec<String>,
}

impl Cmdlet for ConvertFromToml {
    fn process(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> {
        self.lines.push(std::mem::take(&mut self.input_object));
        Ok(())
    }

    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.lines.is_empty() {
            return Ok(());
        }
        let text = self.lines.join("\n");
        let table: Table = text.parse().map_err(|e| parse_error(&text, &e))?;
        let reader = Reader { as_hashtable: self.as_hashtable };
        ps.write(reader.table(table, "").map_err(PsError::terminating)?)
    }
}
```

`allow_empty_string` lets a file's blank lines bind, which a mandatory string parameter otherwise refuses. `ConvertTo-Toml` does not need this: each object it is given becomes its own document, so it does all its work in `process`.

```powershell
$config = Get-Content demo.toml | ConvertFrom-Toml
$config.server
```

```text
name  ports
----  -----
alpha {8000, 8001}
beta  {8002}
```

## Values out of the crate

Each value the crate returns becomes the .NET value a script would use for it:

| TOML | PowerShell |
|---|---|
| table | a PSCustomObject, or an ordered hashtable under `-AsHashtable` |
| array | `object[]` |
| string | `String` |
| integer | `Int64` |
| float | `Double` |
| Boolean | `Boolean` |
| offset date-time | `DateTimeOffset` |
| local date-time | `DateTime`, its `Kind` `Unspecified` |
| local date | `DateTime` at midnight |
| local time | `TimeSpan` |

Scalars convert with `into_ps`. A table has no fixed shape, so the module builds it at run time with `new_psobject` and `add_note`, as [How To Call .NET From Rust](How-To-Call-DotNet-From-Rust.md#building-psobjects-by-hand) shows. `new_psobject` makes a plain PSCustomObject when it is given an empty name. A type with a fixed set of fields is better declared as a `#[psclass]`, as in [How To Return Objects](How-To-Return-Objects.md). An array is a `Vec<PsObject>`, which `into_ps` turns into an `object[]`:

```rust
fn value(&self, value: Value, path: &str) -> PsResult<PsObject> {
    match value {
        Value::String(text) => text.into_ps(),
        Value::Integer(number) => number.into_ps(),
        Value::Float(number) => number.into_ps(),
        Value::Boolean(flag) => flag.into_ps(),
        Value::Datetime(datetime) => datetime_object(&datetime, path),
        Value::Array(items) => {
            let mut objects = Vec::with_capacity(items.len());
            for (index, item) in items.into_iter().enumerate() {
                objects.push(self.value(item, &format!("{path}[{index}]"))?);
            }
            objects.into_ps()
        }
        Value::Table(table) => self.table(table, path),
    }
}
```

Each call is given a path, such as `server[1].name`, which an error uses to name the value. A date or time becomes a count of ticks, 100 ns each, which is how `PsDateTime`, `PsDateTimeOffset` and `PsTimeSpan` store it. The module works out the count in Rust from the calendar date. Digits of a second past the seventh decimal place are dropped, as the TOML specification requires when a reader stores less precision.

PowerShell compares property names without regard to case, so a table whose keys differ only in case cannot become an object. `ConvertFrom-Toml` refuses such a table and names both keys. With `-AsHashtable` it reads the table, since an ordered hashtable made without a comparer treats keys that differ only in case as different keys:

```text
ConvertFrom-Toml: Name and name differ only in case, but property names must differ by more than case; use -AsHashtable
```

## Values into the crate

`ConvertTo-Toml` takes any object, so it first checks what kind of object it has. `PsObject::type_tag()` returns the type of a string, number, date or other scalar in one call into .NET. Only an object with no tag needs more calls:

```rust
fn value(&self, value: &PsObject, path: &str, level: i64) -> PsResult<Value> {
    Ok(match value.type_tag()? {
        PS_TYPE_STRING => Value::String(String::from_ps(value)?),
        PS_TYPE_CHAR => Value::String(char::from_ps(value)?.to_string()),
        PS_TYPE_BOOL => Value::Boolean(bool::from_ps(value)?),
        PS_TYPE_I8 | PS_TYPE_I16 | PS_TYPE_I32 | PS_TYPE_I64 | PS_TYPE_U8 | PS_TYPE_U16 | PS_TYPE_U32 => Value::Integer(i64::from_ps(value)?),
        // ... UInt64, Single, Double, Decimal, DateTime, TimeSpan, Guid
        _ => match kind_of(value)? {
            Kind::DateTimeOffset => offset_value(PsDateTimeOffset::from_ps(value)?),
            // ... BigInteger, enum
            Kind::Dictionary => Value::Table(self.dictionary(value, path, level)?),
            Kind::Collection => Value::Array(self.array(value, path, level)?),
            Kind::Properties => Value::Table(self.properties(value, path, level)?),
        },
    })
}
```

`kind_of` calls the object's `GetType()` and reads the type's name, whether it is an enum, and whether it implements `IDictionary` or `IEnumerable`. A dictionary's keys come from its `Keys` property, and `PsHashtable::get` reads each value. Any other object is read through the properties that `$object.PSObject.Properties` returns in script, for a PSCustomObject and a .NET object alike:

```rust
for property in Vec::<PsObject>::from_ps(&value.get("PSObject")?.get("Properties")?)? {
    let name = String::from_ps(&property.get("Name")?)?;
    self.add(&mut table, name, &property.get("Value")?, path, level)?;
}
```

So `ConvertTo-Toml` writes the objects `ConvertFrom-Toml` returns as the same TOML text:

```text
PS> $config | ConvertTo-Toml
title = "Inventory"

[owner]
name = "Ada"
since = 1979-05-27T07:32:00-07:00

[[server]]
name = "alpha"
ports = [8000, 8001]

[[server]]
name = "beta"
ports = [8002]
```

## Values with no counterpart

A ported crate's types and .NET's rarely match one for one. When a value has no counterpart on the other side, the module refuses it with an error naming the value's key. It never changes the value to a nearby one, and never leaves it out unless asked to.

`ConvertFrom-Toml` refuses three kinds of TOML value:

- a leap second, `23:59:60`, which .NET dates and times cannot store;
- a date in year 0, since a `DateTime` starts at year 1;
- an offset of more than 14 hours, or a time in UTC outside years 1 to 9999, which a `DateTimeOffset` does not allow.

`ConvertTo-Toml` refuses these PowerShell values:

- `$null`, since TOML has no null. With `-SkipNull`, a key whose value is `$null` is left out. A `$null` in an array is refused either way, since leaving it out would move the items after it;
- a dictionary key that is not a string;
- an integer outside the signed 64-bit range of a TOML integer, such as a `UInt64` above `[long]::MaxValue`;
- a `TimeSpan` below zero or of a day or more, since a TOML local time is a time of day;
- tables and arrays nested deeper than `-Depth`, which is 32 when it is not given. This also stops an object whose property refers back to the object itself.

```text
PS> [ordered]@{ name = 'demo'; port = $null } | ConvertTo-Toml
ConvertTo-Toml: port is $null, and TOML has no null; -SkipNull leaves such keys out
PS> [ordered]@{ name = 'demo'; port = $null } | ConvertTo-Toml -SkipNull
name = "demo"
```

`ConvertTo-Toml` writes a Single as the Double with the same shortest decimal form, so `[float]0.1` is written as `0.1` and not as `0.10000000149011612`. A Decimal becomes the nearest Double. A `DateTime` whose `Kind` is `Utc` is written as an offset date-time with `Z`, and every other `DateTime` as a local date-time. A local date read by `ConvertFrom-Toml` therefore goes back as a local date-time at midnight.

## Errors

The crate's error type becomes an error record: a category, an id a script can test for, and a message that says what is wrong and where. The toml crate reports where reading stopped as a byte offset, which the module turns into a line and a column. The crate's own report, which draws the line and marks the place, becomes the record's details:

```rust
fn parse_error(text: &str, error: &toml::de::Error) -> PsError {
    let place = match error.span() {
        Some(span) => {
            let (line, column) = line_and_column(text, span.start);
            format!(" at line {line}, column {column}")
        }
        None => String::new(),
    };
    PsError::new(ErrorCategory::ParserError, "TomlParseError", format!("TOML parse error{place}: {}", error.message()))
        .with_details(error.to_string())
        .terminating()
}
```

```text
PS> '[package]', 'name = demo' | ConvertFrom-Toml
ConvertFrom-Toml: TOML parse error at line 2, column 8: string values must be quoted, expected literal string
PS> $Error[0].Exception.InnerException.Message
TOML parse error at line 2, column 8
  |
2 | name = demo
  |        ^^^^
string values must be quoted, expected literal string
```

The error is terminating, since nothing after it can be read, as `ConvertFrom-Json`'s is. `ConvertTo-Toml` returns its errors without `.terminating()`, so an object it cannot write is reported and the next object is still written, unless the caller passes `-ErrorAction Stop`.

## Output and the other streams

`ps.write` sends each result down the pipeline: one object for each document read, and one string for each document written. `ConvertTo-Toml` leaves the toml crate's last line break off the string, so `Set-Content` writes the file with one line break at its end. A crate that reports progress or warnings through a callback can pass them to `ps.progress`, `pwrs::warning!` and `pwrs::verbose!` on the pipeline thread, as [How To Write A Cmdlet](How-To-Write-A-Cmdlet.md) shows. When the crate calls the callback on a thread of its own, the callback sends them to the pipeline thread as [How To Use Threads](How-To-Use-Threads.md) shows.

## Async functions

A cmdlet's phases are synchronous. A phase that calls an `async` function waits for it to finish, on an executor the module chooses, before the phase returns. Ctrl+C reaches a waiting phase through `ps.on_stop`, whose closure can cancel what the phase is waiting for; see [How To Write A Cmdlet](How-To-Write-A-Cmdlet.md#cancellation). Work that should go on after the command returns runs as a job, as in [How To Run Work As A Job](How-To-Run-Work-As-A-Job.md). A method can also return a .NET `Task` for a script to wait on, through `PsTask<T>`, as in [How To Return Objects](How-To-Return-Objects.md#methods-that-return-tasks).

## Crates that build C code

A crate that wraps a C or C++ library, often through a `-sys` crate, usually compiles that code into the module's library, so nothing more ships with the module. Each machine that builds the module needs the compiler the crate's build script calls. The module is built on each platform it should load on, and the module folders are joined, as in [How To Publish](How-To-Publish.md#a-module-built-on-several-platforms). A crate that links or loads a shared library instead needs that library on each computer, which [How To Ship A Prebuilt Native File](How-To-Ship-A-Prebuilt-Native-File.md) ships beside the module's library.

## License notices

The module's library contains the ported crate and every crate it depends on, so the build lists each in `runtimes/<rid>/THIRD-PARTY-NOTICES.txt` with its license and license files. The Toml module's library links twelve:

| Crate | Version | License |
|---|---|---|
| PoWerRuSt | 1.1.0 | MIT |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| pwrs-sys | 1.1.0 | MIT |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_spanned | 1.1.2 | MIT OR Apache-2.0 |
| toml | 1.1.8+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_writer | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| winnow | 1.0.4 | MIT |

The build allows each of these licenses by default, so the module lists none of them. A crate under any other license stops the build until `allowed-licenses` names it. When a crate compiles C or C++ code whose license files are not among the crate's own, the module lists them for the notices to quote, as [How To Publish](How-To-Publish.md#license-notices) shows.

## Tests

The module's own code is tested at two levels. Rust unit tests cover what needs no host. Here they check the calendar arithmetic for every day from year 1 to year 9999, and compare the line and column of a parse error with the position the toml crate reports. A Pester suite runs every cmdlet in both hosts through `cargo pwrs test`, and reads back what `ConvertTo-Toml` writes:

```text
PS> cargo pwrs test --release
pwrs pester host=7.6.9 pester=6.2.0 passed=22 failed=0
pwrs pester host=5.1.26100.9711 pester=6.2.0 passed=22 failed=0
pwrs: surface check: every cmdlet ran, every pipeline parameter took a record, every ShouldProcess cmdlet called should_process
```

[How To Test A Module](How-To-Test-A-Module.md) covers both kinds of test and what the surface check looks for.
