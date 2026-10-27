---
title: How To Pass Native Data Between Cmdlets
weight: 12
---

Leaving data in Rust from one cmdlet to the next, so a pipeline of Rust stages does not pay for its rows until something needs the rows. Source: `crates/pwrs/src/proxy.rs` (`PsProxy`), `crates/cargo-pwrs/dotnet/Pwrs.Runtime/ProxyCore.cs` (the gate and the memory report), `crates/pwrs-macros/src/classes.rs` (`native_bytes`), and `Hello.Series` with its stages in `examples/hello/src/lib.rs`.

## The convention

A PowerShell pipeline moves one object at a time, and each object a Rust cmdlet writes or reads passes between Rust and .NET on its own. For data that stays in Rust between stages, a table, a buffer, a plan of work, write one proxy object that stands for all of it:

- Each stage takes that object by type, as a `PsProxy<T>` parameter, and writes one object on.
- A stage that transforms writes a new object and leaves the one it was given as it was, so the value of a variable that still refers to the input does not change.
- An exit cmdlet writes the rows, one object each, for commands that take rows.

No stage depends on which commands are next to it. A PowerShell command cannot find out what is before or after it in the pipeline, and it does not need to. The binder passes a stage the object when the object is of its parameter's type, and anything else is refused before the stage runs. The object passes unchanged through commands that do not use it, `Where-Object`, `ForEach-Object`, a variable, an array, and reaches the next stage the same.

## What the rows cost

Three pipelines compute the same sum, of `2i + 1` for `i` from 0 below `n`, with the hello example's `Hello.Series`:

- **one object**: `New-RustSeries $n | Add-RustSeriesStep -Scale 2 | Add-RustSeriesStep -Shift 1 | Measure-RustSeries`, where the last stage runs the plan and sums in Rust, so no number becomes an object;
- **rows out**: the same stages, then `Expand-RustSeries | Measure-Object -Sum`, so every number becomes a .NET object and is summed by PowerShell;
- **objects throughout**: `0..($n - 1) | ForEach-Object { $_ * 2 } | ForEach-Object { $_ + 1 } | Measure-Object -Sum`.

Median of five runs on a Ryzen 9 7900X, with hello built for that CPU, each arm run once at `n` = 1000 before timing and the three rotated in order across runs; every run's sum was checked against `n`².

| Host | n | one object | rows out | objects throughout |
|---|---|---|---|---|
| pwsh 7.6.6 | 10 000 | 0.2 ms | 6.2 ms | 28.6 ms |
| pwsh 7.6.6 | 1 000 000 | 1.2 ms | 552 ms | 2466 ms |
| Windows PowerShell 5.1 | 10 000 | 0.3 ms | 11.7 ms | 61.2 ms |
| Windows PowerShell 5.1 | 1 000 000 | 1.3 ms | 1141 ms | 6030 ms |

The difference is the rows. Where the work ends in a figure, a stage that computes it in Rust never makes them; where the rows are the result, the middle column is what writing them and summing them in PowerShell took.

## Taking the object by type

```rust
#[cmdlet(verb = "Add", noun = "RustSeriesStep", output = ["Hello.Series"])]
#[derive(Default)]
pub struct AddRustSeriesStep {
    /// The series to extend, taken by type from the pipeline.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub series: PsProxy<Series>,
    /// Multiplies every number.
    #[param]
    pub scale: Option<f64>,
    /// Adds to every number.
    #[param]
    pub shift: Option<f64>,
    /// Lets through only the numbers greater than this.
    #[param]
    pub above: Option<f64>,
}

impl Cmdlet for AddRustSeriesStep {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.scale.is_none() && self.shift.is_none() && self.above.is_none() {
            return Err(PsError::new(ErrorCategory::InvalidArgument, "NoStep", "name at least one of -Scale, -Shift and -Above"));
        }
        let mut next = self.series.with(Series::clone)?;
        if let Some(k) = self.scale {
            next.push(Step::Scale(k));
        }
        if let Some(k) = self.shift {
            next.push(Step::Shift(k));
        }
        if let Some(k) = self.above {
            next.push(Step::Above(k));
        }
        ps.write(next)
    }
}
```

A `PsProxy<T>` parameter takes a `#[psclass(proxy)]` object of class `T`. The shell declares the parameter with `T`'s CLR type, so piping a string to it fails with the engine's `InputObjectNotBound`, and naming another class fails with the binder's conversion error, before the cmdlet runs. A proxy with a `#[psfield(skip)]` field cannot be read back by value, and `PsProxy` is how a cmdlet takes one.

- `with(|v: &T| ...)` passes a reference to the value to a closure and returns what the closure returns.
- `with_mut(|v: &mut T| ...)` passes a mutable reference, so the closure can change the value. The object is the same one afterward, and a class that reports its native bytes reports them again.
- `object()` is the object itself, for a stage that changes the value in place and writes the same object on.

Both fail, with the reason, when the object has been disposed, was made by an earlier load of the module, or is of another class or another module. `with` is a shared entry into the object's gate and `with_mut` an exclusive one, so `with` also fails inside a `&mut self` method or another `with_mut` on the thread, and `with_mut` fails inside any call into the object.

## The object's gate

Every call into a proxy's value takes the object's gate: a property read, a method call, and each `with` or `with_mut`. A call from another thread waits for the gate, and the gate stays taken until the call returns. So a method that blocks, such as one waiting for a network client, delays every other call on that object from any runspace until it ends. Whatever ends the wait has to come through another object. On the thread inside the gate, what a second call may do depends on the kind of entry. A property read, a method taking `&self` and a `with` borrow are shared entries, and shared entries nest. So a `&self` method may read the object it runs on. That happens when a script passes an object by value to a method of that same object, as `$c.SameAs($c)` does on `Hello.Counter`. A method taking `&mut self` and a `with_mut` borrow are exclusive entries: they have the only reference to the value, so they are refused while anything is inside the object, and everything else is refused while they run. The refusal is a `PwrsException` saying the object is in use for a method call, and `$null` for a property read, which is what PowerShell's property adapter makes of any exception a getter throws. `$c.Absorb($c)`, whose `absorb` takes `&mut self`, is refused that way. `Dispose` from the thread inside the gate frees the value when its last entry ends.

So a stage ends its borrow of the value before it writes, since a command downstream runs inside that write and may use the same object. `Expand-RustSeries` copies what it needs out of the borrow first and writes the rows from the copy:

```rust
let series = self.series.with(Series::clone)?;
let mut written = Ok(());
series.run(|v| match ps.write(v) {
    Ok(()) => true,
    Err(e) => {
        written = Err(e);
        false
    }
});
written
```

## Telling the garbage collector how much memory the object uses

```rust
#[psclass(name = "Hello.Ballast", mode = proxy, native_bytes = Ballast::claimed_bytes)]
pub struct Ballast {
    /// The bytes it reports.
    pub claimed: u64,
}

impl Ballast {
    fn claimed_bytes(&self) -> usize {
        self.claimed as usize
    }
}
```

The .NET object of a proxy is small whatever the Rust value behind it contains, so without help the garbage collector sees nothing worth collecting, and the Rust value is freed only when other .NET allocations bring a collection. `native_bytes` names a `fn(&Self) -> usize`. The .NET object reports that number with `GC.AddMemoryPressure` when it is made, gets it again after each method call and each `with_mut`, and removes what it reported when the value is freed.

hello's `Measure-RustPressure` makes 64 ballast objects, each reporting 256 MB it never allocates, lets go of each as soon as it is made, and counts what is still alive once the finalizers have run. Three runs a row on the same Ryzen 9 7900X:

| Host | objects made | collections, any generation | of those, full | still alive |
|---|---|---|---|---|
| pwsh 7.6.6, 20 ms apart | 64 | 63 to 64 | 63 to 64 | 2 |
| pwsh 7.6.6, back to back | 64 | 1 | 1 | 64 |
| Windows PowerShell 5.1, 20 ms apart | 64 | 17 | 0 | 20 |
| Windows PowerShell 5.1, back to back | 64 | 8 to 9 | 0 | 11 to 15 |
| either host, the class without `native_bytes` | 64 | 0 | 0 | 64 |

.NET responded to the reports with full collections, and objects made back to back caused only one, which freed none of them. .NET Framework responded with younger collections at every spacing tried, 0 to 50 ms. Without the report no collection ran and every value stayed alive.

Report the memory that would be freed with the value alone. A buffer a value shares through an `Arc` with other objects lives until the last of them is dropped, and each object reporting the whole buffer tells the collector the same memory several times over.

## Stopping long work

A stage that computes for a long time before it writes anything runs its work through `stream_from_thread_until` and checks the `StopSignal` between steps, so Ctrl+C returns promptly; see [How To Use Threads](How-To-Use-Threads.md).
