//! Wrappers over managed objects with PowerShell-specific behavior:
//! script blocks, hashtables, big integers, secure strings and
//! credentials.

use crate::class::PsTyped;
use crate::dynamic::{args_array, PsType};
use crate::host::vtable;
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
/// It holds the source rather than copying it, so enumeration keeps
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
    /// a caller already holds.
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

impl PsSecureString {
    pub fn new(text: &str) -> PsResult<PsSecureString> {
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let status = crate::text::with_utf16(text, |u| unsafe { (vtable().securestring_new)(str16(u), &mut out, &mut err) });
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
    /// grow; the returned `String` is the caller's to keep short.
    pub fn reveal(&self) -> PsResult<String> {
        fn wipe(units: &mut [u16]) {
            for unit in units.iter_mut() {
                unsafe { core::ptr::write_volatile(unit, 0) };
            }
        }
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
            // holds what the read above wrote.
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
