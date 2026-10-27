---
title: How To Run PowerShell In A Rust Program
weight: 19
---

Running PowerShell 7 inside your own executable, with no pwsh process: finding the installed pwsh, starting its .NET runtime in the process, and calling into an assembly. Source: `crates/pwrs-host/src/lib.rs` and `crates/pwrs-host/examples/run_script.rs`.

## Find the PSHOME

`pwrs_host::find_pshome()` returns the folder of the pwsh the process would host, without starting it. It looks, in order, at:

1. `PWRS_PSHOME`, when it is set. A folder it names that does not have `pwsh.dll` and hostfxr is refused, not skipped.
2. The first `pwsh` on PATH, followed through its links to the folder it is in. On Windows a Store install puts an execution alias on PATH, a reparse point whose data names the package folder, and that folder is taken. On Linux a snap's command is a link to the snap launcher, so each snap's `current/opt/powershell` is tried, in name order.
3. The default install folder: `%ProgramFiles%\PowerShell\7` on Windows, `/opt/microsoft/powershell/7` on Linux and `/usr/local/microsoft/powershell/7` on macOS.

A folder counts when it has `pwsh.dll` and hostfxr: `hostfxr.dll`, `libhostfxr.so` or `libhostfxr.dylib`. When none does, the error lists every folder looked in. The example below, run on Windows with no pwsh on PATH, and then with `PWRS_PSHOME` naming a folder without one, exits 2 with:

```text
run_script: no PSHOME found: PWRS_PSHOME is not set, and none of these has pwsh.dll and hostfxr.dll: C:\Program Files\PowerShell\7
run_script: PWRS_PSHOME names C:\Windows, which has no pwsh.dll and hostfxr.dll
```

## Start the runtime

`pwrs_host::Runtime::start(&pshome)` starts the runtime through hostfxr as running `pwsh.dll` does, so every assembly pwsh ships resolves. A process has one runtime: every later call returns the first, and a call naming another PSHOME is refused with an error naming both. A start that failed returns its error to every later call. A PSHOME in a Store package is hosted from a copy under `~/.pwrs/pshome`, made once, since other processes may read the package's files but not map them as executables. One in a snap is hosted once the snap's ICU (the Unicode library .NET uses for culture data on Linux) is loaded, which its pwsh otherwise finds through the snap's own launcher.

## Call into an assembly

`runtime.bind(assembly, "Namespace.Type, Assembly", "Method")` loads the assembly into a load context of its own the first time and returns the address of the `[UnmanagedCallersOnly]` method named. PowerShell's own assemblies resolve from pwsh's, so the assembly references `System.Management.Automation` without shipping it. The program casts the address to the signature the method declares; nothing checks it, and a call through any other signature is undefined behavior. A method the type does not have is refused with `cannot bind <method> of <type> from <assembly>: <hostfxr's result>`.

An entry point that runs the script it is given and returns 1 when the script wrote an error:

```csharp
using System;
using System.Management.Automation;
using System.Runtime.InteropServices;

namespace RunScript;

public static class Entry
{
    [UnmanagedCallersOnly]
    public static unsafe int Run(char* script, int length)
    {
        using var ps = PowerShell.Create();
        ps.AddScript(new string(script, 0, length));
        foreach (var output in ps.Invoke()) Console.WriteLine(output);
        foreach (var error in ps.Streams.Error) Console.Error.WriteLine(error);
        return ps.HadErrors ? 1 : 0;
    }
}
```

The Rust that runs it:

```rust
type Run = unsafe extern "system" fn(script: *const u16, length: i32) -> i32;

fn run(script: &str, assembly: &std::path::Path) -> Result<i32, pwrs_host::Error> {
    let pshome = pwrs_host::find_pshome()?;
    let runtime = pwrs_host::Runtime::start(&pshome)?;
    let entry = runtime.bind(assembly, "RunScript.Entry, RunScript", "Run")?;
    // SAFETY: Run is declared `int Run(char* script, int length)`.
    let run = unsafe { std::mem::transmute::<*mut std::ffi::c_void, Run>(entry.as_ptr()) };
    let units: Vec<u16> = script.encode_utf16().collect();
    let length = i32::try_from(units.len()).map_err(|e| pwrs_host::Error::msg(format!("the script is too long to pass: {e}")))?;
    // SAFETY: `units` outlives the call and has `length` UTF-16 units.
    Ok(unsafe { run(units.as_ptr(), length) })
}
```

`crates/pwrs-host/examples/run_script.rs` is this program, compiling the entry point with pwrs-build's toolchain into a folder under the temporary folder the first time; an executable of your own ships its assembly beside it. On Windows x64 with the Store's pwsh 7.6.7, its own lines, cargo's left out:

```text
> cargo run -p pwrs-host --example run_script -- "'Hello from PowerShell ' + $PSVersionTable.PSVersion"
Hello from PowerShell 7.6.7
> cargo run -p pwrs-host --example run_script -- "'before'; Write-Error 'something went wrong'; 'after'"
before
after
something went wrong
```

The first exits 0 and the second 1.

## Testing

`pwrs_host::Session` runs scripts through PWRS's own test host, which `build_testhost` compiles, as the workspace's in-process tests do; see [How To Test A Module](How-To-Test-A-Module.md). `crates/pwrs-host/tests/inproc.rs` checks the launcher's path as above on each platform it runs on: the PSHOME `find_pshome` returns is the one pwsh reports for itself, an entry point binds and runs, and a second PSHOME is refused.
