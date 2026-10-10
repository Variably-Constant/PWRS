//! A fake host vtable for unit tests: objects live in this process and
//! every entry behaves like the managed runtime for the primitive,
//! string, array, and PSObject cases. Cmdlet-stream entries record what
//! was written. Nothing here touches .NET.
//!
//! Each [`install`] gives the calling test a context of its own: what
//! the entries record, the handles it holds live, and the streams it
//! reports as kept. A handle is the address of its object, which names
//! the context of the thread that made it, so a handle made on a test's
//! thread is read and released on any thread, and a settle or raise a
//! worker makes through one is recorded for that test. Tests run side
//! by side as the harness runs them, sharing nothing but the vtable.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use pwrs_sys::*;
use std::sync::Once;

/// What a fake object holds.
#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    /// `PSTypeName` and note properties.
    PsObject(String, Vec<(String, Value)>),
    Exception(String),
    /// Ticks and kind.
    DateTime(i64, u8),
    TimeSpan(i64),
    /// `ToByteArray` order.
    Guid([u8; 16]),
    Char(u16),
    /// UTF-16 units; never rendered as text.
    Secure(Vec<u16>),
    /// The four `Decimal.GetBits` words: lo, mid, hi, flags.
    Decimal([i32; 4]),
    /// Ticks and the UTC offset in whole minutes.
    DateTimeOffset(i64, i16),
    /// A value of a CLR enum named by the caller: the type name and
    /// the underlying number. The fake resolves no types, so the name
    /// is kept as given and reads back as a number like the real one.
    Enum(String, i64),
}

/// One engine event the fake host was asked to raise.
#[derive(Clone, Debug)]
pub struct Raised {
    /// The value standing in for the event manager.
    pub events: Value,
    pub source_identifier: String,
    pub sender: Value,
    pub args: Value,
    pub message_data: Value,
}

struct Node<T> {
    item: T,
    next: *mut Node<T>,
}

/// Records any thread pushes by compare-exchange at the head, newest
/// first. A `take` swaps the whole list out; a `read` walks it and is
/// used only on lists nothing takes, so no node it reaches is freed.
struct Log<T> {
    head: AtomicPtr<Node<T>>,
}

impl<T> Log<T> {
    const fn new() -> Self {
        Log { head: AtomicPtr::new(core::ptr::null_mut()) }
    }

    fn push(&self, item: T) {
        let node = Box::into_raw(Box::new(Node { item, next: core::ptr::null_mut() }));
        let mut head = self.head.load(Ordering::Acquire);
        loop {
            // SAFETY: `node` came from Box::into_raw above and is not
            // published until the compare-exchange succeeds.
            unsafe { (*node).next = head };
            match self.head.compare_exchange_weak(head, node, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_published) => return,
                Err(newer) => head = newer,
            }
        }
    }

    /// Everything pushed, oldest first, leaving the list empty.
    fn take(&self) -> Vec<T> {
        let mut node = self.head.swap(core::ptr::null_mut(), Ordering::AcqRel);
        let mut items = Vec::new();
        while !node.is_null() {
            // SAFETY: every node came from Box::into_raw in `push`, and the
            // swap above made this call the only holder of the chain.
            let boxed = unsafe { Box::from_raw(node) };
            node = boxed.next;
            items.push(boxed.item);
        }
        items.reverse();
        items
    }
}

impl<T: Clone> Log<T> {
    /// Everything pushed, oldest first, kept.
    fn read(&self) -> Vec<T> {
        let mut node = self.head.load(Ordering::Acquire);
        let mut items = Vec::new();
        while !node.is_null() {
            // SAFETY: the node was written in full before the release half
            // of the compare-exchange that published it, and a list that is
            // read is never taken, so the node is never freed.
            let n = unsafe { &*node };
            items.push(n.item.clone());
            node = n.next;
        }
        items.reverse();
        items
    }
}

/// `(message, id, category, terminating, details)`, `details` being
/// `None` for a record written without any.
type ErrorRow = (String, String, u32, bool, Option<String>);

/// One test's share of the fake host.
struct Context {
    /// Everything written through the write entries, in order.
    output: Log<Value>,
    streams: Log<(u32, String)>,
    /// Every host write, as `write_host` was handed it.
    host: Log<HostWrite>,
    errors: Log<ErrorRow>,
    /// Every progress record, as the managed runtime builds it from
    /// either progress entry.
    progress: Log<crate::Progress>,
    /// Whether each memory view `memory_view_borrow` made was asked for
    /// read-only, in order; read, never taken.
    views_read_only: Log<bool>,
    /// Every task settled: the source's value, the outcome, and the
    /// value or error it was settled with; read, never taken.
    settled: Log<(Value, u32, Value)>,
    /// Every engine event raised, in the order raised; read, never taken.
    raised: Log<Raised>,
    /// Handles made and not yet freed.
    live: AtomicUsize,
    /// String pins handed out and not yet released.
    string_pins_live: AtomicUsize,
    /// Every `string_pin` that succeeded, released or not.
    string_pins_made: AtomicUsize,
    /// Which streams the fake host reports as kept, a bit per kind.
    streams_on: AtomicU32,
    /// Objects written through the direct scalar entries.
    direct_writes: AtomicU64,
    /// Objects written through `write_object`.
    handle_writes: AtomicU64,
}

impl Context {
    const fn new() -> Self {
        Context {
            output: Log::new(),
            streams: Log::new(),
            host: Log::new(),
            errors: Log::new(),
            progress: Log::new(),
            views_read_only: Log::new(),
            settled: Log::new(),
            raised: Log::new(),
            live: AtomicUsize::new(0),
            string_pins_live: AtomicUsize::new(0),
            string_pins_made: AtomicUsize::new(0),
            streams_on: AtomicU32::new(u32::MAX),
            direct_writes: AtomicU64::new(0),
            handle_writes: AtomicU64::new(0),
        }
    }
}

/// The context of a thread no [`install`] ran on, such as a worker a
/// test spawns.
static UNOWNED: Context = Context::new();

std::thread_local! {
    /// Initialized by a constant and with nothing to drop, so reading it
    /// cannot fail, even while the thread exits.
    static CURRENT: Cell<Option<&'static Context>> = const { Cell::new(None) };
}

/// The calling thread's context.
fn here() -> &'static Context {
    match CURRENT.with(Cell::get) {
        Some(context) => context,
        None => &UNOWNED,
    }
}

/// `borrows` while one thread changes an object's value.
const WRITING: usize = usize::MAX;

/// One fake object. A handle is the address of its `Obj`, from
/// `Box::into_raw`, until `free_handle` drops it.
struct Obj {
    owner: &'static Context,
    /// The tag recorded when the value was built: a width the `Value`
    /// does not tell apart, or an array's element tag. Arrays made any
    /// other way have none and cannot be pinned.
    tag: Option<PsTypeTag>,
    /// The threads reading `value`, or [`WRITING`]. A thread that finds
    /// the other kind of access under way is answered with an error,
    /// never made to wait.
    borrows: AtomicUsize,
    value: UnsafeCell<Value>,
}

// SAFETY: `value` is read only under a shared borrow and written only
// under the exclusive one, both taken through `borrows`.
unsafe impl Sync for Obj {}

/// The error an entry answers when another thread is using the object
/// in a way the call cannot share.
const IN_USE: &str = "the fake host's object is in use on another thread";

/// The object behind a non-null handle.
fn obj<'a>(h: PsHandle) -> &'a Obj {
    // SAFETY: a non-null handle the fake host answered is the address of
    // a live Obj until free_handle, and its holder frees it only once it
    // has stopped using it.
    unsafe { &*(h.0 as *const Obj) }
}

/// Runs `f` on the object behind a non-null `h` under a shared borrow,
/// or answers `None` while another thread changes it.
fn read<R>(h: PsHandle, f: impl FnOnce(&Obj, &Value) -> R) -> Option<R> {
    let o = obj(h);
    let mut seen = o.borrows.load(Ordering::Acquire);
    loop {
        if seen == WRITING {
            return None;
        }
        match o.borrows.compare_exchange_weak(seen, seen + 1, Ordering::Acquire, Ordering::Acquire) {
            Ok(_counted) => break,
            Err(now) => seen = now,
        }
    }
    // SAFETY: the shared borrow taken above keeps every writer out.
    let out = f(o, unsafe { &*o.value.get() });
    o.borrows.fetch_sub(1, Ordering::Release);
    Some(out)
}

/// Runs `f` on the value behind a non-null `h` under the exclusive
/// borrow, or answers `None` while another thread reads or changes it.
fn write<R>(h: PsHandle, f: impl FnOnce(&mut Value) -> R) -> Option<R> {
    let o = obj(h);
    if o.borrows.compare_exchange(0, WRITING, Ordering::Acquire, Ordering::Acquire).is_err() {
        return None;
    }
    // SAFETY: the exclusive borrow taken above keeps every other access out.
    let out = f(unsafe { &mut *o.value.get() });
    o.borrows.store(0, Ordering::Release);
    Some(out)
}

/// What a pin handle points at, from `Box::into_raw` until
/// `array_unpin` drops it.
enum Pinned {
    /// An array's elements encoded at its tag, written back at unpin.
    Array { array: PsHandle, tag: PsTypeTag, bytes: Box<[u8]> },
    /// The units of a string's text, dropped at unpin.
    Text { owner: &'static Context, units: Box<[u16]> },
}

/// Bytes per element of a primitive tag; `None` for the rest.
fn elem_size(tag: PsTypeTag) -> Option<usize> {
    match tag {
        PS_TYPE_I8 | PS_TYPE_U8 => Some(1),
        PS_TYPE_I16 | PS_TYPE_U16 => Some(2),
        PS_TYPE_I32 | PS_TYPE_U32 | PS_TYPE_F32 => Some(4),
        PS_TYPE_I64 | PS_TYPE_U64 | PS_TYPE_F64 => Some(8),
        _other => None,
    }
}

/// One value as the bytes of a primitive tag, little-endian.
fn encode(v: &Value, tag: PsTypeTag, out: &mut Vec<u8>) -> Result<(), String> {
    let int = match v {
        Value::Int(i) => *i,
        Value::Null => 0,
        Value::Float(f) if tag == PS_TYPE_F32 => {
            out.extend_from_slice(&(*f as f32).to_le_bytes());
            return Ok(());
        }
        Value::Float(f) if tag == PS_TYPE_F64 => {
            out.extend_from_slice(&f.to_le_bytes());
            return Ok(());
        }
        other => return Err(format!("cannot store {other:?} in a primitive array")),
    };
    match tag {
        PS_TYPE_I8 | PS_TYPE_U8 => out.push(int as u8),
        PS_TYPE_I16 | PS_TYPE_U16 => out.extend_from_slice(&(int as u16).to_le_bytes()),
        PS_TYPE_I32 | PS_TYPE_U32 => out.extend_from_slice(&(int as u32).to_le_bytes()),
        PS_TYPE_F32 => out.extend_from_slice(&(int as f32).to_le_bytes()),
        PS_TYPE_F64 => out.extend_from_slice(&(int as f64).to_le_bytes()),
        _wide => out.extend_from_slice(&(int as u64).to_le_bytes()),
    }
    Ok(())
}

/// One element decoded from the bytes of a primitive tag.
fn decode(bytes: &[u8], tag: PsTypeTag) -> Value {
    let mut word = [0u8; 8];
    word[..bytes.len()].copy_from_slice(bytes);
    match tag {
        PS_TYPE_I8 => Value::Int(bytes[0] as i8 as i64),
        PS_TYPE_U8 => Value::Int(bytes[0] as i64),
        PS_TYPE_I16 => Value::Int(i16::from_le_bytes([bytes[0], bytes[1]]) as i64),
        PS_TYPE_U16 => Value::Int(u16::from_le_bytes([bytes[0], bytes[1]]) as i64),
        PS_TYPE_I32 => Value::Int(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64),
        PS_TYPE_U32 => Value::Int(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64),
        PS_TYPE_F32 => Value::Float(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64),
        PS_TYPE_F64 => Value::Float(f64::from_le_bytes(word)),
        _wide => Value::Int(i64::from_le_bytes(word)),
    }
}

fn alloc(v: Value) -> PsHandle {
    alloc_with(v, None)
}

/// A handle to a new object holding `v`, owned by the calling thread's
/// context; the null handle for `Value::Null`.
fn alloc_with(v: Value, tag: Option<PsTypeTag>) -> PsHandle {
    if let Value::Null = v {
        return PsHandle::NULL;
    }
    let owner = here();
    owner.live.fetch_add(1, Ordering::Relaxed);
    let o = Box::new(Obj { owner, tag, borrows: AtomicUsize::new(0), value: UnsafeCell::new(v) });
    PsHandle(Box::into_raw(o) as *mut c_void)
}

/// The value behind a handle, cloned.
pub fn value(h: PsHandle) -> Value {
    if h.is_null() {
        return Value::Null;
    }
    match read(h, |_, v| v.clone()) {
        Some(v) => v,
        None => Value::Exception(IN_USE.to_string()),
    }
}

/// The context that owns a non-null handle's object.
fn owner(h: PsHandle) -> &'static Context {
    obj(h).owner
}

fn fail(err: *mut PsHandle, msg: &str) -> PsStatus {
    if !err.is_null() {
        unsafe { *err = alloc(Value::Exception(msg.to_string())) };
    }
    PS_ERR_MANAGED_EXCEPTION
}

unsafe fn read16(s: PsStr16) -> String {
    if s.ptr.is_null() || s.len == 0 {
        return String::new();
    }
    String::from_utf16_lossy(unsafe { core::slice::from_raw_parts(s.ptr, s.len) })
}

/// Output this test's context recorded, then cleared.
pub fn take_output() -> Vec<Value> {
    here().output.take()
}

pub fn take_streams() -> Vec<(u32, String)> {
    here().streams.take()
}

/// One `write_host` call as the fake host recorded it: a color is
/// `None` where the module asked for the host's current one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostWrite {
    pub text: String,
    pub foreground: Option<crate::ConsoleColor>,
    pub background: Option<crate::ConsoleColor>,
    pub no_new_line: bool,
}

/// Host writes this test's context recorded, then cleared.
pub fn take_host() -> Vec<HostWrite> {
    here().host.take()
}

/// `(message, id, category, terminating)` records written so far.
pub fn take_errors() -> Vec<(String, String, u32, bool)> {
    here().errors.take().into_iter().map(|(m, id, c, term, _details)| (m, id, c, term)).collect()
}

/// `(message, id, category, terminating, details)` records written so
/// far, `details` being `None` for a record written without any.
pub fn take_errors_with_details() -> Vec<(String, String, u32, bool, Option<String>)> {
    here().errors.take()
}

/// Progress records written so far, then cleared.
pub fn take_progress() -> Vec<crate::Progress> {
    here().progress.take()
}

/// Handles this test's context made that are not yet freed.
pub fn live_handles() -> usize {
    here().live.load(Ordering::Relaxed)
}

/// Writes through the direct scalar entries and through `write_object`
/// this test's context recorded, in that order.
#[cfg(test)]
pub(crate) fn entry_writes() -> (u64, u64) {
    let c = here();
    (c.direct_writes.load(Ordering::Relaxed), c.handle_writes.load(Ordering::Relaxed))
}

unsafe extern "C" fn free_handle(h: PsHandle) {
    if !h.is_null() {
        // SAFETY: a non-null handle is the address of a live Obj from
        // Box::into_raw, and the host frees each handle once.
        let o = unsafe { Box::from_raw(h.0 as *mut Obj) };
        o.owner.live.fetch_sub(1, Ordering::Relaxed);
    }
}
unsafe extern "C" fn clone_handle(h: PsHandle) -> PsHandle {
    alloc(value(h))
}
unsafe extern "C" fn write_object(_c: PsHandle, obj: PsHandle, enumerate: u8, _err: *mut PsHandle) -> PsStatus {
    let c = here();
    c.handle_writes.fetch_add(1, Ordering::Relaxed);
    match (enumerate, value(obj)) {
        (1, Value::Array(items)) => {
            for item in items {
                c.output.push(item);
            }
        }
        (_, other) => c.output.push(other),
    }
    PS_OK
}
unsafe extern "C" fn write_error(_c: PsHandle, message: PsStr16, id: PsStr16, category: u32, _target: PsHandle, terminating: u8, _err: *mut PsHandle) -> PsStatus {
    here().errors.push(unsafe { (read16(message), read16(id), category, terminating != 0, None) });
    PS_OK
}
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn write_error_details(
    _c: PsHandle,
    message: PsStr16,
    id: PsStr16,
    category: u32,
    _target: PsHandle,
    terminating: u8,
    details: PsStr16,
    _err: *mut PsHandle,
) -> PsStatus {
    here().errors.push(unsafe { (read16(message), read16(id), category, terminating != 0, Some(read16(details))) });
    PS_OK
}
/// The fake models no InnerException: the exception's text is the
/// message, then the details on the next line.
unsafe extern "C" fn exception_new(message: PsStr16, details: PsStr16) -> PsHandle {
    alloc(Value::Exception(unsafe { format!("{}\n{}", read16(message), read16(details)) }))
}
unsafe extern "C" fn write_stream(_c: PsHandle, kind: u32, text: PsStr16, _err: *mut PsHandle) -> PsStatus {
    here().streams.push((kind, unsafe { read16(text) }));
    PS_OK
}
/// Refuses a color outside 0 to 15 and -1, as the managed entry does.
unsafe extern "C" fn write_host(_c: PsHandle, text: PsStr16, foreground: i32, background: i32, no_new_line: u8, err: *mut PsHandle) -> PsStatus {
    let color = |value: i32| -> Result<Option<crate::ConsoleColor>, ()> {
        if value == pwrs_sys::PS_HOST_COLOR_CURRENT {
            return Ok(None);
        }
        crate::ConsoleColor::from_value(i64::from(value)).map(Some).ok_or(())
    };
    let (Ok(foreground), Ok(background)) = (color(foreground), color(background)) else {
        unsafe { *err = alloc(Value::Exception(format!("a console color is 0 to 15, or -1 for the host's current color; got {foreground} and {background}"))) };
        return pwrs_sys::PS_ERR_MANAGED_EXCEPTION;
    };
    here().host.push(HostWrite { text: unsafe { read16(text) }, foreground, background, no_new_line: no_new_line != 0 });
    PS_OK
}
/// A negative percent completes the activity, as the managed runtime
/// reads it.
unsafe extern "C" fn write_progress(_c: PsHandle, id: i32, activity: PsStr16, status: PsStr16, percent: i32, _err: *mut PsHandle) -> PsStatus {
    let mut record = unsafe { crate::Progress::new(id, read16(activity), read16(status)) };
    if percent < 0 {
        record = record.completed();
    } else {
        record = record.with_percent(percent.min(100));
    }
    here().progress.push(record);
    PS_OK
}
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn write_progress_record(
    _c: PsHandle,
    id: i32,
    parent: i32,
    activity: PsStr16,
    status: PsStr16,
    operation: PsStr16,
    percent: i32,
    seconds: i32,
    completed: u8,
    _err: *mut PsHandle,
) -> PsStatus {
    let record = unsafe {
        crate::Progress {
            activity_id: id,
            parent_activity_id: parent,
            activity: read16(activity),
            status: read16(status),
            current_operation: read16(operation),
            percent_complete: percent.min(100),
            seconds_remaining: seconds,
            completed: completed != 0,
        }
    };
    here().progress.push(record);
    PS_OK
}
unsafe extern "C" fn should(_c: PsHandle, _a: PsStr16, _b: PsStr16, yes: *mut u8, _err: *mut PsHandle) -> PsStatus {
    unsafe { *yes = 1 };
    PS_OK
}
unsafe extern "C" fn get_parameter(_c: PsHandle, _n: PsStr16, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    unsafe { *out = PsHandle::NULL };
    PS_OK
}
unsafe extern "C" fn parameter_is_bound(_c: PsHandle, _n: PsStr16, bound: *mut u8) -> PsStatus {
    unsafe { *bound = 0 };
    PS_OK
}
unsafe extern "C" fn string_new(s: PsStr16) -> PsHandle {
    alloc(Value::Str(unsafe { read16(s) }))
}
fn to_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => if *b { "True".into() } else { "False".into() },
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => s.clone(),
        Value::Array(items) => items.iter().map(to_text).collect::<Vec<_>>().join(" "),
        Value::PsObject(name, _) => name.clone(),
        Value::Exception(m) => m.clone(),
        Value::DateTime(t, _) => t.to_string(),
        Value::TimeSpan(t) => t.to_string(),
        Value::Guid(b) => b.iter().map(|x| format!("{x:02x}")).collect(),
        Value::Char(c) => String::from_utf16_lossy(core::slice::from_ref(c)),
        Value::Secure(_) => "System.Security.SecureString".into(),
        // Enough to tell two values apart in a test failure; the fake
        // does not do decimal arithmetic, so it renders the words.
        Value::Decimal(bits) => bits.iter().map(|w| w.to_string()).collect::<Vec<_>>().join(","),
        Value::DateTimeOffset(t, o) => format!("{t}{o:+}"),
        // The real host renders an enum as its member name, which
        // needs the type; the fake has only the number.
        Value::Enum(name, value) => format!("{name}:{value}"),
    }
}
unsafe extern "C" fn string_read(h: PsHandle, buf: *mut u16, cap: usize, out_len: *mut usize, _err: *mut PsHandle) -> PsStatus {
    let s: Vec<u16> = to_text(&value(h)).encode_utf16().collect();
    unsafe { *out_len = s.len() };
    let n = s.len().min(cap);
    if !buf.is_null() && n > 0 {
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), buf, n) };
    }
    PS_OK
}
unsafe extern "C" fn i64_new(v: i64) -> PsHandle {
    alloc(Value::Int(v))
}
unsafe extern "C" fn i64_read(h: PsHandle, out: *mut i64, err: *mut PsHandle) -> PsStatus {
    let read = match value(h) {
        Value::Int(i) => i,
        Value::Enum(_name, i) => i,
        Value::Float(f) => f as i64,
        Value::Bool(b) => b as i64,
        Value::Str(s) => match s.trim().parse::<i64>() {
            Ok(i) => i,
            Err(e) => return fail(err, &format!("cannot convert {s:?} to Int64: {e}")),
        },
        other => return fail(err, &format!("cannot convert {other:?} to Int64")),
    };
    unsafe { *out = read };
    PS_OK
}
unsafe extern "C" fn f64_new(v: f64) -> PsHandle {
    alloc(Value::Float(v))
}
unsafe extern "C" fn f64_read(h: PsHandle, out: *mut f64, err: *mut PsHandle) -> PsStatus {
    let read = match value(h) {
        Value::Float(f) => f,
        Value::Int(i) => i as f64,
        other => return fail(err, &format!("cannot convert {other:?} to Double")),
    };
    unsafe { *out = read };
    PS_OK
}
unsafe extern "C" fn bool_new(v: u8) -> PsHandle {
    alloc(Value::Bool(v != 0))
}
unsafe extern "C" fn bool_read(h: PsHandle, out: *mut u8, err: *mut PsHandle) -> PsStatus {
    let read = match value(h) {
        Value::Bool(b) => b as u8,
        Value::Int(i) => (i != 0) as u8,
        Value::Null => 0,
        other => return fail(err, &format!("cannot convert {other:?} to Boolean")),
    };
    unsafe { *out = read };
    PS_OK
}
unsafe extern "C" fn array_len(h: PsHandle, out: *mut usize, err: *mut PsHandle) -> PsStatus {
    let read = match value(h) {
        Value::Array(items) => items.len(),
        Value::Null => 0,
        other => return fail(err, &format!("{other:?} is not a collection")),
    };
    unsafe { *out = read };
    PS_OK
}
unsafe extern "C" fn array_get(h: PsHandle, i: usize, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Array(items) => match items.get(i) {
            Some(v) => {
                unsafe { *out = alloc(v.clone()) };
                PS_OK
            }
            None => fail(err, "index out of range"),
        },
        other => fail(err, &format!("{other:?} is not a collection")),
    }
}
unsafe extern "C" fn array_new(tag: u32, len: usize, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    unsafe { *out = alloc_with(Value::Array(vec![Value::Null; len]), Some(tag)) };
    PS_OK
}
unsafe extern "C" fn array_set(h: PsHandle, i: usize, v: PsHandle, err: *mut PsHandle) -> PsStatus {
    if h.is_null() {
        return fail(err, "array_set on a null handle");
    }
    let item = value(v);
    let stored = write(h, |held| match held {
        Value::Array(items) if i < items.len() => {
            items[i] = item;
            true
        }
        _not_an_array_or_out_of_range => false,
    });
    match stored {
        Some(true) => PS_OK,
        Some(false) => fail(err, "array_set on a non-array or out of range"),
        None => fail(err, IN_USE),
    }
}
/// Pins an array made by `array_new` with a primitive tag: its
/// elements are encoded into a buffer whose address is handed out,
/// and `array_unpin` writes the buffer back.
unsafe extern "C" fn array_pin(h: PsHandle, out: *mut PsPinned, err: *mut PsHandle) -> PsStatus {
    if h.is_null() {
        return fail(err, "the fake host pins only arrays made by array_new");
    }
    let held = read(h, |o, v| match (o.tag, v) {
        (Some(tag), Value::Array(items)) => Ok((tag, items.clone())),
        (None, _) => Err("the fake host pins only arrays made by array_new".to_string()),
        (Some(_), other) => Err(format!("{other:?} is not an array")),
    });
    let (tag, items) = match held {
        Some(Ok(found)) => found,
        Some(Err(msg)) => return fail(err, &msg),
        None => return fail(err, IN_USE),
    };
    let size = match elem_size(tag) {
        Some(s) => s,
        None => return fail(err, &format!("cannot pin an array with element tag {tag}")),
    };
    let mut bytes = Vec::with_capacity(items.len() * size);
    for item in &items {
        if let Err(msg) = encode(item, tag, &mut bytes) {
            return fail(err, &msg);
        }
    }
    let mut bytes = bytes.into_boxed_slice();
    let data = bytes.as_mut_ptr() as *mut c_void;
    let pin = Box::into_raw(Box::new(Pinned::Array { array: h, tag, bytes }));
    unsafe { *out = PsPinned { data, len: items.len(), elem_size: size as u32, pin: PsHandle(pin as *mut c_void) } };
    PS_OK
}
/// Pins the text `h` reads as, as `string_read` reads it: its units are
/// encoded into a buffer whose address is handed out and kept until
/// `array_unpin` drops it. The pin counts against the context that owns
/// `h`, or the calling thread's for the null handle.
unsafe extern "C" fn string_pin(h: PsHandle, out: *mut PsPinned, _err: *mut PsHandle) -> PsStatus {
    let units: Box<[u16]> = to_text(&value(h)).encode_utf16().collect();
    let owner = if h.is_null() { here() } else { owner(h) };
    owner.string_pins_live.fetch_add(1, Ordering::Relaxed);
    owner.string_pins_made.fetch_add(1, Ordering::Relaxed);
    let (data, len) = (units.as_ptr() as *mut c_void, units.len());
    let pin = Box::into_raw(Box::new(Pinned::Text { owner, units }));
    unsafe { *out = PsPinned { data, len, elem_size: 2, pin: PsHandle(pin as *mut c_void) } };
    PS_OK
}
unsafe extern "C" fn array_unpin(p: PsPinned) {
    if p.pin.is_null() {
        return;
    }
    // SAFETY: a non-null pin is the address of the Pinned that array_pin
    // or string_pin made with Box::into_raw, and the host unpins each pin
    // once.
    let pinned = unsafe { Box::from_raw(p.pin.0 as *mut Pinned) };
    match *pinned {
        Pinned::Text { owner, units: _released } => {
            owner.string_pins_live.fetch_sub(1, Ordering::Relaxed);
        }
        Pinned::Array { array, tag, bytes } => {
            let size = elem_size(tag).unwrap_or(1);
            let items: Vec<Value> = bytes.chunks(size).map(|chunk| decode(chunk, tag)).collect();
            if write(array, |held| *held = Value::Array(items)).is_none() {
                eprintln!("pwrs fake host: {IN_USE}; a pinned array's elements were not written back");
            }
        }
    }
}
unsafe extern "C" fn psobject_new(name: PsStr16) -> PsHandle {
    alloc(Value::PsObject(unsafe { read16(name) }, Vec::new()))
}
unsafe extern "C" fn psobject_add_note(obj: PsHandle, name: PsStr16, v: PsHandle, err: *mut PsHandle) -> PsStatus {
    if obj.is_null() {
        return fail(err, "not a PSObject");
    }
    let item = value(v);
    let key = unsafe { read16(name) };
    let added = write(obj, |held| match held {
        Value::PsObject(_, props) => {
            props.push((key, item));
            true
        }
        _not_a_psobject => false,
    });
    match added {
        Some(true) => PS_OK,
        Some(false) => fail(err, "not a PSObject"),
        None => fail(err, IN_USE),
    }
}
unsafe extern "C" fn psobject_get_property(obj: PsHandle, name: PsStr16, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    let key = unsafe { read16(name) };
    match value(obj) {
        Value::PsObject(_, props) => match props.iter().find(|(k, _)| *k == key) {
            Some((_, v)) => {
                unsafe { *out = alloc(v.clone()) };
                PS_OK
            }
            None => fail(err, &format!("no property {key}")),
        },
        other => fail(err, &format!("{other:?} is not a PSObject")),
    }
}
unsafe extern "C" fn unsupported_named(_c: PsHandle, _n: PsStr16, _out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    fail(err, "not supported by the fake host")
}
unsafe extern "C" fn unsupported_set(_c: PsHandle, _n: PsStr16, _v: PsHandle, err: *mut PsHandle) -> PsStatus {
    fail(err, "not supported by the fake host")
}
unsafe extern "C" fn resolve_path(_c: PsHandle, path: PsStr16, _literal: u8, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    let resolved = alloc(Value::Array(vec![Value::Str(unsafe { read16(path) })]));
    unsafe { *out = resolved };
    PS_OK
}
unsafe extern "C" fn invoke_scriptblock(_c: PsHandle, _b: PsHandle, _a: PsHandle, _out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    fail(err, "the fake host cannot run script blocks")
}
unsafe extern "C" fn dyn_call(_o: PsHandle, _n: PsStr16, _a: PsHandle, _out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    fail(err, "the fake host cannot invoke methods")
}
unsafe extern "C" fn dyn_call_static(_t: PsStr16, _n: PsStr16, _a: PsHandle, _out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    fail(err, "the fake host cannot invoke methods")
}
unsafe extern "C" fn dyn_new(_t: PsStr16, _a: PsHandle, _out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    fail(err, "the fake host cannot construct objects")
}
unsafe extern "C" fn factory_new(_id: u32, _f: *const c_void, _out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    fail(err, "the fake host has no generated classes")
}
unsafe extern "C" fn proxy_enter(_obj: PsHandle, _id: u32, _instance: *mut *mut c_void, err: *mut PsHandle) -> PsStatus {
    fail(err, "the fake host has no proxy objects")
}
unsafe extern "C" fn proxy_enter_shared(_obj: PsHandle, _id: u32, _instance: *mut *mut c_void, err: *mut PsHandle) -> PsStatus {
    fail(err, "the fake host has no proxy objects")
}
/// No `proxy_enter` or `proxy_enter_shared` succeeds here, so there is
/// never a borrow to end.
unsafe extern "C" fn proxy_exit(_obj: PsHandle, _changed: u8) {}

/// The one helper name the fake host ships no helper for.
pub const FAKE_MISSING_HELPER: &str = "missing-helper";

/// The path the fake host answers for the helper `name`. Nothing is
/// staged, and the path is longer than the first buffer
/// [`crate::helper_path`] offers, so reading it takes the retry.
pub fn fake_helper_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new("fake-staging").join("s".repeat(300)).join(name)
}

unsafe extern "C" fn helper_path(name: PsStr16, buf: *mut u16, cap: usize, out_len: *mut usize, err: *mut PsHandle) -> PsStatus {
    let name = unsafe { read16(name) };
    if name == FAKE_MISSING_HELPER {
        return fail(err, &format!("the fake host ships no helper {name}"));
    }
    let units: Vec<u16> = fake_helper_path(&name).to_string_lossy().encode_utf16().collect();
    unsafe { *out_len = units.len() };
    let n = units.len().min(cap);
    if !buf.is_null() && n > 0 {
        unsafe { core::ptr::copy_nonoverlapping(units.as_ptr(), buf, n) };
    }
    PS_OK
}
/// Copies a byte buffer into an array of its bytes and frees the
/// buffer through the callback at once, the way .NET Framework does.
unsafe extern "C" fn memory_view_new(tag: u32, ptr: *mut c_void, len: usize, drop_fn: unsafe extern "C" fn(*mut c_void), out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    if tag != PS_TYPE_U8 {
        return fail(err, "the fake host wraps byte memory only");
    }
    let bytes: Vec<Value> = if len == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(ptr as *const u8, len) }.iter().map(|b| Value::Int(*b as i64)).collect() };
    unsafe { drop_fn(ptr) };
    unsafe { *out = alloc(Value::Array(bytes)) };
    PS_OK
}
/// Records the settle, for the context that owns the source: the
/// source's value, the outcome, and the value or error it carried.
unsafe extern "C" fn task_settle(source: PsHandle, outcome: u32, v: PsHandle, _err: *mut PsHandle) -> PsStatus {
    let entry = (value(source), outcome, value(v));
    let context = if source.is_null() { here() } else { owner(source) };
    context.settled.push(entry);
    PS_OK
}

/// Every task this test's context settled so far, on any thread: the
/// source's value, the outcome and the value or error, in the order
/// they were settled.
pub fn settled() -> Vec<(Value, u32, Value)> {
    here().settled.read()
}

/// A task source for a test: a string standing in for the managed
/// object, and the slot a generated method would pack around it.
pub fn task_slot(name: &str, cancelled: *const u8) -> pwrs_sys::PsTaskSlot {
    pwrs_sys::PsTaskSlot { source: alloc(Value::Str(name.to_string())), cancelled }
}

/// The event manager name whose raises the fake host refuses, so a test
/// reaches the error a refused raise returns.
pub const FAKE_REFUSED_EVENTS: &str = "refused-events";

/// Records the raise, for the context that owns the events, or refuses
/// it for [`FAKE_REFUSED_EVENTS`].
unsafe extern "C" fn event_raise(events: PsHandle, source_identifier: PsStr16, sender: PsHandle, args: PsHandle, message_data: PsHandle, err: *mut PsHandle) -> PsStatus {
    let context = if events.is_null() { here() } else { owner(events) };
    let events = value(events);
    if let Value::Str(name) = &events
        && name == FAKE_REFUSED_EVENTS
    {
        return fail(err, "the fake host refuses every raise through these events");
    }
    let entry = Raised { events, source_identifier: unsafe { read16(source_identifier) }, sender: value(sender), args: value(args), message_data: value(message_data) };
    context.raised.push(entry);
    PS_OK
}

/// Every engine event this test's context raised so far, on any thread,
/// in the order raised.
pub fn raised() -> Vec<Raised> {
    here().raised.read()
}

/// The engine events of a fake runspace: a string named `name` stands in
/// for its event manager.
pub fn events(name: &str) -> crate::PsEvents {
    crate::PsEvents::over(object(Value::Str(name.to_string())))
}

/// Copies a byte view into an array of its bytes and releases the view
/// at once, the way .NET Framework does, after refusing one whose
/// refusal byte is set; a refused view is not released, as the host
/// releases only the views it made.
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn memory_view_borrow(
    tag: u32,
    ptr: *mut c_void,
    len: usize,
    read_only: u8,
    refused: *const u8,
    context: *mut c_void,
    release: unsafe extern "C" fn(*mut c_void),
    out: *mut PsHandle,
    err: *mut PsHandle,
) -> PsStatus {
    if tag != PS_TYPE_U8 {
        return fail(err, "the fake host wraps byte memory only");
    }
    if !refused.is_null() && unsafe { refused.read_volatile() } != 0 {
        return fail(err, "This memory view has been revoked by the module that made it.");
    }
    let bytes: Vec<Value> = if len == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(ptr as *const u8, len) }.iter().map(|b| Value::Int(*b as i64)).collect() };
    let handle = alloc(Value::Array(bytes));
    here().views_read_only.push(read_only != 0);
    unsafe { release(context) };
    unsafe { *out = handle };
    PS_OK
}
/// Records one value written through a direct scalar entry.
fn direct_write(v: Value) -> PsStatus {
    let c = here();
    c.direct_writes.fetch_add(1, Ordering::Relaxed);
    c.output.push(v);
    PS_OK
}
unsafe extern "C" fn write_string(_c: PsHandle, text: PsStr16, _err: *mut PsHandle) -> PsStatus {
    direct_write(Value::Str(unsafe { read16(text) }))
}
unsafe extern "C" fn write_i64(_c: PsHandle, v: i64, _err: *mut PsHandle) -> PsStatus {
    direct_write(Value::Int(v))
}
unsafe extern "C" fn write_f64(_c: PsHandle, v: f64, _err: *mut PsHandle) -> PsStatus {
    direct_write(Value::Float(v))
}
unsafe extern "C" fn write_bool(_c: PsHandle, v: u8, _err: *mut PsHandle) -> PsStatus {
    direct_write(Value::Bool(v != 0))
}
unsafe extern "C" fn u64_new(v: u64) -> PsHandle {
    alloc(Value::Int(v as i64))
}
unsafe extern "C" fn u64_read(h: PsHandle, out: *mut u64, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Int(i) => {
            unsafe { *out = i as u64 };
            PS_OK
        }
        other => fail(err, &format!("cannot convert {other:?} to UInt64")),
    }
}
unsafe extern "C" fn datetime_new(ticks: i64, kind: u8, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    if !(0..=3_155_378_975_999_999_999).contains(&ticks) || kind > 2 {
        return fail(err, "ticks or kind out of range");
    }
    unsafe { *out = alloc(Value::DateTime(ticks, kind)) };
    PS_OK
}
unsafe extern "C" fn datetime_read(h: PsHandle, ticks: *mut i64, kind: *mut u8, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::DateTime(t, k) => {
            unsafe { *ticks = t };
            unsafe { *kind = k };
            PS_OK
        }
        other => fail(err, &format!("cannot convert {other:?} to DateTime")),
    }
}
unsafe extern "C" fn timespan_new(ticks: i64) -> PsHandle {
    alloc(Value::TimeSpan(ticks))
}
unsafe extern "C" fn timespan_read(h: PsHandle, ticks: *mut i64, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::TimeSpan(t) => {
            unsafe { *ticks = t };
            PS_OK
        }
        other => fail(err, &format!("cannot convert {other:?} to TimeSpan")),
    }
}
unsafe extern "C" fn guid_new(bytes: *const u8) -> PsHandle {
    let mut b = [0u8; 16];
    unsafe { core::ptr::copy_nonoverlapping(bytes, b.as_mut_ptr(), 16) };
    alloc(Value::Guid(b))
}
unsafe extern "C" fn guid_read(h: PsHandle, out: *mut u8, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Guid(b) => {
            unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), out, 16) };
            PS_OK
        }
        other => fail(err, &format!("cannot convert {other:?} to Guid")),
    }
}
unsafe extern "C" fn char_new(unit: u16) -> PsHandle {
    alloc(Value::Char(unit))
}
unsafe extern "C" fn char_read(h: PsHandle, out: *mut u16, err: *mut PsHandle) -> PsStatus {
    let read = match value(h) {
        Value::Char(c) => c,
        Value::Str(s) => {
            let units: Vec<u16> = s.encode_utf16().collect();
            if units.len() != 1 {
                return fail(err, &format!("cannot convert {s:?} to Char"));
            }
            units[0]
        }
        other => return fail(err, &format!("cannot convert {other:?} to Char")),
    };
    unsafe { *out = read };
    PS_OK
}
unsafe extern "C" fn securestring_new(text: PsStr16, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    if text.len > 65536 {
        return fail(err, "a SecureString holds at most 65536 characters");
    }
    let units = if text.ptr.is_null() || text.len == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(text.ptr, text.len) }.to_vec() };
    unsafe { *out = alloc(Value::Secure(units)) };
    PS_OK
}
unsafe extern "C" fn securestring_read(h: PsHandle, buf: *mut u16, cap: usize, out_len: *mut usize, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Secure(units) => {
            unsafe { *out_len = units.len() };
            let n = units.len().min(cap);
            if !buf.is_null() && n > 0 {
                unsafe { core::ptr::copy_nonoverlapping(units.as_ptr(), buf, n) };
            }
            PS_OK
        }
        other => fail(err, &format!("{other:?} is not a SecureString")),
    }
}
/// The tag an array was made with by `array_new`; the object tag for
/// everything else.
/// The fake host has no CLR, so a read-only view is the source
/// handle cloned. A unit test can check what was wrapped; only the
/// real host can check that a write is refused, which the Pester
/// suite does.
unsafe extern "C" fn readonly_table_new(source: PsHandle, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    unsafe { *out = alloc(value(source)) };
    PS_OK
}

/// Allocates and records the tag of the value's own type, which is
/// how the fake keeps a width the `Value` enum does not distinguish:
/// an i32 and an i64 are both `Value::Int`, and only the tag tells
/// them apart.
fn alloc_tagged(v: Value, tag: PsTypeTag) -> PsHandle {
    alloc_with(v, Some(tag))
}

/// The tag recorded for a handle's object when it was built, if any.
fn recorded_tag(h: PsHandle) -> Option<PsTypeTag> {
    if h.is_null() { None } else { obj(h).tag }
}

unsafe extern "C" fn object_type_tag(h: PsHandle, out: *mut PsTypeTag, _err: *mut PsHandle) -> PsStatus {
    let recorded = recorded_tag(h);
    let tag = match recorded {
        // A width recorded when the value was built wins; an array's
        // element tag is recorded the same way, so only a value whose
        // tag was never set falls through to its variant.
        Some(tag) if !matches!(value(h), Value::Array(_)) => tag,
        _ => match value(h) {
            Value::Null => PS_TYPE_OBJECT,
            Value::Bool(_) => pwrs_sys::PS_TYPE_BOOL,
            Value::Int(_) => pwrs_sys::PS_TYPE_I64,
            Value::Float(_) => pwrs_sys::PS_TYPE_F64,
            Value::Str(_) => pwrs_sys::PS_TYPE_STRING,
            Value::DateTime(..) => PS_TYPE_DATETIME,
            Value::TimeSpan(_) => pwrs_sys::PS_TYPE_TIMESPAN,
            Value::Guid(_) => pwrs_sys::PS_TYPE_GUID,
            Value::Char(_) => pwrs_sys::PS_TYPE_CHAR,
            Value::Decimal(_) => PS_TYPE_DECIMAL,
            _ => PS_TYPE_OBJECT,
        },
    };
    unsafe { *out = tag };
    PS_OK
}

unsafe extern "C" fn i8_new(v: i8) -> PsHandle {
    alloc_tagged(Value::Int(i64::from(v)), pwrs_sys::PS_TYPE_I8)
}
unsafe extern "C" fn i16_new(v: i16) -> PsHandle {
    alloc_tagged(Value::Int(i64::from(v)), pwrs_sys::PS_TYPE_I16)
}
unsafe extern "C" fn i32_new(v: i32) -> PsHandle {
    alloc_tagged(Value::Int(i64::from(v)), pwrs_sys::PS_TYPE_I32)
}
unsafe extern "C" fn u8_new(v: u8) -> PsHandle {
    alloc_tagged(Value::Int(i64::from(v)), pwrs_sys::PS_TYPE_U8)
}
unsafe extern "C" fn u16_new(v: u16) -> PsHandle {
    alloc_tagged(Value::Int(i64::from(v)), pwrs_sys::PS_TYPE_U16)
}
unsafe extern "C" fn u32_new(v: u32) -> PsHandle {
    alloc_tagged(Value::Int(i64::from(v)), pwrs_sys::PS_TYPE_U32)
}
unsafe extern "C" fn f32_new(v: f32) -> PsHandle {
    alloc_tagged(Value::Float(f64::from(v)), pwrs_sys::PS_TYPE_F32)
}

unsafe extern "C" fn decimal_new(lo: i32, mid: i32, hi: i32, flags: i32, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    unsafe { *out = alloc_tagged(Value::Decimal([lo, mid, hi, flags]), PS_TYPE_DECIMAL) };
    PS_OK
}
unsafe extern "C" fn decimal_read(h: PsHandle, out: *mut i32, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Decimal(bits) => {
            unsafe { core::ptr::copy_nonoverlapping(bits.as_ptr(), out, 4) };
            PS_OK
        }
        other => fail(err, &format!("not a Decimal: {other:?}")),
    }
}

unsafe extern "C" fn datetimeoffset_new(ticks: i64, offset_minutes: i16, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    unsafe { *out = alloc(Value::DateTimeOffset(ticks, offset_minutes)) };
    PS_OK
}
unsafe extern "C" fn datetimeoffset_read(h: PsHandle, out_ticks: *mut i64, out_offset: *mut i16, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::DateTimeOffset(ticks, offset) => {
            unsafe { *out_ticks = ticks };
            unsafe { *out_offset = offset };
            PS_OK
        }
        other => fail(err, &format!("not a DateTimeOffset: {other:?}")),
    }
}

/// The fake host runs no commands, so an invocation answers no
/// output. Only the real host can run one, which the Pester suite
/// does.
unsafe extern "C" fn invoke_command(_c: PsHandle, _name: PsStr16, _params: PsHandle, _input: PsHandle, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    unsafe { *out = alloc(Value::Array(Vec::new())) };
    PS_OK
}

/// The fake resolves no type names, so it keeps the one it was given
/// and the value it was given. A test asserting which enum was asked
/// for reads the name back off the value.
unsafe extern "C" fn enum_new(type_name: PsStr16, value: i64, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    let name = unsafe { crate::text::from_str16(type_name.ptr, type_name.len) };
    unsafe { *out = alloc(Value::Enum(name, value)) };
    PS_OK
}

unsafe extern "C" fn array_element_tag(h: PsHandle, out: *mut PsTypeTag, _err: *mut PsHandle) -> PsStatus {
    let tag = match recorded_tag(h) {
        Some(tag) => tag,
        None => PS_TYPE_OBJECT,
    };
    unsafe { *out = tag };
    PS_OK
}
unsafe extern "C" fn exception_describe(err: PsHandle, buf: *mut u16, cap: usize, out_len: *mut usize) -> PsStatus {
    let s: Vec<u16> = to_text(&value(err)).encode_utf16().collect();
    unsafe { *out_len = s.len() };
    let n = s.len().min(cap);
    if !buf.is_null() && n > 0 {
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), buf, n) };
    }
    PS_OK
}

/// Turns a stream on or off, for this test's context, for
/// [`crate::Pipeline::stream_enabled`], so a test can prove that the
/// text is not built when nothing reads it. Every stream starts on.
pub fn set_stream_enabled(kind: pwrs_sys::PsStreamKind, on: bool) {
    let bit = 1u32 << kind;
    if on {
        here().streams_on.fetch_or(bit, Ordering::Relaxed);
    } else {
        here().streams_on.fetch_and(!bit, Ordering::Relaxed);
    }
}

unsafe extern "C" fn stream_enabled(_cmdlet: PsHandle, kind: pwrs_sys::PsStreamKind, out: *mut u8, _err: *mut PsHandle) -> PsStatus {
    unsafe { *out = u8::from(here().streams_on.load(Ordering::Relaxed) & (1u32 << kind) != 0) };
    PS_OK
}

static FAKE: HostVTable = HostVTable {
    size: core::mem::size_of::<HostVTable>() as u32,
    version: PWRS_ABI_VERSION,
    free_handle,
    clone_handle,
    write_object,
    write_error,
    write_stream,
    write_progress,
    should_process: should,
    should_continue: should,
    get_parameter,
    parameter_is_bound,
    string_new,
    string_read,
    i64_new,
    i64_read,
    f64_new,
    f64_read,
    bool_new,
    bool_read,
    array_len,
    array_get,
    array_new,
    array_set,
    array_pin,
    array_unpin,
    psobject_new,
    psobject_add_note,
    psobject_get_property,
    get_variable: unsupported_named,
    set_variable: unsupported_set,
    resolve_path,
    invoke_scriptblock,
    dyn_get: unsupported_named,
    dyn_set: unsupported_set,
    dyn_call,
    dyn_call_static,
    dyn_new,
    factory_new,
    memory_view_new,
    exception_describe,
    write_string,
    write_i64,
    write_f64,
    write_bool,
    u64_new,
    u64_read,
    datetime_new,
    datetime_read,
    timespan_new,
    timespan_read,
    guid_new,
    guid_read,
    char_new,
    char_read,
    stream_enabled,
    readonly_table_new,
    object_type_tag,
    i8_new,
    i16_new,
    i32_new,
    u8_new,
    u16_new,
    u32_new,
    f32_new,
    decimal_new,
    decimal_read,
    datetimeoffset_new,
    datetimeoffset_read,
    invoke_command,
    enum_new,
    proxy_enter,
    proxy_exit,
    securestring_new,
    securestring_read,
    array_element_tag,
    helper_path,
    proxy_enter_shared,
    write_error_details,
    exception_new,
    write_progress_record,
    string_pin,
    memory_view_borrow,
    task_settle,
    event_raise,
    write_host,
};

static INSTALL: Once = Once::new();

/// The calling thread's context in the fake host, from [`install`]:
/// what the entries record, the handles it makes and the streams it
/// keeps belong to it. Dropping it gives the thread back the context it
/// had before.
#[must_use = "the test's context lasts while the guard lives"]
pub struct Host {
    previous: Option<&'static Context>,
    /// The guard restores the context of the thread that made it.
    _thread: core::marker::PhantomData<*const ()>,
}

impl Drop for Host {
    fn drop(&mut self) {
        CURRENT.with(|current| current.set(self.previous));
    }
}

/// Installs the fake vtable once per process and gives the calling
/// thread a context of its own, with nothing recorded, no handles live
/// and every stream on. Hold the returned guard for the length of the
/// test; tests run side by side, each in its own context.
pub fn install() -> Host {
    INSTALL.call_once(|| {
        let status = unsafe { crate::host::install(&FAKE) };
        assert_eq!(status, PS_OK, "fake vtable rejected");
        // A test reads the counters to prove which entry a value took,
        // so they are kept here whatever PWRS_TRACE says.
        crate::trace::force_counting();
    });
    // One context per test, kept for the process: a handle outlives the
    // test that made it when a value holding it is dropped later.
    let context: &'static Context = Box::leak(Box::new(Context::new()));
    let previous = CURRENT.with(|current| current.replace(Some(context)));
    // A test thread stands in for the host calling the module, so its
    // PsObject calls pass the thread check a debug build turns on.
    crate::host::CalledIn::mark_thread();
    Host { previous, _thread: core::marker::PhantomData }
}

/// Whether each memory view this test's context made so far was asked
/// for read-only, in the order they were made.
pub fn views_read_only() -> Vec<bool> {
    here().views_read_only.read()
}

/// String pins this test's context holds that are not yet released.
pub fn live_string_pins() -> usize {
    here().string_pins_live.load(Ordering::Relaxed)
}

/// String pins this test's context was handed since it was installed.
pub fn string_pins_made() -> usize {
    here().string_pins_made.load(Ordering::Relaxed)
}

/// A handle wrapping `v`, owned by the returned object.
pub fn object(v: Value) -> crate::PsObject {
    unsafe { crate::PsObject::from_raw(alloc(v)) }
}
