---
title: How To Fix A Failure
weight: 13
---

What each failure means, keyed by the text you see. Every message
below is the literal one, from `crates/cargo-pwrs/dotnet/Pwrs.Runtime/NativeModule.cs`,
`crates/pwrs/src/host.rs`, `crates/cargo-pwrs/src/descriptor.rs` and
`crates/pwrs-build/src/pwsh.rs`.

## The crate and `cargo-pwrs` move together

**This is the one to check first.** `PoWerRuSt`, `pwrs-sys`,
`pwrs-macros`, `pwrs-build` and `cargo-pwrs` are published together
at one version, and a module is built from two halves that both have
to be that version: the crate your code links, and the C# runtime
`cargo-pwrs` embeds in the module folder. Bumping one alone is the
most common way to a module that will not import.

```text
pwrs_module_init failed with status 4 (runtime ABI 1)
```

The module expects more host vtable entries than the runtime that
built it provides. You updated the `PoWerRuSt` crate and built with
an older `cargo-pwrs`. Update the tool:

```text
cargo install cargo-pwrs --force
```

The refusal is deliberate and it is the safe direction: the module
checks the table's `size` header before using any entry, so an old
runtime is caught at `pwrs_module_init` rather than by calling
through a slot that is not there. The reverse pairing, an older crate
with a newer tool, works: the table only grows.

```text
descriptor ABI 2 is not supported by this cargo-pwrs
```

The same skew at build time rather than import time, and only when
the ABI version itself changes rather than merely growing: the crate
writes a descriptor the tool does not read. Update the tool. An ABI
version change also renames the exports (`pwrs2_*`), so the two
halves cannot be mixed silently.

## Two modules in one Windows PowerShell session

```text
Exception calling "ReloadNative" with "0" argument(s): "The type initializer for 'Pwrs.Modules.Gamma.PwrsModule' threw an exception."
```

Seen only in Windows PowerShell 5.1, only on the second of two PWRS
modules imported into one session, and only when that module declares
classes or enums. It means one of the two was built by `cargo-pwrs`
0.1.8 or earlier and the other by 0.2.0 or later. The first import
works and keeps working; the second is refused. A second module that
declares neither imports and runs beside either.

Windows PowerShell has one load context per process, so the first
module's `Pwrs.Runtime` serves every PWRS module imported after it,
and a module's shell registers its classes and enums with the runtime
it gets. The message the engine prints names only the type
initializer. The cause underneath it is a
`System.MissingMethodException` naming the runtime member that
registration calls: `Pwrs.Factories.Register(UInt32, Func<IntPtr, Object>)`
for a module built by `cargo-pwrs` 0.1.8 or earlier meeting a 0.2.0
runtime, and `Pwrs.NativeModule.get_Factories()` the other way round.

Rebuild both modules with the same `cargo-pwrs`. PowerShell 7 gives each
module a load context and a runtime of its own, so there two modules
built by different versions import side by side. Both editions were
measured in every order of a 0.1.8 and a 0.2.0 module, one of the two
declaring classes and an enum and the other neither, on Windows
PowerShell 5.1.26100.9444 and pwsh 7.6.6.

## A module that loads everywhere but the snap's pwsh

```text
Exception calling "ReloadNative" with "0" argument(s): "The type initializer for 'Pwrs.Modules.MyModule.PwrsModule' threw an exception."
```

On Linux, under a pwsh installed as a snap, the same message can come
from the native library itself. The snap's pwsh runs on the glibc of
the snap's base, 2.35 for core22, so a library built on a host whose
glibc is newer, and which uses a symbol from it, does not load. The
cause sits three exceptions down, and running the module type's
initializer again from script brings it back, here for a module named
`MyModule`:

```powershell
$shell = [AppDomain]::CurrentDomain.GetAssemblies() | Where-Object { $_.GetName().Name -like 'MyModule.Shell.*' }
[System.Runtime.CompilerServices.RuntimeHelpers]::RunClassConstructor($shell.GetType('Pwrs.Modules.MyModule.PwrsModule', $true).TypeHandle)
```

```text
System.DllNotFoundException: Unable to load shared library ... /snap/core22/current/lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found
```

`objdump -T` on the library lists the glibc versions it needs. The
same library loads under a pwsh that runs on the host's own
libraries, such as Microsoft's tarball install. Measured on Ubuntu
24.04, whose glibc is 2.39, with the powershell snap's pwsh 7.6.5, two
of this repository's examples fail this way. `examples/tls`'s library
needs `GLIBC_2.38` for `__isoc23_sscanf` and `__isoc23_strtol`:
aws-lc's C code calls `sscanf` and `strtol`, and glibc 2.38's headers
send those calls to the `__isoc23_` versions for C compiled with
`_GNU_SOURCE`. `examples/hello`'s needs `GLIBC_2.39` for `pidfd_spawnp`
and `pidfd_getpid`, which the Rust standard library links for starting
a process, as hello's helper cmdlets do. calc and memfs need nothing
past `GLIBC_2.34` and load under either.

To load under the snap's pwsh, build the library against the older
glibc. [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild)
links through zig against the glibc whose version follows the target
triple, and `cargo pwrs` hands such a triple to it:

```text
cargo install cargo-zigbuild --locked
cargo pwrs test --release --target x86_64-unknown-linux-gnu.2.35
```

with [zig](https://ziglang.org/download/) on `PATH`. The package's
`[[bin]]` helpers build for the same triple. On the same VM, with zig
0.16.0 and cargo-zigbuild 0.23.4, the tls and hello libraries and
hello's helper built this way need nothing past `GLIBC_2.34`, and under
the snap's pwsh 7.6.5 tls passed 4 of 4, hello 282 of 282 and memfs 11
of 11.

## Import fails

```text
pwrs native library hello.dll for win-x64 not found under ...
```

The native library for this platform is missing from the module
folder. `cargo pwrs build` writes it to
`runtimes/<rid>/native/`, one per platform; a folder built on one
machine and copied to another carries only that machine's. Build on
the target, or join the folders with `cargo pwrs merge`.

```text
System.DllNotFoundException
System.EntryPointNotFoundException: pwrs_cmdlet_create
```

The library was found but is not a pwrs module, or was built from a
crate with no `export_module!`. Every export in
[ABI Reference](../reference/abi-reference/) is required except
`pwrs_module_lifecycle`, `pwrs_transform_invoke` and the three CPU-check
exports, which are optional; a missing one names itself.

```text
Hello needs PowerShell 7.4 or later: its PowerShell 7 half is built against .NET 8, and this is PowerShell 7.3.9 on .NET 7.0.20.
```

The host is a PowerShell 7 older than 7.4. A module's PowerShell 7
half is compiled against .NET 8's reference pack and the
`System.Management.Automation` 7.4 reference, and a host loads it only
when it carries at least those versions; when the loader cannot be
reached on such a host, the module's script reports this in place of
the missing-type error. Run it in PowerShell 7.4 or later, or in
Windows PowerShell 5.1.

```text
Unable to find type [Pwrs.Bootstrap.Loader].
Could not load file or assembly 'System.Diagnostics.Process, Version=9.0.0.0'
```

A module built by cargo-pwrs 0.2.0 or earlier compiled its PowerShell 7
half against the pwsh that built it, so it references that pwsh's .NET
and does not load on an older one. Measured by a consumer: a module
built on pwsh 7.6.6 failed with the first message on 7.5.11, 7.4.20 and
FreeBSD's 7.5.5, and one built on FreeBSD's 7.5.5 failed with the
second on 7.4.20. The cargo-pwrs in this repository compiles that half
against .NET 8 on every machine, so rebuilding the module with it
removes both.

```text
hello.dll was compiled for instruction-set extensions it cannot use here: this CPU does not offer avx512f, avx512bw, ...
```

The native library was compiled for extensions this CPU lacks, usually
by `-C target-cpu=native` in `RUSTFLAGS` or in a cargo config on the
machine that built it. Loading it anyway would end the session at the
first such instruction, so the runtime asks the CPU before running any
of the library's code and refuses the import instead. `cargo pwrs build`
warned about it at the time. Build with
`RUSTFLAGS='-C target-cpu=x86-64'` for a module that loads on any
x86-64 CPU; a module that wants wide instructions picks them at run
time, as [How To Use Instruction Sets](How-To-Use-Instruction-Sets.md)
shows.

The same message with `PWRS_CPU_MAX=<level> withholds ...` means the
variable is set in this process and caps what the module may use below
what it was compiled for. The variable is for testing a module's lower
tiers; unset it, or build the module at the baseline.

## Building fails

```text
cannot run pwsh: ...
```

`cargo pwrs` needs pwsh to fetch the C# compiler and to run it, since
the compiler runs inside the pwsh process. Install PowerShell 7, or
point `PWRS_PWSH` at the executable if it is not on `PATH`.

```text
neither PWRS_HOME, USERPROFILE nor HOME is set; cannot place the toolchain
no bincore/csc.dll under ...
```

The C# compiler is fetched once into `~/.pwrs/toolchain/` and run on
the runtime pwsh already ships; no .NET SDK is involved. Set
`PWRS_HOME` to say where it goes, and `PWRS_TOOLSET` to pin a
different `Microsoft.Net.Compilers.Toolset` version. Each version
gets its own tree, so switching neither refetches nor mixes two
compilers.

## A rebuild does not take effect

The running session holds the library it loaded. `cargo pwrs build`
writes to the module folder, and the next `Import-Module -Force`
stages a fresh copy and takes it over; see
[How To Reload A Module](How-To-Reload-A-Module.md). Windows keeps a
mapped file locked, which is why the load is always a staged copy and
the path the build writes stays free. A value made before the reload
is refused by the proxy that holds it rather than read against the
new body.

## A cmdlet fails at run time

```text
<your message>
```

A cmdlet's `Err(PsError)` is an ordinary non-terminating error: the
engine applies `-ErrorAction`, and `-ErrorVariable` collects the
record. `.terminating()` raises it through `ThrowTerminatingError`
instead. Nothing about this is special to pwrs, which is the point.

A Rust panic crosses as a terminating error carrying the panic
message, and the module keeps working afterwards; it is caught at the
boundary because unwinding into the runtime is undefined. If you see
a panic message as a PowerShell error, the fix is in your Rust, not
in the binding.

```text
MethodInvocationException: Exception calling "ReadLine" with "0" argument(s):
"PowerShell is in NonInteractive mode. Read and Prompt functionality is not available."
```

The host cannot prompt. `ps.host_ui()` reaches `$Host.UI`, and a host
run with `-NonInteractive` refuses; the error arrives as a
`Pwrs.PwrsException` with id `PwrsRuntimeError` and the engine's own
text inside it (Windows PowerShell says "Windows PowerShell is in
NonInteractive mode"). A command that must run unattended
should take a parameter that answers the question instead of asking.
The console host answers `read_line` and `prompt_for_choice` from
redirected standard input, but not `read_line_as_secure_string`,
which reads the console device: a script piping input to a command
that asks for a secret waits forever.

```text
this Series is in use by a call already running on this thread; it can be reached again once that call returns
```

A proxy object was reached from inside an exclusive call into the same
object on the same thread: a method taking `&mut self`, or a script
block run while a cmdlet holds the object through `PsProxy::with_mut`.
A `&mut self` method given its own receiver as a by-value argument is
the common shape, since the argument is read back through the object's
getters while the method holds the only reference. The object is fine,
and the call was refused rather than handed the value a second time. A
`&self` method, a property read and a `with` borrow are shared entries
that nest, so they are never refused this way by each other.
Copy what the script needs out of `with`, and run the script after the
closure returns. A property read in the same place gives `$null`
instead of this message, since PowerShell's property adapter turns a
getter's exception into `$null`; see
[How To Pass Native Data Between Cmdlets](How-To-Pass-Native-Data.md).

```text
a proxy object of this module was expected, and a System.String was passed
a Hello.Counter was passed where another class, or another module's class, was expected
```

`PsProxy::with` or `with_mut` was reached with an object that is not
the module's own object of that class. A `PsProxy<T>` parameter cannot
bind anything else, since the binder checks the type first, so this
comes from a `PsProxy` made from an arbitrary `PsObject` with
`FromPs`, or on Windows PowerShell from another module's object, which
one runtime there serves as well.

## The shell exits with a memory allocation message

```text
memory allocation of <n> bytes failed
```

pwsh or powershell.exe printed this and ended. A module asked the
allocator for memory through an allocation that cannot report
failure (`Vec::with_capacity`, `vec![0; n]`, a push that grows), the
allocator refused, and Rust's answer to that is to abort the process.
An abort is not a panic: nothing at the boundary can catch it.

The fix is in the module. Memory whose size comes from input is
reserved first with `try_reserve` or `try_reserve_exact`, and `?`
turns a refusal into an ordinary error record with id
`PwrsOutOfMemory` and category `ResourceUnavailable`, after which the
next command runs. PWRS's own conversions of managed strings, arrays,
hashtables, `BigInteger` and `SecureString` values, and every string
parameter, reserve that way.

Two things stay fatal. An infallible allocation anywhere else, in the
module or in a crate it uses, still aborts; a custom global allocator
that returns null does not help, because Rust answers the null with
the same abort. And on Linux, where the kernel overcommits memory by
default, a process can be ended by the out-of-memory killer before
any allocation reports failure.

## Tests fail before they run

```text
PWRS_MODULE is not set
```

The suite is being run directly rather than through
`cargo pwrs test`, which sets that variable to the built module
folder. Run the command, or set it yourself and run `Invoke-Pester`.

```text
pwrs: Pester would not load in Core 7.6.6. Tried: Pester 5.7.1 at C:\Users\me\OneDrive\Documents\PowerShell\Modules\Pester\5.7.1. It said: The cloud file provider is not running. Set PWRS_PESTER_PATH to a Pester module that loads in this host, such as one saved with Save-Module -Name Pester.
```

`cargo pwrs test` could not import Pester in the host named. A project
with a saved copy at `target/pester/Pester` uses that; any other takes
the host's own Pester 4 or later, and "Tried" lists each one found.
"It said" is the import's own error: a OneDrive copy that the OneDrive
client is not running to download reads as above. Save a copy that
loads, `Save-Module -Name Pester -Path <folder>`, and set
`PWRS_PESTER_PATH` to the `Pester` folder inside it.

## Nothing above matches

`PWRS_TRACE=1` prints both sides' counters to stderr every 10000
events and `PWRS_TRACE=2` prints every event on the Rust side; see
[How To Trace A Module](How-To-Trace-A-Module.md). It has to be in
the environment the process inherits, because the Rust side reads it
with `getenv`: setting `$env:PWRS_TRACE` inside PowerShell was
observed on Linux to reach the managed counters only.

The status codes in every `failed with status N` message are listed
in [ABI Reference](../reference/abi-reference/): 1 is a managed
exception, 2 a native panic, 3 a pipeline-thread-only entry called
from another thread, 4 an ABI mismatch, 5 a stopped pipeline.
