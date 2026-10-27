//! Helper executables and other files a module ships beside its native
//! library.
//!
//! A package names some of its `[[bin]]` targets under
//! `[package.metadata.pwrs] helpers`, and `cargo pwrs build` builds each
//! with the library, for the same target and profile, and ships it in
//! `runtimes/<rid>/native/`; a macOS build ships one that
//! `[package.metadata.pwrs.macos-bundles]` names as an app bundle there,
//! the executable under its `Contents/MacOS`. The module starts one
//! through [`helper_path`]. A file `[package.metadata.pwrs.native-files]`
//! ships there is reached through [`native_file_path`].

use std::path::PathBuf;

use crate::host::vtable;
use crate::PsResult;
use pwrs_sys::{PsHandle, PsStr16};

/// Where to start the helper executable `name`: a copy of the one the
/// module ships, staged for this process.
///
/// `name` is the `[[bin]]` target's name, without `.exe`. The first call
/// for a helper copies it into the folder the process stages the module's
/// library in, and later calls return the same path. A helper shipped as
/// an app bundle is copied with its whole bundle, and the path is its
/// executable inside the copy, so macOS starts it with the bundle's
/// identity. Start the helper from this path and never from the module
/// folder: while an executable runs, its file cannot be overwritten or
/// deleted, and the module folder must stay free for the next build or
/// update.
///
/// Callable from any thread; nothing in it reaches a runspace. The error
/// names the helper and the folder searched when the module ships no
/// helper of that name.
///
/// ```ignore
/// let out = std::process::Command::new(pwrs::helper_path("guest-host")?).arg("--version").output()?;
/// ```
pub fn helper_path(name: &str) -> PsResult<PathBuf> {
    staged(vtable().helper_path, name, "PwrsHelperPath")
}

/// Where to load or start the file `name` that
/// `[package.metadata.pwrs.native-files]` ships beside the module's
/// library: a copy staged for this process.
///
/// `name` is the file's own name, extension included, as the entry's key
/// writes it. The first call for the file copies it into the folder the
/// process stages the module's library in, and later calls return the
/// same path. Load the file from this path and never from the module
/// folder: while a library is loaded, its file cannot be overwritten or
/// deleted, and the module folder must stay free for the next build or
/// update.
///
/// Callable from any thread; nothing in it reaches a runspace. The error
/// names the file and the folder searched when the module ships no file
/// of that name.
///
/// ```ignore
/// let dxc = pwrs::native_file_path("dxcompiler.dll")?;
/// ```
pub fn native_file_path(name: &str) -> PsResult<PathBuf> {
    staged(vtable().native_file_path, name, "PwrsNativeFilePath")
}

/// The path a staging entry of the host table returns for `name`, read
/// into a buffer grown to the length the entry reports; `error_id` names
/// a path that is not text.
fn staged(entry: unsafe extern "C" fn(PsStr16, *mut u16, usize, *mut usize, *mut PsHandle) -> pwrs_sys::PsStatus, name: &str, error_id: &str) -> PsResult<PathBuf> {
    let name16 = crate::text::to_utf16(name);
    let arg = PsStr16 { ptr: name16.as_ptr(), len: name16.len() };
    let mut buf = vec![0u16; 260];
    loop {
        let mut len = 0usize;
        let mut err = PsHandle::NULL;
        let status = unsafe { entry(arg, buf.as_mut_ptr(), buf.len(), &mut len, &mut err) };
        crate::pipeline::check(status, err)?;
        if len <= buf.len() {
            buf.truncate(len);
            return path_of(&buf, error_id);
        }
        buf.resize(len, 0);
    }
}

/// The path behind UTF-16 units: every unit taken as it is on Windows, where a path
/// is UTF-16 already, and refused elsewhere, as `error_id`, when they are
/// not text.
#[cfg_attr(windows, allow(unused_variables))]
fn path_of(units: &[u16], error_id: &str) -> PsResult<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        Ok(PathBuf::from(std::ffi::OsString::from_wide(units)))
    }
    #[cfg(not(windows))]
    {
        match String::from_utf16(units) {
            Ok(s) => Ok(PathBuf::from(s)),
            Err(e) => Err(crate::PsError::new(crate::ErrorCategory::InvalidResult, error_id, format!("the runtime returned a staged path that is not text: {e}"))),
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
            Ok(p) => panic!("a missing helper returned {}", p.display()),
            Err(e) => assert!(e.message.contains(crate::testing::FAKE_MISSING_HELPER), "{}", e.message),
        }
    }

    #[test]
    fn a_native_file_is_staged_by_its_own_name_through_its_own_entry() {
        let _host = crate::testing::install();
        let path = native_file_path("dxcompiler.dll").expect("the fake stages every native file but one");
        assert_eq!(path, crate::testing::fake_native_file_path("dxcompiler.dll"), "the path is native_file_path's, not helper_path's");
        match native_file_path(crate::testing::FAKE_MISSING_FILE) {
            Ok(p) => panic!("a missing native file returned {}", p.display()),
            Err(e) => assert!(e.message.contains(crate::testing::FAKE_MISSING_FILE), "{}", e.message),
        }
    }
}
