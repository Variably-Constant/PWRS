---
title: Benchmarks
weight: 7
---

What was measured, on which box, with which controls. `docs/PERF.md` in the repository is the normative copy; the harnesses are `benches/wall_clock.ps1`, `benches/ab_modules.ps1`, `benches/dynamic_reads.ps1`, `benches/dynamic_params.ps1`, `crates/pwrs/benches/conversions.rs` and `benches/dotnet/Baseline`.

## The box

Two machines, each table naming its own. An Ubuntu 24.04 VM: AMD Ryzen 7 5700G, KVM guest, 16 vCPU, pwsh 7.6.5 on .NET 10.0.11, rustc 1.97.1, kept free of other work and checked before and after each pass with the load average and the busiest foreign process. And a Windows desktop, which is where a figure needing both PowerShell hosts side by side comes from.

A machine qualifies per run rather than in general: the `CONTROL` rows are what say whether a run can be read.

## The rules

A comparison is readable only when the null control (the same build at two paths) holds every cell inside about 2%, both module folders are complete, and the unchanged C# and advanced-function rows, which are the run's noise floor, are read first. Every case is warmed with a real workload, the case order rotates every repetition so no case keeps the coldest slot, and each call shape carries a `CONTROL` row repeating a PWRS case under a second name so a position effect is visible.

## End to end against a hand-written C# cmdlet

50000 iterations, eight repetitions with the order rotating, minimums. The C# cmdlet is compiled in-process with `Add-Type` and does the same work as `Get-Greeting`; both it and the advanced function ask whether the verbose stream is kept before building its text, as `Get-Greeting` does, so none is charged for a format the others skip.

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

Each `CONTROL` is a PWRS case run a second time under another name, so its distance from that case is the run's noise: 1.0% and 1.0% in the loop, 3.4% and 2.0% in the pipeline.

PWRS over the hand-written C# cmdlet is 1.15x in the loop on both runs, and 1.24x then 1.21x per record in the pipeline. Over the advanced function it is 2.24x and 2.39x faster in the loop, and 1.81x and 1.91x faster per record. A run whose control lands further out than the difference being read is discarded rather than averaged in.

## The call path, by counter

`PWRS_TRACE=1` on the VM, loop shape of 40000 invocations:

| Counter | Value |
|---|---|
| native calls | 40001 |
| binds | 39999 |
| direct writes | 39999 |
| handle writes | 0 |
| phases learned | 2 |
| bind, per bind | 57 ns |
| instance create | 126 ns |
| managed `Run` per phase, loop | 3107 ns |
| native call per phase, loop | 2911 ns |
| managed `Run` per phase, pipeline | 1886 to 1942 ns |
| native call per phase, pipeline | 1792 to 1845 ns |

`phases learned = 2` is Begin and End observed running the trait's default body once and cleared from the type's phase mask, so those phases stop crossing; `binds` is one per invocation because the block's dirty word is zero for the later phases.

## One property read, each side of the boundary

`benches/dynamic_reads.ps1`, 500 objects read 400 times over, minimum of nine rounds, on a Windows host with 24 logical processors running pwsh 7.6.6, checked at 1 to 4% load before each pass. Six runs across two builds of the module: four of one build held every cell within 6%, and a later build of the same source, twelve vtable entries larger, moved the two Rust property cells by about 8% in opposite directions and left every other cell inside its range. The ranges below span all six. The Rust arm does all 200000 reads inside one cmdlet invocation, so the invocation is paid once and divides away. Each PowerShell arm runs inside an empty `foreach` that the Rust arm does not, and is reported net of it.

| Read | PSCustomObject | CLR object (`System.Version`) |
|---|---|---|
| from Rust, name resolved at run time | 110 to 126 ns | 354 to 391 ns |
| `$o.Name`, name compiled into the access | 50 to 56 ns | about 9 ns |
| `$o.psobject.Properties[$n].Value` | 343 to 368 ns | 528 to 558 ns |
| the empty `foreach` alone | 11 ns | 10 to 13 ns |

Crossing to Rust beats resolving a name in script, by 2.9 to 3.3x on a PSCustomObject and 1.4 to 1.6x on a CLR object, and loses to script that has the name at compile time, which PowerShell compiles into a cached call site. The CLR cell there is the difference of two close numbers and sits at this harness's resolution floor; what it supports is that the read is far below a crossing, not the figure itself.

The two object kinds differ because of what answers the read: a PSCustomObject answers from its own property bag, and a CLR object answers through the adapter wrapping it with the value boxed on the way out, which is most of the distance between the two Rust cells.

Read this against [Bulk arrays](#bulk-arrays), the same boundary from the other side: a typed array has storage to pin, so one pin replaces the whole per-element cost, and a row of properties has none, so each name stays its own read.

One property is therefore cheaper read in script than across the boundary whenever script has the name. The crossing is ahead of script that does not have it, and both are far below invoking something per item: a pipeline record is 3.0 to 3.3 us and a command-line invocation 11.1 us.

## Asking an object its type

The same harness and objects, two more runs of nine rounds at 2% load, every cell within 5%. `type_tag()` is one vtable entry answering a `u32`. `type_name()` is a `dyn_call` for `GetType`, a `dyn_get` for `FullName` and a string read, and each of those dynamic calls goes through the engine's member invocation rather than a direct entry.

| Ask | PSCustomObject | CLR object (`System.Version`) |
|---|---|---|
| `type_tag()` from Rust | 13 ns | 12 ns |
| `type_name()` from Rust | 937 to 948 ns | 1032 to 1046 ns |
| `$o.GetType().FullName` in script | 345 to 353 ns | 1143 to 1203 ns |

The tag costs about what one iteration of the empty `foreach` above costs, and the same for both kinds, since it is a switch on the type and never touches the value. It is 70 to 84x cheaper than the name from Rust and 26 to 97x cheaper than asking in script.

The name from Rust is 2.7x what script pays on a PSCustomObject and 0.9x on the CLR object, which is the property finding from the other side: script has a fast path for its own object and the crossing loses to it, while a wrapped CLR object costs script the adapter and the crossing is level with it.

## What dynamic parameters cost

`benches/dynamic_params.ps1` on the Windows desktop (24 logical processors, pwsh 7.6.6 and Windows PowerShell 5.1.26100): 20000 command-line calls per pass end to end, 200000 calls of `GetDynamicParameters` from a C# loop with no binder around it, 15 rounds with the arm order rotating, every difference taken within a round and summarized across rounds by its median. The C# cmdlets are compiled in-process with `Add-Type` and carry `Get-RustReading`'s parameter and output: one with no dynamic parameters, one whose hook returns `$null`, one returning an empty table, one that also builds and pins the bound-parameter table the generated cmdlet builds, one that copies that table a second time, and one adding `-Unit` as `Get-RustReading`'s hook does. Beside them run `Get-RustStaticReading` (no hook), `Get-RustBlindReading` (a hook that reads nothing and adds nothing) and `Get-RustReading` (a hook reading `-Kind`), so PWRS's share splits into the table, the call into the library and the hook's reads. The busy cores of every other process are read across each pass and printed beside the figures.

One run of the harness on this repository's tree, with 3.2 busy cores in other processes across the 7.6 passes and 1.6 across the 5.1 passes. Floors, the `CONTROL` arm less the arm it repeats: 16 ns on 7.6 and -41 ns on 5.1, with quartiles of -92 to 151 and -377 to 611 ns, so an end-to-end row on 5.1 is readable only above about 0.6 us; the direct PWRS rows hold their quartiles within 3% on both hosts.

| ns per call | 7.6 | 5.1 |
|---|---|---|
| PowerShell's own pass, hook returning `$null` | 780 | 1,214 |
| PowerShell's own pass, hook returning an empty table | 1,033 | 2,198 |
| PWRS, the whole: `Get-RustReading` less `Get-RustStaticReading` | 1,639 | 5,152 |
| `GetDynamicParameters` called directly: PWRS less the C# empty hook | 789 | 1,378 |
| of which the bound-parameter table | 95 | 111 |
| of which the call into the library and back | 9 | 98 |
| of which the hook's two reads, `contains` and `get` | 687 | 1,169 |
| a second copy of the table, called directly | 57 | 42 |
| one parameter added: PWRS less `Get-RustStaticReading` | 4,309 | 17,661 |
| one parameter added, called directly: PWRS less the C# hook adding one | 2,189 | 3,242 |

PowerShell's own pass is 0.8 us on 7.6 and 1.2 us on 5.1 with a hook returning `$null`, and 253 and 984 ns more with an empty table, so a hook that adds nothing hands the engine `$null`. PWRS's own work in the direct call is mostly the hook's two reads, 687 ns on 7.6 and 1,169 ns on 5.1 for `contains` and `get` together, against 95 and 111 ns for the bound-parameter table and 9 and 98 ns for the call into the library and back. Each read is a method call on the table from Rust: the key crosses as a managed string, the call runs in the host, and the answer comes back as a handle. The end-to-end share less the direct work, per round, is -193 ns on 7.6 (quartiles -456 to -24) and 1,733 ns on 5.1 (1,076 to 1,969).

A parameter the hook adds costs PWRS 2.2 us on 7.6 and 3.2 us on 5.1 called directly, beyond what the C# control pays to add the same parameter, whose hook reads 128 and 126 ns called directly against its empty hook's 14 and 10. The answer crosses as one string, a line per parameter of seven cells (name, CLR type, mandatory, position, set, help and the validate set), and what remains per parameter is the CLR type resolved by name and the `RuntimeDefinedParameter` and its attributes built on the C# side, plus the hook's own work in Rust.

## UTF-16 conversion, Criterion, on the VM

| Case | ns |
|---|---|
| `text::to_utf16` (ASCII path) | 18.9 |
| `str::encode_utf16().collect()` | 96.1 |
| `text::from_utf16` (ASCII path) | 16.4 |
| `String::from_utf16_lossy` | 95.6 |

## Bulk arrays

`benches/bulk_array.ps1` on the Windows desktop (24 logical processors, pwsh 7.6.6 and Windows PowerShell 5.1.26100): a byte array of 4 MB and of 32 MB in three forms, through the ways a parameter can take it, each arm one call timed on its own, seven rounds with the arm order rotating, the median over rounds. The forms: a bare `byte[]`; the same array wrapped in a `PSObject`, which is how a `byte[]` written by any cmdlet reaches the next one; and, at 4 MB, an `object[]` holding the same values boxed, whose element type differs from the parameter's. The ways: `Get-RustByteSum`, a `Vec<u8>` parameter the shell declares `byte[]`; `Get-RustRawByteSum`, the same parameter marked `raw`, which declares it `object`; `Measure-RustInput`, a `PsObject` parameter declared `byte[]` with `clr` and read through a pin; `Get-RustChecksum`, a `PsObject` parameter declared `object` and read through a pin; and `Get-RustByteRange`, a `Vec<u8>` written out as one `byte[]`. Every arm's answer is checked against the raw arm's before anything is timed, and `CONTROL` is the raw arm again under a second name. The 7.6 columns are one run of the harness in pwsh, with 1.1 busy cores in other processes in the second before it and a median of 0 to 2.7 across its passes; the 5.1 columns are one run in Windows PowerShell on the same module build, with 1.1 before it and a median of 0 to 4.7 across its passes. Single sub-millisecond passes read up to 18 busy cores on 7.6 and 57 on 5.1, since the per-process clock advances in scheduler ticks and a window that short overshoots.

| ms per call | 7.6, 4 MB | 7.6, 32 MB | 5.1, 4 MB | 5.1, 32 MB |
|---|---|---|---|---|
| `Vec<u8>` parameter declared `byte[]`, bare `byte[]` | 0.43 | 4.70 | 159 | 1,295 |
| the same, wrapped `byte[]` | 0.41 | 4.73 | 163 | 1,287 |
| the same, `object[]` | 717 | | 2,152 | |
| the same parameter marked `raw`, bare `byte[]` | 0.44 | 4.71 | 0.94 | 5.12 |
| `raw`, wrapped `byte[]` | 0.44 | 4.74 | 0.95 | 4.72 |
| `PsObject` declared `byte[]`, pinned, bare `byte[]` | 0.23 | 1.03 | 159 | 1,288 |
| the same, wrapped `byte[]` | 0.24 | 1.03 | 162 | 1,295 |
| the same, `object[]` | 718 | | 2,185 | |
| `PsObject` declared `object`, pinned, bare `byte[]` | 0.24 | 1.03 | 0.44 | 1.18 |
| the same, wrapped `byte[]` | 0.25 | 1.02 | 0.40 | 1.13 |
| writing the array out from a `Vec<u8>` | 0.99 | 32.6 | 1.27 | 7.30 |
| CONTROL, the raw parameter and the bare array again | 0.42 | 4.73 | 0.95 | 4.74 |

On pwsh 7 a `byte[]` binds to a parameter declared `byte[]` in the time of its copy into the `Vec<u8>`, about 0.4 ms for 4 MB and 4.7 ms for 32 MB, bare or wrapped: every array-typed parameter carries a transformation the binder runs before its own coercion, which hands it the array a `PSObject` wraps. The parameter marked `raw` pays the same copy. A pinned parameter is the array borrowed where it lies, 0.24 ms and 1.0 ms whichever type it declares, and what remains of that is the binder and the call. An `object[]` is the engine's binder converting the values one by one before the module is entered, 717 ms for 4 MB; the transformation leaves it alone, since it is not the parameter's array type.

On Windows PowerShell 5.1 the engine's binder walks any parameter declared as an array type before the module is entered, bare or wrapped, copied or pinned: 159 to 163 ms for 4 MB and 1.3 s for 32 MB, against 0.9 ms and 5 ms through the parameter marked `raw` and 0.4 ms and 1.2 ms through a `PsObject` declared `object` and pinned, neither of which declares an array. An `object[]` costs the conversion on top, 2.2 s for 4 MB. A module that takes bulk data on both hosts therefore declares the parameter `raw`, or takes a `PsObject` with no `clr` and pins it; declaring the array type buys the binder's own checks and the type in `Get-Help` at that price on 5.1.

## Rust marshaling floor, Criterion, on the VM

Against the fake vtable, with an identity clone/drop as the control cell. Criterion reports its own interval per case, so the Windows desktop serves for these figures where it does not for the wall-clock ones.

| Case | Criterion median |
|---|---|
| control clone/drop | 92.2 ns |
| i64 into_ps | 63.5 ns |
| i64 from_ps | 37.5 ns |
| string into_ps | 336.3 ns |
| string from_ps | 637.2 ns |
| Vec<i64> into_ps, 1000 elems | 50.2 us |
| Vec<i64> from_ps, 1000 elems | 23.7 us |

The `Vec` rows measure the fake test vtable, which allocates a handle and does a map insert under a lock per element, and which serializes tests with a mutex the bench also takes.

## Reproducing

```text
pwsh -File benches/wall_clock.ps1 -Module <folder> -Iterations 50000 -Reps 3
pwsh -File benches/ab_modules.ps1 -A <folder A> -B <folder B> -ALabel before -BLabel after -Iterations 50000 -Reps 3 -Rounds 6
pwsh -File benches/dynamic_reads.ps1 -Module <folder> -Count 500 -Passes 400 -Rounds 9
cargo bench --profile test-fast -p pwrs --bench conversions
```

`benches/dotnet/Baseline` is a BenchmarkDotNet project timing the same comparison through a runspace; it needs a .NET 10 SDK to build, which the `Add-Type` harness does not.
