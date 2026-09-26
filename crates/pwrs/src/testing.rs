//! A fake host vtable for unit tests: objects live in a table inside
//! this process and every entry behaves like the managed runtime for
//! the primitive, string, array, and PSObject cases. Cmdlet-stream
//! entries record what was written. Nothing here touches .NET.

use core::ffi::c_void;
use pwrs_sys::*;
use std::collections::HashMap;
use std::sync::{Mutex, Once};

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

/// A pinned array's bytes, written back into the array at unpin.
struct Pin {
    array: usize,
    tag: PsTypeTag,
    bytes: Box<[u8]>,
}

struct Table {
    next: usize,
    objects: HashMap<usize, Value>,
    /// The element tag an array was made with; arrays made any other
    /// way have none and cannot be pinned.
    tags: HashMap<usize, PsTypeTag>,
    pins: HashMap<usize, Pin>,
    /// Everything written through `write_object`, in order.
    output: Vec<Value>,
    streams: Vec<(u32, String)>,
    errors: Vec<(String, String, u32, bool)>,
}

static TABLE: Mutex<Option<Table>> = Mutex::new(None);

fn table() -> std::sync::MutexGuard<'static, Option<Table>> {
    let mut g = match TABLE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if g.is_none() {
        *g = Some(Table {
            next: 1,
            objects: HashMap::new(),
            tags: HashMap::new(),
            pins: HashMap::new(),
            output: Vec::new(),
            streams: Vec::new(),
            errors: Vec::new(),
        });
    }
    g
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
    if let Value::Null = v {
        return PsHandle::NULL;
    }
    let mut t = table();
    let t = t.as_mut().expect("table initialized by table()");
    let id = t.next;
    t.next += 1;
    t.objects.insert(id, v);
    PsHandle(id as *mut c_void)
}

/// The value behind a handle, cloned.
pub fn value(h: PsHandle) -> Value {
    if h.is_null() {
        return Value::Null;
    }
    match table().as_ref().expect("table initialized by table()").objects.get(&(h.0 as usize)) {
        Some(v) => v.clone(),
        None => Value::Exception(format!("dangling handle {}", h.0 as usize)),
    }
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
    String::from_utf16_lossy(core::slice::from_raw_parts(s.ptr, s.len))
}

/// Output written so far, then cleared.
pub fn take_output() -> Vec<Value> {
    std::mem::take(&mut table().as_mut().expect("table initialized by table()").output)
}

pub fn take_streams() -> Vec<(u32, String)> {
    std::mem::take(&mut table().as_mut().expect("table initialized by table()").streams)
}

/// `(message, id, category, terminating)` records written so far.
pub fn take_errors() -> Vec<(String, String, u32, bool)> {
    std::mem::take(&mut table().as_mut().expect("table initialized by table()").errors)
}

/// Number of live handles.
pub fn live_handles() -> usize {
    table().as_ref().expect("table initialized by table()").objects.len()
}

unsafe extern "C" fn free_handle(h: PsHandle) {
    if !h.is_null() {
        table().as_mut().expect("table initialized by table()").objects.remove(&(h.0 as usize));
    }
}
unsafe extern "C" fn clone_handle(h: PsHandle) -> PsHandle {
    alloc(value(h))
}
unsafe extern "C" fn write_object(_c: PsHandle, obj: PsHandle, enumerate: u8, _err: *mut PsHandle) -> PsStatus {
    let v = value(obj);
    let mut t = table();
    let t = t.as_mut().expect("table initialized by table()");
    match (enumerate, v) {
        (1, Value::Array(items)) => t.output.extend(items),
        (_, other) => t.output.push(other),
    }
    PS_OK
}
unsafe extern "C" fn write_error(_c: PsHandle, message: PsStr16, id: PsStr16, category: u32, _target: PsHandle, terminating: u8, _err: *mut PsHandle) -> PsStatus {
    table().as_mut().expect("table initialized by table()").errors.push((read16(message), read16(id), category, terminating != 0));
    PS_OK
}
unsafe extern "C" fn write_stream(_c: PsHandle, kind: u32, text: PsStr16, _err: *mut PsHandle) -> PsStatus {
    table().as_mut().expect("table initialized by table()").streams.push((kind, read16(text)));
    PS_OK
}
unsafe extern "C" fn write_progress(_c: PsHandle, _id: i32, _a: PsStr16, _s: PsStr16, _p: i32, _err: *mut PsHandle) -> PsStatus {
    PS_OK
}
unsafe extern "C" fn should(_c: PsHandle, _a: PsStr16, _b: PsStr16, yes: *mut u8, _err: *mut PsHandle) -> PsStatus {
    *yes = 1;
    PS_OK
}
unsafe extern "C" fn get_parameter(_c: PsHandle, _n: PsStr16, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    *out = PsHandle::NULL;
    PS_OK
}
unsafe extern "C" fn parameter_is_bound(_c: PsHandle, _n: PsStr16, bound: *mut u8) -> PsStatus {
    *bound = 0;
    PS_OK
}
unsafe extern "C" fn string_new(s: PsStr16) -> PsHandle {
    alloc(Value::Str(read16(s)))
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
    *out_len = s.len();
    let n = s.len().min(cap);
    if !buf.is_null() && n > 0 {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, n);
    }
    PS_OK
}
unsafe extern "C" fn i64_new(v: i64) -> PsHandle {
    alloc(Value::Int(v))
}
unsafe extern "C" fn i64_read(h: PsHandle, out: *mut i64, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Int(i) => {
            *out = i;
            PS_OK
        }
        Value::Enum(_name, i) => {
            *out = i;
            PS_OK
        }
        Value::Float(f) => {
            *out = f as i64;
            PS_OK
        }
        Value::Bool(b) => {
            *out = b as i64;
            PS_OK
        }
        Value::Str(s) => match s.trim().parse::<i64>() {
            Ok(i) => {
                *out = i;
                PS_OK
            }
            Err(e) => fail(err, &format!("cannot convert {s:?} to Int64: {e}")),
        },
        other => fail(err, &format!("cannot convert {other:?} to Int64")),
    }
}
unsafe extern "C" fn f64_new(v: f64) -> PsHandle {
    alloc(Value::Float(v))
}
unsafe extern "C" fn f64_read(h: PsHandle, out: *mut f64, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Float(f) => {
            *out = f;
            PS_OK
        }
        Value::Int(i) => {
            *out = i as f64;
            PS_OK
        }
        other => fail(err, &format!("cannot convert {other:?} to Double")),
    }
}
unsafe extern "C" fn bool_new(v: u8) -> PsHandle {
    alloc(Value::Bool(v != 0))
}
unsafe extern "C" fn bool_read(h: PsHandle, out: *mut u8, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Bool(b) => {
            *out = b as u8;
            PS_OK
        }
        Value::Int(i) => {
            *out = (i != 0) as u8;
            PS_OK
        }
        Value::Null => {
            *out = 0;
            PS_OK
        }
        other => fail(err, &format!("cannot convert {other:?} to Boolean")),
    }
}
unsafe extern "C" fn array_len(h: PsHandle, out: *mut usize, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Array(items) => {
            *out = items.len();
            PS_OK
        }
        Value::Null => {
            *out = 0;
            PS_OK
        }
        other => fail(err, &format!("{other:?} is not a collection")),
    }
}
unsafe extern "C" fn array_get(h: PsHandle, i: usize, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Array(items) => match items.get(i) {
            Some(v) => {
                *out = alloc(v.clone());
                PS_OK
            }
            None => fail(err, "index out of range"),
        },
        other => fail(err, &format!("{other:?} is not a collection")),
    }
}
unsafe extern "C" fn array_new(tag: u32, len: usize, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    let h = alloc(Value::Array(vec![Value::Null; len]));
    table().as_mut().expect("table initialized by table()").tags.insert(h.0 as usize, tag);
    *out = h;
    PS_OK
}
unsafe extern "C" fn array_set(h: PsHandle, i: usize, v: PsHandle, err: *mut PsHandle) -> PsStatus {
    let item = value(v);
    let stored = {
        let mut t = table();
        let t = t.as_mut().expect("table initialized by table()");
        match t.objects.get_mut(&(h.0 as usize)) {
            Some(Value::Array(items)) if i < items.len() => {
                items[i] = item;
                true
            }
            Some(_not_an_array) => false,
            None => false,
        }
    };
    if stored { PS_OK } else { fail(err, "array_set on a non-array or out of range") }
}
/// Pins an array made by `array_new` with a primitive tag: its
/// elements are encoded into a buffer whose address is handed out,
/// and `array_unpin` writes the buffer back.
unsafe extern "C" fn array_pin(h: PsHandle, out: *mut PsPinned, err: *mut PsHandle) -> PsStatus {
    let id = h.0 as usize;
    let mut guard = table();
    let t = guard.as_mut().expect("table initialized by table()");
    let tag = match t.tags.get(&id) {
        Some(tag) => *tag,
        None => {
            drop(guard);
            return fail(err, "the fake host pins only arrays made by array_new");
        }
    };
    let size = match elem_size(tag) {
        Some(s) => s,
        None => {
            drop(guard);
            return fail(err, &format!("cannot pin an array with element tag {tag}"));
        }
    };
    let items = match t.objects.get(&id) {
        Some(Value::Array(items)) => items.clone(),
        Some(other) => {
            let msg = format!("{other:?} is not an array");
            drop(guard);
            return fail(err, &msg);
        }
        None => {
            drop(guard);
            return fail(err, "dangling handle");
        }
    };
    let mut bytes = Vec::with_capacity(items.len() * size);
    for item in &items {
        if let Err(msg) = encode(item, tag, &mut bytes) {
            drop(guard);
            return fail(err, &msg);
        }
    }
    let mut bytes = bytes.into_boxed_slice();
    let pin_id = t.next;
    t.next += 1;
    *out = PsPinned { data: bytes.as_mut_ptr() as *mut c_void, len: items.len(), elem_size: size as u32, pin: PsHandle(pin_id as *mut c_void) };
    t.pins.insert(pin_id, Pin { array: id, tag, bytes });
    PS_OK
}
unsafe extern "C" fn array_unpin(p: PsPinned) {
    let mut guard = table();
    let t = guard.as_mut().expect("table initialized by table()");
    let pin = match t.pins.remove(&(p.pin.0 as usize)) {
        Some(pin) => pin,
        None => return,
    };
    let size = elem_size(pin.tag).unwrap_or(1);
    let items: Vec<Value> = pin.bytes.chunks(size).map(|chunk| decode(chunk, pin.tag)).collect();
    t.objects.insert(pin.array, Value::Array(items));
}
unsafe extern "C" fn psobject_new(name: PsStr16) -> PsHandle {
    alloc(Value::PsObject(read16(name), Vec::new()))
}
unsafe extern "C" fn psobject_add_note(obj: PsHandle, name: PsStr16, v: PsHandle, err: *mut PsHandle) -> PsStatus {
    let item = value(v);
    let key = read16(name);
    let added = {
        let mut t = table();
        let t = t.as_mut().expect("table initialized by table()");
        match t.objects.get_mut(&(obj.0 as usize)) {
            Some(Value::PsObject(_, props)) => {
                props.push((key, item));
                true
            }
            Some(_not_a_psobject) => false,
            None => false,
        }
    };
    if added { PS_OK } else { fail(err, "not a PSObject") }
}
unsafe extern "C" fn psobject_get_property(obj: PsHandle, name: PsStr16, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    let key = read16(name);
    match value(obj) {
        Value::PsObject(_, props) => match props.iter().find(|(k, _)| *k == key) {
            Some((_, v)) => {
                *out = alloc(v.clone());
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
    *out = alloc(Value::Array(vec![Value::Str(read16(path))]));
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
    let name = read16(name);
    if name == FAKE_MISSING_HELPER {
        return fail(err, &format!("the fake host ships no helper {name}"));
    }
    let units: Vec<u16> = fake_helper_path(&name).to_string_lossy().encode_utf16().collect();
    *out_len = units.len();
    let n = units.len().min(cap);
    if !buf.is_null() && n > 0 {
        core::ptr::copy_nonoverlapping(units.as_ptr(), buf, n);
    }
    PS_OK
}
/// Copies a byte buffer into an array of its bytes and frees the
/// buffer through the callback at once, the way .NET Framework does.
unsafe extern "C" fn memory_view_new(tag: u32, ptr: *mut c_void, len: usize, drop_fn: unsafe extern "C" fn(*mut c_void), out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    if tag != PS_TYPE_U8 {
        return fail(err, "the fake host wraps byte memory only");
    }
    let bytes: Vec<Value> = if len == 0 { Vec::new() } else { core::slice::from_raw_parts(ptr as *const u8, len).iter().map(|b| Value::Int(*b as i64)).collect() };
    drop_fn(ptr);
    *out = alloc(Value::Array(bytes));
    PS_OK
}
unsafe extern "C" fn write_string(_c: PsHandle, text: PsStr16, _err: *mut PsHandle) -> PsStatus {
    table().as_mut().expect("table initialized by table()").output.push(Value::Str(read16(text)));
    PS_OK
}
unsafe extern "C" fn write_i64(_c: PsHandle, v: i64, _err: *mut PsHandle) -> PsStatus {
    table().as_mut().expect("table initialized by table()").output.push(Value::Int(v));
    PS_OK
}
unsafe extern "C" fn write_f64(_c: PsHandle, v: f64, _err: *mut PsHandle) -> PsStatus {
    table().as_mut().expect("table initialized by table()").output.push(Value::Float(v));
    PS_OK
}
unsafe extern "C" fn write_bool(_c: PsHandle, v: u8, _err: *mut PsHandle) -> PsStatus {
    table().as_mut().expect("table initialized by table()").output.push(Value::Bool(v != 0));
    PS_OK
}
unsafe extern "C" fn u64_new(v: u64) -> PsHandle {
    alloc(Value::Int(v as i64))
}
unsafe extern "C" fn u64_read(h: PsHandle, out: *mut u64, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Int(i) => {
            *out = i as u64;
            PS_OK
        }
        other => fail(err, &format!("cannot convert {other:?} to UInt64")),
    }
}
unsafe extern "C" fn datetime_new(ticks: i64, kind: u8, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    if !(0..=3_155_378_975_999_999_999).contains(&ticks) || kind > 2 {
        return fail(err, "ticks or kind out of range");
    }
    *out = alloc(Value::DateTime(ticks, kind));
    PS_OK
}
unsafe extern "C" fn datetime_read(h: PsHandle, ticks: *mut i64, kind: *mut u8, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::DateTime(t, k) => {
            *ticks = t;
            *kind = k;
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
            *ticks = t;
            PS_OK
        }
        other => fail(err, &format!("cannot convert {other:?} to TimeSpan")),
    }
}
unsafe extern "C" fn guid_new(bytes: *const u8) -> PsHandle {
    let mut b = [0u8; 16];
    core::ptr::copy_nonoverlapping(bytes, b.as_mut_ptr(), 16);
    alloc(Value::Guid(b))
}
unsafe extern "C" fn guid_read(h: PsHandle, out: *mut u8, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Guid(b) => {
            core::ptr::copy_nonoverlapping(b.as_ptr(), out, 16);
            PS_OK
        }
        other => fail(err, &format!("cannot convert {other:?} to Guid")),
    }
}
unsafe extern "C" fn char_new(unit: u16) -> PsHandle {
    alloc(Value::Char(unit))
}
unsafe extern "C" fn char_read(h: PsHandle, out: *mut u16, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Char(c) => {
            *out = c;
            PS_OK
        }
        Value::Str(s) => {
            let units: Vec<u16> = s.encode_utf16().collect();
            if units.len() == 1 {
                *out = units[0];
                PS_OK
            } else {
                fail(err, &format!("cannot convert {s:?} to Char"))
            }
        }
        other => fail(err, &format!("cannot convert {other:?} to Char")),
    }
}
unsafe extern "C" fn securestring_new(text: PsStr16, out: *mut PsHandle, err: *mut PsHandle) -> PsStatus {
    if text.len > 65536 {
        return fail(err, "a SecureString holds at most 65536 characters");
    }
    let units = if text.ptr.is_null() || text.len == 0 { Vec::new() } else { core::slice::from_raw_parts(text.ptr, text.len).to_vec() };
    *out = alloc(Value::Secure(units));
    PS_OK
}
unsafe extern "C" fn securestring_read(h: PsHandle, buf: *mut u16, cap: usize, out_len: *mut usize, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Secure(units) => {
            *out_len = units.len();
            let n = units.len().min(cap);
            if !buf.is_null() && n > 0 {
                core::ptr::copy_nonoverlapping(units.as_ptr(), buf, n);
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
    *out = alloc(value(source));
    PS_OK
}

/// Allocates and records the tag of the value's own type, which is
/// how the fake keeps a width the `Value` enum does not distinguish:
/// an i32 and an i64 are both `Value::Int`, and only the tag tells
/// them apart.
fn alloc_tagged(v: Value, tag: PsTypeTag) -> PsHandle {
    let h = alloc(v);
    table().as_mut().expect("table initialized by table()").tags.insert(h.0 as usize, tag);
    h
}

unsafe extern "C" fn object_type_tag(h: PsHandle, out: *mut PsTypeTag, _err: *mut PsHandle) -> PsStatus {
    let recorded = table()
        .as_ref()
        .expect("table initialized by table()")
        .tags
        .get(&(h.0 as usize))
        .copied();
    *out = match recorded {
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
    *out = alloc_tagged(Value::Decimal([lo, mid, hi, flags]), PS_TYPE_DECIMAL);
    PS_OK
}
unsafe extern "C" fn decimal_read(h: PsHandle, out: *mut i32, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::Decimal(bits) => {
            core::ptr::copy_nonoverlapping(bits.as_ptr(), out, 4);
            PS_OK
        }
        other => fail(err, &format!("not a Decimal: {other:?}")),
    }
}

unsafe extern "C" fn datetimeoffset_new(ticks: i64, offset_minutes: i16, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    *out = alloc(Value::DateTimeOffset(ticks, offset_minutes));
    PS_OK
}
unsafe extern "C" fn datetimeoffset_read(h: PsHandle, out_ticks: *mut i64, out_offset: *mut i16, err: *mut PsHandle) -> PsStatus {
    match value(h) {
        Value::DateTimeOffset(ticks, offset) => {
            *out_ticks = ticks;
            *out_offset = offset;
            PS_OK
        }
        other => fail(err, &format!("not a DateTimeOffset: {other:?}")),
    }
}

/// The fake host runs no commands, so an invocation answers no
/// output. Only the real host can run one, which the Pester suite
/// does.
unsafe extern "C" fn invoke_command(_c: PsHandle, _name: PsStr16, _params: PsHandle, _input: PsHandle, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    *out = alloc(Value::Array(Vec::new()));
    PS_OK
}

/// The fake resolves no type names, so it keeps the one it was given
/// and the value it was given. A test asserting which enum was asked
/// for reads the name back off the value.
unsafe extern "C" fn enum_new(type_name: PsStr16, value: i64, out: *mut PsHandle, _err: *mut PsHandle) -> PsStatus {
    *out = alloc(Value::Enum(crate::text::from_str16(type_name.ptr, type_name.len), value));
    PS_OK
}

unsafe extern "C" fn array_element_tag(h: PsHandle, out: *mut PsTypeTag, _err: *mut PsHandle) -> PsStatus {
    *out = match table().as_ref().expect("table initialized by table()").tags.get(&(h.0 as usize)) {
        Some(tag) => *tag,
        None => PS_TYPE_OBJECT,
    };
    PS_OK
}
unsafe extern "C" fn exception_describe(err: PsHandle, buf: *mut u16, cap: usize, out_len: *mut usize) -> PsStatus {
    let s: Vec<u16> = to_text(&value(err)).encode_utf16().collect();
    *out_len = s.len();
    let n = s.len().min(cap);
    if !buf.is_null() && n > 0 {
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf, n);
    }
    PS_OK
}

/// Which streams the fake host reports as kept. Every stream starts on.
static STREAMS_ON: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(u32::MAX);

/// Turns a stream on or off for [`crate::Pipeline::stream_enabled`], so
/// a test can prove that the text is not built when nothing reads it.
pub fn set_stream_enabled(kind: pwrs_sys::PsStreamKind, on: bool) {
    use core::sync::atomic::Ordering;
    let bit = 1u32 << kind;
    let mut seen = STREAMS_ON.load(Ordering::Relaxed);
    loop {
        let next = if on { seen | bit } else { seen & !bit };
        match STREAMS_ON.compare_exchange_weak(seen, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(current) => seen = current,
        }
    }
}

unsafe extern "C" fn stream_enabled(_cmdlet: PsHandle, kind: pwrs_sys::PsStreamKind, out: *mut u8, _err: *mut PsHandle) -> PsStatus {
    *out = u8::from(STREAMS_ON.load(core::sync::atomic::Ordering::Relaxed) & (1u32 << kind) != 0);
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
};

static INSTALL: Once = Once::new();

static EXCLUSIVE: Mutex<()> = Mutex::new(());

/// Installs the fake table once per process and takes the fake host
/// for the caller. The table is process-global, so two tests writing
/// output at once would interleave; holding the returned guard for
/// the length of a test is what keeps each one's output its own.
pub fn install() -> std::sync::MutexGuard<'static, ()> {
    INSTALL.call_once(|| {
        let status = unsafe { crate::host::install(&FAKE) };
        assert_eq!(status, PS_OK, "fake vtable rejected");
        // A test reads the counters to prove which entry a value took,
        // so they are kept here whatever PWRS_TRACE says.
        crate::trace::force_counting();
    });
    let guard = match EXCLUSIVE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    // Every test starts with all streams on, whatever the last one set.
    STREAMS_ON.store(u32::MAX, core::sync::atomic::Ordering::Relaxed);
    // A test thread stands in for the host calling the module, so its
    // PsObject calls pass the thread check a debug build turns on.
    crate::host::CalledIn::mark_thread();
    guard
}

/// A handle wrapping `v`, owned by the returned object.
pub fn object(v: Value) -> crate::PsObject {
    unsafe { crate::PsObject::from_raw(alloc(v)) }
}
