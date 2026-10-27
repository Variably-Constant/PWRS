//! In-process hosting of the .NET runtime that pwsh ships, for an
//! executable that runs PowerShell 7 inside its own process, and a
//! PowerShell session inside it for tests.
//!
//! [`find_pshome`] finds the PSHOME of the installed pwsh without
//! starting it. [`Runtime::start`] starts the runtime from a PSHOME
//! through `hostfxr` as if running `pwsh.dll`, which makes every assembly
//! pwsh ships resolvable, and [`Runtime::bind`] returns the address of an
//! assembly's `UnmanagedCallersOnly` entry point, which the caller calls
//! through the signature the method declares. Windows PowerShell 5.1 has
//! no hostfxr.
//!
//! One runtime per process: every later [`Runtime::start`] returns the
//! first runtime. [`Session`] runs scripts through `Pwrs.TestHost.dll`,
//! which [`build_testhost`] compiles; a session runs its scripts in a
//! runspace of the thread that runs them, so sessions on different
//! threads run side by side.

use serde::Deserialize;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

pub use pwrs_build::Error;

#[cfg(windows)]
type CharT = u16;
#[cfg(not(windows))]
type CharT = std::os::raw::c_char;

#[cfg(windows)]
fn to_chars(s: &str) -> Vec<CharT> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
#[cfg(not(windows))]
fn to_chars(s: &str) -> Vec<CharT> {
    s.bytes().map(|b| b as CharT).chain(std::iter::once(0)).collect()
}

#[repr(C)]
struct InitParams {
    size: usize,
    host_path: *const CharT,
    dotnet_root: *const CharT,
}

type InitForCommandLine = unsafe extern "system" fn(argc: i32, argv: *const *const CharT, params: *const InitParams, out: *mut *mut c_void) -> i32;
type GetRuntimeDelegate = unsafe extern "system" fn(ctx: *mut c_void, kind: i32, out: *mut *mut c_void) -> i32;
type LoadAssemblyAndGetFunctionPointer = unsafe extern "system" fn(
    assembly: *const CharT,
    type_name: *const CharT,
    method: *const CharT,
    delegate_type: *const CharT,
    reserved: *mut c_void,
    out: *mut *mut c_void,
) -> i32;

const HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER: i32 = 5;
const UNMANAGED_CALLERS_ONLY: *const CharT = usize::MAX as *const CharT;

type RunFn = unsafe extern "system" fn(script: *const u16, len: usize) -> *mut c_void;
type ReadFn = unsafe extern "system" fn(handle: *mut c_void, buf: *mut u16, cap: usize) -> usize;
type FreeFn = unsafe extern "system" fn(handle: *mut c_void);

/// The .NET runtime pwsh ships, started in this process from a PSHOME,
/// one per process; [`Runtime::bind`] returns the entry points of the
/// assemblies it loads.
pub struct Runtime {
    pshome: PathBuf,
    _snap_icu: Vec<libloading::Library>,
    _hostfxr: libloading::Library,
    load: LoadAssemblyAndGetFunctionPointer,
}

unsafe impl Send for Runtime {}
unsafe impl Sync for Runtime {}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").field("pshome", &self.pshome).finish_non_exhaustive()
    }
}

/// The runtime, started once for the process by the first
/// [`Runtime::start`]; a start that failed returns its error to every
/// later one.
static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

impl Runtime {
    /// Starts the runtime from `pshome` the first time it is called in a
    /// process, through hostfxr as running `pwsh.dll` does, so every
    /// assembly pwsh ships resolves; every later call returns that
    /// runtime. One naming another PSHOME is refused, since a process has
    /// one runtime, and a start that failed returns its error to every
    /// later one. A PSHOME in
    /// a Store package is hosted from a mirror under `~/.pwrs/pshome`, and
    /// one in a snap once the snap's ICU is loaded.
    pub fn start(pshome: &Path) -> Result<&'static Runtime, Error> {
        let rt = match RUNTIME.get_or_init(|| start_runtime_once(pshome).map_err(|e| e.to_string())) {
            Ok(rt) => rt,
            Err(why) => return Err(Error::msg(format!("the hosted runtime did not start: {why}"))),
        };
        if !same_folder(&rt.pshome, pshome) {
            return Err(Error::msg(format!(
                "this process's runtime started from {}, and a process has one runtime, so {} starts none",
                rt.pshome.display(),
                pshome.display()
            )));
        }
        Ok(rt)
    }

    /// The PSHOME the runtime was started from, as it was given.
    pub fn pshome(&self) -> &Path {
        &self.pshome
    }

    /// The address of the `[UnmanagedCallersOnly]` method `method` of the
    /// type `type_name`, assembly-qualified as `"Namespace.Type, Assembly"`,
    /// in the assembly at `assembly`, which the runtime loads into a load
    /// context of its own the first time. The caller calls it through the
    /// signature the method declares, which nothing here can check: a call
    /// through any other is undefined behavior.
    pub fn bind(&self, assembly: &Path, type_name: &str, method: &str) -> Result<NonNull<c_void>, Error> {
        let asm = to_chars(&assembly.display().to_string());
        let ty = to_chars(type_name);
        let m = to_chars(method);
        let mut out: *mut c_void = std::ptr::null_mut();
        let rc = unsafe { (self.load)(asm.as_ptr(), ty.as_ptr(), m.as_ptr(), UNMANAGED_CALLERS_ONLY, std::ptr::null_mut(), &mut out) };
        match NonNull::new(out) {
            Some(address) if rc == 0 => Ok(address),
            _ => Err(Error::msg(format!("cannot bind {method} of {type_name} from {}: 0x{rc:08x}", assembly.display()))),
        }
    }
}

/// Whether `a` and `b` name one folder: the same path once each is
/// resolved, or the same text when either cannot be.
fn same_folder(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        (resolved_a, resolved_b) => {
            if let Err(e) = &resolved_a {
                eprintln!("pwrs-host: cannot resolve {}: {e}", a.display());
            }
            if let Err(e) = &resolved_b {
                eprintln!("pwrs-host: cannot resolve {}: {e}", b.display());
            }
            a == b
        }
    }
}

fn hostfxr_file() -> &'static str {
    if cfg!(windows) {
        "hostfxr.dll"
    } else if cfg!(target_os = "macos") {
        "libhostfxr.dylib"
    } else {
        "libhostfxr.so"
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(to).map_err(|e| Error::msg(format!("cannot create {}: {e}", to.display())))?;
    let rd = std::fs::read_dir(from).map_err(|e| Error::msg(format!("cannot read {}: {e}", from.display())))?;
    for entry in rd {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", from.display())))?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| Error::msg(format!("cannot copy {} to {}: {e}", src.display(), dst.display())))?;
        }
    }
    Ok(())
}

/// A pwsh installed as a Store package lives under WindowsApps, whose
/// files other processes may read but not map as executables. Such an
/// install is mirrored once into `~/.pwrs/pshome/<folder name>` and
/// hosted from there; any other install is hosted in place.
fn hostable_pshome(pshome: &Path) -> Result<PathBuf, Error> {
    let under_windows_apps = pshome.components().any(|c| c.as_os_str().eq_ignore_ascii_case("WindowsApps"));
    if !under_windows_apps {
        return Ok(pshome.to_path_buf());
    }
    let folder = match pshome.file_name() {
        Some(f) => f.to_string_lossy().into_owned(),
        None => return Err(Error::msg(format!("{} has no folder name", pshome.display()))),
    };
    let home = match std::env::var("USERPROFILE").or_else(|_not_windows| std::env::var("HOME")) {
        Ok(h) => PathBuf::from(h),
        Err(e) => return Err(Error::msg(format!("neither USERPROFILE nor HOME is set: {e}"))),
    };
    let mirror = home.join(".pwrs").join("pshome").join(folder);
    if !mirror.join(".complete").is_file() {
        eprintln!("pwrs-host: mirroring {} to {}", pshome.display(), mirror.display());
        copy_tree(pshome, &mirror)?;
        std::fs::write(mirror.join(".complete"), "").map_err(|e| Error::msg(format!("cannot mark the mirror complete: {e}")))?;
    }
    Ok(mirror)
}

/// The executable a pwsh install runs.
const PWSH_FILE: &str = if cfg!(windows) { "pwsh.exe" } else { "pwsh" };

/// Whether `dir` has what [`Runtime::start`] loads: `pwsh.dll` and hostfxr.
fn holds_runtime(dir: &Path) -> bool {
    dir.join("pwsh.dll").is_file() && dir.join(hostfxr_file()).is_file()
}

/// The PSHOME of the pwsh this process would host, found without
/// starting pwsh: the folder `PWRS_PSHOME` names when it is set; else the
/// folder of the first pwsh on PATH, followed through its links, and on
/// Windows through a Store install's alias to its package folder, or on
/// Linux through a snap's command to the snaps' PowerShell; else the
/// platform's default install folder. A folder counts when it has
/// `pwsh.dll` and hostfxr; one `PWRS_PSHOME` names that lacks them is
/// refused rather than skipped, and when none counts the error lists
/// every folder looked in.
pub fn find_pshome() -> Result<PathBuf, Error> {
    find_in(std::env::var_os("PWRS_PSHOME"), std::env::var_os("PATH"), &install_folders())
}

/// [`find_pshome`] over the `PWRS_PSHOME` and `PATH` given and the
/// install folders `installs`.
fn find_in(named: Option<std::ffi::OsString>, path: Option<std::ffi::OsString>, installs: &[PathBuf]) -> Result<PathBuf, Error> {
    if let Some(named) = named.filter(|n| !n.is_empty()) {
        let dir = PathBuf::from(named);
        if holds_runtime(&dir) {
            return Ok(dir);
        }
        return Err(Error::msg(format!("PWRS_PSHOME names {}, which has no pwsh.dll and {}", dir.display(), hostfxr_file())));
    }
    let mut looked = Vec::new();
    for dir in path.iter().flat_map(std::env::split_paths) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(PWSH_FILE);
        if std::fs::symlink_metadata(&candidate).is_err() {
            continue;
        }
        match homes_of(&candidate) {
            Ok(homes) => {
                for home in homes {
                    if holds_runtime(&home) {
                        return Ok(home);
                    }
                    looked.push(format!("{} (from {})", home.display(), candidate.display()));
                }
            }
            Err(why) => looked.push(format!("{} ({why})", candidate.display())),
        }
    }
    for dir in installs {
        if holds_runtime(dir) {
            return Ok(dir.clone());
        }
        looked.push(dir.display().to_string());
    }
    Err(Error::msg(format!(
        "no PSHOME found: PWRS_PSHOME is not set, and none of these has pwsh.dll and {}: {}",
        hostfxr_file(),
        if looked.is_empty() { String::from("no pwsh on PATH and no install folder") } else { looked.join(", ") }
    )))
}

/// The folders a pwsh found at `candidate` may run from: a Store alias's
/// package folder, the folder its links end in, or, for a snap's command,
/// each snap's PowerShell.
fn homes_of(candidate: &Path) -> Result<Vec<PathBuf>, String> {
    #[cfg(windows)]
    if let Some(target) = app_exec_link_target(candidate)? {
        return Ok(target.parent().map(Path::to_path_buf).into_iter().collect());
    }
    let resolved = std::fs::canonicalize(candidate).map_err(|e| format!("cannot be followed: {e}"))?;
    #[cfg(target_os = "linux")]
    if resolved.file_name().is_some_and(|n| n == "snap") {
        return snap_homes();
    }
    Ok(resolved.parent().map(|p| plain_path(p.to_path_buf())).into_iter().collect())
}

/// `path` without the `\\?\` that resolving a path on Windows puts before
/// a drive letter, as hostfxr and pwsh name the folder.
fn plain_path(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// The PowerShell each snap under `/snap` has in its current
/// revision's `opt/powershell`, in name order: a snap's command runs
/// `/usr/bin/snap`, which picks the snap by the name it was run as.
#[cfg(target_os = "linux")]
fn snap_homes() -> Result<Vec<PathBuf>, String> {
    let mut homes = Vec::new();
    for entry in std::fs::read_dir("/snap").map_err(|e| format!("its snaps cannot be listed: {e}"))? {
        let entry = entry.map_err(|e| format!("its snaps cannot be listed: {e}"))?;
        homes.push(entry.path().join("current").join("opt").join("powershell"));
    }
    homes.sort();
    Ok(homes)
}

/// The executable a Store app's execution alias at `path` runs, or none
/// when `path` is no such alias. The alias is a reparse point whose data
/// is a version and then strings: the package's family name, the app's
/// user model id, and the executable's path.
#[cfg(windows)]
fn app_exec_link_target(path: &Path) -> Result<Option<PathBuf>, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_NOT_A_REPARSE_POINT, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, MAXIMUM_REPARSE_DATA_BUFFER_SIZE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call; no access
    // rights are requested, and reading a reparse point needs none.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!("cannot be opened: {}", std::io::Error::last_os_error()));
    }
    let mut buf = vec![0u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize];
    let mut returned = 0u32;
    // SAFETY: `handle` is open, and `buf` is writable for the length given.
    let ok = unsafe { DeviceIoControl(handle, FSCTL_GET_REPARSE_POINT, std::ptr::null(), 0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut returned, std::ptr::null_mut()) };
    let failure = std::io::Error::last_os_error();
    // SAFETY: `handle` is open, and is closed once.
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        if failure.raw_os_error() == Some(ERROR_NOT_A_REPARSE_POINT as i32) {
            return Ok(None);
        }
        return Err(format!("its reparse point cannot be read: {failure}"));
    }
    alias_target(&buf[..returned as usize]).map(|t| t.map(PathBuf::from))
}

/// The executable an execution alias's reparse data names, or none when
/// the data is another kind of reparse point's: the tag, the data's
/// length and two reserved bytes, then the alias's version and its
/// NUL-ended UTF-16 strings, the third of which is the executable.
#[cfg(any(windows, test))]
fn alias_target(data: &[u8]) -> Result<Option<String>, String> {
    /// The reparse tag of an execution alias.
    const IO_REPARSE_TAG_APPEXECLINK: u32 = 0x8000_001B;
    match data.get(0..4) {
        Some(tag) if u32::from_le_bytes([tag[0], tag[1], tag[2], tag[3]]) == IO_REPARSE_TAG_APPEXECLINK => {}
        _ => return Ok(None),
    }
    let strings = data.get(12..).ok_or_else(|| String::from("is an execution alias with no data"))?;
    let units: Vec<u16> = strings.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect();
    match units.split(|u| *u == 0).nth(2).map(String::from_utf16_lossy) {
        Some(target) if !target.is_empty() => Ok(Some(target)),
        _ => Err(String::from("is an execution alias whose data names no executable")),
    }
}

/// Each platform's default install folder for PowerShell 7.
fn install_folders() -> Vec<PathBuf> {
    if cfg!(windows) {
        let program_files = std::env::var_os("ProgramFiles").filter(|p| !p.is_empty()).map_or_else(|| PathBuf::from(r"C:\Program Files"), PathBuf::from);
        vec![program_files.join("PowerShell").join("7")]
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from("/usr/local/microsoft/powershell/7")]
    } else {
        vec![PathBuf::from("/opt/microsoft/powershell/7")]
    }
}

/// A pwsh installed as a snap runs on the ICU the snap bundles, which
/// its executable finds through an RPATH into the snap and which the
/// snap's launcher names in `CLR_ICU_VERSION_OVERRIDE`. A process outside
/// the snap has neither, and .NET ends a process in which no ICU loads.
/// So the snap's ICU is loaded here by full path, each library before
/// the one that needs it, where .NET's lookup by name finds it already
/// loaded, and the override names its version unless one is set. A
/// PSHOME outside a snap, or a snap with no ICU, loads nothing.
#[cfg(target_os = "linux")]
fn snap_icu(pshome: &Path) -> Result<Vec<libloading::Library>, Error> {
    use libloading::os::unix::{Library, RTLD_GLOBAL, RTLD_NOW};

    let root = match pshome.ancestors().find(|a| a.join("meta").join("snap.yaml").is_file()) {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };
    let lib_root = root.join("usr").join("lib");
    let mut dirs = vec![lib_root.clone()];
    for path in entries_if_present(&lib_root)? {
        if path.is_dir() {
            dirs.push(path);
        }
    }
    // The newest `libicuuc.so.<major>.<minor>` the snap has, with the
    // directory it is in.
    let mut found: Option<(PathBuf, String)> = None;
    for dir in &dirs {
        for path in entries_if_present(dir)? {
            let name = match path.file_name() {
                Some(n) => n.to_string_lossy().into_owned(),
                None => continue,
            };
            let version = match name.strip_prefix("libicuuc.so.") {
                Some(v) => v,
                None => continue,
            };
            if !is_dotted_version(version) {
                continue;
            }
            let newer = match &found {
                Some((_, best)) => version_key(version) > version_key(best),
                None => true,
            };
            if newer {
                found = Some((dir.clone(), version.to_string()));
            }
        }
    }
    let (dir, version) = match found {
        Some(f) => f,
        None => return Ok(Vec::new()),
    };
    let major = match version.split_once('.') {
        Some((major, _minor)) => major,
        None => version.as_str(),
    };
    let mut loaded = Vec::new();
    for name in ["libicudata", "libicuuc", "libicui18n"] {
        let path = dir.join(format!("{name}.so.{major}"));
        let lib = unsafe { Library::open(Some(&path), RTLD_NOW | RTLD_GLOBAL) }
            .map_err(|e| Error::msg(format!("cannot load the snap's {}: {e}", path.display())))?;
        loaded.push(libloading::Library::from(lib));
    }
    if std::env::var_os("CLR_ICU_VERSION_OVERRIDE").is_none() {
        // SAFETY: the hosted runtime's threads are the only ones that
        // read the environment outside std's own synchronization, and it
        // has not started: this runs inside RUNTIME's one initialization,
        // before hostfxr is initialized.
        unsafe { std::env::set_var("CLR_ICU_VERSION_OVERRIDE", &version) };
    }
    Ok(loaded)
}

/// The entries of `dir`, or none when `dir` does not exist.
#[cfg(target_os = "linux")]
fn entries_if_present(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::msg(format!("cannot read {}: {e}", dir.display()))),
    };
    let mut paths = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| Error::msg(format!("cannot read an entry of {}: {e}", dir.display())))?;
        paths.push(entry.path());
    }
    Ok(paths)
}

/// `70.1` and the like: two or more runs of digits joined by dots.
#[cfg(target_os = "linux")]
fn is_dotted_version(v: &str) -> bool {
    v.contains('.') && v.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Orders dotted versions by number: of two runs of digits the longer
/// is the larger, and two of one length compare as text.
#[cfg(target_os = "linux")]
fn version_key(v: &str) -> Vec<(usize, &str)> {
    v.split('.').map(|p| (p.len(), p)).collect()
}

#[cfg(not(target_os = "linux"))]
fn snap_icu(_pshome: &Path) -> Result<Vec<libloading::Library>, Error> {
    Ok(Vec::new())
}

fn start_runtime_once(given: &Path) -> Result<Runtime, Error> {
    let pshome = &hostable_pshome(given)?;
    let snap_icu = snap_icu(pshome)?;
    let lib = unsafe { libloading::Library::new(pshome.join(hostfxr_file())) }
        .map_err(|e| Error::msg(format!("cannot load hostfxr from {}: {e}", pshome.display())))?;
    let init: InitForCommandLine = unsafe {
        let sym: libloading::Symbol<InitForCommandLine> = lib
            .get(b"hostfxr_initialize_for_dotnet_command_line\0")
            .map_err(|e| Error::msg(format!("hostfxr has no hostfxr_initialize_for_dotnet_command_line: {e}")))?;
        *sym
    };
    let get_delegate: GetRuntimeDelegate = unsafe {
        let sym: libloading::Symbol<GetRuntimeDelegate> =
            lib.get(b"hostfxr_get_runtime_delegate\0").map_err(|e| Error::msg(format!("hostfxr has no hostfxr_get_runtime_delegate: {e}")))?;
        *sym
    };

    let app = to_chars(&pshome.join("pwsh.dll").display().to_string());
    let host_path = to_chars(&pshome.join(if cfg!(windows) { "pwsh.exe" } else { "pwsh" }).display().to_string());
    let root = to_chars(&pshome.display().to_string());
    let argv = [app.as_ptr()];
    let params = InitParams { size: std::mem::size_of::<InitParams>(), host_path: host_path.as_ptr(), dotnet_root: root.as_ptr() };
    let mut ctx: *mut c_void = std::ptr::null_mut();
    let rc = unsafe { init(1, argv.as_ptr(), &params, &mut ctx) };
    if rc != 0 || ctx.is_null() {
        return Err(Error::msg(format!("hostfxr_initialize_for_dotnet_command_line failed with 0x{rc:08x}")));
    }
    let mut delegate: *mut c_void = std::ptr::null_mut();
    let rc = unsafe { get_delegate(ctx, HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER, &mut delegate) };
    if rc != 0 || delegate.is_null() {
        return Err(Error::msg(format!("hostfxr_get_runtime_delegate failed with 0x{rc:08x}")));
    }
    let load: LoadAssemblyAndGetFunctionPointer = unsafe { std::mem::transmute(delegate) };
    Ok(Runtime { pshome: given.to_path_buf(), _snap_icu: snap_icu, _hostfxr: lib, load })
}

/// Everything a script produced.
#[derive(Debug, Deserialize, Default)]
pub struct RunResult {
    #[serde(rename = "Output")]
    pub output: Vec<String>,
    #[serde(rename = "Errors")]
    pub errors: Vec<String>,
    #[serde(rename = "Verbose")]
    pub verbose: Vec<String>,
    #[serde(rename = "Warning")]
    pub warning: Vec<String>,
    #[serde(rename = "Information")]
    pub information: Vec<String>,
    /// Exception that escaped `Invoke`, formatted as `Type: message`.
    #[serde(rename = "Terminating")]
    pub terminating: Option<String>,
}

/// A PowerShell session inside this process. Its scripts run in the
/// test host's runspace for the calling thread, so a session stays on
/// the thread that started it, and the state one script leaves is there
/// for the next.
pub struct Session {
    run: RunFn,
    read: ReadFn,
    free: FreeFn,
    /// Ties the session to its thread: the runspace is that thread's.
    _thread: std::marker::PhantomData<*const ()>,
}

impl Session {
    /// Starts the runtime from `pshome` (default: the pwsh on PATH) and
    /// loads the test host assembly at `testhost_dll`.
    pub fn start(pshome: Option<&Path>, testhost_dll: &Path) -> Result<Session, Error> {
        let pshome = match pshome {
            Some(p) => p.to_path_buf(),
            None => pwrs_build::pwsh::pshome()?,
        };
        let rt = Runtime::start(&pshome)?;
        let get = |method: &str| rt.bind(testhost_dll, "Pwrs.TestHost.Host, Pwrs.TestHost", method);
        let run = get("Run")?;
        let read = get("Read")?;
        let free = get("Free")?;
        // SAFETY: the test host declares Run, Read and Free with exactly
        // these signatures.
        Ok(Session {
            run: unsafe { std::mem::transmute::<*mut c_void, RunFn>(run.as_ptr()) },
            read: unsafe { std::mem::transmute::<*mut c_void, ReadFn>(read.as_ptr()) },
            free: unsafe { std::mem::transmute::<*mut c_void, FreeFn>(free.as_ptr()) },
            _thread: std::marker::PhantomData,
        })
    }

    /// Runs a script and returns every stream.
    pub fn run(&self, script: &str) -> Result<RunResult, Error> {
        let utf16: Vec<u16> = script.encode_utf16().collect();
        let handle = unsafe { (self.run)(utf16.as_ptr(), utf16.len()) };
        if handle.is_null() {
            return Err(Error::msg("test host returned no result"));
        }
        let len = unsafe { (self.read)(handle, std::ptr::null_mut(), 0) };
        let mut buf = vec![0u16; len];
        unsafe { (self.read)(handle, buf.as_mut_ptr(), buf.len()) };
        unsafe { (self.free)(handle) };
        let json = String::from_utf16_lossy(&buf);
        serde_json::from_str(&json).map_err(|e| Error::msg(format!("test host returned malformed JSON: {e}\n{json}")))
    }

    /// Imports a module folder built by `cargo pwrs build`.
    pub fn import_module(&self, module_dir: &Path) -> Result<RunResult, Error> {
        let path = module_dir.display().to_string().replace('\'', "''");
        self.run(&format!("Import-Module '{path}' -ErrorAction Stop"))
    }
}

/// Whether the folder `built` has a whole build whose stamp reads
/// `current`.
fn built_from(built: &Path, current: &str) -> bool {
    match std::fs::read_to_string(built.join("Pwrs.TestHost.stamp")) {
        Ok(stamp) => stamp == current && built.join("Pwrs.TestHost.dll").is_file(),
        Err(_no_build_there) => false,
    }
}

/// Builds started by this process, which name their folders.
static BUILDS: AtomicU64 = AtomicU64::new(0);

/// Compiles `Pwrs.TestHost.dll` for net10.0 with the fetched toolchain
/// into `out_dir/current`, returning its path. Each compile writes a
/// folder of its own, stamp last, and renames the whole folder to
/// `current`, so a `current` that exists is a whole build; of two
/// compiles racing, in one process or two, the second finds the first's
/// folder there and returns it.
pub fn build_testhost(out_dir: &Path) -> Result<PathBuf, Error> {
    let source = include_str!("../dotnet/Pwrs.TestHost/Host.cs");
    // The reference pack is part of what was built, so a new one
    // rebuilds the host as a changed source does.
    let current = format!("{} {}", source.len(), pwrs_build::toolchain::CORE_REF_VERSION);
    let published = out_dir.join("current");
    let dll = published.join("Pwrs.TestHost.dll");
    if built_from(&published, &current) {
        return Ok(dll);
    }
    let tc = pwrs_build::Toolchain::ensure()?;
    let build = out_dir.join(format!("build-{}-{}", std::process::id(), BUILDS.fetch_add(1, Ordering::Relaxed)));
    let src_dir = build.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|e| Error::msg(format!("cannot create {}: {e}", src_dir.display())))?;
    let src = src_dir.join("Host.cs");
    std::fs::write(&src, source).map_err(|e| Error::msg(format!("cannot write {}: {e}", src.display())))?;
    let work = build.join("work");
    let env = pwrs_build::CompileEnv {
        refs: &tc.net10_refs,
        defines: pwrs_build::toolchain::CORE_DEFINES,
        work: &work,
        debug: pwrs_build::DebugSymbols::Emit,
        path_map: &[],
    };
    tc.compile(&env, &[src], &[], &build.join("Pwrs.TestHost.dll"))?;
    let stamp = build.join("Pwrs.TestHost.stamp");
    std::fs::write(&stamp, &current).map_err(|e| Error::msg(format!("cannot write {}: {e}", stamp.display())))?;
    // A build of other source in the way goes first; a failure to remove
    // it is reported by the rename below, which then finds it still there.
    if published.exists()
        && !built_from(&published, &current)
        && let Err(e) = std::fs::remove_dir_all(&published)
    {
        eprintln!("pwrs-host: cannot remove the earlier build {}: {e}", published.display());
    }
    match std::fs::rename(&build, &published) {
        Ok(()) => Ok(dll),
        Err(_another_build_was_renamed_first) if built_from(&published, &current) => {
            if let Err(e) = std::fs::remove_dir_all(&build) {
                eprintln!("pwrs-host: cannot remove the duplicate build {}: {e}", build.display());
            }
            Ok(dll)
        }
        Err(e) => Err(Error::msg(format!("cannot move {} to {}: {e}", build.display(), published.display()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh scratch folder named for `test`.
    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pwrs-host-{test}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("clear the scratch folder");
        }
        std::fs::create_dir_all(&dir).expect("make the scratch folder");
        dir
    }

    /// Lays out at `dir` the files a PSHOME is told by: pwsh, `pwsh.dll`
    /// and hostfxr, each empty.
    fn pshome_at(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).expect("make the PSHOME");
        for file in [PWSH_FILE, "pwsh.dll", hostfxr_file()] {
            std::fs::write(dir.join(file), b"").expect("write a PSHOME file");
        }
        dir.to_path_buf()
    }

    fn found(outcome: Result<PathBuf, Error>) -> PathBuf {
        match outcome {
            Ok(p) => p,
            Err(e) => panic!("no PSHOME was found: {e}"),
        }
    }

    fn refused(outcome: Result<PathBuf, Error>) -> String {
        match outcome {
            Ok(p) => panic!("{} was found where none should be", p.display()),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn a_pshome_is_found_by_pwrs_pshome_then_path_then_an_install_folder() {
        let root = scratch("find");
        let named = pshome_at(&root.join("named"));
        let on_path = pshome_at(&root.join("on-path"));
        let installed = pshome_at(&root.join("installed"));
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).expect("make an empty folder");
        let path = |dirs: &[&PathBuf]| match std::env::join_paths(dirs) {
            Ok(p) => Some(p),
            Err(e) => panic!("cannot join a PATH: {e}"),
        };
        assert_eq!(found(find_in(Some(named.clone().into()), path(&[&on_path]), std::slice::from_ref(&installed))), named, "PWRS_PSHOME comes first");
        let wrong = refused(find_in(Some(empty.clone().into()), path(&[&on_path]), std::slice::from_ref(&installed)));
        assert!(wrong.starts_with(&format!("PWRS_PSHOME names {}, which has no pwsh.dll", empty.display())), "one PWRS_PSHOME names is not skipped: {wrong}");
        let from_path = found(find_in(Some(std::ffi::OsString::new()), path(&[&empty, &on_path]), std::slice::from_ref(&installed)));
        assert!(same_folder(&from_path, &on_path), "an empty PWRS_PSHOME is none, and a PATH folder without pwsh is skipped: {}", from_path.display());
        assert_eq!(found(find_in(None, path(&[&empty]), &[empty.join("absent"), installed.clone()])), installed, "with no pwsh on PATH, the first install folder with one");
        let none = refused(find_in(None, None, std::slice::from_ref(&empty)));
        assert!(none.starts_with("no PSHOME found: PWRS_PSHOME is not set") && none.contains(&empty.display().to_string()), "{none}");
        let pwsh_alone = root.join("pwsh-alone");
        std::fs::create_dir_all(&pwsh_alone).expect("make a folder");
        std::fs::write(pwsh_alone.join(PWSH_FILE), b"").expect("write pwsh");
        let alone = refused(find_in(None, path(&[&pwsh_alone]), &[]));
        assert!(alone.contains(&format!("(from {})", pwsh_alone.join(PWSH_FILE).display())), "a pwsh whose folder lacks the runtime is listed: {alone}");
        std::fs::remove_dir_all(&root).expect("remove the scratch folder");
    }

    #[cfg(unix)]
    #[test]
    fn a_pwsh_on_path_is_followed_through_its_links_to_its_pshome() {
        let root = scratch("links");
        let home = pshome_at(&root.join("opt").join("powershell"));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("make bin");
        std::os::unix::fs::symlink(home.join("pwsh"), bin.join("pwsh")).expect("link pwsh into bin");
        let path = std::env::join_paths([&bin]).expect("join a PATH");
        let from_link = found(find_in(None, Some(path), &[]));
        assert!(same_folder(&from_link, &home), "{} is not {}", from_link.display(), home.display());
        std::fs::remove_dir_all(&root).expect("remove the scratch folder");
    }

    /// An execution alias's reparse data: the tag, the data's length and
    /// two reserved bytes, then the version and the NUL-ended strings.
    fn alias_data(tag: u32, strings: &[&str]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend(tag.to_le_bytes());
        data.extend([0u8; 4]);
        data.extend(3u32.to_le_bytes());
        for s in strings {
            for unit in s.encode_utf16() {
                data.extend(unit.to_le_bytes());
            }
            data.extend([0u8; 2]);
        }
        data
    }

    #[test]
    fn an_execution_alias_names_the_executable_it_runs() {
        let target = r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.6.0_x64__8wekyb3d8bbwe\pwsh.exe";
        let alias = alias_data(0x8000_001B, &["Microsoft.PowerShell_8wekyb3d8bbwe", "Microsoft.PowerShell_8wekyb3d8bbwe!App", target, "0"]);
        assert_eq!(alias_target(&alias), Ok(Some(String::from(target))));
        assert_eq!(alias_target(&alias_data(0xA000_000C, &["C:\\elsewhere"])), Ok(None), "a symbolic link is no alias");
        let short = alias_target(&alias[..20]);
        assert!(matches!(&short, Err(why) if why.contains("names no executable")), "{short:?}");
    }

    #[cfg(windows)]
    #[test]
    fn a_store_alias_on_this_machine_leads_to_the_pshome_pwsh_reports() {
        let alias = match std::env::var_os("LOCALAPPDATA") {
            Some(local) => PathBuf::from(local).join("Microsoft").join("WindowsApps").join("pwsh.exe"),
            None => panic!("LOCALAPPDATA is not set"),
        };
        if std::fs::symlink_metadata(&alias).is_err() {
            eprintln!("no Store alias at {}; this machine's pwsh is not a Store install", alias.display());
            return;
        }
        let target = match app_exec_link_target(&alias) {
            Ok(Some(t)) => t,
            other => panic!("{} reads as {other:?}", alias.display()),
        };
        let reported = match pwrs_build::pwsh::pshome() {
            Ok(p) => p,
            Err(e) => panic!("pwsh did not report its PSHOME: {e}"),
        };
        match target.parent() {
            Some(home) => assert!(same_folder(home, &reported), "the alias runs {}, and pwsh reports {}", target.display(), reported.display()),
            None => panic!("{} has no folder", target.display()),
        }
    }
}
