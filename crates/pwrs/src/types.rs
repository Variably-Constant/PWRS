//! Wrappers over managed objects with PowerShell-specific behavior:
//! script blocks, hashtables, strings kept as the object, big integers,
//! secure strings and credentials.

use crate::class::PsTyped;
use crate::dynamic::{args_array, PsType};
use crate::host::vtable;
use crate::pinned::PsStringUnits;
use crate::pipeline::{check, str16};
use crate::{FromPs, IntoPs, Pipeline, PsObject, PsResult};
use pwrs_sys::PsHandle;
use std::collections::HashMap;

/// A `ScriptBlock`. Invocation needs the pipeline thread, so `call`
/// takes the [`Pipeline`] token. The default is `$null`.
#[derive(Default)]
pub struct PsScriptBlock(pub PsObject);

impl PsScriptBlock {
    /// Runs the block with `args` bound to `$args` and returns every
    /// output object.
    pub fn call(&self, ps: &Pipeline<'_>, args: &[PsObject]) -> PsResult<Vec<PsObject>> {
        let arr = args_array(args)?;
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().invoke_scriptblock)(ps.cmdlet_handle(), self.0.as_raw(), arr.as_raw(), &mut out, &mut err) };
        check(status, err)?;
        let results = unsafe { PsObject::from_raw(out) };
        <Vec<PsObject> as FromPs>::from_ps(&results)
    }
}

impl FromPs for PsScriptBlock {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        Ok(PsScriptBlock(obj.clone()))
    }
}

impl IntoPs for PsScriptBlock {
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(self.0)
    }
}

/// A `System.Collections.Hashtable` (or any `IDictionary`). The
/// default is `$null`; use [`PsHashtable::new`] for an empty table.
#[derive(Default)]
pub struct PsHashtable(pub PsObject);

impl PsHashtable {
    pub fn new() -> PsResult<PsHashtable> {
        Ok(PsHashtable(PsType::from_name("System.Collections.Hashtable").new(&[])?))
    }

    /// An empty table that keeps its keys in the order they were added:
    /// PowerShell 7's own `System.Management.Automation.OrderedHashtable`,
    /// or on Windows PowerShell 5.1, which has none, the runtime's
    /// `Pwrs.OrderedHashtable`, which behaves as it does. Either is a
    /// `Hashtable` to a script, so `-is [hashtable]`, `ContainsKey` and a
    /// `[hashtable]` parameter take it, while `Keys`, `Values`, enumeration
    /// and `ConvertTo-Json` follow the order. Keys compare by case, and a
    /// key removed and set again comes last.
    pub fn ordered() -> PsResult<PsHashtable> {
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().ordered_table_new)(&mut out, &mut err) };
        check(status, err)?;
        Ok(PsHashtable(unsafe { PsObject::from_raw(out) }))
    }

    /// The value under `key`, or `$null` when absent.
    pub fn get(&self, key: &str) -> PsResult<PsObject> {
        self.0.call("get_Item", &[key.into_ps()?])
    }

    pub fn set(&self, key: &str, value: PsObject) -> PsResult<()> {
        self.0.call("set_Item", &[key.into_ps()?, value])?;
        Ok(())
    }

    pub fn contains(&self, key: &str) -> PsResult<bool> {
        bool::from_ps(&self.0.call("ContainsKey", &[key.into_ps()?])?)
    }

    pub fn len(&self) -> PsResult<usize> {
        Ok(i64::from_ps(&self.0.get("Count")?)? as usize)
    }

    pub fn is_empty(&self) -> PsResult<bool> {
        Ok(self.len()? == 0)
    }

    /// Keys converted to strings.
    pub fn keys(&self) -> PsResult<Vec<String>> {
        let keys = self.0.get("Keys")?;
        <Vec<String> as FromPs>::from_ps(&keys)
    }
}

impl FromPs for PsHashtable {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        Ok(PsHashtable(obj.clone()))
    }
}

impl IntoPs for PsHashtable {
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(self.0)
    }
}

/// A table script reads through both `$t.key` and `$t['key']` and
/// cannot write through either.
///
/// Writing through either form raises a terminating error rather than
/// being ignored, because a write that looks like it worked and did
/// not is worse than the sharing it guards against.
///
/// It refers to the source rather than copying it, so enumeration follows
/// the source's order and making one costs nothing per key. A view,
/// not a snapshot: a change made through the source shows through.
///
/// A nested table comes back wrapped as well, from the indexer, from
/// `Values` and from enumeration, so the refusal reaches all the way
/// down. Each such read builds a wrapper, so two reads of one key
/// match by content and not by reference.
#[derive(Default)]
pub struct PsReadOnlyTable(pub PsObject);

impl PsReadOnlyTable {
    /// A read-only view over an existing `IDictionary`, which is what
    /// a caller already has.
    pub fn over(source: &PsObject) -> PsResult<PsReadOnlyTable> {
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().readonly_table_new)(source.as_raw(), &mut out, &mut err) };
        check(status, err)?;
        Ok(PsReadOnlyTable(unsafe { PsObject::from_raw(out) }))
    }

    /// The value under `key`, or `$null` when absent.
    pub fn get(&self, key: &str) -> PsResult<PsObject> {
        self.0.call("get_Item", &[key.into_ps()?])
    }

    pub fn contains(&self, key: &str) -> PsResult<bool> {
        bool::from_ps(&self.0.call("Contains", &[key.into_ps()?])?)
    }

    pub fn len(&self) -> PsResult<usize> {
        Ok(i64::from_ps(&self.0.get("Count")?)? as usize)
    }

    pub fn is_empty(&self) -> PsResult<bool> {
        Ok(self.len()? == 0)
    }

    /// Keys converted to strings.
    pub fn keys(&self) -> PsResult<Vec<String>> {
        let keys = self.0.get("Keys")?;
        <Vec<String> as FromPs>::from_ps(&keys)
    }
}

impl FromPs for PsReadOnlyTable {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        Ok(PsReadOnlyTable(obj.clone()))
    }
}

impl IntoPs for PsReadOnlyTable {
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(self.0)
    }
}

/// A `System.String` kept as the .NET object. A cmdlet parameter of this
/// type is declared `string`, so it binds as a `[string]` parameter does,
/// and the module holds the caller's own string, not a copy of its text.
/// The text is read when asked for: [`PsString::text`] decodes it, and
/// [`PsString::units`] pins its UTF-16 units where the runtime keeps them,
/// to read in place. [`PsString::object`] is the string itself, which can
/// key a `ConditionalWeakTable`, a table that keeps no string alive. The
/// default is `$null`.
#[derive(Default, Clone, Debug)]
pub struct PsString(PsObject);

impl PsString {
    /// The string as the .NET object: for a parameter, the caller's own.
    /// `into_ps` hands the same object over.
    pub fn object(&self) -> &PsObject {
        &self.0
    }

    /// Whether this is `$null`, as an optional parameter left out is.
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    /// The text, decoded from UTF-16; `$null` reads as an empty string.
    pub fn text(&self) -> PsResult<String> {
        String::from_ps(&self.0)
    }

    /// The UTF-16 units, pinned where the runtime keeps the string and
    /// read in place, with no copy; `$null` has none. The view cannot
    /// write, and its pin is released when it drops, which must be before
    /// the lifecycle phase that made it returns.
    pub fn units(&self) -> PsResult<PsStringUnits<'_>> {
        crate::convert::string_units(&self.0)
    }
}

impl PsTyped for PsString {
    const CLR_NAME: &'static str = "System.String";
    const VALUE_TYPE: bool = false;
}

impl FromPs for PsString {
    /// A string is kept as itself and `$null` as `$null`. Any other object
    /// is kept as the string the engine converts it to, as a `[string]`
    /// parameter's binder converts it.
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        if obj.is_null() {
            return Ok(PsString(PsObject::null()));
        }
        let mut tag = pwrs_sys::PS_TYPE_OBJECT;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().object_type_tag)(obj.as_raw(), &mut tag, &mut err) };
        check(status, err)?;
        if tag == pwrs_sys::PS_TYPE_STRING {
            return Ok(PsString(obj.clone()));
        }
        Ok(PsString(String::from_ps(obj)?.into_ps()?))
    }
}

impl IntoPs for PsString {
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(self.0)
    }
}

impl<V: IntoPs> IntoPs for HashMap<String, V> {
    fn into_ps(self) -> PsResult<PsObject> {
        let table = PsHashtable::new()?;
        for (k, v) in self {
            table.set(&k, v.into_ps()?)?;
        }
        Ok(table.0)
    }
}

impl<V: FromPs> FromPs for HashMap<String, V> {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let table = PsHashtable(obj.clone());
        let keys = table.keys()?;
        let mut out = HashMap::new();
        out.try_reserve(keys.len())?;
        for k in keys {
            let v = table.get(&k)?;
            out.insert(k, V::from_ps(&v)?);
        }
        Ok(out)
    }
}

/// A `System.Numerics.BigInteger` as little-endian two's-complement
/// bytes, the representation `ToByteArray` and the byte-array
/// constructor share.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct PsBigInt {
    pub bytes: Vec<u8>,
}

impl PsBigInt {
    pub fn is_negative(&self) -> bool {
        self.bytes.last().is_some_and(|b| b & 0x80 != 0)
    }

    /// `self * 2`, extending by a byte when the sign bit would change.
    pub fn doubled(&self) -> PsBigInt {
        let negative = self.is_negative();
        let mut out = Vec::with_capacity(self.bytes.len() + 1);
        let mut carry = 0u8;
        for &b in &self.bytes {
            out.push((b << 1) | carry);
            carry = b >> 7;
        }
        let sign_ok = out.last().is_some_and(|b| (b & 0x80 != 0) == negative);
        if !sign_ok || out.is_empty() {
            out.push(if negative { 0xFF } else { 0x00 });
        }
        PsBigInt { bytes: out }
    }
}

impl FromPs for PsBigInt {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let arr = obj.call("ToByteArray", &[])?;
        let pinned = arr.pin::<u8>()?;
        Ok(PsBigInt { bytes: crate::fallible::copy_of(&pinned)? })
    }
}

impl IntoPs for PsBigInt {
    fn into_ps(self) -> PsResult<PsObject> {
        let arr = PsObject::from_slice(&self.bytes)?;
        PsType::from_name("System.Numerics.BigInteger").new(&[arr])
    }
}

/// A `System.Security.SecureString`. [`PsSecureString::new`] builds a
/// read-only one from Rust text; [`PsSecureString::reveal`] decrypts
/// it. The default is `$null`.
#[derive(Default, Clone)]
pub struct PsSecureString(pub PsObject);

/// Zeroes `units` with writes the optimizer does not remove.
fn wipe(units: &mut [u16]) {
    for unit in units.iter_mut() {
        unsafe { core::ptr::write_volatile(unit, 0) };
    }
}

/// Runs `f` with `text` as UTF-16 written into `units`, then zeroes what
/// was written, whatever `f` returns. `units` has room for `text.len()`
/// units, more than the UTF-16 of `text` takes, so writing never moves
/// it and leaves no copy behind in a freed allocation.
fn with_wiped_utf16<R>(units: &mut Vec<u16>, text: &str, f: impl FnOnce(&[u16]) -> R) -> R {
    debug_assert!(units.capacity() >= text.len());
    units.clear();
    units.extend(text.encode_utf16());
    let r = f(units);
    wipe(units);
    r
}

impl PsSecureString {
    /// A read-only `SecureString` containing `text`. The UTF-16 copy passed
    /// to the host is zeroed before it is freed, whether the host takes
    /// it or fails; `text` itself is the caller's.
    pub fn new(text: &str) -> PsResult<PsSecureString> {
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let mut units: Vec<u16> = crate::fallible::vec_with_capacity(text.len())?;
        let status = with_wiped_utf16(&mut units, text, |u| unsafe { (vtable().securestring_new)(str16(u), &mut out, &mut err) });
        check(status, err)?;
        Ok(PsSecureString(unsafe { PsObject::from_raw(out) }))
    }

    /// Number of characters, read without decrypting.
    pub fn len(&self) -> PsResult<usize> {
        let mut len = 0usize;
        let mut err = PsHandle::NULL;
        let status = unsafe { (vtable().securestring_read)(self.0.as_raw(), core::ptr::null_mut(), 0, &mut len, &mut err) };
        check(status, err)?;
        Ok(len)
    }

    pub fn is_empty(&self) -> PsResult<bool> {
        Ok(self.len()? == 0)
    }

    /// The text. The UTF-16 copy made on the way is zeroed before it
    /// is freed, on every path out, including a buffer given up to
    /// grow; the returned `String` is the caller's, to drop soon.
    pub fn reveal(&self) -> PsResult<String> {
        let first = self.len()?;
        let mut buf: Vec<u16> = crate::fallible::vec_with_capacity(first)?;
        buf.resize(first, 0);
        let mut len = 0usize;
        let text = loop {
            let mut err = PsHandle::NULL;
            let status = unsafe { (vtable().securestring_read)(self.0.as_raw(), buf.as_mut_ptr(), buf.len(), &mut len, &mut err) };
            if let Err(e) = check(status, err) {
                break Err(e);
            }
            if len <= buf.len() {
                break crate::text::try_from_utf16(&buf[..len]);
            }
            // Growing can move the buffer and free the old one, which
            // contains what the read above wrote.
            wipe(&mut buf);
            if let Err(e) = crate::fallible::grow_to(&mut buf, len, 0) {
                break Err(e);
            }
        };
        wipe(&mut buf);
        text
    }
}

impl core::fmt::Debug for PsSecureString {
    /// The type name only; never the text.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("PsSecureString")
    }
}

impl PsTyped for PsSecureString {
    const CLR_NAME: &'static str = "System.Security.SecureString";
    const VALUE_TYPE: bool = false;
}

impl FromPs for PsSecureString {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        Ok(PsSecureString(obj.clone()))
    }
}

impl IntoPs for PsSecureString {
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(self.0)
    }
}

/// A `System.Management.Automation.PSCredential`: the user name and
/// the password as a secure string.
#[derive(Default, Clone, Debug)]
pub struct PsCredential {
    pub user_name: String,
    pub password: PsSecureString,
}

impl PsCredential {
    pub fn new(user_name: &str, password: &str) -> PsResult<PsCredential> {
        Ok(PsCredential { user_name: user_name.to_string(), password: PsSecureString::new(password)? })
    }
}

impl PsTyped for PsCredential {
    const CLR_NAME: &'static str = "System.Management.Automation.PSCredential";
    const VALUE_TYPE: bool = false;
}

impl FromPs for PsCredential {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        Ok(PsCredential { user_name: String::from_ps(&obj.get("UserName")?)?, password: PsSecureString::from_ps(&obj.get("Password")?)? })
    }
}

impl IntoPs for PsCredential {
    /// The engine's constructor rejects an empty user name.
    fn into_ps(self) -> PsResult<PsObject> {
        PsType::from_name("System.Management.Automation.PSCredential").new(&[self.user_name.into_ps()?, self.password.into_ps()?])
    }
}

/// Text conversion of any object through `LanguagePrimitives`.
pub fn display_string(obj: &PsObject) -> PsResult<String> {
    String::from_ps(obj)
}

#[cfg(test)]
mod tests {
    use super::with_wiped_utf16;

    #[test]
    fn the_copy_passed_to_the_host_stays_put_and_is_zero_once_the_call_returns() {
        for text in ["", "hunter2", "pässwörd ✓ 𝄞"] {
            let mut units = Vec::with_capacity(text.len());
            let before = units.as_ptr();
            let (seen, during) = with_wiped_utf16(&mut units, text, |u| (u.to_vec(), u.as_ptr()));
            assert_eq!(seen, text.encode_utf16().collect::<Vec<u16>>(), "{text}");
            assert_eq!(during, before, "{text}: the buffer moved while it was written");
            assert_eq!(units.len(), seen.len());
            assert!(units.iter().all(|&u| u == 0), "{text}: {units:?}");
        }
    }
}
