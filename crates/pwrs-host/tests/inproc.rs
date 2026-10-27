//! Starts the pwsh runtime in this process and drives the engine.
//! Needs pwsh on PATH; skips with a message when it is absent.

use pwrs_host::{build_testhost, Session};
use std::path::PathBuf;

fn session() -> Option<Session> {
    if pwrs_build::pwsh::pshome().is_err() {
        eprintln!("pwsh not found; skipping in-process tests");
        return None;
    }
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("pwrs-host");
    let dll = build_testhost(&out).expect("build Pwrs.TestHost");
    Some(Session::start(None, &dll).expect("start session"))
}

#[test]
fn runs_a_script_and_captures_streams() {
    let Some(s) = session() else { return };
    let r = s.run("1 + 1; Write-Verbose -Verbose 'v'; Write-Warning 'w'; Write-Error 'e'").expect("run");
    assert_eq!(r.output, vec!["2".to_string()], "{r:?}");
    assert_eq!(r.verbose, vec!["v".to_string()], "{r:?}");
    assert_eq!(r.warning, vec!["w".to_string()], "{r:?}");
    assert_eq!(r.errors.len(), 1, "{r:?}");
    assert!(r.terminating.is_none(), "{:?}", r.terminating);

    let r = s.run("throw 'boom'").expect("run");
    assert!(r.terminating.as_deref().is_some_and(|t| t.contains("boom")), "{:?}", r);
}

/// The launcher's own path: the PSHOME found without starting pwsh is
/// the one pwsh reports, the runtime starts there, an entry point binds
/// and runs, a method the assembly lacks is refused, and so is a second
/// PSHOME, since a process has one runtime.
#[test]
fn finds_the_pshome_pwsh_reports_and_binds_an_entry_point_there() {
    let Ok(reported) = pwrs_build::pwsh::pshome() else {
        eprintln!("pwsh not found; skipping the launcher test");
        return;
    };
    let found = pwrs_host::find_pshome().expect("find a PSHOME");
    let canonical = |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|e| panic!("cannot resolve {}: {e}", p.display()));
    assert_eq!(canonical(&found), canonical(&reported), "find_pshome found {}, and pwsh reports {}", found.display(), reported.display());
    let rt = pwrs_host::Runtime::start(&found).expect("start the runtime");
    assert_eq!(canonical(rt.pshome()), canonical(&found), "the runtime started from {}", rt.pshome().display());
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("pwrs-host");
    let dll = build_testhost(&out).expect("build Pwrs.TestHost");
    let run = rt.bind(&dll, "Pwrs.TestHost.Host, Pwrs.TestHost", "Run").expect("bind Run");
    let read = rt.bind(&dll, "Pwrs.TestHost.Host, Pwrs.TestHost", "Read").expect("bind Read");
    let free = rt.bind(&dll, "Pwrs.TestHost.Host, Pwrs.TestHost", "Free").expect("bind Free");
    type Run = unsafe extern "system" fn(*const u16, usize) -> *mut std::ffi::c_void;
    type Read = unsafe extern "system" fn(*mut std::ffi::c_void, *mut u16, usize) -> usize;
    type Free = unsafe extern "system" fn(*mut std::ffi::c_void);
    // SAFETY: the test host declares Run, Read and Free with these signatures.
    let (run, read, free) = unsafe {
        (
            std::mem::transmute::<*mut std::ffi::c_void, Run>(run.as_ptr()),
            std::mem::transmute::<*mut std::ffi::c_void, Read>(read.as_ptr()),
            std::mem::transmute::<*mut std::ffi::c_void, Free>(free.as_ptr()),
        )
    };
    let script: Vec<u16> = "6 * 7".encode_utf16().collect();
    // SAFETY: the script outlives the call, the handle is read into a
    // buffer of the length Read reports, and freed once.
    let json = unsafe {
        let handle = run(script.as_ptr(), script.len());
        assert!(!handle.is_null(), "Run returned no result");
        let mut buf = vec![0u16; read(handle, std::ptr::null_mut(), 0)];
        read(handle, buf.as_mut_ptr(), buf.len());
        free(handle);
        String::from_utf16_lossy(&buf)
    };
    let result: serde_json::Value = serde_json::from_str(&json).unwrap_or_else(|e| panic!("Run returned {json}, which is not JSON: {e}"));
    assert_eq!(result["Output"], serde_json::json!(["42"]), "{json}");
    let missing = rt.bind(&dll, "Pwrs.TestHost.Host, Pwrs.TestHost", "NoSuchMethod").expect_err("a method the type lacks");
    assert!(missing.to_string().starts_with("cannot bind NoSuchMethod of Pwrs.TestHost.Host, Pwrs.TestHost from"), "{missing}");
    let elsewhere = pwrs_host::Runtime::start(&std::env::temp_dir()).expect_err("a second PSHOME");
    assert!(elsewhere.to_string().contains("a process has one runtime"), "{elsewhere}");
}

#[test]
fn imports_the_hello_module_when_built() {
    let Some(s) = session() else { return };
    let module = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("pwrs").join("Hello");
    if !module.join("Hello.psd1").is_file() {
        eprintln!("Hello module not built; skipping");
        return;
    }
    let r = s.import_module(&module).expect("import");
    assert!(r.terminating.is_none(), "{:?}", r);
    let r = s.run("Get-Greeting -Name inproc -Count 2").expect("run");
    assert_eq!(r.output, vec!["Hello, inproc!".to_string(), "Hello, inproc!".to_string()]);
    let r = s.run("'a','b' | Get-Greeting").expect("run");
    assert_eq!(r.output, vec!["Hello, a!".to_string(), "Hello, b!".to_string()]);
    let r = s.run("Get-Greeting -Name x -Fail").expect("run");
    assert!(r.output.is_empty());
    assert!(r.errors.iter().any(|e| e.contains("GreetingRefused")), "{:?}", r.errors);
}
