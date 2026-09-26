---
title: How To Make A Module Fast
weight: 45
---

Most of what a PWRS cmdlet costs is decided by how it is called and how
it is declared, not by the binding layer. The figures below come from
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
the largest lever available to a module.

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

The runtime watches for Begin and End running the trait's default body,
records that on the type, and stops calling native for them on every
later instance. Implementing `begin` or `end` to do nothing costs a
native call per invocation that an unimplemented one does not, because
only the default body is recognized.

## Take bulk data through a pin or a raw parameter

A 4 MB `byte[]` bound to a `Vec<u8>` parameter declared `byte[]`
costs 0.47 ms in pwsh 7.6.6, the copy into the vector, and 165 ms in
Windows PowerShell 5.1, where the engine's binder walks any parameter
declared as an array type before the module is entered, bare or
wrapped in a `PSObject`. The same array through `#[param(raw)]` is
0.66 ms and 0.91 ms, and through a `PsObject` parameter that the body
pins with `pin::<u8>()`, 0.26 ms and 0.38 ms, the array borrowed
where it lies. An `object[]` of the values boxed is the binder
converting them one by one, 786 ms and 2.2 s. `docs/PERF.md` carries
the table.

## Do not cross the boundary to read one property

A property read taken from Rust is 110 to 116 ns on a PSCustomObject
and 384 to 391 ns on a CLR object, where the value is boxed on its way
out through the adapter. The same read in script is 50 to 55 ns, and
about 9 ns on the CLR object when the name is written into the source,
because PowerShell compiles a member access into a cached call site.
Reading `$o.Name` in script and passing the value in beats reaching
back out for it.

The crossing is ahead where script has to resolve the name itself:
`$o.psobject.Properties[$n].Value` is 343 to 363 ns and 528 to 544 ns
on those same two objects. It is ahead by much more where the
alternative is invoking something per item, at 3.0 to 3.3 us per
pipeline record. `docs/PERF.md` carries the table.

This is the previous section from the other side. A typed array has
storage to pin, so one pin replaces the per-element cost outright. A
row of properties has no storage to pin, so each name stays its own
read and batching the rows does not collapse them.

## Dispatch on the tag, not the type name

`PsObject::type_tag()` is one crossing answering a `u32`.
`type_name()` is `GetType`, then `FullName`, then a string read, so
three crossings and a marshalled string before you compare anything.
Code that must handle an object whose type it does not know at compile
time matches the tag first and reaches for the name only when the
answer is `PS_TYPE_OBJECT`.

For a collection, ask once rather than per element:
`array_element_tag` answers for the whole array, which is what lets a
`Vec<T>` of a pinnable primitive take one pin instead of the
element-by-element path.

## Build one string, not many

`Pipeline::verbose`, `debug`, `warning` and `information` take a `&str`,
so a caller that formats one pays the format whether or not the stream
is on: the engine decides that after the crossing. The
[`verbose!`](Pipeline-Reference.md) family asks first and builds the
text only when the record is kept.

## Know what an import costs

A warm re-import of the `hello` module is 9.57 ms on PowerShell 7.6 and
13.23 ms on Windows PowerShell 5.1. Built up a layer at a time on 7.6,
the shell assembly on its own is 1.23 ms, wrapping it in the generated
script module is 3.72 ms, adding the manifest is 4.57 ms, and adding the
manifest's format file is 7.77 ms.

Only the last of those is yours to move. A module with no `#[psclass]`
output classes gets no `Format.ps1xml` and no `FormatsToProcess`, and
imports 3.2 ms faster for it. That happens on its own; there is no
setting. Where a module does have output classes, most of that 3.2 ms is
the format subsystem rather than the file, since a format file defining
no views still costs 2.26 ms, so a shorter one is barely a cheaper one.

A module with hand-written C# pays once more on Windows PowerShell: the
first import in a process copies the four .NET Framework assemblies the
module ships beside the staged shell. Measured on `hello` on the Windows
build machine, 21 fresh processes per arm, that first import took
184.69 ms against 178.86 ms without the step by minimum, 8.38 ms more
by median, and a warm re-import in the same process was unchanged. A
module without hand-written C# has no such step.

An import happens once per session, so none of this decides a module's
throughput. The sections above do.

`benches/import_cost.ps1 -Layers` measures it, and
`benches/environment.ps1` reports the host state a PowerShell figure
does not transfer without.

## Read the counters before changing anything

`PWRS_TRACE=1` reports what actually crossed: how many native calls,
how many binds, how many phases the mask skipped, and the per-phase
timings. [How To Trace A Module](How-To-Trace-A-Module.md) reads a line.
A change to the binding layer is worth about one percent of a call, so
the counters are what tell a guess from a measurement.
