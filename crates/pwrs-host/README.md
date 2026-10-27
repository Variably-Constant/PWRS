<div align="center">

<img src="https://github.com/Variably-Constant/PWRS/raw/main/assets/pwrs-logo.svg" alt="PWRS logo" width="200"/>

# pwrs-host

**PowerShell 7 inside your own Rust process.**

[![crates.io](https://img.shields.io/crates/v/pwrs-host?style=flat-square)](https://crates.io/crates/pwrs-host)
[![docs.rs](https://img.shields.io/docsrs/pwrs-host?style=flat-square)](https://docs.rs/pwrs-host)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](https://github.com/Variably-Constant/PWRS/blob/main/LICENSE)

**[Documentation](https://variably-constant.github.io/PWRS/)** | [How To Run PowerShell In A Rust Program](https://variably-constant.github.io/PWRS/docs/how-to/how-to-run-powershell-in-a-rust-program/) | [API reference](https://docs.rs/pwrs-host) | [Repository](https://github.com/Variably-Constant/PWRS)

</div>

Starts the .NET runtime of the installed pwsh inside the calling process through `hostfxr`, as running `pwsh.dll` does, so every assembly pwsh ships resolves, and returns the address of an assembly's `[UnmanagedCallersOnly]` entry point for the program to call. No .NET SDK and no pwsh process are involved: `find_pshome` finds the PSHOME from `PWRS_PSHOME`, the pwsh on PATH followed through its links (a Store install's alias on Windows, a snap's command on Linux), or the platform's default install folder.

## Quick start

```text
cargo add pwrs-host
```

A C# entry point, compiled into `RunScript.dll` against PowerShell 7's references:

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

The Rust program that runs it:

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

The repository's `run_script` example is this program, compiling the entry point with pwrs-build's toolchain the first time. On Windows x64 with the Store's pwsh 7.6.6, its own lines, cargo's left out:

```text
> cargo run -p pwrs-host --example run_script -- "'Hello from PowerShell ' + $PSVersionTable.PSVersion"
Hello from PowerShell 7.6.6
> cargo run -p pwrs-host --example run_script -- "'before'; Write-Error 'something went wrong'; 'after'"
before
after
something went wrong
```

The first exits 0 and the second 1.

## What it guarantees

- One runtime per process. Every later `Runtime::start` returns the first runtime, and one naming another PSHOME is refused.
- `bind` returns an address and checks nothing about its signature: calling it through any signature but the one the method declares is undefined behavior.
- A Store install is hosted from a mirror under `~/.pwrs/pshome`, since other processes may read its files but not map them as executables; a snap's PowerShell is hosted once the snap's ICU is loaded.
- `find_pshome` refuses a `PWRS_PSHOME` that has no `pwsh.dll` and hostfxr rather than passing over it, and when nothing is found lists every folder it looked in.

`Session` and `build_testhost` run scripts through PWRS's own test host, as the workspace's in-process tests do; see [How To Test A Module](https://variably-constant.github.io/PWRS/docs/how-to/how-to-test-a-module/).

`PoWerRuSt`, `pwrs-sys`, `pwrs-macros`, `pwrs-build`, `pwrs-host` and `cargo-pwrs` are published together at one version. MIT licensed.
