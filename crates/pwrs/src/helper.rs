//! Helper executables a module ships beside its native library.
//!
//! A package names some of its `[[bin]]` targets under
//! `[package.metadata.pwrs] helpers`, and `cargo pwrs build` builds each
//! with the library, for the same target and profile, and ships it in
//! `runtimes/<rid>/native/`. The module starts one through
//! [`helper_path`].

use std::path::PathBuf;

use crate::host::vtable;
use crate::PsResult;
use pwrs_sys::{PsHandle, PsStr16};

/// Where to start the helper executable `name`: a copy of the one the
/// module ships, staged for this process.
///
/// `name` is the `[[bin]]` target's name, without `.exe`. The first
/// request for a helper's bytes copies it into the folder the process
/// stages the module's library in, and later requests answer the same
/// path. Start the helper from this path and never from the module
/// folder: a running executable holds its file, and the module folder
/// must stay free for the next build or update.
///
/// Callable from any thread; nothing in it reaches a runspace. The error
/// names the helper and the folder searched when the module ships no
/// helper of that name.
///
/// ```ignore
/// let out = std::process::Command::new(pwrs::helper_path("guest-host")?).arg("--version").output()?;
/// ```
pub fn helper_path(name: &str) -> PsResult<PathBuf> {
    let name16 = crate::text::to_utf16(name);
    let arg = PsStr16 { ptr: name16.as_ptr(), len: name16.len() };
    let mut buf = vec![0u16; 260];
    loop {
        let mut len = 0usize;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().helper_path)(arg, buf.as_mut_ptr(), buf.len(), &mut len, &mut err) };
        crate::pipeline::check(status, err)?;
        if len <= buf.len() {
            buf.truncate(len);
            return path_of(&buf);
        }
        buf.resize(len, 0);
    }
}

/// The path behind UTF-16 units: every unit kept on Windows, where a path
/// is UTF-16 already, and refused elsewhere when they are not text.
fn path_of(units: &[u16]) -> PsResult<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        Ok(PathBuf::from(std::ffi::OsString::from_wide(units)))
    }
    #[cfg(not(windows))]
    {
        match String::from_utf16(units) {
            Ok(s) => Ok(PathBuf::from(s)),
            Err(e) => Err(crate::PsError::new(
                crate::ErrorCategory::InvalidResult,
                "PwrsHelperPath",
                format!("the runtime answered a helper path that is not text: {e}"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_longer_than_the_first_buffer_is_read_whole() {
        let _host = crate::testing::install();
        let path = helper_path("guest-host").expect("the fake stages every helper but one");
        let text = path.to_string_lossy().into_owned();
        assert!(text.len() > 260, "the fake's path is long enough to need a second call: {}", text.len());
        assert!(text.ends_with("guest-host"), "{text}");
        assert_eq!(path, crate::testing::fake_helper_path("guest-host"));
    }

    #[test]
    fn a_helper_the_module_does_not_ship_is_an_error_naming_it() {
        let _host = crate::testing::install();
        match helper_path(crate::testing::FAKE_MISSING_HELPER) {
            Ok(p) => panic!("a missing helper answered {}", p.display()),
            Err(e) => assert!(e.message.contains(crate::testing::FAKE_MISSING_HELPER), "{}", e.message),
        }
    }
}
