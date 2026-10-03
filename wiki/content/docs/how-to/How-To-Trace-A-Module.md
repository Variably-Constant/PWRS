---
title: How To Trace A Module
weight: 10
---

Reading the counters both sides of the boundary keep. Source: `crates/pwrs/src/trace.rs` and the `Pwrs.Trace` class in `crates/cargo-pwrs/dotnet/Pwrs.Runtime/Native.cs`.

## Turning it on

The variable has to be in the environment the pwsh process inherits, so it goes in front of the command rather than inside the session:

```bash
PWRS_TRACE=1 pwsh -NoProfile -Command "Import-Module ./target/pwrs/Hello/Hello.psd1; 1..30000 | ForEach-Object { Get-Greeting -Name x } | Out-Null"
```

Assigning it inside PowerShell with `$env:PWRS_TRACE = 1` was observed on Linux to turn on the managed counters only. The Rust side reads the variable with `getenv`, which does not see an assignment made through `$env:` there, so the Rust lines never appear and the managed lines alone look like a complete trace.

At `1` both sides print a summary every 10000 events; at `2` the Rust side prints a line per event, which is usable for a handful of invocations only. Lines go to standard error. With the variable unset the counter sites are skipped, so a traced run and an untraced one are not the same code path.

## The Rust line

```text
pwrs trace create: created=20000 released=19999 live=1 invocations=20001 binds=19999 direct_writes=19999 handle_writes=0 phases_learned=2 window: bind_ns_avg=58 body_ns_avg=3229
```

| Field | Meaning |
|---|---|
| `created`, `released`, `live` | instances made by `pwrs_cmdlet_create`, freed by `pwrs_cmdlet_release`, and the difference; `live` climbing without bound is a leak of one instance per invocation |
| `invocations` | phase calls through `pwrs_cmdlet_invoke` |
| `binds` | parameter binds actually performed; a phase whose block was not dirty skips its bind |
| `direct_writes`, `handle_writes` | outputs through the scalar entries, and outputs through the generic handle entry |
| `phases_learned` | phases observed running the trait's default body and recorded as empty for their type; at most two per cmdlet type |
| `bind_ns_avg`, `body_ns_avg` | averages over the window since the previous line: per bind, and per invocation for the cmdlet's own phase body, which includes the engine's downstream work that `WriteObject` runs synchronously |

## The managed line

```text
pwrs trace managed phase: phases=30000 skipped=59993 creates=29998 window: run_ns_avg=3107 native_ns_avg=2911 create_ns_avg=126
```

| Field | Meaning |
|---|---|
| `phases` | generated `Run(phase)` calls that reached native |
| `skipped` | Begin or End calls the learned phase mask let the shell skip |
| `creates` | native instance creations |
| `run_ns_avg` | per phase, the whole generated `Run`: packing the block, the native call, unpacking the result |
| `native_ns_avg` | per phase, the native call alone |
| `create_ns_avg` | per creation |

`run_ns_avg` carries the counter's own boundary along with the work: the timestamp that opens the native window, the one that closes it, and the interlocked add that records it. A read costs roughly 31 ns and the add roughly 8 on a Zen+ Windows host, so about 70 ns of it belongs to the counter; `benches/timer_cost.ps1` measures both on whichever host it runs on. `run_ns_avg` minus `native_ns_avg` reads 65 to 87 ns per phase there, so the packing and the transition are what that difference leaves over the boundary.

`native_ns_avg` is the native call, and the native call re-enters managed code through the write entries, so the engine's downstream `WriteObject` work is inside it. On the pipeline shape perf attributes 5.1 to 6.1 percent of process time to the native library against a `native_ns_avg` of about 1650 ns per record on the same run: most of that figure is the engine, and the Rust body is the smaller part. `pwrs::trace`'s own `body_ns_avg` measures the body including that re-entry, for the same reason.

The numbers measured on the project's quiet VM are in [Benchmarks](Benchmarks.md).

## Reading the counters in Rust

`pwrs::trace::snapshot()` returns a `Counters` struct with every field, and `pwrs::trace::report(what)` prints a line on demand; both are usable from a module's own code or tests.
