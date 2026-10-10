//! Shared build machinery: the pwsh hosts on this machine, the fetched
//! compiler toolset, C# compilation through it, and the trace of the
//! processes it starts.

pub mod pwsh;
pub mod toolchain;
pub mod trace;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub use toolchain::{CompileEnv, DebugSymbols, Toolchain};

/// Files and folders this process has written under a temporary name.
static TEMPORARIES: AtomicU64 = AtomicU64::new(0);

/// `path` with a suffix naming this process and one of its writes: the
/// name a file or folder is written under before it is renamed to
/// `path`, which no other writer, in this process or another, also uses.
pub(crate) fn temporary(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{}-{}.partial", std::process::id(), TEMPORARIES.fetch_add(1, Ordering::Relaxed)));
    PathBuf::from(name)
}

/// Writes `text` to `path` under a temporary name and renames it into
/// place, so a reader of `path` finds the file before or after, whole.
pub(crate) fn write_whole(path: &Path, text: &str) -> Result<(), Error> {
    let partial = temporary(path);
    std::fs::write(&partial, text).map_err(|e| Error::msg(format!("cannot write {}: {e}", partial.display())))?;
    match std::fs::rename(&partial, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            if let Err(left) = std::fs::remove_file(&partial) {
                eprintln!("pwrs: cannot remove {}: {left}", partial.display());
            }
            Err(Error::msg(format!("cannot move {} to {}: {e}", partial.display(), path.display())))
        }
    }
}

#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub fn msg(s: impl Into<String>) -> Error {
        Error(s.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}
