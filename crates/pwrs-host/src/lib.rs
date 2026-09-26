//! In-process hosting of the .NET runtime that pwsh ships, and a
//! PowerShell session inside it.
//!
//! The runtime is started through `hostfxr` from `$PSHOME` as if
//! running `pwsh.dll`, which makes every assembly pwsh ships resolvable,
//! then `Pwrs.TestHost.dll` is loaded and its `UnmanagedCallersOnly`
//! entry points are called directly. Windows PowerShell 5.1 has no
//! hostfxr and is covered by spawning `powershell.exe` instead.
//!
//! One runtime per process: a second [`Session::start`] reuses the
//! first runtime.

use serde::Deserialize;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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

struct Runtime {
    _snap_icu: Vec<libloading::Library>,
    _hostfxr: libloading::Library,
    load: LoadAssemblyAndGetFunctionPointer,
}

unsafe impl Send for Runtime {}
unsafe impl Sync for Runtime {}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

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
    static MIRROR_LOCK: Mutex<()> = Mutex::new(());
    let _guard = match MIRROR_LOCK.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if !mirror.join(".complete").is_file() {
        eprintln!("pwrs-host: mirroring {} to {}", pshome.display(), mirror.display());
        copy_tree(pshome, &mirror)?;
        std::fs::write(mirror.join(".complete"), "").map_err(|e| Error::msg(format!("cannot mark the mirror complete: {e}")))?;
    }
    Ok(mirror)
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
    // The newest `libicuuc.so.<major>.<minor>` the snap carries, with
    // the directory it is in.
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
        std::env::set_var("CLR_ICU_VERSION_OVERRIDE", &version);
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

fn start_runtime(pshome: &Path) -> Result<&'static Runtime, Error> {
    static START_LOCK: Mutex<()> = Mutex::new(());
    let _guard = match START_LOCK.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(rt) = RUNTIME.get() {
        return Ok(rt);
    }
    let pshome = &hostable_pshome(pshome)?;
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
    let rt = Runtime { _snap_icu: snap_icu, _hostfxr: lib, load };
    match RUNTIME.set(rt) {
        Ok(()) => {}
        Err(_already_set_by_another_thread) => {}
    }
    Ok(RUNTIME.get().expect("runtime set just above"))
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

/// A PowerShell session inside this process.
pub struct Session {
    run: RunFn,
    read: ReadFn,
    free: FreeFn,
    lock: Mutex<()>,
}

impl Session {
    /// Starts the runtime from `pshome` (default: the pwsh on PATH) and
    /// loads the test host assembly at `testhost_dll`.
    pub fn start(pshome: Option<&Path>, testhost_dll: &Path) -> Result<Session, Error> {
        let pshome = match pshome {
            Some(p) => p.to_path_buf(),
            None => pwrs_build::pwsh::pshome()?,
        };
        let rt = start_runtime(&pshome)?;
        let asm = to_chars(&testhost_dll.display().to_string());
        let type_name = to_chars("Pwrs.TestHost.Host, Pwrs.TestHost");
        let get = |method: &str| -> Result<*mut c_void, Error> {
            let m = to_chars(method);
            let mut out: *mut c_void = std::ptr::null_mut();
            let rc = unsafe { (rt.load)(asm.as_ptr(), type_name.as_ptr(), m.as_ptr(), UNMANAGED_CALLERS_ONLY, std::ptr::null_mut(), &mut out) };
            if rc != 0 || out.is_null() {
                return Err(Error::msg(format!("cannot bind {method} from {}: 0x{rc:08x}", testhost_dll.display())));
            }
            Ok(out)
        };
        let run = get("Run")?;
        let read = get("Read")?;
        let free = get("Free")?;
        Ok(Session {
            run: unsafe { std::mem::transmute::<*mut c_void, RunFn>(run) },
            read: unsafe { std::mem::transmute::<*mut c_void, ReadFn>(read) },
            free: unsafe { std::mem::transmute::<*mut c_void, FreeFn>(free) },
            lock: Mutex::new(()),
        })
    }

    /// Runs a script and returns every stream.
    pub fn run(&self, script: &str) -> Result<RunResult, Error> {
        let _guard = match self.lock.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
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

/// Compiles `Pwrs.TestHost.dll` for net10.0 with the fetched toolchain
/// into `out_dir`, returning its path.
pub fn build_testhost(out_dir: &Path) -> Result<PathBuf, Error> {
    static BUILD_LOCK: Mutex<()> = Mutex::new(());
    let _guard = match BUILD_LOCK.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let dll = out_dir.join("Pwrs.TestHost.dll");
    let source = include_str!("../dotnet/Pwrs.TestHost/Host.cs");
    let stamp = out_dir.join("Pwrs.TestHost.stamp");
    // The reference pack is part of what was built, so a new one
    // rebuilds the host as a changed source does.
    let current = format!("{} {}", source.len(), pwrs_build::toolchain::CORE_REF_VERSION);
    let fresh = match std::fs::read_to_string(&stamp) {
        Ok(previous) => previous == current && dll.is_file(),
        Err(_missing) => false,
    };
    if fresh {
        return Ok(dll);
    }
    let tc = pwrs_build::Toolchain::ensure()?;
    let src_dir = out_dir.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|e| Error::msg(format!("cannot create {}: {e}", src_dir.display())))?;
    let src = src_dir.join("Host.cs");
    std::fs::write(&src, source).map_err(|e| Error::msg(format!("cannot write {}: {e}", src.display())))?;
    let work = out_dir.join("work");
    let env = pwrs_build::CompileEnv {
        refs: &tc.net10_refs,
        defines: pwrs_build::toolchain::CORE_DEFINES,
        work: &work,
        debug: pwrs_build::DebugSymbols::Emit,
    };
    tc.compile(&env, &[src], &[], &dll)?;
    std::fs::write(&stamp, current).map_err(|e| Error::msg(format!("cannot write {}: {e}", stamp.display())))?;
    Ok(dll)
}
