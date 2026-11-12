//! The C ABI shared by every pwrs native module and the managed
//! `Pwrs.Runtime` support assembly.
//!
//! Nothing here is PowerShell-typed. Every managed object is passed as
//! a [`PsHandle`] (a `GCHandle` the runtime allocated), every string as
//! a UTF-16 `(ptr, len)` pair, every primitive inline, and every
//! failure as a [`PsStatus`] plus an exception handle written through
//! an out-pointer. No panic or exception unwinds across this boundary
//! in either direction.
//!
//! [`HostVTable`] is the stable ABI. It is append-only: a newer
//! runtime gives an older module a larger table, and the module reads
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
/// A managed exception was thrown inside the runtime entry; it is
/// written to `err`.
pub const PS_ERR_MANAGED_EXCEPTION: PsStatus = 1;
/// The native side panicked. `err` receives a managed string with the
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

/// A `write_host` color that takes the host's current color, as
/// `Write-Host` does for a color it is not given.
pub const PS_HOST_COLOR_CURRENT: i32 = -1;

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

/// A borrowed UTF-16 string passed across the boundary. It never frees
/// the text it points at.
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

/// The task a `#[psmethods]` method receives in its argument block:
/// the managed task source, and a byte the host sets nonzero once the
/// caller's `CancellationToken` is canceled, readable until the task is
/// finished through `task_settle`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PsTaskSlot {
    pub source: PsHandle,
    pub cancelled: *const u8,
}

/// How `task_settle` finishes a task.
pub type PsTaskOutcome = u32;
/// Completed with a value.
pub const PS_TASK_VALUE: PsTaskOutcome = 0;
/// Faulted with an error.
pub const PS_TASK_FAULT: PsTaskOutcome = 1;
/// Canceled.
pub const PS_TASK_CANCEL: PsTaskOutcome = 2;

/// Per-module descriptor the build tool reads from the built cdylib.
/// JSON, produced by the proc macros; the C# generator consumes it.
#[repr(C)]
pub struct ModuleDescriptor {
    pub json_utf8: *const u8,
    pub json_len: usize,
}

/// Function table the managed runtime passes to the native module at
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
    /// Wraps memory Rust allocated as a `Memory<T>`; `drop_fn(ptr)` runs
    /// when its managed memory manager is disposed or collected. It is
    /// `memory_view_borrow` with `ptr` as the context, writable and
    /// never refused.
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
    // calls and a GCHandle; these cost one call and none.
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
    /// A read-only `SecureString` containing `text`; fails past 65536
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
    /// Whether a record written to `kind` would go somewhere (be shown,
    /// saved in a variable or redirected), from the stream's common
    /// parameter where it is bound and from the
    /// session's preference variable otherwise. The write entries do
    /// not check; after the call, the engine drops a record for a
    /// stream that is off.
    pub stream_enabled: unsafe extern "C" fn(cmdlet: PsHandle, kind: PsStreamKind, out_enabled: *mut u8, err: *mut PsHandle) -> PsStatus,

    // ---- read-only views ----
    /// Wraps an `IDictionary` in a table script reads through both
    /// `$t.key` and `$t['key']` and cannot write through either.
    ///
    /// The type is constructed here rather than through `dyn_new`
    /// because the assembly that defines it is loaded into the module's
    /// own context, where the engine's type-name resolver does not
    /// look.
    pub readonly_table_new: unsafe extern "C" fn(source: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,

    // ---- object type tag (any thread) ----
    /// The tag of one object's own type, `PS_TYPE_OBJECT` for a type
    /// outside the tag vocabulary.
    ///
    /// `array_element_tag` gives the tag of an array's elements; this
    /// gives the object's own, so a caller dispatching on a type not
    /// fixed at compile time matches a `u32` after one call instead of
    /// reading `GetType().FullName` over three and comparing strings.
    pub object_type_tag: unsafe extern "C" fn(h: PsHandle, out_tag: *mut PsTypeTag, err: *mut PsHandle) -> PsStatus,

    // ---- scalars at their own width (any thread) ----
    /// Each builds the CLR type of its own name rather than widening
    /// to `Int64` / `Double`. The engine types an operator's result by
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
    /// returns, in that order: lo, mid, hi, flags. Not the order the
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
    /// its native bytes has them read again.
    pub proxy_exit: unsafe extern "C" fn(obj: PsHandle, changed: u8),

    // ---- helper executables (any thread) ----
    /// The path of a staged copy of the helper executable `name`, one
    /// the module ships in its `runtimes/<rid>/native/` folder, copied
    /// into `buf` (up to `cap` UTF-16 units) with the full length
    /// written to `out_len` so a caller can size and retry. `name` is
    /// the file name without `.exe`. The first call for a helper copies
    /// it into the folder this process stages the module's library in,
    /// and every later one returns the same path, so a running helper
    /// uses no file in the module folder. Fails for a name that is not a
    /// bare file name and when the module ships no helper of that name.
    pub helper_path: unsafe extern "C" fn(name: PsStr16, buf: *mut u16, cap: usize, out_len: *mut usize, err: *mut PsHandle) -> PsStatus,

    // ---- a proxy's value lent to native code for reading (the calling thread) ----
    /// As `proxy_enter`, entering the gate as shared, the way a property
    /// read or a method taking `&self` does: refused only while an
    /// exclusive entry (a method taking `&mut self`, a `proxy_enter`
    /// borrow) runs on this thread, and nesting inside shared ones.
    /// Paired with `proxy_exit` with `changed` zero.
    pub proxy_enter_shared: unsafe extern "C" fn(obj: PsHandle, class_id: u32, out_instance: *mut *mut c_void, err: *mut PsHandle) -> PsStatus,

    // ---- errors with details ----
    /// `write_error` for an error with details: the record's exception
    /// has `message` as its Message and an inner exception whose
    /// Message is `details`, so the default error view still shows the
    /// one line and `$err.Exception.InnerException.Message` has the
    /// rest. Pipeline thread.
    pub write_error_details: unsafe extern "C" fn(
        cmdlet: PsHandle,
        message: PsStr16,
        error_id: PsStr16,
        category: PsErrorCategory,
        target: PsHandle,
        terminating: u8,
        details: PsStr16,
        err: *mut PsHandle,
    ) -> PsStatus,
    /// A handle to an exception with `message` as its Message and an
    /// inner exception whose Message is `details`. An entry point that
    /// fails returns it through its `err` out-parameter, and the
    /// host throws it as it is. Any thread.
    pub exception_new: unsafe extern "C" fn(message: PsStr16, details: PsStr16) -> PsHandle,

    // ---- progress records with every field ----
    /// `WriteProgress` of a record with every field a `ProgressRecord`
    /// has: `completed` nonzero makes it the record that completes the
    /// activity, and otherwise it is processing, whatever `percent`
    /// is. A negative `percent` or `seconds_remaining` shows none, a
    /// `percent` above 100 is 100, a negative `parent_activity_id`
    /// leaves the activity unnested, and an empty `current_operation`
    /// sets none. Pipeline thread.
    pub write_progress_record: unsafe extern "C" fn(
        cmdlet: PsHandle,
        activity_id: i32,
        parent_activity_id: i32,
        activity: PsStr16,
        status: PsStr16,
        current_operation: PsStr16,
        percent: i32,
        seconds_remaining: i32,
        completed: u8,
        err: *mut PsHandle,
    ) -> PsStatus,

    // ---- strings read in place ----
    /// Pins the string `h` converts to, as `string_read` converts it,
    /// where it lies: `out` takes the address of its first UTF-16 unit,
    /// its length in units, an element size of 2 and the pin, which is
    /// released with `array_unpin` before the phase that pinned it
    /// returns. The units are read, never written.
    pub string_pin: unsafe extern "C" fn(h: PsHandle, out: *mut PsPinned, err: *mut PsHandle) -> PsStatus,

    // ---- memory views with an owner (any thread) ----
    /// Wraps `len` elements of `tag` at `ptr` as a `Memory<T>`, or as a
    /// `ReadOnlyMemory<T>` when `read_only` is nonzero, over that memory
    /// on .NET, and copies them into a `T[]` on .NET Framework. On
    /// success the host takes `context` and calls `release(context)`
    /// exactly once: when the view's manager is disposed or collected on
    /// .NET, and before this returns on .NET Framework. On failure it
    /// has not called `release` and never will.
    ///
    /// `refused`, when not null, points at a byte the module sets
    /// nonzero to refuse the view. The host reads it here, refusing a
    /// view already refused, and on .NET before every `GetSpan` and
    /// `Pin`; a refusal is an `ObjectDisposedException`. The byte stays
    /// readable until `release` runs. A disposed view's `GetSpan` and
    /// `Pin` throw `ObjectDisposedException` too.
    pub memory_view_borrow: unsafe extern "C" fn(
        tag: PsTypeTag,
        ptr: *mut c_void,
        len: usize,
        read_only: u8,
        refused: *const u8,
        context: *mut c_void,
        release: unsafe extern "C" fn(*mut c_void),
        out: *mut PsHandle,
        err: *mut PsHandle,
    ) -> PsStatus,

    // ---- tasks (any thread) ----
    /// Finishes the task whose source a `PsTaskSlot` had: with `value`
    /// as its result for `PS_TASK_VALUE` (null for a task of no value),
    /// faulted for `PS_TASK_FAULT` with `value` the handle a failing export
    /// returns (an exception, or a string `[id] message`), and canceled
    /// for `PS_TASK_CANCEL`. The first call wins and later ones do
    /// nothing; finishing stops listening to the cancellation token and
    /// frees the slot's cancellation byte. Continuations run
    /// asynchronously, never on the calling thread. `value` stays the
    /// caller's.
    pub task_settle: unsafe extern "C" fn(source: PsHandle, outcome: PsTaskOutcome, value: PsHandle, err: *mut PsHandle) -> PsStatus,

    // ---- engine events (any thread) ----
    /// Raises the engine event `source_identifier` through `events`, a
    /// `PSEventManager`, as `New-Event` does: with `sender` as its sender
    /// (null for none), `args` as its arguments (an array is its
    /// elements, null none, anything else the one argument) and
    /// `message_data` as its MessageData (null for none). The event is
    /// queued, or passed to the actions subscribed to it, on the calling
    /// thread before this returns, so the events one thread raises arrive
    /// in the order raised; an action runs later, on the runspace's pipeline
    /// thread. The handles stay the caller's.
    pub event_raise: unsafe extern "C" fn(
        events: PsHandle,
        source_identifier: PsStr16,
        sender: PsHandle,
        args: PsHandle,
        message_data: PsHandle,
        err: *mut PsHandle,
    ) -> PsStatus,

    // ---- host output (pipeline thread) ----
    /// Writes `text` to the host as `Write-Host` does: a
    /// `HostInformationMessage` written to the information stream with
    /// the `PSHOST` tag. `foreground` and `background` are
    /// `System.ConsoleColor` values 0 to 15, or [`PS_HOST_COLOR_CURRENT`]
    /// for the color the host's `RawUI` reports, which is what
    /// `Write-Host` takes when a color is not given. A host with no
    /// `RawUI` leaves such a color unset. `no_new_line` nonzero leaves the
    /// line open. A color outside that range is a managed exception.
    pub write_host: unsafe extern "C" fn(
        cmdlet: PsHandle,
        text: PsStr16,
        foreground: i32,
        background: i32,
        no_new_line: u8,
        err: *mut PsHandle,
    ) -> PsStatus,

    // ---- files a module ships beside its native library (any thread) ----
    /// Writes the path of a copy of the file `name` the module ships in
    /// `runtimes/<rid>/native/`, as `[package.metadata.pwrs.native-files]`
    /// puts one there, into `buf` (up to `cap` UTF-16 units) with the full
    /// length written to `out_len` so a caller can size and retry. `name`
    /// is the file's own name, extension included. The copy is staged as
    /// `helper_path` stages a helper, so a library loaded from it uses no
    /// file in the module folder. Fails for a name that is not a bare file
    /// name and when the module ships no file of that name.
    pub native_file_path: unsafe extern "C" fn(name: PsStr16, buf: *mut u16, cap: usize, out_len: *mut usize, err: *mut PsHandle) -> PsStatus,

    // ---- the removal hook's removal (the thread running the hook) ----
    /// The removal the module's removal hook is running for on the
    /// calling thread. `cause` is [`REMOVAL_REMOVE_MODULE`] for
    /// `Remove-Module` and [`REMOVAL_RUNSPACE_CLOSED`] for the importing
    /// runspace closing or breaking. `runspace` receives that runspace's
    /// `InstanceId` as the 16 bytes `Guid.ToByteArray` gives, and
    /// `has_runspace` 0 for an import made on a thread with no default
    /// runspace, which leaves `runspace` as it was. Fails on a thread
    /// running no removal hook.
    pub lifecycle_removal: unsafe extern "C" fn(cause: *mut u32, runspace: *mut u8, has_runspace: *mut u8, err: *mut PsHandle) -> PsStatus,

    // ---- the events of objects (any thread) ----
    /// Raises the event `name` of a generated proxy object: its handlers
    /// run with the values in `args`, an array's elements, none for null and
    /// anything else as the one argument, each converted to its
    /// parameter's type. For a delegate `(object sender, T e)` the object
    /// is the sender and the one argument is `e`. A raise made by the
    /// thread inside a call into the object is delivered when that
    /// thread's last entry leaves; any other runs its handlers before the
    /// entry returns. A handler's exception comes back through `err`.
    pub object_event_raise: unsafe extern "C" fn(obj: PsHandle, name: PsStr16, args: PsHandle, err: *mut PsHandle) -> PsStatus,
    /// Subscribes `callback` to the event `name` of `source`, an object,
    /// or a type for its static event, and writes the subscription's
    /// handle to `out`. On success the subscription owns `context` and
    /// calls `release` with it once, after the handler is removed and no
    /// call into `callback` runs; on failure the caller still owns it.
    pub object_event_subscribe: unsafe extern "C" fn(
        source: PsHandle,
        name: PsStr16,
        callback: PsEventCallback,
        context: *mut c_void,
        release: unsafe extern "C" fn(*mut c_void),
        out: *mut PsHandle,
        err: *mut PsHandle,
    ) -> PsStatus,
    /// Removes a subscription's handler without waiting for a call into
    /// its callback running on another thread. The handle stays the
    /// caller's to free.
    pub object_event_unsubscribe: unsafe extern "C" fn(subscription: PsHandle, err: *mut PsHandle) -> PsStatus,
    /// `proxy_exit` that, when the borrow was the thread's last entry,
    /// delivers the raises made during the thread's entries; a handler's
    /// exception comes back through `err`. `proxy_exit` delivers none of
    /// them.
    pub proxy_exit_raising: unsafe extern "C" fn(obj: PsHandle, changed: u8, err: *mut PsHandle) -> PsStatus,
    /// `ShouldProcess` in its other forms, chosen by `form`: 0 is
    /// `(target)`, from `a`; 1 is `(verboseDescription, verboseWarning,
    /// caption)`, from `a`, `b` and `caption`; 2 is the same with the
    /// `ShouldProcessReason`, written to `out_reason`, 1 for `WhatIf` and
    /// 0 for `None`.
    pub should_process_form:
        unsafe extern "C" fn(cmdlet: PsHandle, form: u8, a: PsStr16, b: PsStr16, caption: PsStr16, out_yes: *mut u8, out_reason: *mut u8, err: *mut PsHandle) -> PsStatus,
    /// `ShouldContinue(query, caption, ref yesToAll, ref noToAll)`, or with
    /// `hasSecurityImpact` before the refs, false for `impact` 1 and true
    /// for 2; 0 omits it. The two refs are the runtime's for the cmdlet's
    /// invocation, so an answer to all applies across its records.
    pub should_continue_all: unsafe extern "C" fn(cmdlet: PsHandle, query: PsStr16, caption: PsStr16, impact: u8, out_yes: *mut u8, err: *mut PsHandle) -> PsStatus,
    /// `PagingParameters.First`, `Skip` and `IncludeTotalCount` of a
    /// cmdlet declaring `SupportsPaging`: `First` is `u64::MAX` and
    /// `Skip` 0 when the caller did not give them. A cmdlet without
    /// paging fails, its error naming the cmdlet.
    pub paging: unsafe extern "C" fn(cmdlet: PsHandle, out_first: *mut u64, out_skip: *mut u64, out_include_total_count: *mut u8, err: *mut PsHandle) -> PsStatus,
    /// `WriteObject(PagingParameters.NewTotalCount(total, accuracy))`: the
    /// total count the caller requests with `-IncludeTotalCount`, as the
    /// engine makes it. An
    /// `accuracy` of 1.0 is an exact count, 0.0 one not known, and a value
    /// between them an estimate.
    pub write_total_count: unsafe extern "C" fn(cmdlet: PsHandle, total: u64, accuracy: f64, err: *mut PsHandle) -> PsStatus,

    // ---- jobs ----
    /// A `RustJob` named `name` (the engine's default name when empty),
    /// whose command is the cmdlet's name, registered through the cmdlet's
    /// `JobManager` so the job cmdlets find it; it is Running, and its
    /// handle goes to `out`. On success the job owns `context`: Stop-Job,
    /// or Remove-Job -Force while it runs, calls `stop` with it once, and
    /// `job_finish` calls `release` with it once, after which `stop` is
    /// never called. On failure the caller still owns it. Pipeline thread.
    pub job_new: unsafe extern "C" fn(
        cmdlet: PsHandle,
        name: PsStr16,
        stop: unsafe extern "C" fn(*mut c_void),
        context: *mut c_void,
        release: unsafe extern "C" fn(*mut c_void),
        out: *mut PsHandle,
        err: *mut PsHandle,
    ) -> PsStatus,
    /// Adds `value` to the job's output, or each element of it when
    /// `enumerate` is nonzero and it is a collection. Any thread.
    pub job_write: unsafe extern "C" fn(job: PsHandle, value: PsHandle, enumerate: u8, err: *mut PsHandle) -> PsStatus,
    /// Adds `text` to the job's verbose, debug, warning or information
    /// stream, as `kind` says. Any thread.
    pub job_write_stream: unsafe extern "C" fn(job: PsHandle, kind: PsStreamKind, text: PsStr16, err: *mut PsHandle) -> PsStatus,
    /// Adds an error record to the job's error stream, with `details` as
    /// its exception's inner message when not empty. Any thread.
    pub job_write_error:
        unsafe extern "C" fn(job: PsHandle, message: PsStr16, id: PsStr16, category: u32, target: PsHandle, details: PsStr16, err: *mut PsHandle) -> PsStatus,
    /// Adds a progress record to the job's progress stream, its fields as
    /// `write_progress_record` reads them. Any thread.
    pub job_write_progress: unsafe extern "C" fn(
        job: PsHandle,
        activity_id: i32,
        parent_activity_id: i32,
        activity: PsStr16,
        status: PsStr16,
        current_operation: PsStr16,
        percent: i32,
        seconds_remaining: i32,
        completed: u8,
        err: *mut PsHandle,
    ) -> PsStatus,
    /// Ends the job: Stopped when its `stop` has been called, else Failed with
    /// an exception of `message`, `details` inside it when not empty, when
    /// `failed` is nonzero, else Completed. Releases the context `job_new`
    /// was given. Any thread.
    pub job_finish: unsafe extern "C" fn(job: PsHandle, failed: u8, message: PsStr16, details: PsStr16, err: *mut PsHandle) -> PsStatus,

    // ---- transactions ----
    /// The PowerShell transaction the cmdlet runs in: a handle to its
    /// `System.Transactions.Transaction` goes to `out` when
    /// `TransactionAvailable()` is true, which needs `-UseTransaction`
    /// inside `Start-Transaction`, and null otherwise, as always in
    /// PowerShell 7, which gives no cmdlet `-UseTransaction`. A cmdlet
    /// whose `[Cmdlet]` does not declare `SupportsTransactions` fails, its
    /// error naming it. Pipeline thread.
    pub transaction_current: unsafe extern "C" fn(cmdlet: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    /// Enlists a resource in `transaction`, a handle `transaction_current`
    /// or a provider call gave, as a volatile enlistment. On success the
    /// enlistment owns `context`: when the transaction is completed it
    /// calls `prepare` with it, and it calls `end` with it once, with
    /// `committed` 1 when the transaction commits and 0 when it rolls back,
    /// a `prepare` that failed included. On failure, such as for a
    /// transaction that has ended, the caller still owns it. Any thread;
    /// `prepare` and `end` run on the thread completing or rolling back
    /// the transaction, which for a timeout is a timer's.
    pub transaction_enlist: unsafe extern "C" fn(transaction: PsHandle, prepare: PsEnlistPrepare, end: PsEnlistEnd, context: *mut c_void, err: *mut PsHandle) -> PsStatus,

    // ---- steppable pipelines ----
    /// Begins the command named `name` as a steppable pipeline whose
    /// output and streams are `cmdlet`'s, as a C# cmdlet wrapping another
    /// command does: `GetSteppablePipeline(CommandOrigin, ...)` over
    /// `& $name @parameters`, the name resolved as a typed command is,
    /// then `Begin(cmdlet)`. `parameters` is a hashtable handle or null.
    /// Each of `-WhatIf`, `-Confirm`, `-Verbose`, `-Debug`,
    /// `-ErrorAction`, `-WarningAction` and `-InformationAction` the
    /// caller gave `cmdlet` is added when `parameters` does not name it
    /// and the command takes it. A handle to the `SteppablePipeline` goes
    /// to `out`, and `out_delegates` is 1 when the command takes
    /// `-WhatIf`, so a `SupportsShouldProcess` the cmdlet declares is the
    /// command's to honor, and 0 otherwise. An exception passing through
    /// ends the command as one passing through a write does. Pipeline
    /// thread.
    pub steppable_begin: unsafe extern "C" fn(cmdlet: PsHandle, name: PsStr16, parameters: PsHandle, out: *mut PsHandle, out_delegates: *mut u8, err: *mut PsHandle) -> PsStatus,
    /// `Process(input)` on a pipeline `steppable_begin` gave, or
    /// `Process()` when `has_input` is 0; what the command writes goes to
    /// `cmdlet`'s pipeline. An exception passing through ends the command
    /// as one passing through a write does. Pipeline thread.
    pub steppable_process: unsafe extern "C" fn(cmdlet: PsHandle, steppable: PsHandle, input: PsHandle, has_input: u8, err: *mut PsHandle) -> PsStatus,
    /// `End()` on a pipeline `steppable_begin` gave, the same way.
    /// Pipeline thread.
    pub steppable_end: unsafe extern "C" fn(cmdlet: PsHandle, steppable: PsHandle, err: *mut PsHandle) -> PsStatus,
    /// `Clean()` where the host has it (PowerShell 7.3 and later), then
    /// `Dispose()`, on a pipeline `steppable_begin` gave. The handle is
    /// still the caller's to free. Pipeline thread.
    pub steppable_dispose: unsafe extern "C" fn(cmdlet: PsHandle, steppable: PsHandle, err: *mut PsHandle) -> PsStatus,

    // ---- wrapped commands ----
    /// For a cmdlet that wraps a command, a new hashtable of the arguments
    /// the caller bound to the parameters it copies from that command, by
    /// parameter name: those of its properties and those declared at bind
    /// time. Pipeline thread.
    pub wrapped_arguments: unsafe extern "C" fn(cmdlet: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
    /// The record the pipeline passes to the wrapping cmdlet this phase, as it
    /// was piped, before any binding: `out_has` is 1 and the record goes to
    /// `out`, or `out_has` is 0 when the cmdlet takes no pipeline input.
    /// Pipeline thread.
    pub wrapped_input: unsafe extern "C" fn(cmdlet: PsHandle, out: *mut PsHandle, out_has: *mut u8, err: *mut PsHandle) -> PsStatus,
    /// Begins the wrapped command, with `arguments` (a hashtable handle)
    /// bound by name and the caller's preference common parameters added
    /// as `steppable_begin` adds them, as a steppable pipeline whose output
    /// goes through `hook`: each record the command writes is given to
    /// `hook` with `hook_ctx`, and what the hook writes is the cmdlet's
    /// output. `out_delegates` is as `steppable_begin`'s. `hook` is called
    /// only during this call and the `wrapped_process` and `wrapped_end`
    /// calls that pass it. Pipeline thread.
    pub wrapped_begin: unsafe extern "C" fn(cmdlet: PsHandle, arguments: PsHandle, hook: PsWrappedHook, hook_ctx: *mut c_void, out_delegates: *mut u8, err: *mut PsHandle) -> PsStatus,
    /// `Process(input)` on the wrapped command, or `Process()` when
    /// `has_input` is 0, its output going through `hook`. Pipeline thread.
    pub wrapped_process: unsafe extern "C" fn(cmdlet: PsHandle, input: PsHandle, has_input: u8, hook: PsWrappedHook, hook_ctx: *mut c_void, err: *mut PsHandle) -> PsStatus,
    /// `End()` on the wrapped command, its output going through `hook`,
    /// then `Clean()` where the host has it and `Dispose()`. Pipeline
    /// thread.
    pub wrapped_end: unsafe extern "C" fn(cmdlet: PsHandle, hook: PsWrappedHook, hook_ctx: *mut c_void, err: *mut PsHandle) -> PsStatus,

    // ---- named exceptions ----
    /// A handle to an exception of the .NET type `type_name`, resolved as
    /// PowerShell resolves a type literal, with `message` as its Message
    /// and, when `details` is not empty, an inner exception whose Message
    /// is `details`. Its `Data` holds `PwrsErrorId` (`error_id`) and
    /// `PwrsErrorCategory` (the category's name). A name that resolves to
    /// no exception type, or to one with no constructor taking a message,
    /// gives a `Pwrs.PwrsException` whose Message names the type and why
    /// it could not be made, then `message`. Any thread.
    pub exception_named: unsafe extern "C" fn(type_name: PsStr16, message: PsStr16, error_id: PsStr16, category: PsErrorCategory, details: PsStr16) -> PsHandle,
    /// `WriteError` or `ThrowTerminatingError`, as `terminating` says, of a
    /// record whose exception is `exception`, a handle `exception_named`
    /// gave, with `error_id`, `category` and `target` as `write_error`
    /// takes them. The handle stays the caller's to free. Pipeline thread.
    pub write_error_exception: unsafe extern "C" fn(
        cmdlet: PsHandle,
        exception: PsHandle,
        error_id: PsStr16,
        category: PsErrorCategory,
        target: PsHandle,
        terminating: u8,
        err: *mut PsHandle,
    ) -> PsStatus,
    /// Adds an error record whose exception is `exception`, a handle
    /// `exception_named` gave, to the job's error stream, with `error_id`,
    /// `category` and `target` as `job_write_error` takes them. The handle
    /// stays the caller's to free. Any thread.
    pub job_write_error_exception:
        unsafe extern "C" fn(job: PsHandle, exception: PsHandle, error_id: PsStr16, category: PsErrorCategory, target: PsHandle, err: *mut PsHandle) -> PsStatus,
    /// Ends the job as `job_finish` does with `failed` nonzero, the job's
    /// reason being `exception`, a handle `exception_named` gave: Stopped
    /// when its `stop` has been called, else Failed. The handle stays the
    /// caller's to free. Releases the context `job_new` was given. Any
    /// thread.
    pub job_finish_exception: unsafe extern "C" fn(job: PsHandle, exception: PsHandle, err: *mut PsHandle) -> PsStatus,

    // ---- folders a module ships beside its native library (any thread) ----
    /// Writes the path of a copy of the folder `name` the module ships
    /// under `runtimes/<rid>/`, as `[package.metadata.pwrs.libraries]`
    /// puts one there, into `buf` (up to `cap` UTF-16 units) with the full
    /// length written to `out_len` so a caller can size and retry. The copy
    /// is staged as `native_file_path` stages a file, so a library loaded
    /// from it uses no file in the module folder. Fails for a name that is
    /// not a bare folder name and when the module ships no folder of that
    /// name.
    pub library_folder: unsafe extern "C" fn(name: PsStr16, buf: *mut u16, cap: usize, out_len: *mut usize, err: *mut PsHandle) -> PsStatus,

    // ---- ordered tables (any thread) ----
    /// Makes an empty `Hashtable` that keeps its keys in the order they
    /// were added: `System.Management.Automation.OrderedHashtable` on
    /// PowerShell 7, and the runtime's own `Pwrs.OrderedHashtable` on
    /// Windows PowerShell 5.1, which has none. Keys compare as a
    /// `Hashtable`'s do, by case.
    pub ordered_table_new: unsafe extern "C" fn(out: *mut PsHandle, err: *mut PsHandle) -> PsStatus,
}

/// What `object_event_subscribe` calls on the raising thread for each
/// raise: the subscription's `context`, the sender (null for a delegate
/// that takes none, whose arguments are then all in `args`) and an
/// `object[]` of the other arguments. The handles are the caller's and
/// live for the call. A status other than [`PS_OK`] throws the error
/// written to `err` to the raiser.
pub type PsEventCallback = unsafe extern "C" fn(context: *mut c_void, sender: PsHandle, args: PsHandle, err: *mut PsHandle) -> PsStatus;

/// The prepare step of a transaction, called to check whether an enlisted
/// resource can commit: [`PS_OK`] lets it, and any other status rolls the
/// transaction back with the error written to `err`.
pub type PsEnlistPrepare = unsafe extern "C" fn(context: *mut c_void, err: *mut PsHandle) -> PsStatus;

/// The end of an enlistment: `committed` is 1 when its transaction
/// committed and 0 when it rolled back. The context is the callee's to
/// free.
pub type PsEnlistEnd = unsafe extern "C" fn(context: *mut c_void, committed: u8);

/// What a wrapped command's output goes through: called on the pipeline
/// thread with the context the wrapping call passed and one record the
/// command wrote, whose handle is the caller's and lives for the call. A
/// status other than [`PS_OK`] throws the error written to `err` into the wrapped
/// command's pipeline, which the wrapping call then returns.
pub type PsWrappedHook = unsafe extern "C" fn(context: *mut c_void, record: PsHandle, err: *mut PsHandle) -> PsStatus;

/// The cause `lifecycle_removal` gives a removal by `Remove-Module`.
pub const REMOVAL_REMOVE_MODULE: u32 = 0;
/// The cause `lifecycle_removal` gives a removal because the importing
/// runspace closed or broke.
pub const REMOVAL_RUNSPACE_CLOSED: u32 = 1;

/// Names of the exports every pwrs native module provides. The
/// runtime resolves them with `NativeLibrary.GetExport` on .NET and
/// `GetProcAddress` / `dlsym` on .NET Framework.
pub mod exports {
    /// `extern "C" fn(vtable: *const HostVTable) -> PsStatus`
    pub const MODULE_INIT: &str = "pwrs_module_init";
    /// `extern "C" fn() -> ModuleDescriptor`
    pub const MODULE_DESCRIPTOR: &str = "pwrs_module_descriptor";
    /// `extern "C" fn(cmdlet_id: u32, out: *mut *mut c_void, phases: *mut PsPhaseMask, err: *mut PsHandle) -> PsStatus`;
    /// makes the Rust instance the managed cmdlet then uses, and writes
    /// the type's phase mask: the phases not yet found empty.
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
    /// bytes of native memory a proxy value owns, as its class's
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
    /// `extern "C" fn(cmdlet_id: u32, parameter: u32, value: PsHandle, err: *mut PsHandle) -> PsStatus`;
    /// runs the `validate_with` function of parameter `parameter` of
    /// cmdlet `cmdlet_id` over the value the binder is about to assign,
    /// already converted to the parameter's type. An error refuses the
    /// argument. The runtime binds it optionally, so a module built
    /// before it existed still loads.
    pub const VALIDATE_INVOKE: &str = "pwrs_validate_invoke";
    pub const DYNPARAMS_INVOKE: &str = "pwrs_dynparams_invoke";
    /// `extern "C" fn(provider_id: u32, op: u32, instance: *mut c_void, args: PsHandle, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus`;
    /// runs one provider operation against the current drive's instance
    /// (null for the operations called before there is a drive, and for
    /// the ones that make drives).
    pub const PROVIDER_INVOKE: &str = "pwrs_provider_invoke";
    /// `extern "C" fn(op: u32, err: *mut PsHandle) -> PsStatus`; runs
    /// the module's import hook for op 0 and its removal hook for op
    /// 1, and does nothing for a hook the module did not declare. The
    /// runtime binds it optionally, so a module built before it
    /// existed still loads.
    pub const MODULE_LIFECYCLE: &str = "pwrs_module_lifecycle";
    /// Data, not a function: NUL-terminated ASCII naming what the library
    /// was compiled to require. On x86-64, `PWRS-CPU/1` and then a space
    /// and `name,level,leaf,subleaf,register,bit,xcr0` for each extension.
    /// On AArch64, `PWRS-ARM/1`, a space and the operating system the
    /// target runs, then a space and `name,tier,report` for each feature,
    /// where `report` is how that system indicates whether the CPU has it,
    /// as `pwrs::cpu::REQUIREMENTS` describes. The runtime reads it before
    /// calling any other export and refuses the import when the machine
    /// lacks one, or its operating system cannot report one; a library
    /// without it is not checked.
    pub const CPU_REQUIREMENTS: &str = "pwrs_cpu_requirements";
    /// `extern "C" fn(leaf: u32, subleaf: u32, out: *mut u32)` on
    /// x86-64 only: CPUID, EAX to EDX written to `out[0..4]`. Naked, so
    /// it runs no instruction beyond those it names.
    pub const CPUID: &str = "pwrs_cpuid";
    /// `extern "C" fn(xcr: u32) -> u64` on x86-64 only: XGETBV. Naked;
    /// valid only when CPUID leaf 1 reports OSXSAVE.
    pub const XGETBV: &str = "pwrs_xgetbv";
    /// `extern "C" fn(question: u32, name: *const u8, name_len: usize,
    /// out: *mut u8, out_len: usize) -> i32`: returns, for one of
    /// [`cpu_query`](crate::cpu_query)'s numbers, `pwrs::cpu`'s own value,
    /// for the runtime's `Pwrs.Cpu`. Unlike the three above it runs
    /// compiled Rust, which may use any extension the library was built
    /// for, so the runtime calls it only once the import check has passed.
    /// The runtime binds it optionally, so a module built before it
    /// existed still loads.
    pub const CPU_QUERY: &str = "pwrs_cpu_query";
}

/// The numbers the `pwrs_cpu_query` export takes as `question`, and the
/// statuses it returns. For a name, it writes the name, as `PWRS_CPU_MAX`
/// spells it, to `out` and returns its length in bytes; for an extension,
/// it takes the extension's `target_feature` name, such as `avx2` or
/// `sve`, and returns 1 or 0.
pub mod cpu_query {
    /// The x86-64 psABI level `pwrs::cpu::level` reaches.
    pub const LEVEL: u32 = 0;
    /// The AArch64 feature tier `pwrs::cpu::arm::tier` reaches.
    pub const ARM_TIER: u32 = 1;
    /// The cap `PWRS_CPU_MAX` sets on the library's architecture.
    pub const CAP: u32 = 2;
    /// Whether `pwrs::cpu::has`, or `arm::has` on AArch64, allows the
    /// extension: detected, and at or below the cap.
    pub const HAS: u32 = 3;
    /// Whether `pwrs::cpu::detected`, or `arm::detected` on AArch64,
    /// reports the extension, whatever the cap.
    pub const DETECTED: u32 = 4;
    /// There is no value for that number on the library's architecture,
    /// or the extension's name is not one the architecture has.
    pub const NONE: i32 = -1;
    /// The number is not one the library knows.
    pub const UNKNOWN_QUESTION: i32 = -2;
    /// The name is longer than `out`.
    pub const TOO_LONG: i32 = -3;
}
