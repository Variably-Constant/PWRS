//! The helper executable the Hello module ships beside its native
//! library, which `Invoke-HelloHelper` and `Start-HelloHelper` start
//! through `pwrs::helper_path`.
//!
//! ```text
//! hello-helper echo <word>...   prints the words, joined by spaces
//! hello-helper where            prints the path it runs from
//! hello-helper wait             reads standard input to its end, then prints `released`
//! hello-helper bundle           prints the identifier of the app bundle it runs in, on macOS
//! ```

use std::io::Read;
use std::process::ExitCode;

/// The `CFBundleIdentifier` of the bundle the process runs in: the main
/// bundle, which CoreFoundation and `NSBundle.mainBundle` both read from
/// the folders around the executable. None outside a bundle that names one.
#[cfg(target_os = "macos")]
fn main_bundle_identifier() -> Option<String> {
    use std::ffi::{c_char, c_void, CStr};
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFBundleGetMainBundle() -> *const c_void;
        fn CFBundleGetIdentifier(bundle: *const c_void) -> *const c_void;
        fn CFStringGetCString(string: *const c_void, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
    }
    const UTF8: u32 = 0x0800_0100;
    let mut buffer: [c_char; 1024] = [0; 1024];
    // The main bundle and its identifier belong to CoreFoundation, under
    // the Get rule, so neither is released here.
    unsafe {
        let bundle = CFBundleGetMainBundle();
        if bundle.is_null() {
            return None;
        }
        let identifier = CFBundleGetIdentifier(bundle);
        if identifier.is_null() || CFStringGetCString(identifier, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) == 0 {
            return None;
        }
        Some(CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned())
    }
}

#[cfg(not(target_os = "macos"))]
fn main_bundle_identifier() -> Option<String> {
    None
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("echo") => {
            println!("{}", args[1..].join(" "));
            ExitCode::SUCCESS
        }
        Some("where") => match std::env::current_exe() {
            Ok(path) => {
                println!("{}", path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("hello-helper: cannot read the path it runs from: {e}");
                ExitCode::FAILURE
            }
        },
        Some("wait") => {
            let mut input = Vec::new();
            match std::io::stdin().read_to_end(&mut input) {
                Ok(_) => {
                    println!("released");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("hello-helper: cannot read standard input: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("bundle") => match main_bundle_identifier() {
            Some(identifier) => {
                println!("{identifier}");
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("hello-helper: runs in no app bundle that names an identifier");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: hello-helper echo <word>... | where | wait | bundle");
            ExitCode::from(2)
        }
    }
}
