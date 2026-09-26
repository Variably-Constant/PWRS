---
title: How To Use Instruction Sets
weight: 46
---

A module's library is native code running inside the host process, so it can use any instruction the CPU offers, AVX2 and AVX-512 among them, in Windows PowerShell 5.1 as in PowerShell 7. What .NET Framework's JIT cannot emit concerns managed code only. Source: `crates/pwrs/src/cpu.rs`, `crates/cargo-pwrs/dotnet/Pwrs.Runtime/CpuCheck.cs`, and `Measure-RustTieredSum` in `examples/hello/src/lib.rs`.

## Build for every CPU, choose at run time

Compile the crate for the baseline, which is what a build with no `-C target-cpu` does, write each wide kernel under `#[target_feature]`, and pick one per call with `pwrs::cpu::has`:

```rust
use pwrs::cpu::{has, Isa};

fn lanes(x: &[f64]) -> [f64; 8] {
    #[cfg(target_arch = "x86_64")]
    {
        if has(Isa::Avx512f) {
            return unsafe { lanes_avx512(x) };
        }
        if has(Isa::Avx2) {
            return unsafe { lanes_avx2(x) };
        }
    }
    lanes_scalar(x)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn lanes_avx512(x: &[f64]) -> [f64; 8] {
    // std::arch::x86_64 intrinsics
}
```

`has` is true when the CPU reports the extension, the operating system enables the state it needs, and `PWRS_CPU_MAX` does not cap it away. `pwrs::cpu::detected` answers without the cap, and `pwrs::cpu::compiled` lists what the crate itself was compiled for. The `multiversion` crate generates the same kind of tiers from one body, and works in a module unchanged.

## What a native-CPU build does

`-C target-cpu=native`, in `RUSTFLAGS` or in a cargo config, compiles every function for the building machine's CPU, and a library built that way runs only on CPUs with the same extensions. The runtime checks before running any of the library's code: `export_module!` records what the crate was compiled for, and an import on a CPU without one of them is refused with the missing extensions named, instead of the session ending at the first such instruction. `cargo pwrs build` warns about a library compiled beyond its target's baseline, and `cargo pwrs publish` refuses one unless `[package.metadata.pwrs] cpu-features` lists what it requires. [How To Fix A Failure](How-To-Fix-A-Failure.md#import-fails) has the message.

Built with `target-cpu=native` on a Ryzen 9 7900X, hello's library required 40 extensions, and `cargo pwrs build` of a module built the same way warned about 38 of them for `x86_64-pc-windows-msvc`, whose baseline already includes `sse3` and `cmpxchg16b`. Imported on a Ryzen 7 2700, in pwsh 7.6.6 and in Windows PowerShell 5.1, it was refused in both, naming the fifteen that CPU lacks: `avx512f`, `avx512bw`, `avx512cd`, `avx512dq`, `avx512vl`, `avx512ifma`, `avx512vbmi`, `avx512vbmi2`, `avx512vnni`, `avx512bitalg`, `avx512vpopcntdq`, `avx512bf16`, `gfni`, `vaes` and `vpclmulqdq`.

## Test every tier on one machine

A machine with AVX-512 runs only the widest tier unless something holds it back. `PWRS_CPU_MAX` caps `has` and the import check at a psABI level, `x86-64`, `x86-64-v2`, `x86-64-v3` or `x86-64-v4`, and `cargo pwrs test` reruns the Pester suites under each level it is given:

```text
cargo pwrs test --release --cpu-tiers x86-64-v4,x86-64-v3,x86-64
```

or, for every run, in the crate's manifest:

```toml
[package.metadata.pwrs]
test-cpu-tiers = ["x86-64-v4", "x86-64-v3", "x86-64"]
```

A library compiled for the native CPU is refused under a cap below it, so the crate is built for the baseline for this.

Tiers that must agree have to add in one order. `Measure-RustTieredSum` in hello sums element `i` into running sum `i % 8` at every tier, eight lanes in one AVX-512 register, two AVX2 registers or an array, and combines the eight in a fixed tree, so its tiers give the same bits. `examples/hello/tests/Cpu.Tests.ps1` compares those bits with the same order computed in script.

## Test a tier the machine lacks

`PWRS_CPU_MAX` lowers what a machine offers; it cannot raise it. Intel SDE can, for a native program: it runs the program under an emulated CPU whose CPUID reports the extensions of the chip named. hello's `tiered_sum_tests` compares every vector tier `pwrs::cpu::detected` reports with scalar, bit for bit. A second test runs only when `SDE_COMMAND_LINE` is set, which SDE sets for the program it runs, and there requires scalar, AVX2 and AVX-512F all compared; on any other run it returns at once, so it never fails in CI. Build the test binary for the baseline, since a native build carries extensions outside the kernels that an emulated chip may lack, and run it under a chip with AVX-512:

```powershell
$env:RUSTFLAGS = '-C target-cpu=x86-64'
cargo test --profile test-fast -p pwrs-example-hello --no-run
sde -spr -- <the unittests executable cargo printed> tiered_sum_tests --nocapture
```

With SDE 10.13.1 on a Ryzen 9 7900X, both tests passed natively, the second returning at once; under `-spr` both passed with the three tiers compared; under `-hsw`, an emulated Haswell without AVX-512, the first passed on scalar and AVX2 and the second failed, naming `-spr`. SDE does not reach a module's Pester suite: neither pwsh 7.6.6 nor Windows PowerShell 5.1 starts under it on Windows 11, where pwsh exits with 0xC000000D before printing anything and `powershell.exe` stops Pin itself.

## Measure the wider tier

A wider tier need not be faster. On a Ryzen 9 7900X, `Measure-RustTieredSum` over a `Double[]` took, per call including the cmdlet's own crossing, the median of seven calls in a fresh process under each cap:

| Host | Doubles | AVX-512 | AVX2 | scalar |
|---|---|---|---|---|
| pwsh 7.6.6 | 262 144 | 0.041 ms | 0.046 ms | 0.099 ms |
| pwsh 7.6.6 | 1 048 576 | 0.097 ms | 0.110 ms | 0.347 ms |
| pwsh 7.6.6 | 8 388 608 | 1.348 ms | 1.353 ms | 2.527 ms |
| Windows PowerShell 5.1 | 262 144 | 0.060 ms | 0.060 ms | 0.112 ms |
| Windows PowerShell 5.1 | 1 048 576 | 0.112 ms | 0.112 ms | 0.337 ms |
| Windows PowerShell 5.1 | 8 388 608 | 1.310 ms | 1.370 ms | 2.560 ms |

The AVX-512 tier was at most 13% faster than AVX2 there, and no faster at two of the sizes, while either was 1.9 to 3.6 times faster than scalar. `PWRS_CPU_MAX` is how to take the same numbers for a module's own kernels on the machines it is meant for.

## Take arrays through a pin

A kernel's input is best a typed array pinned where it lies. A parameter declared as the array type is the engine's binder's to check before the module runs: [docs/PERF.md](https://github.com/Variably-Constant/PWRS/blob/main/docs/PERF.md) measures a 4 MB `byte[]` at 165 ms that way in Windows PowerShell 5.1, where the binder walks the array, against 0.38 ms through a `PsObject` parameter pinned in the body, and at 0.47 ms against 0.26 ms in pwsh 7.6.6, where the difference is the copy. Declare the parameter `#[param(raw)]` and borrow it with `pin::<f64>()`, as `Measure-RustTieredSum` does.
