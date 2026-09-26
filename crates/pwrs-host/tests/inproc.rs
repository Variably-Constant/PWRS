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
    assert_eq!(r.output, vec!["2".to_string()]);
    assert_eq!(r.verbose, vec!["v".to_string()]);
    assert_eq!(r.warning, vec!["w".to_string()]);
    assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
    assert!(r.terminating.is_none(), "{:?}", r.terminating);

    let r = s.run("throw 'boom'").expect("run");
    assert!(r.terminating.as_deref().is_some_and(|t| t.contains("boom")), "{:?}", r);
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
