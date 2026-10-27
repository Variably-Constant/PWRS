---
title: How To Make A Module Fast
weight: 10
---

Most of what a PWRS cmdlet costs depends on how it is called and how it
is declared, not on the layer that connects it to PowerShell. The
figures below come from
`benches/call_shape.ps1` against the `hello` example on a Windows
desktop, 50000 greetings through each shape.

## Take pipeline input

| Shape | Per item |
|---|---|
| `1..N \| ForEach-Object { Get-Greeting -Name x }` | 14.0 us |
| `foreach ($i in 1..N) { Get-Greeting -Name x }` | 11.1 us |
| `1..N \| Get-Greeting` | 3.0 to 3.3 us |

A command-line invocation builds a cmdlet instance, binds every
parameter and runs Begin, Process and End. A pipeline record reuses the
instance, binds only the parameters the engine reassigned, and runs
Process alone. That is the 3.4x between the last two rows, and it is
the biggest saving available to a module.

Declare the parameter a caller will pipe with `value_from_pipeline`, or
`value_from_pipeline_by_property_name` when it should bind from a
property of a piped object:

```rust
#[param(mandatory, position = 0, value_from_pipeline)]
pub name: String,
```

`ForEach-Object` costs about 2.9 us per item more than the `foreach`
statement in the same loop. Neither is PWRS: a caller who must loop
pays it whatever the cmdlet is written in.

## Let the phase mask work

When Begin or End runs the trait's default body, the runtime records
that on the type and stops calling into Rust for that phase on every
later instance. Implementing `begin` or `end` to do nothing costs a
call into Rust per invocation that leaving them out does not, because
only the default body makes that record.

## Take bulk data through a pin or a raw parameter

A 4 MB `byte[]` bound to a `Vec<u8>` parameter declared `byte[]`
costs 0.43 ms in pwsh 7.6.6, the copy into the vector, and 159 ms in
Windows PowerShell 5.1, where PowerShell's binder goes through every
element of a parameter declared as an array type before it calls the
module, whether the array came bare or wrapped in a `PSObject`. The
same array through `#[param(raw)]` is 0.44 ms and 0.94 ms, and through
a `PsObject` parameter that the body pins with `pin::<u8>()`, 0.24 ms
and 0.44 ms, the array read where it is. Passed as an `object[]` of
boxed values, the binder converts them one by one: 717 ms and 2.2 s.
`docs/PERF.md` has the full table.

## Read one property in script, not from Rust

A property read taken from Rust is 110 to 116 ns on a PSCustomObject
and 384 to 391 ns on a CLR object, where the value is boxed on its way
out through the adapter. The same read in script is 50 to 55 ns, and
about 9 ns on the CLR object when the name is written into the source,
because PowerShell compiles a member access into a cached call site.
Reading `$o.Name` in script and passing the value in is faster than
reading it from Rust.

Reading from Rust is faster where script has to look the name up
itself: `$o.psobject.Properties[$n].Value` is 343 to 363 ns and 528 to
544 ns on those same two objects. It is faster by much more where the
alternative is invoking something per item, at 3.0 to 3.3 us per
pipeline record. `docs/PERF.md` has the table.

This is the previous section seen from the other side. A typed array
has storage to pin, so one pin replaces the cost of reading each
element. A row of properties has no storage to pin, so each name is
still its own read, and passing many rows at once does not change that.

## Check the type tag, not the type name

`PsObject::type_tag()` is one call into .NET that returns a `u32`.
`type_name()` calls `GetType`, then `FullName`, then reads a string, so
it makes three calls and copies a string before you compare anything.
Code that handles objects whose type is not fixed at compile time
checks the tag first, and gets the name only when the tag is
`PS_TYPE_OBJECT`.

For a collection, PWRS checks once, not per element: the host's
`array_element_tag` entry returns the element type of the whole array,
which is what lets a `Vec<T>` parameter of a pinnable primitive take
one pin instead of converting element by element.

## Build one string, not many

`Pipeline::verbose`, `debug`, `warning` and `information` take a `&str`,
so the text is formatted whether or not the stream is on: PowerShell
checks that only after the call. The
[`verbose!`](Pipeline-Reference.md) family checks first and builds the
text only when the record will go somewhere.

## Know what an import costs

A warm re-import of the `hello` module is 9.57 ms on PowerShell 7.6 and
13.23 ms on Windows PowerShell 5.1. Built up a layer at a time on 7.6,
the shell assembly on its own is 1.23 ms, wrapping it in the generated
script module is 3.72 ms, adding the manifest is 4.57 ms, and adding the
manifest's format file is 7.77 ms.

Only the last of those depends on the module. A module with no
`#[psclass]` output classes gets no `Format.ps1xml` and no
`FormatsToProcess`, and imports 3.2 ms faster for it. That happens by
itself; there is no setting. Where a module does have output classes,
most of that 3.2 ms is PowerShell's format system, not the file: a
format file defining no views still costs 2.26 ms. The views themselves
cost far less. Measured in fresh processes against a second copy of the
same file, hello's 36 views add about 1 ms to a warm re-import in pwsh
7.6.7 and 0.5 ms in Windows PowerShell 5.1, and 2.6 to 4.6 ms to the
first import in pwsh. `docs/PERF.md` has the figures.

On Windows PowerShell, a module's first import in a process copies
.NET Framework assemblies the module ships beside the staged shell,
where .NET Framework looks for a shell's dependencies. Every module
ships the four access-control assemblies its runtime refers to:
`Microsoft.Win32.Registry`, `System.IO.FileSystem.AccessControl`,
`System.Security.AccessControl` and `System.Security.Principal.Windows`.
A module with hand-written C# also ships the four its shell was compiled
against: `System.Buffers`, `System.Memory`, `System.Numerics.Vectors`
and `System.Runtime.CompilerServices.Unsafe`. A later import in the same
process finds them there and copies nothing, and PowerShell 7 skips the
step.

Measured on `hello` on the Windows desktop, 21 fresh Windows PowerShell
5.1 processes for each form of the script, the step adds 5.9 ms to the
first import by minimum and 2.2 ms by median, and a warm re-import is
unchanged. Two identical copies of the script without the step,
measured beside them, differed by 2.5% and 0.7%, which is the noise.
`memfs` does not import on Windows PowerShell without the step at all,
since its provider uses the access-control types.

An import happens once per session, so none of this limits how fast a
module runs. The sections above do.

`benches/import_cost.ps1 -Layers` measures it, and
`benches/environment.ps1` reports the state of the host, which a
PowerShell figure from one machine needs before it can be compared
with another's.

## Read the counters before changing anything

`PWRS_TRACE=1` reports what actually happened: how many calls into
Rust, how many binds, how many phases the mask skipped, and the
per-phase timings. [How To Trace A Module](How-To-Trace-A-Module.md)
shows how to read a line. A change to PWRS's own layer is worth about
one percent of a call, so measure with the counters before changing
anything.
