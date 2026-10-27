//! Runs a PowerShell script inside this process: finds the installed
//! pwsh without starting it, starts its runtime, binds a C# entry point,
//! and calls it with the script, exiting with what it returns.
//!
//! The entry point is compiled with pwrs-build's toolchain the first
//! time, into a folder under the temporary folder; an executable of your
//! own ships its assembly beside it instead.
//!
//!   cargo run -p pwrs-host --example run_script -- "'Hello from PowerShell ' + $PSVersionTable.PSVersion"

use std::path::PathBuf;

/// The entry point: runs the script it is given in a runspace of its
/// own, writes each output as a line of text and each error to standard
/// error, and returns 1 when the script wrote an error and 0 otherwise.
const ENTRY: &str = r#"
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
"#;

/// The signature `RunScript.Entry.Run` declares.
type Run = unsafe extern "system" fn(script: *const u16, length: i32) -> i32;

fn main() {
    let script = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    match run(&script) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("run_script: {e}");
            std::process::exit(2);
        }
    }
}

fn run(script: &str) -> Result<i32, pwrs_host::Error> {
    let pshome = pwrs_host::find_pshome()?;
    let runtime = pwrs_host::Runtime::start(&pshome)?;
    let entry = runtime.bind(&entry_assembly()?, "RunScript.Entry, RunScript", "Run")?;
    // SAFETY: Run is declared `int Run(char* script, int length)`.
    let run = unsafe { std::mem::transmute::<*mut std::ffi::c_void, Run>(entry.as_ptr()) };
    let units: Vec<u16> = script.encode_utf16().collect();
    let length = i32::try_from(units.len()).map_err(|e| pwrs_host::Error::msg(format!("the script is too long to pass: {e}")))?;
    // SAFETY: `units` outlives the call and has `length` UTF-16 units.
    Ok(unsafe { run(units.as_ptr(), length) })
}

/// `RunScript.dll`, compiled from `ENTRY` against PowerShell 7's
/// references the first time and taken from where it was put after that.
fn entry_assembly() -> Result<PathBuf, pwrs_host::Error> {
    let dir = std::env::temp_dir().join("pwrs-host-run-script");
    let dll = dir.join("RunScript.dll");
    if dll.is_file() {
        return Ok(dll);
    }
    let tc = pwrs_build::Toolchain::ensure()?;
    std::fs::create_dir_all(&dir).map_err(|e| pwrs_host::Error::msg(format!("cannot create {}: {e}", dir.display())))?;
    let source = dir.join("Entry.cs");
    std::fs::write(&source, ENTRY).map_err(|e| pwrs_host::Error::msg(format!("cannot write {}: {e}", source.display())))?;
    let work = dir.join("work");
    let env = pwrs_build::CompileEnv { refs: &tc.net10_refs, defines: pwrs_build::toolchain::CORE_DEFINES, work: &work, debug: pwrs_build::DebugSymbols::Omit, path_map: &[] };
    tc.compile(&env, &[source], &[], &dll)?;
    Ok(dll)
}
