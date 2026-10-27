---
title: How To Debug A Module
weight: 21
---

Stopping a debugger in a module's Rust code, reading values there, and finding where a panic started. Source: `crates/cargo-pwrs/dotnet/Pwrs.Runtime/NativeModule.cs`, which copies the library and its `.pdb` before loading them, and `module_init` in `crates/pwrs/src/runtime.rs`, which sets the panic hook.

A module's Rust code is a library that `pwsh` or `powershell.exe` loads, so a debugger debugs the shell. The sessions below stop in [hello](../reference/Hello-Tour.md)'s `Get-Greeting`, whose `process` starts at line 36 of `examples/hello/src/lib.rs`:

```rust
impl Cmdlet for GetGreeting {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.panic {
            panic!("requested panic for {}", self.name);
        }
```

Line 37 runs once for each name, and `-Panic` makes line 38 panic. For a module of your own, use its library's name where the commands say `pwrs_example_hello`, and its own file and line where they say `hello/src/lib.rs` and 37. The library's name is the crate's name with each `-` written as `_`, unless the crate's `[lib]` section gives it another.

In the listings, `<pwrs>` stands for the PWRS repository's folder, and `<user>` for your user name. Each command runs in `<pwrs>`.

## Build a module a debugger can read

```powershell
cargo pwrs build --manifest-dir examples/hello
```

Without `--release`, the library is built in Cargo's `dev` profile, which writes symbols: the information that ties machine code to function names and source lines. On Windows the symbols are in a `.pdb` file, and the build puts the library's `.pdb` beside it, in the module folder's `runtimes\win-x64\native`. On Linux and FreeBSD they are inside the `.so` file. Cargo's `release` profile writes no debug information for the crate's own code, so a breakpoint on a line needs a build without `--release`. [The cargo pwrs Reference](../reference/Cargo-Pwrs-Reference.md) lists the files a debug build writes.

## Where the library runs from

`Import-Module` does not load the library in the module folder. It copies the library into a folder under the temporary folder, named for the process and the module, and loads the copy. On Windows it copies the library's `.pdb` beside the copy first. The copy's name has a mark added to it, and a debugger names the library after the copy:

```text
pwrs_example_hello_8df33e2133815b6_34aa00            WinDbg, Windows
libpwrs_example_hello.8df33e602fe422d-16cae20.so     gdb and lldb, Linux
libpwrs_example_hello.8df33e7463272d0-16e9808.so     lldb, FreeBSD
libpwrs_example_hello.8df33ed57426a74-6244e8.dylib   lldb, macOS
```

The commands below match the library with a `*` after its name, or find it by the name of a source file, so the mark never has to be typed.

## Windows: WinDbg

`winget install Microsoft.WinDbg` installs WinDbg. Its package has `cdb`, WinDbg's console form, which takes the same commands as WinDbg's window, and the sessions here use it. The package does not put `cdb` on the PATH, so find it first:

```powershell
$cdb = Join-Path (Get-AppxPackage Microsoft.WinDbg).InstallLocation 'amd64\cdb.exe'
```

### Stop at a line

Put these commands in a file, here `line.txt`:

```text
.sympath+ <pwrs>\target\pwrs\Hello\runtimes\win-x64\native
.lines -e
sxe -c "ld pwrs_example_hello*; bp `hello\\src\\lib.rs:37` \"k 3; dv; dx self->name; qd\"; g" ld:pwrs_example_hello*
g
```

- `.sympath+` adds the module folder's `runtimes\win-x64\native` to the folders WinDbg searches for `.pdb` files. WinDbg searches those folders, not the folder the library was loaded from.
- `.lines -e` has WinDbg show the source line of each call.
- `sxe ... ld:pwrs_example_hello*` waits for the library to load, then runs the commands in quotes. `ld` loads the library's symbols, and `bp` sets a breakpoint, a place where the debugger stops the program, on line 37. `g` lets the shell go on.
- Inside the quotes, each `\` is written twice and each `"` as `\"`.
- At the breakpoint, `k 3` shows the last three calls, `dv` the arguments and local variables, and `dx self->name` one value. `qd` ends the debugger and lets the shell finish; leave it out to stay at the breakpoint and type commands.
- The last `g` starts the shell.

Start the shell under `cdb` with the file:

```powershell
& $cdb -cf line.txt pwsh -NoProfile -Command "Import-Module .\target\pwrs\Hello\Hello.psd1; Get-Greeting -Name debugger"
```

From the library's load to the greeting, it printed:

```text
*** WARNING: Unable to verify checksum for C:\Users\<user>\AppData\Local\Temp\pwrs-load\72456\1502513159\native\pwrs_example_hello.8df33e2133815b6-34aa00\pwrs_example_hello.8df33e2133815b6-34aa00.dll
Symbols loaded for pwrs_example_hello_8df33e2133815b6_34aa00
Child-SP          RetAddr               Call Site
000000b0`5d1cdde0 00007ffd`5f2bec4d     pwrs_example_hello_8df33e2133815b6_34aa00!pwrs_example_hello::impl$0::process+0x39 [<pwrs>\examples\hello\src\lib.rs @ 37]
000000b0`5d1ce480 00007ffd`5f45adb4     pwrs_example_hello_8df33e2133815b6_34aa00!pwrs::cmdlet::run_phase<pwrs_example_hello::GetGreeting>+0x32d [<pwrs>\crates\pwrs\src\cmdlet.rs @ 141]
000000b0`5d1ce920 00007ffd`5f42335e     pwrs_example_hello_8df33e2133815b6_34aa00!pwrs::runtime::cmdlet_invoke::closure$0+0xe4 [<pwrs>\crates\pwrs\src\runtime.rs @ 461]
           self = 0x000002a4`ed547e38
             ps = 0x000000b0`5d1ce540
           iter = <value unavailable>
         switch = <value unavailable>
self->name       : "debugger" [Type: alloc::string::String]
    [<Raw View>]     [Type: alloc::string::String]
    [len]            : 0x8 [Type: unsigned __int64]
    [capacity]       : 0x8 [Type: unsigned __int64]
    [chars]          : "debugger"
Hello, debugger!
```

The calls are `process` at line 37, PWRS's `run_phase` that called it, and `cmdlet_invoke`, where the call from PowerShell enters the library. `iter` and `switch` are local variables of later lines, so they have no value yet. `dx` shows a Rust `String` as its text. Before the stop, WinDbg also prints lines about first-chance CLR exceptions, code `e0434352`; they are .NET's own, and they stop nothing.

Windows PowerShell takes the same file with `powershell` in place of `pwsh`, and it stopped at the same line with the same values.

### Attach to a shell that is running

To attach is to connect a debugger to a process that is already running. In the shell, `$PID` is its process id. Put these commands in `attach.txt`:

```text
.sympath+ <pwrs>\target\pwrs\Hello\runtimes\win-x64\native
.lines -e
ld pwrs_example_hello*
bp `hello\src\lib.rs:37` "k 3; dv; qd"
g
```

Then attach with that id:

```powershell
& $cdb -p <process id> -cf attach.txt
```

The library is already loaded, so `ld` loads its symbols at once. Without `ld`, the breakpoint waited for symbols that WinDbg never loaded, and the debugger never stopped. Outside quotes, each `\` is written once. With the shell calling `Get-Greeting`, the debugger stopped at line 37, and after `qd` the shell went on running, in pwsh and in Windows PowerShell.

### Find where a panic started

A panic is how Rust code stops on an error it does not return. Every panic passes through the function `std::panicking::rust_panic`, so a breakpoint there stops before PWRS catches the panic. Put these commands in `panic.txt`:

```text
.sympath+ <pwrs>\target\pwrs\Hello\runtimes\win-x64\native
.lines -e
sxe -c "ld pwrs_example_hello*; bm pwrs_example_hello*!std::panicking::rust_panic \"k 8; g\"; g" ld:pwrs_example_hello*
g
```

`bm` sets a breakpoint on a function by its name, in whichever copy of the library is loaded. At the stop, `k 8` shows eight calls, and `g` lets the panic go on:

```powershell
& $cdb -cf panic.txt pwsh -NoProfile -Command "Import-Module .\target\pwrs\Hello\Hello.psd1; Get-Greeting -Name debugger -Panic; Get-Greeting -Name again"
```

```text
  0: 00007ffd`5d6a8990 @!"pwrs_example_hello_8df33e2133815b6_34aa00!std::panicking::rust_panic"
Child-SP          RetAddr               Call Site
000000b5`5374d7b8 00007ffd`5d6b787e     pwrs_example_hello_8df33e2133815b6_34aa00!std::panicking::rust_panic [/rustc/48a229ceaefd4985c50990b14116b6d856af0985/library\std\src\panicking.rs @ 886]
000000b5`5374d7c0 00007ffd`5d6a854e     pwrs_example_hello_8df33e2133815b6_34aa00!std::panicking::panic_with_hook+0x11a [/rustc/48a229ceaefd4985c50990b14116b6d856af0985/library\std\src\panicking.rs @ 840]
000000b5`5374d870 00007ffd`5d6a6bef     pwrs_example_hello_8df33e2133815b6_34aa00!std::panicking::panic_handler::closure$0+0x4e [/rustc/48a229ceaefd4985c50990b14116b6d856af0985/library\std\src\panicking.rs @ 688]
000000b5`5374d8d0 00007ffd`5d6a8bee     pwrs_example_hello_8df33e2133815b6_34aa00!std::sys::backtrace::__rust_end_short_backtrace<std::panicking::panic_handler::closure_env$0,never$>+0xf [/rustc/48a229ceaefd4985c50990b14116b6d856af0985/library\std\src\sys\backtrace.rs @ 185]
000000b5`5374d900 00007ffd`5d6d628d     pwrs_example_hello_8df33e2133815b6_34aa00!std::panicking::panic_handler+0x1e [/rustc/48a229ceaefd4985c50990b14116b6d856af0985/library\std\src\panicking.rs @ 679]
000000b5`5374d950 00007ffd`5d5a5ec2     pwrs_example_hello_8df33e2133815b6_34aa00!core::panicking::panic_fmt+0x2d [/rustc/48a229ceaefd4985c50990b14116b6d856af0985/library\core\src\panicking.rs @ 80]
000000b5`5374d9b0 00007ffd`5d4eec4d     pwrs_example_hello_8df33e2133815b6_34aa00!pwrs_example_hello::impl$0::process+0x92 [<pwrs>\examples\hello\src\lib.rs @ 38]
000000b5`5374e050 00007ffd`5d68adb4     pwrs_example_hello_8df33e2133815b6_34aa00!pwrs::cmdlet::run_phase<pwrs_example_hello::GetGreeting>+0x32d [<pwrs>\crates\pwrs\src\cmdlet.rs @ 141]
```

Under Rust's own panic functions, the first call of the module's code is the panic's line, here `process` at line 38. After `g`, PWRS caught the panic and PowerShell reported it as an error, and the next command ran:

```text
Get-Greeting: native module panicked: requested panic for debugger
Hello, again!
```

Windows PowerShell stopped at the same place with the same file. `sxe eh`, which stops at every C++ exception, is no way to find a panic in Windows PowerShell. .NET Framework throws C++ exceptions of its own, and the first stop was in one of those, before the library had loaded. Leave a stop at an exception with `g`. Leaving it with `qd` made pwsh end with `Fatal error.`.

## Linux: gdb or lldb

### gdb

```bash
gdb -ex 'set breakpoint pending on' -ex 'break hello/src/lib.rs:37' -ex run \
    --args pwsh -NoProfile -Command "Import-Module ./target/pwrs/Hello/Hello.psd1; Get-Greeting -Name debugger"
```

pwsh starts without the library, so `set breakpoint pending on` lets gdb take a breakpoint in a library that has not loaded yet. gdb stopped on the thread that runs the pipeline:

```text
Thread 21 "Pipeline Execut" hit Breakpoint 1, pwrs_example_hello::{impl#0}::process (self=0x7ffa340760d8, ps=0x7ffa45bfd7e8) at examples/hello/src/lib.rs:37
37	        if self.panic {
```

gdb shows Rust values as their text once Rust's printers are loaded. rustup installs them with the toolchain, and this, before `-ex run`, loads them:

```bash
-ex "source $(rustc --print sysroot)/lib/rustlib/etc/gdb_load_rust_pretty_printers.py"
```

With them, `print self.name` and `print *self` printed:

```text
$1 = "debugger"
$2 = pwrs_example_hello::GetGreeting {name: "debugger", count: core::option::Option<i64>::None, fail: false, terminate: false, panic: false}
```

Without them, gdb printed the `String` as the fields inside it, and so did `rust-gdb`, the wrapper rustup installs.

For a panic, `break rust_panic` did not stop, and `break __rustc::rust_panic` did:

```bash
gdb -ex 'set breakpoint pending on' -ex 'break __rustc::rust_panic' -ex run \
    --args pwsh -NoProfile -Command "Import-Module ./target/pwrs/Hello/Hello.psd1; Get-Greeting -Name debugger -Panic"
```

`bt 8` at the stop printed:

```text
#0  std::panicking::rust_panic () at library/std/src/panicking.rs:887
#1  0x00007ffa449520e5 in std::panicking::panic_with_hook () at library/std/src/panicking.rs:840
#2  0x00007ffa44939912 in std::panicking::panic_handler::{closure#0} () at library/std/src/panicking.rs:688
#3  0x00007ffa4492ecc9 in std::sys::backtrace::__rust_end_short_backtrace<std::panicking::panic_handler::{closure_env#0}, !> () at library/std/src/sys/backtrace.rs:182
#4  0x00007ffa4493a40d in std::panicking::panic_handler () at library/std/src/panicking.rs:679
#5  0x00007ffa4496e19c in core::panicking::panic_fmt () at library/core/src/panicking.rs:80
#6  0x00007ffa4483c05b in pwrs_example_hello::{impl#0}::process (self=0x7ffa28098c58, ps=0x7ffa453fd7e8) at examples/hello/src/lib.rs:38
#7  0x00007ffa4478c34d in pwrs::cmdlet::run_phase<pwrs_example_hello::GetGreeting> (hdr=0x7ffa28098c20, phase=1, cmdlet=..., params=0x7ffa453fe5a8) at crates/pwrs/src/cmdlet.rs:141
```

### lldb

```bash
lldb -o 'breakpoint set --file hello/src/lib.rs --line 37' -o 'process launch' \
    -- pwsh -NoProfile -Command "Import-Module ./target/pwrs/Hello/Hello.psd1; Get-Greeting -Name debugger"
```

lldb takes a breakpoint it cannot place yet, and places it when the library loads:

```text
(lldb) breakpoint set --file hello/src/lib.rs --line 37
Breakpoint 1: no locations (pending).
WARNING:  Unable to resolve breakpoint to any actual locations.
(lldb) process launch
1 location added to breakpoint 1
Process 3671555 launched: '/home/<user>/pwsh/pwsh' (x86_64)
Process 3671555 stopped
* thread #20, name = 'Pipeline Execut', stop reason = breakpoint 1.1
    frame #0: 0x00007ffa4503bfeb libpwrs_example_hello.8df33e602fe422d-16cae20.so`<pwrs_example_hello::GetGreeting as pwrs::cmdlet::Cmdlet>::process(self=0x00007ffa3408a138, ps=0x00007ffa45bfd7e8) at lib.rs:37:9
   34  	
   35  	impl Cmdlet for GetGreeting {
   36  	    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
-> 37  	        if self.panic {
    	        ^
   38  	            panic!("requested panic for {}", self.name);
   39  	        }
   40  	        if self.fail || self.terminate {
```

For a panic, `breakpoint set --name rust_panic` stopped at `__rustc::rust_panic at panicking.rs:887:25`, with `process` at `lib.rs:38:13` six calls below it.

lldb shows a `String` as the fields inside it. `rust-lldb`, the wrapper rustup installs, loads Rust's formatters into lldb. With the lldb 18.1.3 of Ubuntu 24.04, it showed `self->name` as its eight characters, one to a line, after a Python error in one of the formatters.

### Attach

On the Ubuntu 24.04 measured here, `kernel.yama.ptrace_scope` is 1, which lets a debugger attach only to a process it started itself. Attaching as the shell's own user, gdb and then lldb refused:

```text
Could not attach to process.  If your uid matches the uid of the target
process, check the setting of /proc/sys/kernel/yama/ptrace_scope, or try
again as the root user.  For more details, see /etc/sysctl.d/10-ptrace.conf
```

```text
error: attach failed: The current value of ptrace_scope is 1, which can cause ptrace to fail to attach to a running process. To fix this, run:
	sudo sysctl -w kernel.yama.ptrace_scope=0
```

Run as root, each attached, stopped at line 37 and detached, and the shell went on running:

```bash
sudo gdb -p <process id> -ex 'break hello/src/lib.rs:37' -ex continue
```

```text
Breakpoint 1 at 0x7e02afe3bfeb: file examples/hello/src/lib.rs, line 37.
[Switching to Thread 0x7e02d0bff6c0 (LWP 3674512)]

Thread 3 "Pipeline Execut" hit Breakpoint 1, pwrs_example_hello::{impl#0}::process (self=0x7e02b40edbe8, ps=0x7e02d0bfd6c8) at examples/hello/src/lib.rs:37
37	        if self.panic {
```

`detach` lets the shell go on without gdb. lldb attached the same way with `sudo lldb -p <process id>`, then `breakpoint set --file hello/src/lib.rs --line 37`, `continue` and `detach`.

## FreeBSD: lldb

FreeBSD 15.0's own lldb, 19.1.7, takes the lldb commands above. With pwsh 7.5.5, it stopped at line 37 and at `rust_panic` as on Linux. `security.bsd.unprivileged_proc_debug` is 1 on the FreeBSD measured here, which lets a debugger attach to a process of the same user, so lldb attached without root:

```bash
lldb -p <process id> -o 'breakpoint set --file hello/src/lib.rs --line 37' -o continue -o 'thread backtrace -c 3' -o detach
```

```text
   34  	
   35  	impl Cmdlet for GetGreeting {
   36  	    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
-> 37  	        if self.panic {
    	        ^
   38  	            panic!("requested panic for {}", self.name);
   39  	        }
   40  	        if self.fail || self.terminate {
(lldb) thread backtrace -c 3
* thread #22, name = 'Pipeline Executi', stop reason = breakpoint 1.1
  * frame #0: 0x00001503aab4aa4b libpwrs_example_hello.8df33e7463272d0-16e9808.so`<pwrs_example_hello::GetGreeting as pwrs::cmdlet::Cmdlet>::process(self=0x000025989ba0fdc8, ps=0x00001503a5e55f28) at lib.rs:37:9
    frame #1: 0x00001503aaae76bd libpwrs_example_hello.8df33e7463272d0-16e9808.so`pwrs::cmdlet::run_phase::<pwrs_example_hello::GetGreeting>(hdr=0x000025989ba0fd90, phase=1, cmdlet=PsHandle @ 0x00001503a5e56258, params=0x00001503a5e56ce8) at cmdlet.rs:141:41
    frame #2: 0x00001503aabdde5e libpwrs_example_hello.8df33e7463272d0-16e9808.so`pwrs::runtime::cmdlet_invoke::{closure#0} at runtime.rs:460:29
(lldb) detach
Process 63732 detached
```

The shell went on running after the detach. FreeBSD's lldb runs its scripts in Lua, not Python, so Rust's lldb formatters cannot load there: `rust-lldb` stopped with `lua failed to import`, naming its `lldb_lookup.py`, `invalid extension`. Without them, lldb shows a `String` as its fields, as on Linux.

## macOS: lldb

Apple's lldb, from Xcode or its Command Line Tools, takes the lldb commands above. On a GitHub-hosted runner with the macos-26-arm64 image, lldb-2100.0.17.203 with pwsh 7.6.7 stopped at line 37 as on Linux:

```text
* thread #20, name = 'Pipeline Execution Thread', stop reason = breakpoint 1.1
    frame #0: 0x000000014560f590 libpwrs_example_hello.8df33ed57426a74-6244e8.dylib`<pwrs_example_hello::GetGreeting as pwrs::cmdlet::Cmdlet>::process(self=0x000000091b0af3d8, ps=0x0000000171fb5768) at lib.rs:37:9
```

For a panic, `breakpoint set --name rust_panic` stopped at `__rustc::rust_panic at panicking.rs:887:25`, with `process` at `lib.rs:38:13` six calls below it.

### Attach

To attach lldb to a pwsh that is running, start that pwsh with the environment variable `PAL_MachExceptionMode` set to 2:

```bash
PAL_MachExceptionMode=2 pwsh
```

.NET's runtime reads the variable when it starts. Without it, macOS sends a breakpoint trap on one of the runtime's threads to the runtime, not to lldb. A trap lldb placed is not one of the runtime's own breakpoints, so the runtime runs the same instruction again, and it traps again. So the thread uses a whole processor, the shell stops making progress, and lldb never stops. The value 2 leaves breakpoint traps to a native debugger. The runtime does that by itself when a debugger is attached as it starts, which is why a pwsh that lldb starts needs no variable.

With the variable set, lldb attached as the shell's own user, and as root with `sudo`. Each stopped at line 37 and detached, and the shell went on running:

```bash
lldb -p <process id> -o 'breakpoint set --file hello/src/lib.rs --line 37' -o continue -o 'thread backtrace -c 3' -o detach
```

```text
(lldb) continue
warning: This version of LLDB has no plugin for the language "rust". Inspection of frame variables will be limited.
Process 27559 resuming
Process 27559 stopped
* thread #20, name = 'Pipeline Execution Thread', stop reason = breakpoint 1.1
    frame #0: 0x0000000157207590 libpwrs_example_hello.8df33ed57426a74-6244e8.dylib`<pwrs_example_hello::GetGreeting as pwrs::cmdlet::Cmdlet>::process(self=0x0000000b1b0a9768, ps=0x000000016fe39648) at lib.rs:37:9
   34  	
   35  	impl Cmdlet for GetGreeting {
   36  	    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
-> 37  	        if self.panic {
    	        ^
   38  	            panic!("requested panic for {}", self.name);
   39  	        }
   40  	        if self.fail || self.terminate {
Target 0: (pwsh) stopped.
(lldb) thread backtrace -c 3
* thread #20, name = 'Pipeline Execution Thread', stop reason = breakpoint 1.1
  * frame #0: 0x0000000157207590 libpwrs_example_hello.8df33ed57426a74-6244e8.dylib`<pwrs_example_hello::GetGreeting as pwrs::cmdlet::Cmdlet>::process(self=0x0000000b1b0a9768, ps=0x000000016fe39648) at lib.rs:37:9
    frame #1: 0x000000015725127c libpwrs_example_hello.8df33ed57426a74-6244e8.dylib`pwrs::cmdlet::run_phase::<pwrs_example_hello::GetGreeting>(hdr=0x0000000b1b0a9730, phase=1, cmdlet=PsHandle @ 0x000000016fe39980, params=0x000000016fe3a518) at cmdlet.rs:141:41
    frame #2: 0x0000000157381080 libpwrs_example_hello.8df33ed57426a74-6244e8.dylib`pwrs::runtime::cmdlet_invoke::{closure#0} at runtime.rs:460:29
(lldb) detach
Process 27559 detached
```

Apple's lldb has no plugin for Rust. It warned that `Inspection of frame variables will be limited`, and `frame variable self` printed the pointer alone.

## A panic without a debugger

Without a debugger, a panic reaches PowerShell as a terminating error, and the module still works afterward. Both hosts gave the same four values:

```powershell
try { Get-Greeting -Name debugger -Panic } catch { $_.FullyQualifiedErrorId; $_.CategoryInfo.Category; $_.Exception.GetType().FullName; $_.Exception.Message }
```

```text
PwrsNativePanic,Pwrs.Modules.Hello.GetGreetingCommand
InvalidOperation
Pwrs.PwrsException
native module panicked: requested panic for debugger
```

PWRS replaces Rust's panic hook when a module loads, so a panic prints nothing of its own, and setting `RUST_BACKTRACE=1` added nothing in either host. The error has the panic's message but not where it started, which is what the debugger sessions above find. [How To Fix A Failure](How-To-Fix-A-Failure.md#a-cmdlet-fails-at-run-time) covers errors at run time.

## The .NET side, and tracing

The generated shell and the PWRS runtime are .NET assemblies, and a debug build writes a `.pdb` for each. So a .NET stack trace through them gives each frame's file and line, as [How To Add Hybrid C#](How-To-Add-Hybrid-CSharp.md#what-is-compiled) shows. To follow the calls between PowerShell and Rust without a debugger, set `PWRS_TRACE`, as [How To Trace A Module](How-To-Trace-A-Module.md) shows.
