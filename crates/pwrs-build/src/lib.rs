//! Shared build machinery: the pwsh hosts on this machine, the fetched
//! compiler toolset, and C# compilation through it.

pub mod pwsh;
pub mod toolchain;

pub use toolchain::{CompileEnv, DebugSymbols, Toolchain};

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
