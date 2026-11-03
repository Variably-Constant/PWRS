---
title: How To Use Instruction Sets
weight: 11
---

A module's library is native code running inside the host process, so it can use any instruction the CPU offers, AVX2 and AVX-512 among them, in Windows PowerShell 5.1 as in PowerShell 7. What .NET Framework's JIT cannot emit concerns managed code only. Source: `crates/pwrs/src/cpu.rs`, `crates/cargo-pwrs/dotnet/Pwrs.Runtime/CpuCheck.cs`, and `Measure-RustTieredSum` in `examples/hello/src/lib.rs`. The tutorial's chapter [Fast On Every CPU](../tutorials/Fast-On-Every-CPU.md) goes through it a step at a time with FileFacts.

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

`has` is true when the CPU reports the extension, the operating system enables the state it needs, and `PWRS_CPU_MAX` does not cap it away. `pwrs::cpu::detected` returns what the CPU has, ignoring the cap, and `pwrs::cpu::compiled` lists what the crate itself was compiled for. `pwrs::cpu::level()` is the highest psABI level whose every extension `has` reports, computed once per process. Hand-written C# in the module gets the same values from this library through `Pwrs.Cpu`: `Pwrs.Cpu.Has("avx2")` is `has(Isa::Avx2)`, and [How To Add Hybrid C#](How-To-Add-Hybrid-CSharp.md#choosing-a-path-by-the-cpu) lists the rest.

## One body, every tier

`#[pwrs::tiered]` writes the tiers from one body. The body is compiled once for each x86-64 psABI level above the baseline (`x86-64-v2`, `x86-64-v3`, `x86-64-v4`) and each AArch64 feature tier above `neon` (`dotprod`, `i8mm`, `sve`, `sve2`), and the function runs the copy for the tier `pwrs::cpu::level()` or `pwrs::cpu::arm::tier()` returns. Any other architecture runs the baseline copy.

```rust
#[pwrs::tiered]
fn squares(values: &[f32], out: &mut [f32]) {
    for (o, v) in out.iter_mut().zip(values) {
        *o = v * v + 0.5;
    }
}
```

Each copy is the same Rust compiled with more target features. The compiler may vectorize it, but never reorders or fuses floating-point arithmetic, so every copy gives the same bits. `crates/pwrs/tests/tiered.rs` runs five such functions under every tier the machine supports, over inputs that include NaNs, infinities and subnormals, and compares a digest of their outputs. On a Ryzen 9 7900X the four x86-64 tiers gave the same digest.

The attribute takes a free function whose arguments are plain names, generic over types and constants or not. It refuses an async, const or extern function, a method, a pattern argument, and an `impl Trait` argument or return type, each with the reason.

## Ready-made kernels

`pwrs::simd` has kernels with a body for each tier written with the architecture's own instructions, and a bit-exact contract: every tier returns what the kernel's scalar form returns, for every input. A kernel runs the body for the tier `level()` or `arm::tier()` returns, so `PWRS_CPU_MAX` and `--cpu-tiers` choose among them as among a module's own.

| Kernel | What it returns | Bodies |
|---|---|---|
| `simd::bytes::find(haystack, needle)` | the leftmost position of `needle`, as `find_scalar` | SSE2, AVX2, AVX-512, NEON |
| `simd::bytes::count(haystack, byte)` | how many times `byte` occurs, as `count_scalar` | SSE2, AVX2, AVX-512, NEON |
| `simd::bytes::nth_byte(haystack, byte, n)`, `nth_byte_back(haystack, byte, n)` | the position of the `n`th occurrence of `byte` counting from one at the start or at the end, `None` for fewer or for 0 | SSE2, AVX2, AVX-512, NEON |
| `simd::bytes::ByteClass` | for a set of ASCII bytes or its complement, the run a slice starts with (`run`), its first class byte (`find`), its longest run, whether it has a run of `least` (`has_run_of`) and every run at least `least` long (`runs`); `WORD`, `WHITESPACE` and `DIGIT` are ready-made | SSSE3, AVX2 and AVX-512BW shuffles, NEON TBL |
| `simd::ints::sum_u64(values)`, `xor_u64(values)` | the wrapping sum, and every value XORed together | SSE2, AVX2, AVX-512, NEON |
| `simd::ints::count_ones(values)` | how many bits are set across the slice | POPCNT, AVX2 and AVX-512BW nibble tables, AVX-512 VPOPCNTDQ, NEON |
| `simd::ints::min_i64(values)`, `max_i64(values)` | the smallest and largest value, `None` for none | SSE4.2, AVX2, AVX-512, NEON |
| `simd::ints::and_u64`, `or_u64`, `and_not_u64(a, b, out)` | the element-wise operation over the shortest of the three slices | SSE2, AVX2, AVX-512, NEON |
| `simd::filter::eq_i32(column, value)`, `range_i32(column, lo, hi)`, `in_i32(column, values)` | a bitmap of the rows equal to `value`, from `lo` to `hi` inclusive, or equal to any of `values`, row `i` at bit `i % 64` of word `i / 64`; `indices` and `indices_page` read it back into rows | SSE2, AVX2, AVX-512, NEON |
| `simd::distance::l1_u8(a, b)`, `l1_u32(a, b)` | the sum of the absolute differences of `a[i]` and `b[i]` over the shorter slice, as a `u64` | SSE2 `psadbw`, AVX2, AVX-512BW, NEON |
| `simd::floats::sum_f64(values)`, `sum_f32(values)` | the sum in the module's order, 0.0 for none | SSE2, AVX2, AVX-512, NEON |
| `simd::floats::dot_f64(a, b)`, `dot_f32(a, b)` | the sum of `a[i] * b[i]` over the shorter slice, each product rounded on its own | SSE2, AVX2, AVX-512, NEON |
| `simd::floats::axpy_f64(y, a, x)`, `axpy_f32(y, a, x)` | `y[i] += a * x[i]` over the shorter slice, each product rounded on its own | SSE2, AVX, AVX-512, NEON |
| `simd::floats::prefix_sum_f32(values)`, `prefix_sum_f64(values)` | each value turned into the sum of it and every value before it, in the order the paragraph below gives | SSE2, AVX2, AVX-512, NEON |
| `simd::floats::compensated::sum_f64(values)`, `dot_f64(a, b)` | the same sum and dot of `f64` with every rounding error recorded and added back, nearly as accurate as twice the precision rounded once | SSE2, AVX2, AVX-512, NEON; the dot with FMA from x86-64-v3 and on NEON |
| `simd::checksums::crc32c(data)`, `crc32c_append(crc, data)` | the CRC-32C, `0xE3069283` for `b"123456789"`, a stream a piece at a time | SSE4.2 `crc32`, AArch64 `crc` |
| `simd::checksums::crc32(data)`, `crc32_append(crc, data)` | the CRC-32 zip, gzip and PNG use, `0xCBF43926` for `b"123456789"`, a stream a piece at a time | PCLMULQDQ folding from x86-64-v2, VPCLMULQDQ at 256 and 512 bits, AArch64 PMULL |
| `simd::gf256::mul_add(out, src, coef)` | `out[i] ^= coef * src[i]` in GF(256) with polynomial `0x11D`, the Reed-Solomon field | SSSE3, AVX2 and AVX-512BW nibble tables, GFNI at 256 and 512 bits, NEON TBL |

Integer addition and XOR give the same total in any order, and a minimum is the same whatever order the values are compared in, so these return what their scalar forms return, whatever the lane width. A floating-point sum is different: adding in lanes changes where it rounds, so `simd::floats` makes the order part of the result. Element `i` goes into running sum `i % 8` for `f64` and `i % 16` for `f32`. The sums are then combined by halving: the first half adds the second until one sum is left, which for eight sums is `Measure-RustTieredSum`'s tree. Every tier uses those running sums and combines them the same way, so each returns the scalar form's bits; a NaN result is a NaN at every tier, with the payload Rust leaves unspecified. The result can differ in its last bits from a left-to-right loop's.

`simd::floats::compensated` uses the same running sums and records, beside each one, the exact rounding error of every addition into it. A dot product also records each product's own rounding error, which a fused multiply-add gives exactly. The errors are added to the result at the end, unless the result is an infinity or a NaN, so a sum that cancels comes out nearly as accurate as one computed in twice the precision.

A prefix sum has an order of its own. In each block of 16 `f32` or 8 `f64` values from the start, each value adds the value one place before it, then two places before, then four, and for `f32` eight. Each step adds the values the step before left, and adds `-0.0` where there is nothing that far back, which leaves a value unchanged. Then the whole block adds the last sum of the block before it.

The crate's tests call every body the CPU runs with inputs on both sides of each vector width and compare each result with the scalar form's result. They print each tier they ran, and each tier they skipped because the CPU lacks its instructions.

## What a native-CPU build does

`-C target-cpu=native`, in `RUSTFLAGS` or in a cargo config, compiles every function for the building machine's CPU, and a library built that way runs only on CPUs with the same extensions. The runtime checks before running any of the library's code. `export_module!` records what the crate was compiled for, and an import on a CPU without one of them is refused with the missing extensions named, instead of the session ending at the first such instruction. `cargo pwrs build` warns about a library compiled beyond its target's baseline, and `cargo pwrs publish` refuses one unless `[package.metadata.pwrs] cpu-features` lists what it requires. [How To Fix A Failure](How-To-Fix-A-Failure.md#import-fails) has the message.

Built with `target-cpu=native` on a Ryzen 9 7900X, hello's library required 40 extensions, and `cargo pwrs build` of a module built the same way warned about 38 of them for `x86_64-pc-windows-msvc`, whose baseline already includes `sse3` and `cmpxchg16b`. Imported on a Ryzen 7 2700, in pwsh 7.6.6 and in Windows PowerShell 5.1, it was refused in both, naming the fifteen that CPU lacks: `avx512f`, `avx512bw`, `avx512cd`, `avx512dq`, `avx512vl`, `avx512ifma`, `avx512vbmi`, `avx512vbmi2`, `avx512vnni`, `avx512bitalg`, `avx512vpopcntdq`, `avx512bf16`, `gfni`, `vaes` and `vpclmulqdq`.

## Test every tier on one machine

A machine with AVX-512 runs only the widest tier unless something limits it. `PWRS_CPU_MAX` caps `has` and the import check at a psABI level, `x86-64`, `x86-64-v2`, `x86-64-v3` or `x86-64-v4`, and `cargo pwrs test` reruns the Pester suites under each level it is given:

```text
cargo pwrs test --release --cpu-tiers x86-64-v4,x86-64-v3,x86-64
```

or, for every run, in the crate's manifest:

```toml
[package.metadata.pwrs]
test-cpu-tiers = ["x86-64-v4", "x86-64-v3", "x86-64"]
```

A library compiled for the native CPU is refused under a cap below it, so the crate is built for the baseline for this.

On x86-64 a tier run also limits each host's JIT to the tier, so hand-written C# takes the vector path the tier allows, as the Rust kernels do. pwsh gets .NET's switches set to 0. Under .NET 10 the switch is `DOTNET_EnableSSE42` for `x86-64`, `DOTNET_EnableAVX2` for `x86-64-v2` and `DOTNET_EnableAVX512` for `x86-64-v3`; each one turns off the extensions of every level above. Under .NET 8 and 9 there is one switch per extension: `DOTNET_EnableSSE3` and `DOTNET_EnableAVX` also turn off the extensions built on them, and `DOTNET_EnableLZCNT` turns off LZCNT alone. The run sets both kinds, since each runtime ignores the other's. Before each tier's run, `cargo pwrs test` starts a pwsh with the switches set and reads which extensions above the tier `System.Runtime.Intrinsics.X86` still reports there. If there are any, it stops and names them, so a .NET that ignores the switches cannot run the suites at the machine's own level under the tier's name. Windows PowerShell's .NET Framework 4.8 JIT reads none of these: it widens `Vector<T>` to AVX2 wherever the CPU has it. The one switch it reads, `COMPlus_FeatureSIMD=0`, turns `Vector<T>`'s acceleration off altogether. So a run below `x86-64-v3` sets that one, and there `Vector<T>` runs slower than a scalar loop.

On a Ryzen 9 7900X, pwsh 7.6.9 on .NET 10.0.14 reported SSE2 alone under the `x86-64` switches, SSE4.2 and POPCNT without AVX, BMI or LZCNT under `x86-64-v2`'s, and AVX2 without AVX-512 under `x86-64-v3`'s. `Vector<float>` was 4 wide under the `x86-64` and `x86-64-v2` switches and 8 wide under `x86-64-v3`'s. .NET 9.0.19 needed `DOTNET_EnableAVX` and `DOTNET_EnableLZCNT` beside `DOTNET_EnableAVX2` to stop at `x86-64-v2`. Windows PowerShell 5.1, with the .NET Framework build of `System.Numerics.Vectors` loaded, accelerated `Vector<float>` 8 wide and summed 4 million floats about 5.8 times as fast as a scalar loop. It ignored `COMPlus_EnableAVX2` and `COMPlus_EnableAVX`, and under `COMPlus_FeatureSIMD=0` it ran `Vector<float>` 4 wide without acceleration, about 7 times slower than the loop. `Hybrid.Tests.ps1` in hello checks the path `Measure-RustHybridDot` takes under each tier. Below `x86-64-v3` it is `Vector`, 4 lanes wide, in pwsh and `Scalar` in Windows PowerShell. From it up, it is `Vector256` in pwsh where the CPU has AVX2 and `Vector` in Windows PowerShell. Run under `x86-64`, `x86-64-v2` and `x86-64-v3` on that machine, hello's suites passed in both hosts under each.

Tiers that must agree have to add in one order. `Measure-RustTieredSum` in hello sums element `i` into running sum `i % 8` at every tier: eight lanes in one AVX-512 register, two AVX2 registers or an array. It combines the eight in a fixed tree, so its tiers give the same bits. `examples/hello/tests/Cpu.Tests.ps1` compares those bits with the same order computed in script.

## AArch64

On AArch64 the operating system reports the CPU's features, and `pwrs::cpu::arm` returns what it reports: `arm::has(arm::Feature::Sve)`, `arm::detected`, `arm::compiled`, and `arm::tier()`, the highest feature tier whose every feature `has` reports. `PWRS_CPU_MAX` names a feature tier there, `neon`, `dotprod`, `i8mm`, `sve` or `sve2`, each including the ones before it, and `cargo pwrs test --cpu-tiers` takes those names on an AArch64 machine. A feature no tier includes, such as `crc`, `aes` or `sha2`, is withheld under any cap, as an x86-64 extension outside the psABI levels is. The runtime refuses the other architecture's names as it refuses any name it does not know, so a cap meant for one machine cannot pass silently on another.

Before any of the library's code runs, the runtime's import check reads each feature the library was compiled for beyond its target's baseline from the operating system, through the calls Rust's own detection uses there: `getauxval` on Linux, `IsProcessorFeaturePresent` on Windows and `sysctlbyname` on macOS. The baseline is the features the target compiles every crate with: `neon` everywhere, and on macOS fifteen, among them `dotprod`, `aes`, `crc` and `sha3`. Every CPU that runs the target has them, and a system's report of one can be missing, as macOS's name for Advanced SIMD is on some versions, so the check does not read them. It refuses a library compiled for a feature the CPU lacks, and names the feature. It also refuses a library compiled for a feature the operating system has no way to report, since it cannot confirm that the CPU has it. Windows reports none of `sm4`, `fhm`, `fcma`, `frintts`, `dit` and `rand`, and macOS reports none of `sve`, `sve2` and its four extensions, `f32mm`, `f64mm`, `sm4` and `rand`. A build for the target's baseline needs none of those, but a `-C target-cpu=native` build on a Snapdragon X takes all six that Windows cannot report, so it is refused even on that machine; build for the baseline and choose at run time. Under `PWRS_CPU_MAX` the check also refuses a feature above the cap's tier, again except the baseline, so a macOS module imports under any cap, while `arm::has` and `arm::tier` still honor the cap and a kernel ladder takes its `neon` body under `PWRS_CPU_MAX=neon`.

hello's `Cpu.Tests.ps1` gives the check each system's list, with every feature in it, and stand-in functions in place of the system's calls, so the lists of all three systems are checked on any machine and in both hosts. It also makes the check's real call on the machine running the suite, for a feature every such machine has. Under `qemu-aarch64` the crate's tests compare every Linux report with Rust's own detection. On Apple silicon in CI they compare every macOS report beyond the baseline, and print how the baseline's names read there. The Windows on Arm path has not run on Arm hardware.

An x86-64 Linux machine runs AArch64 tests under `qemu-aarch64`, whose `max` CPU has every tier up to `sve2`:

```text
QEMU_CPU=max QEMU_LD_PREFIX=/usr/aarch64-linux-gnu \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER=qemu-aarch64 \
cargo test --target aarch64-unknown-linux-gnu
```

The emulator gives each tier's results, not its speed.

## Test a tier the machine lacks

`PWRS_CPU_MAX` lowers what a machine offers; it cannot raise it. Intel SDE can, for a native program: it runs the program under an emulated CPU whose CPUID reports the extensions of the chip named. hello's `tiered_sum_tests` compares every vector tier `pwrs::cpu::detected` reports with scalar, bit for bit. A second test runs only when `SDE_COMMAND_LINE` is set, which SDE sets for the program it runs, and there requires scalar, AVX2 and AVX-512F all compared; on any other run it returns at once, so it never fails in CI. Build the test binary for the baseline, since a native build uses extensions outside the kernels that an emulated chip may lack, and run it under a chip with AVX-512:

```powershell
$env:RUSTFLAGS = '-C target-cpu=x86-64'
cargo test --profile test-fast -p pwrs-example-hello --no-run
sde -spr -- <the unittests executable cargo printed> tiered_sum_tests --nocapture
```

With SDE 10.13.1 on a Ryzen 9 7900X, both tests passed natively, the second returning at once. Under `-spr` both passed with the three tiers compared. Under `-hsw`, an emulated Haswell without AVX-512, the first passed on scalar and AVX2 and the second failed, naming `-spr`. SDE does not reach a module's Pester suite: neither pwsh 7.6.6 nor Windows PowerShell 5.1 starts under it on Windows 11, where pwsh exits with 0xC000000D before printing anything and `powershell.exe` stops Pin itself.

## Measure the wider tier

A wider tier need not be faster. On a Ryzen 9 7900X, `Measure-RustTieredSum` over a `Double[]` took, per call, including the cmdlet's own call into Rust, the median of seven calls in a fresh process under each cap:

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

A kernel's input is best a typed array pinned where it lies. PowerShell's binder checks a parameter declared as the array type before the module runs. [docs/PERF.md](https://github.com/Variably-Constant/PWRS/blob/main/docs/PERF.md) measures a 4 MB `byte[]` taken that way at 159 ms in Windows PowerShell 5.1, where the binder goes through every element, against 0.44 ms through a `PsObject` parameter pinned in the body. In pwsh 7.6.6 the two cost 0.43 ms and 0.24 ms, and the difference is the copy. Declare the parameter `#[param(raw)]` and borrow it with `pin::<f64>()`, as `Measure-RustTieredSum` does.
