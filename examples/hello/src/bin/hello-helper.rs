//! The helper executable the Hello module ships beside its native
//! library, which `Invoke-HelloHelper` and `Start-HelloHelper` start
//! through `pwrs::helper_path`.
//!
//! ```text
//! hello-helper echo <word>...   prints the words, joined by spaces
//! hello-helper where            prints the path it runs from
//! hello-helper wait             reads standard input to its end, then prints `released`
//! ```

use std::io::Read;
use std::process::ExitCode;

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
        _ => {
            eprintln!("usage: hello-helper echo <word>... | where | wait");
            ExitCode::from(2)
        }
    }
}
