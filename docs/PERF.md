# PWRS performance

Every number here says which machine it came from and which controls
it passed. Two machines appear and each table names its own: an Ubuntu
24.04 VM (Ryzen 7 5700G, KVM guest, 16 vCPU, pwsh 7.6.5 on .NET
10.0.11, rustc 1.97.1) kept free of other work, and a Windows desktop,
which is where a figure needing both PowerShell hosts side by side
comes from.

A machine qualifies per run rather than in general. The control rows
are what say whether a run can be read: a `CONTROL` repeats a case
under a second name, and a run whose control lands further from the
case it repeats than the difference being read is discarded rather
than averaged in.

## End to end against a hand-written C# cmdlet

`benches/wall_clock.ps1` times a built module against a C# cmdlet
compiled in-process with `Add-Type` and against an advanced function.
Every case is warmed with a real workload, the case order rotates every
repetition so no case keeps the coldest slot, and each call shape
carries a `CONTROL` repeating a PWRS case under a second name.
`benches/min_ab.ps1` interleaves two module folders through it in
separate processes, swapping their order every round and reducing by
minimum rather than median, which reads a busy machine as a slower one.

A comparison is readable only when (1) the null control, the same
build at two paths, holds every cell inside about 2%, (2) both module
folders are complete, since a folder missing the format file its
manifest names shifts every cell including the unchanged C# and
advanced-function cells by 5 to 8%, and (3) the unchanged C# and
advanced-function rows, the run's noise floor, are read first.

### Windows desktop, 50000 iterations, eight rotated reps

The module and all three baselines ask whether the verbose stream is
kept before building its text, so none is charged for a format the
others skip. The case order rotates every rep, and each call shape
carries its own `CONTROL`, a PWRS case repeated under a second name.

Minimum of eight repetitions, 50000 iterations per case. Three runs;
the third is excluded because its loop control landed 10.3% from the
case it duplicates, which is wider than the difference being read. Its
medians were two to three times its minimums, so the box had a
neighbor.

| Case | run 1 | run 2 |
|---|---|---|
| PWRS `Get-Greeting -Name x` in a loop | 696 ms | 701 ms |
| C# cmdlet, same loop | 607 ms | 609 ms |
| advanced function, same loop | 1562 ms | 1676 ms |
| CONTROL, the PWRS loop case again | 689 ms | 694 ms |
| PWRS `1..N \| Get-Greeting` | 151 ms | 151 ms |
| C# cmdlet, pipeline | 122 ms | 125 ms |
| advanced function, pipeline | 274 ms | 289 ms |
| CONTROL, the PWRS pipeline case again | 146 ms | 148 ms |

Each `CONTROL` is a PWRS case run a second time under another name, so
its distance from that case is the run's noise: 1.0% and 1.0% in the
loop, 3.4% and 2.0% in the pipeline. Every ratio below is read against
that floor.

PWRS over the hand-written C# cmdlet is 1.15x in the loop on both runs,
and 1.24x then 1.21x per record in the pipeline. Over the advanced
function it is 2.24x and 2.39x faster in the loop, and 1.81x and 1.91x
faster per record.

Read against the profile: perf attributes 5.1 to 6.1 percent of process
time to the native library, so the Rust half accounts for well under
half of the gap above. perf cannot separate the generated managed shell
from the engine, because both are JIT-compiled into one mapping, but the
counters can: `run_ns_avg` less `native_ns_avg` reads 65 to 87 ns per
phase on the Windows desktop across two runs. The counter's own
boundary inside that window is about 70 ns: two
`Stopwatch.GetTimestamp()` reads at roughly 31 ns each and an
interlocked add at roughly 8, which `benches/timer_cost.ps1` measures
on whichever host it runs on. The managed packing and the transition
are what the difference leaves.

## What the call shape costs

`benches/call_shape.ps1`, the same 50000 greetings through five shapes
on the Windows desktop, minimum of five rounds. The empty rows are the
loop construct alone and are small enough to move between runs; the
rows that invoke the cmdlet held within 1% across two runs.

| Shape | Per item |
|---|---|
| `ForEach-Object`, empty block | 1.9 to 6.4 us |
| `foreach` statement, empty body | 0.06 to 0.11 us |
| `1..N \| ForEach-Object { Get-Greeting -Name x }` | 14.0 us |
| `foreach ($i in 1..N) { Get-Greeting -Name x }` | 11.1 us |
| `1..N \| Get-Greeting` | 3.0 to 3.3 us |

A command-line invocation builds an instance, binds every parameter and
runs Begin, Process and End; a pipeline record reuses the instance,
binds the parameters the engine reassigned, and runs Process alone.
That is the 3.4x between the last two rows. `ForEach-Object` costs
about 2.9 us per item more than the `foreach` statement, which a caller
pays whatever the cmdlet is written in.

Read against the framework's own share: PWRS adds 1.78 us to a
command-line invocation and 0.58 us to a pipeline record, so the shape
a cmdlet is called in moves more than the binding layer does.

## What a property read costs on each side

`benches/dynamic_reads.ps1`, 500 objects read 400 times over, minimum
of nine rounds, on a Windows host with 24 logical processors running
pwsh 7.6.6, checked at 1 to 4% load before each pass. Six runs across
two builds of the module: four of one build held every cell within
6%, and a later build of the same source, twelve vtable entries
larger, moved the two Rust property cells by about 8% in opposite
directions and left every other cell inside its range. The ranges
below span all six. The Rust arm does all 200000 reads inside one cmdlet
invocation, so the invocation is paid once and divides away. Each
PowerShell arm runs inside an empty `foreach` that the Rust arm does
not, and is reported net of it.

| Read | PSCustomObject | CLR object (`System.Version`) |
|---|---|---|
| from Rust, name resolved at run time | 110 to 126 ns | 354 to 391 ns |
| `$o.Name`, name compiled into the access | 50 to 56 ns | about 9 ns |
| `$o.psobject.Properties[$n].Value` | 343 to 368 ns | 528 to 558 ns |
| the empty `foreach` alone | 11 ns | 10 to 13 ns |

Crossing to Rust beats resolving a name in script, by 2.9 to 3.3x on a
PSCustomObject and 1.4 to 1.6x on a CLR object, and loses to script
that has the name at compile time. PowerShell compiles `$o.Name` into
a cached call site, and no crossing competes with that. The CLR cell
there is the difference of two close numbers and sits at this
harness's resolution floor; what it supports is that the read is far
below a crossing, not the figure itself.

The two object kinds differ because of what answers the read. A
PSCustomObject answers from its own property bag. A CLR object answers
through the adapter wrapping it and the value is boxed on the way out,
which is most of the distance between the two Rust cells.

Read this against the bulk-array table below, which is the same
boundary from the other side. A typed array has storage to pin, so one
pin carries 4 MB in 0.26 ms, while an `object[]` of the same values,
which the binder converts one by one, costs 786 ms. A row of
properties has no such storage: each name is its own read at the
figures above, and nothing collapses them.

One property is therefore cheaper read in script than across the
boundary whenever script has the name. The crossing is ahead of script
that does not have it, and both are far below invoking something per
item: a pipeline record is 3.0 to 3.3 us and a command-line invocation
11.1 us, one to two orders of magnitude above every read in this
table.

## What asking an object its type costs

The same harness and objects, two more runs of nine rounds at 2%
load, every cell within 5%. `type_tag()` is one vtable entry
answering a `u32`. `type_name()` is a `dyn_call` for `GetType`, a
`dyn_get` for `FullName` and a string read, and each of those dynamic
calls goes through the engine's member invocation rather than a
direct entry.

| Ask | PSCustomObject | CLR object (`System.Version`) |
|---|---|---|
| `type_tag()` from Rust | 13 ns | 12 ns |
| `type_name()` from Rust | 937 to 948 ns | 1032 to 1046 ns |
| `$o.GetType().FullName` in script | 345 to 353 ns | 1143 to 1203 ns |

The tag costs about what one iteration of the empty `foreach` above
costs, and the same for both kinds, since it is a switch on the type
and never touches the value. It is 70 to 84x cheaper than the name
from Rust and 26 to 97x cheaper than asking in script.

The name from Rust is 2.7x what script pays on a PSCustomObject and
0.9x on the CLR object, which is the property finding from the other
side: script has a fast path for its own object and the crossing
loses to it, while a wrapped CLR object costs script the adapter and
the crossing is level with it.

## What dynamic parameters cost

`benches/dynamic_params.ps1` on the Windows desktop (24 logical
processors, pwsh 7.6.6 and Windows PowerShell 5.1.26100): 20000
command-line calls per pass end to end, 200000 calls of
`GetDynamicParameters` from a C# loop with no binder around it, 15
rounds with the arm order rotating, every difference taken within a
round and summarised across rounds by its median. The C# cmdlets are
compiled in-process with `Add-Type` and carry `Get-RustReading`'s
parameter and output: one with no dynamic parameters, one whose hook
returns `$null`, one returning an empty table, one that also builds and
pins the bound-parameter table the generated cmdlet builds, one that
copies that table a second time, and one adding `-Unit` as
`Get-RustReading`'s hook does. Beside them run `Get-RustStaticReading`
(no hook), `Get-RustBlindReading` (a hook that reads nothing and adds
nothing) and `Get-RustReading` (a hook reading `-Kind`), so PWRS's
share splits into the table, the call into the library and the hook's
reads. The busy cores of every other process are read across each
pass and printed beside the figures.

Two runs of the harness, before and after the three changes made
since 0.2.1 (one bound table instead of two, `$null` handed to the
engine when a hook adds nothing, and a table read from Rust answered
by the dictionary itself instead of the adapter and reflection). The before run had
4.7 to 5.5 busy cores in other processes per pass on 7.6 and 10 to 14
on 5.1. The after run shared the box with a neighbor's load generator:
its 7.6 end-to-end passes read 5.6 to 11 busy cores, its 5.1 passes 22,
and its direct passes, which last milliseconds, 22 to 49 with single
readings of 56 and 63 on a 24-processor box, since the per-process
clock advances in scheduler ticks and a window that short overshoots.
Only paired differences are read from it. Floors, the `CONTROL` arm
less the arm it repeats: -43 and -1 ns on 7.6, 92 and 1,124 ns on 5.1,
so the 5.1 end-to-end rows of the after run are readable only above
about 2 us; the direct rows hold their quartiles within 3% on both
hosts in both runs.

| ns per call | 7.6 before | 7.6 after | 5.1 before | 5.1 after |
|---|---|---|---|---|
| PowerShell's own pass, hook returning `$null` | 898 | 908 | 1,819 | 1,756 |
| PowerShell's own pass, hook returning an empty table | 1,134 | 1,243 | 2,708 | 2,987 |
| PWRS, the whole: `Get-RustReading` less `Get-RustStaticReading` | 3,414 | 1,900 | 18,757 | 7,649 |
| `GetDynamicParameters` called directly: PWRS less the C# empty hook | 1,993 | 1,412 | 10,227 | 2,709 |
| of which the bound-parameter table | 162 | 156 | 308 | 205 |
| of which the call into the library and back | | 150 | | 410 |
| of which the hook's two reads, `contains` and `get` | | 1,091 | | 2,105 |
| a second copy of the table | | 94 | | 79 |
| one parameter added: PWRS less `Get-RustStaticReading` | 6,170 | 4,930 | 42,475 | 28,397 |
| one parameter added, called directly: PWRS less the C# hook adding one | 3,964 | 4,536 | 16,160 | 8,134 |

The before run had no arm splitting the call from the reads, so those
cells are empty; its direct row less its table row, 1,831 and 9,919,
is the two together.

PowerShell's own pass is about 1 us on 7.6 and 2 to 3 us on 5.1, and
`$null` is cheaper than an empty table by 236 to 335 ns on 7.6 and
889 to 1,231 ns on 5.1, which is why a hook that adds nothing now
returns it. The bound-parameter table was 5% of PWRS's whole on 7.6
and 2% on 5.1 before, and the second copy of it 79 to 101 ns. What
remains after the changes is the hook's two reads: 1,091 ns on 7.6 and
2,105 ns on 5.1 for `contains` and `get` together, against 1,831 and
9,919 with the call before. A read from Rust still builds a managed
string for the key and an `object[]` for the arguments, crosses into
the host and back, and allocates and frees a handle for the key, the
arguments and the answer, which is what the remaining 550 to 1,050 ns
per read is.

A parameter the hook adds cost PWRS 4.5 us on 7.6 and 8.1 us on 5.1 in
the after run, beyond what the C# control pays to add the same
parameter, which is 0.19 and 0.21 us above its empty hook: a `string[7]`
row crossing as seven managed strings and being read back cell by cell,
with the CLR type resolved by name. A third run of the harness, after
the answer was packed into one string (a line per parameter, seven
cells, `$null` for none), on a quieter box, 1.9 busy cores in other
processes across the 7.6 passes and 4.6 across the 5.1 passes, floors
40 and -59 ns:

| ns per call, packed answer | 7.6 | 5.1 |
|---|---|---|
| PWRS, the whole: `Get-RustReading` less `Get-RustStaticReading` | 1,571 | 5,512 |
| `GetDynamicParameters` called directly: PWRS less the C# empty hook | 769 | 1,444 |
| one parameter added: PWRS less `Get-RustStaticReading` | 3,998 | 18,021 |
| one parameter added, called directly: PWRS less the C# hook adding one | 2,200 | 3,507 |

The added parameter is 2.2 and 3.5 us called directly, against 4.5
and 8.1 before the packing; the C# control's own cost for it was 0.13
and 0.14 us in that run. What remains per parameter is the CLR type
resolved by name and the `RuntimeDefinedParameter` and its attributes
built on the C# side, plus the hook's own work in Rust.

## What an import costs

`benches/import_cost.ps1`, the `hello` module, minimum of 250 rounds on
the Windows desktop. The first import of a session pays assembly load
and JIT; these are warm figures. Reduced by minimum, so competing load
raises a round rather than the result, and 250 rounds puts every figure
at the same floor a 30-round run reaches at its best.

| Step | PowerShell 7.6 | Windows PowerShell 5.1 |
|---|---|---|
| `Import-Module -Force`, warm | 9.57 ms | 13.23 ms |
| `Loader.Load` | 0.31 ms | 0.44 ms |
| drop any earlier binary module | 0.21 ms | 0.39 ms |
| `Import-Module -Assembly` | 0.21 ms | 0.34 ms |

The lines inside the generated `.psm1` are the three indented rows, about
8% of the import. That is not the cost of having a script module at all,
which is the more useful decomposition. Building the same import up one
layer at a time on 7.6, minimum of 200 rounds:

| What is imported | Cost | Added | What the layer buys |
|---|---|---|---|
| the shell assembly on its own | 1.23 ms | | nothing else |
| wrapped in a script module | 3.72 ms | 2.49 ms | edition selection, load-context isolation |
| with the manifest | 4.57 ms | 0.85 ms | metadata, and export by name |
| with `FormatsToProcess` | 7.77 ms | 3.20 ms | default table and list views |

Each layer is load-bearing. A `.psd1` is static data, so `RootModule`
cannot choose `net10.0` against `netstandard2.0` at runtime and only a
script can; on .NET the same script hands each module root its own
`AssemblyLoadContext`, so two modules built against different
`Pwrs.Runtime` versions do not collide. Naming every cmdlet in
`CmdletsToExport` is what puts the module on the discovery fast path.

The format file is the largest layer at 3.20 ms, and most of it is the
subsystem rather than the file: a format file defining no views at all
still costs 2.26 ms, leaving 0.94 ms for this module's nine views. A
view is emitted per output class and is what formats that class, so
there is none to remove. `cargo pwrs build` writes no format file, and
no `FormatsToProcess`, for a module with no output classes, which is the
only way not to pay the 3.20 ms.

An import happens once per session. These figures decide nothing about
a module's throughput; the per-call sections above do.

The script is written as language statements rather than cmdlets in a
pipeline, because a cmdlet call builds a pipeline, binds parameters
through the full binder and runs begin/process/end, while a statement
compiles into the script. Measured side by side in one run:

| Construct | 7.6 | 5.1 |
|---|---|---|
| `foreach` with `if` | 0.210 ms | 0.388 ms |
| `Get-Module \| Where-Object` | 0.398 ms, 1.90x | 0.560 ms, 1.44x |
| `[IO.Path]::Combine` | 0.096 ms | 0.189 ms |
| `Join-Path`, nested | 0.239 ms, 2.49x | 0.485 ms, 2.57x |

The statement form wins on both hosts, so the generated script needs no
per-host branch for it; the branch it does carry picks the target
framework folder, not a faster construct. The two replacements are worth
0.33 ms of a 9.57 ms import on 7.6 and 0.47 ms of 13.23 ms on 5.1.

The manifest declares `CmdletsToExport` and `AliasesToExport` by name
with no wildcard, which is what lets `Get-Command` and module
auto-loading answer from the manifest without importing the module.

These figures come from a machine with no PowerShell logging policy set
and Defender real-time protection on. Those are the state a PowerShell
figure does not transfer without, so `benches/environment.ps1` reports
them.

## What the call path costs, by counter

`PWRS_TRACE=1` prints both sides' counters every 10000 events, and the
counter sites are skipped when it is unset. The Rust side reads it with
`getenv`, so it has to be in the environment the process inherits;
assigning it inside PowerShell through `$env:` reaches the managed
counters only. Each line's figures are the average over the events
since the previous line, so warm-up stays out of the steady state.

### Native calls per invocation, Ubuntu VM, loop shape

| Loop calls | Native calls | Binds | Handle writes | Direct writes |
|---|---|---|---|---|
| 40000 | 40001 | 39999 | 0 | 39999 |

Three effects, each read off the counters: `phases_learned=2` says the
runtime observed Begin and End run the trait's default body once and
cleared them from the type's phase mask, so `skipped=79999` of the
later phases never cross; `binds` is one per invocation because the
block's dirty word is zero for the later phases; every output string
takes the one-crossing entry.

### Steady-state cost per phase, Ubuntu VM

| Shape | Managed Run() per phase | Native call per phase | Rust body | Bind | Instance create |
|---|---|---|---|---|---|
| loop | 3107 ns | 2911 ns | 2542 to 3229 ns | 57 ns | 126 ns |
| pipeline | 1886 to 1942 ns | 1792 to 1845 ns | per record | per record | once |

The Rust body is the cmdlet's `process`, which includes the engine's
own `WriteObject` work: PowerShell runs the downstream pipeline
synchronously inside that call, and a hand-written C# cmdlet pays the
same. `native_ns_avg` less the Rust body is not the native wrapper for
that same reason, since the write entries re-enter managed code and the
engine's work lands inside the native window. A profile is what
separates them, and it puts the whole native library at 5.1 to 6.1
percent of process time with `pwrs_cmdlet_invoke` itself at 0.13 to
0.29 percent.

So what PWRS adds is the managed packing and the transition, which sits
inside a `run_ns_avg` less `native_ns_avg` of 65 to 87 ns against a
counter boundary of about 70, the bind at 57 ns, and per command-line
invocation one instance create at 126 ns and its release.

Pooling instances would remove the allocation and the free from that
creation and nothing else, and `crates/pwrs/benches/conversions.rs`
measures that pair at 1.6 ns. The default construction, the header, the
crossing and the registry read are paid either way, and a reused
instance would have to be reset, since `bind` writes only the
parameters marked dirty and the rest would carry over from the previous
invocation.

## Bulk arrays: where a large argument's time goes

`benches/bulk_array.ps1` on the Windows desktop (24 logical
processors, pwsh 7.6.6 and Windows PowerShell 5.1.26100): a byte array
of 4 MB and of 32 MB in three forms, through the ways a parameter can
take it, each arm one call timed on its own, seven rounds with the arm
order rotating, the median over rounds. The forms: a bare `byte[]`;
the same array wrapped in a `PSObject`, which is how a `byte[]`
written by any cmdlet reaches the next one; and, at 4 MB, an
`object[]` holding the same values boxed, whose element type differs
from the parameter's. The ways: `Get-RustByteSum`, a `Vec<u8>`
parameter the shell declares `byte[]`; `Get-RustRawByteSum`, the same
parameter marked `raw`, which declares it `object`; `Measure-RustInput`,
a `PsObject` parameter declared `byte[]` with `clr` and read through a
pin; `Get-RustChecksum`, a `PsObject` parameter declared `object` and
read through a pin; and `Get-RustByteRange`, a `Vec<u8>` written out
as one `byte[]`. Every arm's answer is checked against the raw arm's
before anything is timed, and `CONTROL` is the raw arm again under a
second name. The run had 3.8 busy cores in other processes in the
second before its 7.6 half and 6.8 before its 5.1 half; the 5.1
passes read 4 to 8 busy cores, and single sub-millisecond passes on
7.6 read up to 67, since the per-process clock advances in scheduler
ticks and a window that short overshoots. Two more runs of the
harness, three minutes before and after under heavier load (a
compiler at up to 41 cores in some passes of the first, 10 busy cores
before the third and 10 to 11 across its 5.1 passes), agreed with
every row below within its quartiles except the 32 MB array written
out on 7.6, which they read at 30.9 and 27.2 ms against the 13.5
below.

| ms per call | 7.6, 4 MB | 7.6, 32 MB | 5.1, 4 MB | 5.1, 32 MB |
|---|---|---|---|---|
| `Vec<u8>` parameter declared `byte[]`, bare `byte[]` | 0.47 | 5.18 | 165 | 1,351 |
| the same, wrapped `byte[]` | 0.46 | 5.22 | 167 | 1,348 |
| the same, `object[]` | 786 | | 2,171 | |
| the same parameter marked `raw`, bare `byte[]` | 0.66 | 5.21 | 0.91 | 5.19 |
| `raw`, wrapped `byte[]` | 0.42 | 5.18 | 0.92 | 5.22 |
| `PsObject` declared `byte[]`, pinned, bare `byte[]` | 0.26 | 1.13 | 162 | 1,330 |
| the same, wrapped `byte[]` | 0.25 | 1.13 | 161 | 1,353 |
| the same, `object[]` | 784 | | 2,214 | |
| `PsObject` declared `object`, pinned, bare `byte[]` (third run) | 0.26 | 1.09 | 0.38 | 1.26 |
| the same, wrapped `byte[]` (third run) | 0.26 | 1.11 | 0.35 | 1.24 |
| writing the array out from a `Vec<u8>` | 0.68 | 13.5 | 1.30 | 8.22 |
| CONTROL, the raw parameter and the bare array again | 0.45 | 5.30 | 0.95 | 5.38 |

The two rows marked as the third run's are an arm added after the
second run and carry that run's readings.

On pwsh 7 a `byte[]` binds to a parameter declared `byte[]` in the
time of its copy into the `Vec<u8>`, about 0.5 ms for 4 MB and 5 ms
for 32 MB, bare or wrapped: every array-typed parameter carries a
transformation the binder runs before its own coercion, which hands
it the array a `PSObject` wraps. The parameter marked `raw` pays the
same copy. A pinned parameter is the array borrowed where it lies,
0.26 ms and 1.1 ms whichever type it declares, and what remains of
that is the binder and the call. An `object[]` is the engine's binder
converting the values one by one before the module is entered, 786 ms
for 4 MB; the transformation leaves it alone, since it is not the
parameter's array type.

On Windows PowerShell 5.1 the engine's binder walks any parameter
declared as an array type before the module is entered, bare or
wrapped, copied or pinned: 160 to 167 ms for 4 MB and 1.3 s for 32
MB, against 0.9 ms and 5 ms through the parameter marked `raw` and
0.4 ms and 1.3 ms through a `PsObject` declared `object` and pinned,
neither of which declares an array. An `object[]` costs the
conversion on top, 2.2 s for 4 MB. A module that takes bulk data on both hosts therefore
declares the parameter `raw`, or takes a `PsObject` with no `clr` and
pins it; declaring the array type buys the binder's own checks and the
type in `Get-Help` at that price on 5.1.

## UTF-16 conversion, Criterion, Ubuntu VM

`crates/pwrs/benches/conversions.rs`, 32-character ASCII string,
median of the Criterion interval.

| Case | ns |
|---|---|
| `text::to_utf16` (ASCII path) | 18.9 |
| `str::encode_utf16().collect()` | 96.1 |
| `text::from_utf16` (ASCII path) | 16.4 |
| `String::from_utf16_lossy` | 95.6 |

Every string parameter read and every string written goes through
these; the ASCII path is one widening or narrowing pass LLVM
vectorizes, and non-ASCII input falls through to std.

## Rust marshalling floor, Criterion, Windows desktop

The Rust-only conversion layer against the fake vtable, with an
identity clone/drop as the control cell. Criterion reports its own
interval per case, so this box serves for these figures where it does
not for the wall-clock ones.

| Case | Criterion median |
|---|---|
| control clone/drop | 92.2 ns |
| i64 into_ps | 63.5 ns |
| i64 from_ps | 37.5 ns |
| string into_ps | 336.3 ns |
| string from_ps | 637.2 ns |
| Vec<i64> into_ps, 1000 elems | 50.2 us |
| Vec<i64> from_ps, 1000 elems | 23.7 us |

The `Vec` rows measure the fake test vtable, whose `array_get`
allocates a handle and does a map insert under a lock per element, and
which serializes tests with a mutex the bench also takes.

## Build settings that matter

Modules ship from the `release` profile with `lto = "fat"`,
`codegen-units = 1` and `panic = "unwind"`; `cargo pwrs new` writes the
same profile into a new module. Unwinding stays on because the exports
catch panics at the boundary and report them as errors; `panic =
"abort"` would take the host process down with the module.

Two further levers reach a module through `RUSTFLAGS` and need nothing
from `cargo pwrs`: `-C target-cpu=native`, for a build that stays on the
machine that made it, and profile-guided optimization through
`-Cprofile-generate` and `-Cprofile-use`. Both pay in a module's own
compute-bound code. The binding layer has no loop or branch-heavy path
for either to work on, so PWRS sets neither for distributable builds.

On the managed side the runtime is compiled with `/optimize+`, the
exports are called through unmanaged function pointers on .NET,
`[module: SkipLocalsInit]` covers the runtime and the generated shell,
and the small helpers around the vtable entries are marked
`AggressiveInlining` with the exception paths `NoInlining`.
`[SuppressGCTransition]` is not used. The runtime requires a suppressed
call to run for under a microsecond, make no callback into the runtime,
never block, never throw and touch no concurrency primitive, and
`pwrs_cmdlet_invoke` encloses the cmdlet's whole body, so it meets none
of those. `benches/gc_transition_cost.ps1` measures the transition at 6
to 10 ns per call across two runs on the Windows desktop, against one
native call per phase.

## Optional deeper harness

`benches/dotnet/Baseline` is a BenchmarkDotNet project that times the
same comparison with warmup and statistics through a runspace. It needs
a .NET 10 SDK to build; the `Add-Type` harness above needs none and is
what produced these numbers.
