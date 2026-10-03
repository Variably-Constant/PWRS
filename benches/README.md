# PWRS benchmarks

The harnesses that produced the numbers in `docs/PERF.md`, and the
discipline a number needs before it is quoted.

## Harnesses

- `wall_clock.ps1`: times a built module against a hand-written C#
  cmdlet compiled in-process with `Add-Type` and against a PowerShell
  advanced function, all in one host. Every case is warmed with a real
  workload, the case order rotates every repetition so no case keeps
  the coldest slot, each call shape carries a `CONTROL` repeating a
  PWRS case under a second name, and process diagnostics (collections,
  allocated bytes, working set, CPU time) are printed with the table.
  `-Csv` emits rows for a driver.
- `ab_modules.ps1`: runs `wall_clock.ps1` against two module folders
  in separate processes, swapping their order every round, and reports
  the median per case on each side with the change. The C# and
  advanced-function rows are the same code on both sides and are the
  run's noise floor.
- `min_ab.ps1`: the same interleaving reduced by minimum rather than
  median, since competing load only ever adds time. It compares the
  two folders file by file before timing anything, and exits non-zero
  when a case whose code is identical on both sides moves further than
  `-FloorPercent`.
- `timer_cost.ps1`: what one `Stopwatch.GetTimestamp()` and one
  `Interlocked.Add` cost on the host. That is the boundary the managed
  phase counter carries inside `run_ns_avg`.
- `call_shape.ps1`: the same greetings through five call shapes, so the
  cost of a loop construct and the cost of invoking the cmdlet are read
  apart. A command-line invocation is about 11 us per item against 3 us
  per pipeline record.
- `dynamic_reads.ps1`: what one property read costs when Rust takes it
  against when PowerShell takes it, over the same objects in one
  process. The Rust arm reads inside a single cmdlet invocation so the
  invocation divides away; the PowerShell arms are a compiled member
  access and a run-time lookup through `psobject.Properties`, and the
  empty `foreach` they share is timed as its own arm and subtracted.
  Both a PSCustomObject and a CLR object, since one answers from its
  own property bag and the other through the adapter wrapping it. A
  second set of arms asks each object its type, through `type_tag`
  (one crossing) and `type_name` (three and a string) from Rust, and
  through `GetType().FullName` in script.
- `dynamic_params.ps1`: what giving a cmdlet dynamic parameters costs
  each call, and which part of that is PWRS's rather than PowerShell's.
  Compiled C# cmdlets with no dynamic parameters, with a hook returning
  `$null`, an empty table, the bound-parameter table the generated
  cmdlet builds, a second copy of it, and one added parameter are timed
  in one process beside `Get-RustStaticReading` (no hook),
  `Get-RustBlindReading` (a hook that reads nothing and adds nothing)
  and `Get-RustReading` (a hook that reads `-Kind`), so PWRS's share
  splits into the snapshot, the call into the library and the hook's
  reads. A second set of arms calls `GetDynamicParameters` on one
  instance from a C# loop, with no binder around it, and the residual
  between the two routes is printed. Every difference is taken within a
  round, the arm order rotates every round, and the busy cores of every
  other process are read across each pass and printed beside the
  figures.
- `bulk_array.ps1`: what a large array costs a cmdlet to take, in each
  form it can arrive in (a bare `byte[]`, the same array wrapped in a
  `PSObject` as any cmdlet's output is, and an `object[]` of the values
  boxed) through each way a parameter can receive it: a `Vec<u8>`
  declared `byte[]`, the same marked `raw`, a `PsObject` declared
  `byte[]` and pinned, a `PsObject` declared `object` and pinned, and
  a `Vec<u8>` written out as one `byte[]`.
  Every arm's answer is checked against the raw arm's before anything
  is timed; the busy cores of every other process are read across each
  pass and printed beside the figures.
- `import_cost.ps1`: what importing a built module costs and which part
  of it is the generated `.psm1`, plus the cmdlet and statement forms of
  the two things that script does, timed side by side in one run.
  `-Layers` builds the same import up against a copy of the module, from
  the shell assembly alone through the script module, the manifest and
  the manifest's format file, and repeats the last one with a format
  file defining no views, which separates the cost of engaging the
  format subsystem from the cost of the views in the file.
- `environment.ps1`: reports the host state a PowerShell figure does not
  transfer without: the module logging, script block logging and
  transcription policies, Defender's real-time protection and engine
  version, server GC, the three busiest processes and CPU load. It warns
  above 15% load.
- `gc_transition_cost.ps1`: what the managed-to-native GC transition
  costs per call, as the same trivial import with and without
  `[SuppressGCTransition]`. PWRS makes one native call per phase and
  keeps the transition, so this is what that choice costs.
- `../crates/pwrs/benches/conversions.rs` (Criterion): the Rust-only
  conversion layer and the UTF-16 text paths against the fake vtable,
  with an identity clone/drop control cell.
  `cargo bench --profile test-fast -p pwrs --bench conversions`.
- `dotnet/Baseline` (BenchmarkDotNet): the C# baseline and the PWRS
  cmdlet through one runspace. Needs a .NET 10 SDK to build; the
  `Add-Type` harness needs none.

## Discipline

- Run `environment.ps1` first and report what it prints with the table.
  A figure taken at 39% load moved further between two runs of the same
  code than the difference being read, and was discarded.
- Measure on a quiet machine, and read the load average and the
  busiest foreign process before and after the run. Never on a shared
  or loaded one.
- Before reading a delta, run the null control: the same build at two
  paths. Every cell must sit inside about 2%.
- Check both module folders are complete (`Hello.Format.ps1xml` and
  all three assemblies per framework); a folder missing a file the
  manifest names shifts every cell.
- Report the box, the host version, the iteration count, the
  repetitions and the rounds with every table.
- Discard a run whose control lands further out than the difference
  being read, rather than averaging it in.
