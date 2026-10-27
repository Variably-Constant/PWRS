---
title: The Call Path
weight: 3
---

What one invocation of a Rust cmdlet costs, piece by piece. Source: `crates/cargo-pwrs/dotnet/Pwrs.Runtime/RustCmdlet.cs`, the generated `Run(phase)` in `crates/cargo-pwrs/src/generate.rs`, `crates/pwrs/src/cmdlet.rs` and `runtime.rs`, `pipeline.rs`, `text.rs`, `trace.rs`. Numbers are from [Benchmarks](Benchmarks.md).

## One invocation, step by step

For `Get-Greeting -Name x` typed at the prompt:

1. The engine constructs the generated cmdlet object and binds `-Name` by calling the generated property setter, which stores the value and sets bit 0 of two words, `_bound` and `_dirty`.
2. The engine calls `BeginProcessing`. The generated override calls `NeedsPhase(Begin)`, which creates the Rust instance (`pwrs_cmdlet_create`, one allocation with a dispatch header and the cmdlet struct in it) and gets the type's phase mask with it. For the first instance of the type the mask has all three phases set. Begin runs: the block is packed on the stack (dirty word, bound word, the pinned string), and one native call is made. The Rust side binds the parameters because the block is dirty, calls `begin`, which is the trait's default and marks the token, and returns. The runtime clears Begin from the type's mask.
3. `ProcessRecord`: the dirty word is now zero, so the Rust side skips the bind and calls `process`. Its `verbose!` checks once whether a verbose record would go somewhere, with one call into .NET, and builds no text when it would not. It then formats the greeting and writes it through `write_string`: one call, no `GCHandle`, the managed side allocates the .NET string and calls `WriteObject`, which runs the downstream pipeline synchronously.
4. `EndProcessing`: the default `end` marks the token; the runtime clears End from the mask.
5. `Dispose`: `pwrs_cmdlet_release` frees the instance, and the cmdlet's handle to itself is freed.

For the second and every later instance of `Get-Greeting`, step 2 receives a mask with only Process set. `BeginProcessing` then clears the dirty word and returns without a native call, and `EndProcessing` returns without one too. The whole invocation is one create, one native call with one bind (the instance has not been bound yet), one stream check, one write, and one release. Clearing the word stops the arguments, assigned before `begin`, from reading as piped to the surface check of `cargo pwrs test`, which counts a parameter assigned during `process` as one that came down the pipeline. For pipeline input, one instance handles every record: the setter runs per record, the dirty word is set, and each `ProcessRecord` binds and runs.

## The pieces

**One allocation per cmdlet.** The .NET cmdlet stores a pointer to its Rust instance for its whole life. There is no global table keyed by handle, so an invocation takes no lock and does no lookup; the native side reads the pointer it is given.

**Bind on change only.** The generated setters record which parameters were bound, a bit each in masks of one 64-bit word per 64 parameters. The engine calls a setter only for a supplied parameter, so there is no dictionary lookup. The Rust side binds before the first phase and again only when a dirty word is non-zero.

**Phase masks recorded per type.** Rust cannot check whether a trait method was overridden, but the default bodies are PWRS code and can leave a mark when they run. Recording that once per type is enough, and an implemented `begin` or `end` never sets the mark, so it is always called.

**Direct scalar writes.** Four entries of their own write strings, `i64`, `f64` and `bool`, and `isize` uses the `i64` one; a `Some` passes its value to its own type's entry. The generic path costs `string_new`, `write_object` and `free_handle`, three calls into .NET and a `GCHandle`.

The other numeric types take the generic path: each is built by its own constructor entry and written as a handle, three calls, so it arrives as its own CLR type. That matters because in PowerShell the type of an operator's result depends on the types of its operands. An `Int32` that arrived as an `Int64` would change what the caller's next operator does, and an `f32` widened to a `Double` would show `0.1` as `0.100000001490116`.

**Function pointers.** On .NET, managed-to-native calls are `delegate* unmanaged` and native-to-managed entries are `UnmanagedCallersOnly` statics. The vtable entries' small helpers are marked for aggressive inlining and the exception paths for no inlining; the entry class skips locals initialization.

**Text.** UTF-16 to UTF-8 and back take a single-pass ASCII path LLVM vectorizes and fall through to std for anything else. Output strings reuse a buffer stored on the cmdlet instance, taken out of its slot for the call so a callee that re-enters with the same slot gets its own.

**Whole-program optimization.** Modules build in the `release` profile with fat LTO and one codegen unit, so the `pwrs` wrappers inline into the cmdlet body. Unwinding stays on because PWRS catches panics before they leave Rust.

## What is left, measured

On an Ubuntu 24.04 VM on a Ryzen 7 5700G, with the counters on, per phase: 57 ns per bind, 126 ns per instance creation. The managed packing and transition are inside the difference between `run_ns_avg` and `native_ns_avg`, and the counter's own two timestamp reads and interlocked add account for most of that difference. The cmdlet's own `process`, which includes the time the later commands in the pipeline take, since `WriteObject` runs them before it returns, is what remains, and a hand-written C# cmdlet pays that too. End to end, the loop shape costs 1.8 us per call more than the C# cmdlet and the pipeline shape 0.6 us per record more; both shapes are faster than a PowerShell advanced function.

## Where the counters are

`PWRS_TRACE=1` prints them; see [How To Trace A Module](How-To-Trace-A-Module.md). Every figure above is one of those counters or the wall-clock harness, not an estimate.
