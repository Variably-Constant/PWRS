---
title: The Call Path
weight: 3
---

What one invocation of a Rust cmdlet costs, piece by piece. Source: `crates/cargo-pwrs/dotnet/Pwrs.Runtime/RustCmdlet.cs`, the generated `Run(phase)` in `crates/cargo-pwrs/src/generate.rs`, `crates/pwrs/src/cmdlet.rs` and `runtime.rs`, `pipeline.rs`, `text.rs`, `trace.rs`. Numbers are from [Benchmarks](Benchmarks.md).

## One invocation, step by step

For `Get-Greeting -Name x` typed at the prompt:

1. The engine constructs the generated cmdlet object and binds `-Name` by calling the generated property setter, which stores the value and sets bit 0 of two words, `_bound` and `_dirty`.
2. The engine calls `BeginProcessing`. The generated override asks `NeedsPhase(Begin)`; on the first instance of the type that creates the Rust instance (`pwrs_cmdlet_create`, one allocation holding a dispatch header and the cmdlet struct) and receives the type's phase mask, initially all three phases. Begin runs: the block is packed on the stack (dirty word, bound word, the pinned string), one native call is made, the Rust side binds the parameters because the block is dirty, calls `begin`, which is the trait's default and marks the token, and returns. The runtime clears Begin from the type's mask.
3. `ProcessRecord`: the dirty word is now zero, so the Rust side skips the bind and calls `process`, which formats the greeting and writes it through `write_string`: one crossing, no `GCHandle`, the managed side allocates the .NET string and calls `WriteObject`, which runs the downstream pipeline synchronously.
4. `EndProcessing`: the default `end` marks the token; the runtime clears End from the mask.
5. `Dispose`: `pwrs_cmdlet_release` frees the instance; the handle the cmdlet held to itself is freed.

For the second and every later instance of `Get-Greeting`, step 2 receives a mask with only Process set, so `BeginProcessing` and `EndProcessing` return without a native call, and the whole invocation is one create, one native call with one bind, one write, and one release. For pipeline input, one instance handles every record: the setter runs per record, the dirty word is set, and each `ProcessRecord` binds and runs.

## The pieces

**One allocation per cmdlet.** The managed cmdlet holds a pointer to its Rust instance for its lifetime. There is no global table keyed by handle, so an invocation takes no lock and does no lookup; the native side reads the pointer it is handed.

**Bind on change only.** The generated setters record boundness; the engine calls a setter only for a supplied parameter, so no dictionary is consulted. The Rust side binds before the first phase and again only when the dirty word is non-zero.

**Learned phase masks.** Rust cannot ask whether a trait method was overridden, but the default bodies are PWRS code and can leave a mark. One observation per type is enough, and an implemented `begin` or `end` never sets the mark, so it is always called.

**Direct scalar writes.** Four dedicated entries carry strings, `i64`, `f64` and `bool`, and `isize` reaches the `i64` one; a `Some` hands the value to its own type's entry. The generic path costs `string_new`, `write_object` and `free_handle`, three crossings and a `GCHandle`.

The narrower widths do not take that shortcut, and the reason is correctness rather than oversight. A dedicated write entry per width would have to widen to fit `long`, and the engine types every operator's answer by its operands' widths, so an `Int32` returning as an `Int64` changes what the caller's next operator does. Each of those widths is built by its own constructor entry and written as a handle, which buys the CLR type for one crossing.

**Function pointers.** On .NET, managed-to-native calls are `delegate* unmanaged` and native-to-managed entries are `UnmanagedCallersOnly` statics. The vtable entries' small helpers are marked for aggressive inlining and the exception paths for no inlining; the entry class skips locals initialization.

**Text.** UTF-16 to UTF-8 and back take a single-pass ASCII path LLVM vectorizes and fall through to std for anything else. Output strings reuse a buffer carried on the cmdlet instance, taken out of its slot for the call so a callee that re-enters with the same slot gets its own.

**Whole-program optimization.** Modules build in the `release` profile with fat LTO and one codegen unit, so the `pwrs` wrappers inline into the cmdlet body. Unwinding stays on because the boundary depends on catching panics.

## What is left, measured

On an Ubuntu 24.04 VM on a Ryzen 7 5700G, with the counters on, per phase: 57 ns per bind, 126 ns per instance creation. The managed packing and transition sit inside a `run_ns_avg` less `native_ns_avg` that the counter's own two timestamp reads and interlocked add largely account for. The cmdlet's own `process`, which includes the engine's downstream work that `WriteObject` runs synchronously, is what remains, and a hand-written C# cmdlet pays that too. End to end, the loop shape costs 1.8 us per call more than the C# cmdlet and the pipeline shape 0.6 us per record more; both shapes are faster than a PowerShell advanced function.

## Where the counters are

`PWRS_TRACE=1` prints them; see [How To Trace A Module](How-To-Trace-A-Module.md). Every figure above is one of those counters or the wall-clock harness, not an estimate.
