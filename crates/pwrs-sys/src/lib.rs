//! The C ABI shared by every pwrs native module and the managed
//! `Pwrs.Runtime` support assembly.
//!
//! Nothing here is PowerShell-typed. Every managed object crosses as a
//! [`PsHandle`] (a `GCHandle` the runtime allocated), every string as
//! a UTF-16 `(ptr, len)` pair, every primitive inline, and every
//! failure as a [`PsStatus`] plus an exception handle written through
//! an out-pointer. No unwinding crosses this boundary in either
//! direction.
//!
//! [`HostVTable`] is the stable ABI. It is append-only: a newer
//! runtime hands an older module a larger table, and the module reads
//! `size` before touching any entry past the version it was compiled
//! against. `docs/ABI.md` lists every entry with the version that
//! introduced it.

#![no_std]

use core::ffi::c_void;

/// Opaque handle to a managed object. Wraps the `IntPtr` of a
/// `GCHandle`. Null means `$null`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PsHandle(pub *mut c_void);

impl PsHandle {
    pub const NULL: PsHandle = PsHandle(core::ptr::null_mut());
    #[inline]
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }
}

/// Result code of every vtable entry and every native export. `0` is
/// success; the exception object, when there is one, arrives through
/// the `err` out-pointer.
pub type PsStatus = i32;

pub const PS_OK: PsStatus = 0;
/// A managed exception was thrown inside the runtime entry; `err`
/// holds it.
pub const PS_ERR_MANAGED_EXCEPTION: PsStatus = 1;
/// The native side panicked. `err` holds a managed string with the
/// panic message rather than an exception.
pub const PS_ERR_NATIVE_PANIC: PsStatus = 2;
/// A pipeline-thread-only entry was called from another thread.
pub const PS_ERR_WRONG_THREAD: PsStatus = 3;
/// The module and runtime disagree on ABI version.
pub const PS_ERR_ABI_MISMATCH: PsStatus = 4;
/// The pipeline was stopped; the cmdlet must return promptly.
pub const PS_ERR_PIPELINE_STOPPED: PsStatus = 5;

/// ABI version this crate describes. Bumped only when an existing
/// entry changes meaning; appending entries changes `size` alone.
pub const PWRS_ABI_VERSION: u32 = 1;

/// Which PowerShell stream a `write_stream` call targets.
pub type PsStreamKind = u32;
pub const PS_STREAM_VERBOSE: PsStreamKind = 1;
pub const PS_STREAM_DEBUG: PsStreamKind = 2;
pub const PS_STREAM_WARNING: PsStreamKind = 3;
pub const PS_STREAM_INFORMATION: PsStreamKind = 4;

/// Lifecycle phase passed to the cmdlet entry point.
pub type PsPhase = u32;
pub const PS_PHASE_BEGIN: PsPhase = 0;
pub const PS_PHASE_PROCESS: PsPhase = 1;
pub const PS_PHASE_END: PsPhase = 2;

/// Bit set `pwrs_cmdlet_create` reports: the phases this cmdlet type
/// needs a native call for. Every bit is set until the runtime has
/// observed a phase run the trait's default body, after which that
/// phase's bit is clear for every later instance of the type.
pub type PsPhaseMask = u32;
pub const PS_PHASE_MASK_BEGIN: PsPhaseMask = 1;
pub const PS_PHASE_MASK_PROCESS: PsPhaseMask = 2;
pub const PS_PHASE_MASK_END: PsPhaseMask = 4;
pub const PS_PHASE_MASK_ALL: PsPhaseMask = 7;

/// `System.Management.Automation.ErrorCategory`, by numeric value.
pub type PsErrorCategory = u32;

/// Element type tag for typed arrays and parameter-block slots.
pub type PsTypeTag = u32;
pub const PS_TYPE_OBJECT: PsTypeTag = 0;
pub const PS_TYPE_BOOL: PsTypeTag = 1;
pub const PS_TYPE_I8: PsTypeTag = 2;
pub const PS_TYPE_I16: PsTypeTag = 3;
pub const PS_TYPE_I32: PsTypeTag = 4;
pub const PS_TYPE_I64: PsTypeTag = 5;
pub const PS_TYPE_U8: PsTypeTag = 6;
pub const PS_TYPE_U16: PsTypeTag = 7;
pub const PS_TYPE_U32: PsTypeTag = 8;
pub const PS_TYPE_U64: PsTypeTag = 9;
pub const PS_TYPE_F32: PsTypeTag = 10;
pub const PS_TYPE_F64: PsTypeTag = 11;
pub const PS_TYPE_STRING: PsTypeTag = 12;
pub const PS_TYPE_CHAR: PsTypeTag = 13;
pub const PS_TYPE_DATETIME: PsTypeTag = 14;
pub const PS_TYPE_TIMESPAN: PsTypeTag = 15;
pub const PS_TYPE_GUID: PsTypeTag = 16;
pub const PS_TYPE_DECIMAL: PsTypeTag = 17;

/// A borrowed UTF-16 string crossing the boundary. Never owns.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PsStr16 {
    pub ptr: *const u16,
    pub len: usize,
}

/// A pinned view of a managed primitive array. `pin` is released
/// with `array_unpin` before the phase that pinned it returns.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PsPinned {
    pub data: *mut c_void,
    pub len: usize,
    pub elem_size: u32,
    pub pin: PsHandle,
}

/// Per-module descriptor the build tool reads from the built cdylib.
/// JSON, produced by the proc macros; the C# generator consumes it.
#[repr(C)]
pub struct ModuleDescriptor {
    pub json_utf8: *const u8,
    pub json_len: usize,
}

/// Function table the managed runtime hands the native module at
/// `pwrs_module_init`. Every entry is `extern "C"`, never unwinds,
/// and reports failure through its return code. Entries marked
/// "pipeline thread" return [`PS_ERR_WRONG_THREAD`] elsewhere.
#[repr(C)]
pub struct HostVTable {
    /// `size_of::<HostVTable>()` as the runtime built it.
    pub size: u32,
    /// [`PWRS_ABI_VERSION`] the runtime implements.
    pub version: u32,

    // ---- handle lifecycle ----
    pub free_handle: unsafe extern "C" fn(h: PsHandle),
    pub clone_handle: unsafe extern "C" fn(h: PsHandle) -> PsHandle,

    // ---- cmdlet streams (pipeline thread) ----
    pub write_object: unsafe extern "C" fn(cmdlet: PsHandle, obj: PsHandle, enumerate: u8, err: *mut PsHandle) -> PsStatus,
    pub write_error: unsafe extern "C" fn(
        cmdlet: PsHandle,
        message: PsStr16,
        error_id: PsStr16,
        category: PsErrorCategory,
        target: PsHandle,
        terminating: u8,
        err: *mut PsHandle,
    ) -> PsStatus,
    pub write_stream: unsafe extern "C" fn(cmdlet: PsHandle, kind: PsStreamKind, text: PsStr16, err: *mut PsHandle) -> PsStatus,
    pub write_progress: unsafe extern "C" fn(
        cmdlet: PsHandle,
        activity_id: i32,
        activity: PsStr16,
        status: PsStr16,
        percent: i32,
        err: *mut PsHandle,
    ) -> PsStatus,
    pub should_process: unsafe extern "C" fn(cmdlet: PsHandle, target: PsStr16, action: PsStr16, out_yes: *mut u8, err: *mut PsHandle) -> PsStatus,
    pub should_continue: unsafe extern "C" fn(cmdlet: PsHandle, query: PsStr16, caption: PsStr16, out_yes: *mut u8, err: *mut PsHandle) -> PsStatus,

    // ---- parameters (pipeline thread) ----
    pub get_parameter: unsafe extern "C" fn(cmdlet: PsHandle, name: PsStr16, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    pub parameter_is_bound: unsafe extern "C" fn(cmdlet: PsHandle, name: PsStr16, out_bound: *mut u8) -> PsStatus,

    // ---- primitives ----
    pub string_new: unsafe extern "C" fn(s: PsStr16) -> PsHandle,
    /// Copies up to `cap` UTF-16 units into `buf`; writes the full
    /// length to `out_len` so a caller can size and retry.
    pub string_read: unsafe extern "C" fn(h: PsHandle, buf: *mut u16, cap: usize, out_len: *mut usize, err: *mut PsHandle) -> PsStatus,
    pub i64_new: unsafe extern "C" fn(v: i64) -> PsHandle,
    pub i64_read: unsafe extern "C" fn(h: PsHandle, out: *mut i64, err: *mut PsHandle) -> PsStatus,
    pub f64_new: unsafe extern "C" fn(v: f64) -> PsHandle,
    pub f64_read: unsafe extern "C" fn(h: PsHandle, out: *mut f64, err: *mut PsHandle) -> PsStatus,
    pub bool_new: unsafe extern "C" fn(v: u8) -> PsHandle,
    pub bool_read: unsafe extern "C" fn(h: PsHandle, out: *mut u8, err: *mut PsHandle) -> PsStatus,

    // ---- arrays ----
    pub array_len: unsafe extern "C" fn(h: PsHandle, out: *mut usize, err: *mut PsHandle) -> PsStatus,
    pub array_get: unsafe extern "C" fn(h: PsHandle, index: usize, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    pub array_new: unsafe extern "C" fn(tag: PsTypeTag, len: usize, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    pub array_set: unsafe extern "C" fn(h: PsHandle, index: usize, value: PsHandle, err: *mut PsHandle) -> PsStatus,
    /// Pins a primitive array for zero-copy access; fails on
    /// non-primitive element types.
    pub array_pin: unsafe extern "C" fn(h: PsHandle, out: *mut PsPinned, err: *mut PsHandle) -> PsStatus,
    pub array_unpin: unsafe extern "C" fn(p: PsPinned),

    // ---- PSObject ----
    pub psobject_new: unsafe extern "C" fn(type_name: PsStr16) -> PsHandle,
    pub psobject_add_note: unsafe extern "C" fn(obj: PsHandle, name: PsStr16, value: PsHandle, err: *mut PsHandle) -> PsStatus,
    pub psobject_get_property: unsafe extern "C" fn(obj: PsHandle, name: PsStr16, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,

    // ---- session state (pipeline thread) ----
    pub get_variable: unsafe extern "C" fn(cmdlet: PsHandle, name: PsStr16, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    pub set_variable: unsafe extern "C" fn(cmdlet: PsHandle, name: PsStr16, value: PsHandle, err: *mut PsHandle) -> PsStatus,
    /// Resolves a PSPath to provider paths as an `object[]` of strings.
    pub resolve_path: unsafe extern "C" fn(cmdlet: PsHandle, path: PsStr16, literal: u8, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    pub invoke_scriptblock: unsafe extern "C" fn(cmdlet: PsHandle, block: PsHandle, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,

    // ---- dynamic .NET access (any thread) ----
    pub dyn_get: unsafe extern "C" fn(obj: PsHandle, name: PsStr16, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    pub dyn_set: unsafe extern "C" fn(obj: PsHandle, name: PsStr16, value: PsHandle, err: *mut PsHandle) -> PsStatus,
    pub dyn_call: unsafe extern "C" fn(obj: PsHandle, name: PsStr16, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    pub dyn_call_static: unsafe extern "C" fn(type_name: PsStr16, name: PsStr16, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    pub dyn_new: unsafe extern "C" fn(type_name: PsStr16, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,

    // ---- generated types ----
    /// Instantiates a copied `#[psclass]` type from a packed field
    /// block whose layout the generator emitted on both sides.
    pub factory_new: unsafe extern "C" fn(class_id: u32, fields: *const c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    /// Wraps Rust-owned memory as a `Memory<T>`; `drop_fn` runs when
    /// the managed owner is collected.
    pub memory_view_new: unsafe extern "C" fn(
        tag: PsTypeTag,
        ptr: *mut c_void,
        len: usize,
        drop_fn: unsafe extern "C" fn(*mut c_void),
        out: *mut PsHandle,
        err: *mut PsHandle,
    ) -> PsStatus,

    // ---- diagnostics ----
    /// Formats an exception handle as `Type: Message`.
    pub exception_describe: unsafe extern "C" fn(err: PsHandle, buf: *mut u16, cap: usize, out_len: *mut usize) -> PsStatus,

    // ---- direct output writes (pipeline thread) ----
    // These skip the handle round trip: writing a scalar through
    // `string_new` + `write_object` + `free_handle` costs three
    // crossings and a GCHandle; these cost one crossing and none.
    pub write_string: unsafe extern "C" fn(cmdlet: PsHandle, text: PsStr16, err: *mut PsHandle) -> PsStatus,
    pub write_i64: unsafe extern "C" fn(cmdlet: PsHandle, value: i64, err: *mut PsHandle) -> PsStatus,
    pub write_f64: unsafe extern "C" fn(cmdlet: PsHandle, value: f64, err: *mut PsHandle) -> PsStatus,
    pub write_bool: unsafe extern "C" fn(cmdlet: PsHandle, value: u8, err: *mut PsHandle) -> PsStatus,

    // ---- unsigned 64-bit primitives (any thread) ----
    pub u64_new: unsafe extern "C" fn(v: u64) -> PsHandle,
    pub u64_read: unsafe extern "C" fn(h: PsHandle, out: *mut u64, err: *mut PsHandle) -> PsStatus,

    // ---- dates, time spans, GUIDs, chars (any thread) ----
    /// A `DateTime` from ticks (100 ns units from the start of year 1)
    /// and a `DateTimeKind`: 0 unspecified, 1 UTC, 2 local. Fails for
    /// ticks outside the type's range or a kind above 2.
    pub datetime_new: unsafe extern "C" fn(ticks: i64, kind: u8, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    pub datetime_read: unsafe extern "C" fn(h: PsHandle, out_ticks: *mut i64, out_kind: *mut u8, err: *mut PsHandle) -> PsStatus,
    /// A `TimeSpan` from ticks (100 ns units; negative allowed).
    pub timespan_new: unsafe extern "C" fn(ticks: i64) -> PsHandle,
    pub timespan_read: unsafe extern "C" fn(h: PsHandle, out_ticks: *mut i64, err: *mut PsHandle) -> PsStatus,
    /// A `Guid` from and to its 16 bytes in `Guid.ToByteArray` order.
    pub guid_new: unsafe extern "C" fn(bytes: *const u8) -> PsHandle,
    pub guid_read: unsafe extern "C" fn(h: PsHandle, out_bytes: *mut u8, err: *mut PsHandle) -> PsStatus,
    /// A `char` from and to one UTF-16 code unit.
    pub char_new: unsafe extern "C" fn(unit: u16) -> PsHandle,
    pub char_read: unsafe extern "C" fn(h: PsHandle, out_unit: *mut u16, err: *mut PsHandle) -> PsStatus,

    // ---- secure strings (any thread) ----
    /// A read-only `SecureString` holding `text`; fails past 65536
    /// units, the type's limit.
    pub securestring_new: unsafe extern "C" fn(text: PsStr16, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    /// Decrypts into `buf` (up to `cap` UTF-16 units) and writes the
    /// full length to `out_len` so a caller can size and retry; the
    /// runtime zeroes its own copy before returning. With `cap` 0 it
    /// reports the length without decrypting.
    pub securestring_read: unsafe extern "C" fn(h: PsHandle, buf: *mut u16, cap: usize, out_len: *mut usize, err: *mut PsHandle) -> PsStatus,

    // ---- typed arrays (any thread) ----
    /// The element tag of an array whose element type has a tag
    /// (`PS_TYPE_U8` for a `byte[]`, and so on); `PS_TYPE_OBJECT` for
    /// any other array and for any other object.
    pub array_element_tag: unsafe extern "C" fn(h: PsHandle, out_tag: *mut PsTypeTag, err: *mut PsHandle) -> PsStatus,

    // ---- stream preferences (pipeline thread) ----
    /// Whether the engine would keep a record written to `kind`, from
    /// the stream's common parameter where it is bound and from the
    /// session's preference variable otherwise. The write entries do
    /// not check; the engine decides after the crossing.
    pub stream_enabled: unsafe extern "C" fn(cmdlet: PsHandle, kind: PsStreamKind, out_enabled: *mut u8, err: *mut PsHandle) -> PsStatus,

    // ---- read-only views ----
    /// Wraps an `IDictionary` in a table script reads through both
    /// `$t.key` and `$t['key']` and cannot write through either.
    ///
    /// The type is constructed here rather than through `dyn_new`
    /// because the assembly holding it is loaded into the module's
    /// own context, where the engine's type-name resolver does not
    /// look.
    pub readonly_table_new: unsafe extern "C" fn(source: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,

    // ---- object type tag (any thread) ----
    /// The tag of one object's own type, `PS_TYPE_OBJECT` for a type
    /// outside the tag vocabulary.
    ///
    /// `array_element_tag` answers for an array's elements; this
    /// answers for the object itself, so a caller dispatching on a
    /// type it does not know at compile time matches a `u32` after
    /// one crossing instead of reading `GetType().FullName` over
    /// three and comparing strings.
    pub object_type_tag: unsafe extern "C" fn(h: PsHandle, out_tag: *mut PsTypeTag, err: *mut PsHandle) -> PsStatus,

    // ---- scalars at their own width (any thread) ----
    /// Each builds the CLR type of its own name rather than widening
    /// to `Int64` / `Double`. The engine types an operator's answer by
    /// its operands' widths, so a value that left script as an `Int32`
    /// and returns as an `Int64` changes what the caller's next
    /// operator does with it.
    pub i8_new: unsafe extern "C" fn(v: i8) -> PsHandle,
    pub i16_new: unsafe extern "C" fn(v: i16) -> PsHandle,
    pub i32_new: unsafe extern "C" fn(v: i32) -> PsHandle,
    pub u8_new: unsafe extern "C" fn(v: u8) -> PsHandle,
    pub u16_new: unsafe extern "C" fn(v: u16) -> PsHandle,
    pub u32_new: unsafe extern "C" fn(v: u32) -> PsHandle,
    pub f32_new: unsafe extern "C" fn(v: f32) -> PsHandle,

    // ---- decimal (any thread) ----
    /// A `System.Decimal` from the four words `Decimal.GetBits`
    /// answers, in that order: lo, mid, hi, flags. Not the order the
    /// value has in memory.
    pub decimal_new: unsafe extern "C" fn(lo: i32, mid: i32, hi: i32, flags: i32, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    /// The four `Decimal.GetBits` words of a `System.Decimal`, written
    /// to `out_bits` as lo, mid, hi, flags.
    pub decimal_read: unsafe extern "C" fn(h: PsHandle, out_bits: *mut i32, err: *mut PsHandle) -> PsStatus,

    // ---- date and time with an offset (any thread) ----
    /// A `System.DateTimeOffset` from ticks and an offset in whole
    /// minutes from UTC.
    pub datetimeoffset_new: unsafe extern "C" fn(ticks: i64, offset_minutes: i16, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    /// The ticks and whole-minute UTC offset of a
    /// `System.DateTimeOffset`.
    pub datetimeoffset_read: unsafe extern "C" fn(h: PsHandle, out_ticks: *mut i64, out_offset_minutes: *mut i16, err: *mut PsHandle) -> PsStatus,

    // ---- command invocation (pipeline thread) ----
    /// Runs the command `name` names, a cmdlet, function or alias the
    /// session can see, with `parameters` an `IDictionary` bound by
    /// name or null, and `input` piped to it or null; `out` receives an
    /// `object[]` of everything it wrote. Resolved and invoked in the
    /// current runspace without a script block, so nothing is parsed.
    /// The command's non-terminating errors are written to the calling
    /// cmdlet's error stream; a terminating one is the status.
    pub invoke_command: unsafe extern "C" fn(cmdlet: PsHandle, name: PsStr16, parameters: PsHandle, input: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,

    // ---- a value of a CLR enum this module did not declare (any thread) ----
    /// A value of the enum `type_name` names, from its underlying
    /// number. The name is resolved the way the engine resolves a type
    /// literal, so anything `[System.ConsoleColor]` reaches in script
    /// is reachable here. A module's own `#[psenum]` types go through
    /// `factory_new` instead, which needs no name.
    pub enum_new: unsafe extern "C" fn(type_name: PsStr16, value: i64, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,

    // ---- a proxy's value lent to native code (the calling thread) ----
    /// Checks that `obj` is a live proxy of class `class_id` made by this
    /// module's current load, takes the object's gate exclusively, as a
    /// method taking `&mut self` does, and writes the value's pointer to
    /// `out_instance`. Refused while any call into the same object is
    /// running on this thread. Each success is paired with one
    /// `proxy_exit` on the same thread.
    pub proxy_enter: unsafe extern "C" fn(obj: PsHandle, class_id: u32, out_instance: *mut *mut c_void, err: *mut PsHandle) -> PsStatus,
    /// Ends what `proxy_enter` began and releases the gate. `changed` is
    /// nonzero when the value may have changed, so a class that reports
    /// its native bytes is asked for them again.
    pub proxy_exit: unsafe extern "C" fn(obj: PsHandle, changed: u8),

    // ---- helper executables (any thread) ----
    /// The path of a staged copy of the helper executable `name`, one
    /// the module ships in its `runtimes/<rid>/native/` folder, copied
    /// into `buf` (up to `cap` UTF-16 units) with the full length
    /// written to `out_len` so a caller can size and retry. `name` is
    /// the file name without `.exe`. The first request for a helper's
    /// bytes copies it into the folder this process stages the module's
    /// library in, and every later one answers the same path, so a
    /// running helper holds no file in the module folder. Fails for a
    /// name that is not a bare file name and when the module ships no
    /// helper of that name.
    pub helper_path: unsafe extern "C" fn(name: PsStr16, buf: *mut u16, cap: usize, out_len: *mut usize, err: *mut PsHandle) -> PsStatus,

    // ---- a proxy's value lent to native code for reading (the calling thread) ----
    /// As `proxy_enter`, entering the gate as shared, the way a property
    /// read or a method taking `&self` does: refused only while an
    /// exclusive entry (a method taking `&mut self`, a `proxy_enter`
    /// borrow) runs on this thread, and nesting inside shared ones.
    /// Paired with `proxy_exit` with `changed` zero.
    pub proxy_enter_shared: unsafe extern "C" fn(obj: PsHandle, class_id: u32, out_instance: *mut *mut c_void, err: *mut PsHandle) -> PsStatus,
}

/// Names of the exports every pwrs native module provides. The
/// runtime resolves them with `NativeLibrary.GetExport` on .NET and
/// `GetProcAddress` / `dlsym` on .NET Framework.
pub mod exports {
    /// `extern "C" fn(vtable: *const HostVTable) -> PsStatus`
    pub const MODULE_INIT: &str = "pwrs_module_init";
    /// `extern "C" fn() -> ModuleDescriptor`
    pub const MODULE_DESCRIPTOR: &str = "pwrs_module_descriptor";
    /// `extern "C" fn(cmdlet_id: u32, out: *mut *mut c_void, phases: *mut PsPhaseMask, err: *mut PsHandle) -> PsStatus`;
    /// makes the Rust instance the managed cmdlet then owns and
    /// reports the type's learned phase mask.
    pub const CMDLET_CREATE: &str = "pwrs_cmdlet_create";
    /// `extern "C" fn(instance: *mut c_void, phase: PsPhase, cmdlet: PsHandle, params: *const c_void, err: *mut PsHandle) -> PsStatus`
    pub const CMDLET_INVOKE: &str = "pwrs_cmdlet_invoke";
    /// `extern "C" fn(instance: *mut c_void)`; called from a foreign
    /// thread, sets the stop flag only.
    pub const CMDLET_STOP: &str = "pwrs_cmdlet_stop";
    /// `extern "C" fn(instance: *mut c_void)`; drops the Rust instance
    /// when the managed cmdlet is disposed.
    pub const CMDLET_RELEASE: &str = "pwrs_cmdlet_release";
    /// `extern "C" fn(class_id: u32, instance: *mut c_void)`; frees a
    /// proxy object's box when its managed wrapper is disposed or
    /// finalized.
    pub const PROXY_DROP: &str = "pwrs_proxy_drop";
    /// `extern "C" fn(class_id: u32, field_id: u32, instance: *mut c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`
    pub const PROXY_GET: &str = "pwrs_proxy_get";
    /// `extern "C" fn(class_id: u32, method_id: u32, instance: *mut c_void, args: *const c_void, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`;
    /// runs a `#[psmethods]` method of a proxy object with its packed
    /// argument block and returns the result as a handle (null for a
    /// method returning `()`).
    pub const PROXY_CALL: &str = "pwrs_proxy_call";
    /// `extern "C" fn(class_id: u32, instance: *mut c_void) -> u64`; the
    /// bytes of native memory a proxy value keeps alive, as its class's
    /// `native_bytes` function reports them, and 0 for a class without
    /// one. The runtime reports the figure to the garbage collector. It
    /// binds the export optionally, so a module built before it existed
    /// still loads.
    pub const PROXY_BYTES: &str = "pwrs_proxy_bytes";
    pub const COMPLETER_INVOKE: &str = "pwrs_completer_invoke";
    /// `extern "C" fn(transform_id: u32, value: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`;
    /// runs a `#[transform]` function over the value the binder is
    /// about to assign to a parameter, and returns what to assign
    /// instead. The runtime binds it optionally, so a module built
    /// before it existed still loads.
    pub const TRANSFORM_INVOKE: &str = "pwrs_transform_invoke";
    pub const DYNPARAMS_INVOKE: &str = "pwrs_dynparams_invoke";
    /// `extern "C" fn(provider_id: u32, op: u32, instance: *mut c_void, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`;
    /// runs one provider operation against the instance serving the
    /// current drive (null for the operations asked before a drive is
    /// known, and for the ones that make drives).
    pub const PROVIDER_INVOKE: &str = "pwrs_provider_invoke";
    /// `extern "C" fn(op: u32, err: *mut PsHandle) -> PsStatus`; runs
    /// the module's import hook for op 0 and its removal hook for op
    /// 1, and does nothing for a hook the module did not declare. The
    /// runtime binds it optionally, so a module built before it
    /// existed still loads.
    pub const MODULE_LIFECYCLE: &str = "pwrs_module_lifecycle";
    /// Data, not a function: NUL-terminated ASCII, `PWRS-CPU/1` and then
    /// a space and `name,level,leaf,subleaf,register,bit,xcr0` for each
    /// x86-64 extension the library was compiled to require. The runtime
    /// reads it before calling any other export and refuses the import
    /// when the CPU lacks one; a library without it is not checked.
    pub const CPU_REQUIREMENTS: &str = "pwrs_cpu_requirements";
    /// `extern "C" fn(leaf: u32, subleaf: u32, out: *mut u32)` on
    /// x86-64 only: CPUID, EAX to EDX written to `out[0..4]`. Naked, so
    /// it runs no instruction beyond those it names.
    pub const CPUID: &str = "pwrs_cpuid";
    /// `extern "C" fn(xcr: u32) -> u64` on x86-64 only: XGETBV. Naked;
    /// valid only when CPUID leaf 1 reports OSXSAVE.
    pub const XGETBV: &str = "pwrs_xgetbv";
}
