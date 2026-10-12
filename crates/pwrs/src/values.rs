//! Value types with a CLR counterpart: dates, time spans, GUIDs and
//! characters. Each crosses through its own vtable entries and can
//! stand as a parameter, a class field, or an output.

use crate::class::PsTyped;
use crate::convert::conv_err;
use crate::host::vtable;
use crate::pipeline::check;
use crate::{ErrorCategory, FromPs, IntoPs, PsError, PsObject, PsResult};
use core::fmt;
use core::str::FromStr;
use pwrs_sys::{PsHandle, PsTypeTag, PS_OK, PS_TYPE_CHAR, PS_TYPE_DATETIME, PS_TYPE_DECIMAL, PS_TYPE_GUID, PS_TYPE_TIMESPAN};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Ticks per second; a tick is 100 ns.
pub const TICKS_PER_SECOND: i64 = 10_000_000;
/// Ticks in one minute, the unit a `DateTimeOffset` carries its UTC
/// offset in.
pub const TICKS_PER_MINUTE: i64 = 60 * TICKS_PER_SECOND;
/// `DateTime.UnixEpoch.Ticks`.
pub const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;
/// `DateTime.MaxValue.Ticks`.
pub const MAX_DATETIME_TICKS: i64 = 3_155_378_975_999_999_999;

fn invalid(message: impl Into<String>) -> PsError {
    PsError::new(ErrorCategory::InvalidData, "PwrsConversionError", message)
}

/// `System.DateTimeKind`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DateTimeKind {
    #[default]
    Unspecified = 0,
    Utc = 1,
    Local = 2,
}

impl DateTimeKind {
    fn from_u8(v: u8) -> PsResult<Self> {
        match v {
            0 => Ok(DateTimeKind::Unspecified),
            1 => Ok(DateTimeKind::Utc),
            2 => Ok(DateTimeKind::Local),
            other => Err(invalid(format!("{other} is not a DateTimeKind"))),
        }
    }
}

/// A `System.DateTime`: ticks of 100 ns from the start of year 1, and
/// the kind. The ticks of a `Local` or `Unspecified` value are wall
/// clock time in the engine's zone; [`PsDateTime::to_utc`] asks the
/// engine to shift them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PsDateTime {
    pub ticks: i64,
    pub kind: DateTimeKind,
}

impl PsDateTime {
    pub const fn new(ticks: i64, kind: DateTimeKind) -> Self {
        PsDateTime { ticks, kind }
    }

    pub const fn utc(ticks: i64) -> Self {
        PsDateTime { ticks, kind: DateTimeKind::Utc }
    }

    /// The engine's `ToUniversalTime()`: a `Utc` value as it is, a
    /// `Local` value shifted by the engine's zone, an `Unspecified`
    /// value treated as local.
    pub fn to_utc(&self) -> PsResult<PsDateTime> {
        if self.kind == DateTimeKind::Utc {
            return Ok(*self);
        }
        let obj = (*self).into_ps()?;
        PsDateTime::from_ps(&obj.call("ToUniversalTime", &[])?)
    }
}

/// Whole ticks in `d`; fails when they exceed `i64`.
fn duration_ticks(d: Duration) -> PsResult<i64> {
    i64::try_from(d.as_nanos() / 100).map_err(|overflow| invalid(format!("duration exceeds the TimeSpan range: {overflow}")))
}

impl TryFrom<SystemTime> for PsDateTime {
    type Error = PsError;

    /// A `Utc` value; precision below a tick is dropped. Fails past
    /// the year 9999.
    fn try_from(t: SystemTime) -> PsResult<Self> {
        let ticks = match t.duration_since(UNIX_EPOCH) {
            Ok(after) => UNIX_EPOCH_TICKS.checked_add(duration_ticks(after)?),
            Err(before) => UNIX_EPOCH_TICKS.checked_sub(duration_ticks(before.duration())?),
        };
        match ticks {
            Some(t) if (0..=MAX_DATETIME_TICKS).contains(&t) => Ok(PsDateTime::utc(t)),
            Some(t) => Err(invalid(format!("{t} ticks is outside the DateTime range"))),
            None => Err(invalid("system time is outside the DateTime range")),
        }
    }
}

impl TryFrom<PsDateTime> for SystemTime {
    type Error = PsError;

    /// Only a `Utc` value converts; `to_utc` first for the others.
    fn try_from(d: PsDateTime) -> PsResult<Self> {
        if d.kind != DateTimeKind::Utc {
            return Err(invalid("only a UTC DateTime converts to SystemTime; call to_utc first"));
        }
        let delta = d.ticks - UNIX_EPOCH_TICKS;
        let magnitude = delta.unsigned_abs();
        let span = Duration::new(magnitude / TICKS_PER_SECOND as u64, ((magnitude % TICKS_PER_SECOND as u64) * 100) as u32);
        let time = if delta >= 0 { UNIX_EPOCH.checked_add(span) } else { UNIX_EPOCH.checked_sub(span) };
        match time {
            Some(t) => Ok(t),
            None => Err(invalid(format!("{} ticks is outside the SystemTime range", d.ticks))),
        }
    }
}

impl PsTyped for PsDateTime {
    const CLR_NAME: &'static str = "System.DateTime";
    const VALUE_TYPE: bool = true;
}

impl FromPs for PsDateTime {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut ticks = 0i64;
        let mut kind = 0u8;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().datetime_read)(obj.as_raw(), &mut ticks, &mut kind, &mut err) };
        if s != PS_OK {
            return Err(conv_err("DateTime", err));
        }
        Ok(PsDateTime { ticks, kind: DateTimeKind::from_u8(kind)? })
    }
}

impl IntoPs for PsDateTime {
    const TYPE_TAG: PsTypeTag = PS_TYPE_DATETIME;
    fn into_ps(self) -> PsResult<PsObject> {
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().datetime_new)(self.ticks, self.kind as u8, &mut out, &mut err) };
        check(s, err)?;
        Ok(unsafe { PsObject::from_raw(out) })
    }
}

/// A `System.TimeSpan` in ticks of 100 ns; negative spans are
/// allowed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PsTimeSpan {
    pub ticks: i64,
}

impl PsTimeSpan {
    pub const fn from_ticks(ticks: i64) -> Self {
        PsTimeSpan { ticks }
    }

    pub const fn is_negative(&self) -> bool {
        self.ticks < 0
    }
}

impl TryFrom<Duration> for PsTimeSpan {
    type Error = PsError;

    /// Precision below a tick is dropped. Fails when the duration
    /// exceeds `TimeSpan.MaxValue`.
    fn try_from(d: Duration) -> PsResult<Self> {
        duration_ticks(d).map(PsTimeSpan::from_ticks)
    }
}

impl TryFrom<PsTimeSpan> for Duration {
    type Error = PsError;

    /// Fails for a negative span.
    fn try_from(t: PsTimeSpan) -> PsResult<Self> {
        if t.is_negative() {
            return Err(invalid(format!("{} ticks is negative and does not convert to Duration", t.ticks)));
        }
        let ticks = t.ticks as u64;
        Ok(Duration::new(ticks / TICKS_PER_SECOND as u64, ((ticks % TICKS_PER_SECOND as u64) * 100) as u32))
    }
}

impl PsTyped for PsTimeSpan {
    const CLR_NAME: &'static str = "System.TimeSpan";
    const VALUE_TYPE: bool = true;
}

impl FromPs for PsTimeSpan {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut ticks = 0i64;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().timespan_read)(obj.as_raw(), &mut ticks, &mut err) };
        if s == PS_OK { Ok(PsTimeSpan { ticks }) } else { Err(conv_err("TimeSpan", err)) }
    }
}

impl IntoPs for PsTimeSpan {
    const TYPE_TAG: PsTypeTag = PS_TYPE_TIMESPAN;
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(unsafe { PsObject::from_raw((vtable().timespan_new)(self.ticks)) })
    }
}

/// A `System.Decimal` as the four words `Decimal.GetBits` answers:
/// `lo`, `mid`, `hi` and `flags`, where `flags` carries the sign in
/// bit 31 and the scale, 0 to 28, in bits 16 to 23.
///
/// This is the documented order, not the order the value has in
/// memory. A pinned `Decimal[]` hands back memory order instead; see
/// [`PsDecimalBits`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PsDecimal {
    pub lo: i32,
    pub mid: i32,
    pub hi: i32,
    pub flags: i32,
}

impl PsDecimal {
    /// The four words in `Decimal.GetBits` order.
    pub fn from_bits(lo: i32, mid: i32, hi: i32, flags: i32) -> Self {
        PsDecimal { lo, mid, hi, flags }
    }

    /// True when the sign bit is set. A negative zero is possible and
    /// compares equal to zero in the engine but not in this type.
    pub fn is_negative(&self) -> bool {
        self.flags < 0
    }

    /// The scale, 0 to 28: how many digits sit after the point.
    pub fn scale(&self) -> u8 {
        ((self.flags >> 16) & 0xFF) as u8
    }
}

impl PsTyped for PsDecimal {
    const CLR_NAME: &'static str = "System.Decimal";
    const VALUE_TYPE: bool = true;
}

impl FromPs for PsDecimal {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut bits = [0i32; 4];
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().decimal_read)(obj.as_raw(), bits.as_mut_ptr(), &mut err) };
        if s == PS_OK {
            Ok(PsDecimal { lo: bits[0], mid: bits[1], hi: bits[2], flags: bits[3] })
        } else {
            Err(conv_err("Decimal", err))
        }
    }
}

impl IntoPs for PsDecimal {
    const TYPE_TAG: PsTypeTag = PS_TYPE_DECIMAL;
    fn into_ps(self) -> PsResult<PsObject> {
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let s = unsafe {
            (vtable().decimal_new)(self.lo, self.mid, self.hi, self.flags, &mut out, &mut err)
        };
        if s == PS_OK { Ok(unsafe { PsObject::from_raw(out) }) } else { Err(conv_err("Decimal", err)) }
    }
}

/// One `System.Decimal` as it sits in memory, for pinning a
/// `Decimal[]`: `flags`, `hi`, `lo`, `mid`, which is
/// [`PsDecimal`]'s order permuted `[3], [2], [0], [1]`.
///
/// Measured on x64 Windows under .NET 10 and .NET Framework 4.8 and
/// identical on both. The layout is runtime-internal on every target,
/// so [`crate::init`] asserts it once per process against
/// `Decimal.GetBits` and refuses to run where it does not hold,
/// rather than returning reinterpreted words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct PsDecimalBits {
    pub flags: i32,
    pub hi: i32,
    pub lo: i32,
    pub mid: i32,
}

impl From<PsDecimalBits> for PsDecimal {
    fn from(b: PsDecimalBits) -> Self {
        PsDecimal { lo: b.lo, mid: b.mid, hi: b.hi, flags: b.flags }
    }
}

impl From<PsDecimal> for PsDecimalBits {
    fn from(d: PsDecimal) -> Self {
        PsDecimalBits { flags: d.flags, hi: d.hi, lo: d.lo, mid: d.mid }
    }
}

impl PsTyped for PsDecimalBits {
    const CLR_NAME: &'static str = "System.Decimal";
    const VALUE_TYPE: bool = true;
}

impl FromPs for PsDecimalBits {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        PsDecimal::from_ps(obj).map(Into::into)
    }
}

impl IntoPs for PsDecimalBits {
    /// The tag `pin` and `from_slice` match against, so a
    /// `Decimal[]` is the array this element builds and pins.
    const TYPE_TAG: PsTypeTag = PS_TYPE_DECIMAL;
    fn into_ps(self) -> PsResult<PsObject> {
        PsDecimal::from(self).into_ps()
    }
}

/// A `System.DateTimeOffset`: ticks of 100 ns from the start of year
/// 1, and the offset from UTC in whole minutes.
///
/// Distinct from [`PsDateTime`], whose `kind` says only which clock a
/// value belongs to. An offset names the displacement itself, so a
/// value keeps its meaning away from the host that produced it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PsDateTimeOffset {
    pub ticks: i64,
    pub offset_minutes: i16,
}

impl PsDateTimeOffset {
    pub fn new(ticks: i64, offset_minutes: i16) -> Self {
        PsDateTimeOffset { ticks, offset_minutes }
    }

    /// The same instant as ticks on the UTC clock.
    pub fn to_utc_ticks(&self) -> i64 {
        self.ticks - i64::from(self.offset_minutes) * TICKS_PER_MINUTE
    }
}

impl PsTyped for PsDateTimeOffset {
    const CLR_NAME: &'static str = "System.DateTimeOffset";
    const VALUE_TYPE: bool = true;
}

impl FromPs for PsDateTimeOffset {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut ticks = 0i64;
        let mut offset_minutes = 0i16;
        let mut err = PsHandle::NULL;
        let s = unsafe {
            (vtable().datetimeoffset_read)(obj.as_raw(), &mut ticks, &mut offset_minutes, &mut err)
        };
        if s == PS_OK {
            Ok(PsDateTimeOffset { ticks, offset_minutes })
        } else {
            Err(conv_err("DateTimeOffset", err))
        }
    }
}

impl IntoPs for PsDateTimeOffset {
    fn into_ps(self) -> PsResult<PsObject> {
        let mut out = PsHandle::NULL;
        let mut err = PsHandle::NULL;
        let s = unsafe {
            (vtable().datetimeoffset_new)(self.ticks, self.offset_minutes, &mut out, &mut err)
        };
        if s == PS_OK {
            Ok(unsafe { PsObject::from_raw(out) })
        } else {
            Err(conv_err("DateTimeOffset", err))
        }
    }
}

/// A `System.Management.Automation.ErrorRecord` read by its parts: the
/// category, the fully qualified error id, the exception's message,
/// and the target object, which is `$null` when the record carries
/// none. A record the pipeline hands a cmdlet and one caught in
/// script and passed in read the same way.
#[derive(Clone, Debug)]
pub struct PsErrorRecord {
    pub category: ErrorCategory,
    pub error_id: String,
    pub message: String,
    pub target: PsObject,
}

impl PsTyped for PsErrorRecord {
    const CLR_NAME: &'static str = "System.Management.Automation.ErrorRecord";
    const VALUE_TYPE: bool = false;
}

impl FromPs for PsErrorRecord {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let code = i64::from_ps(&obj.get("CategoryInfo")?.get("Category")?)?;
        let category = u32::try_from(code)
            .ok()
            .and_then(ErrorCategory::from_code)
            .ok_or_else(|| PsError::new(ErrorCategory::InvalidData, "PwrsErrorCategory", format!("{code} is not an ErrorCategory")))?;
        Ok(PsErrorRecord {
            category,
            error_id: String::from_ps(&obj.get("FullyQualifiedErrorId")?)?,
            message: String::from_ps(&obj.get("Exception")?.get("Message")?)?,
            target: obj.get("TargetObject")?,
        })
    }
}

/// A `System.Guid` as the 16 bytes `Guid.ToByteArray` produces: the
/// first three fields little-endian, the last eight bytes in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PsGuid {
    pub bytes: [u8; 16],
}

fn swap_fields(b: [u8; 16]) -> [u8; 16] {
    [b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]
}

impl PsGuid {
    pub const NIL: PsGuid = PsGuid { bytes: [0; 16] };

    /// The bytes in RFC 4122 order, every field big-endian: the order
    /// of the textual form and of the `uuid` crate.
    pub fn to_rfc4122(&self) -> [u8; 16] {
        swap_fields(self.bytes)
    }

    pub fn from_rfc4122(bytes: [u8; 16]) -> PsGuid {
        PsGuid { bytes: swap_fields(bytes) }
    }
}

impl fmt::Display for PsGuid {
    /// The `D` format: 32 lowercase hex digits in five hyphenated
    /// groups.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.to_rfc4122().iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for PsGuid {
    type Err = PsError;

    /// The `N`, `D`, `B` and `P` formats: 32 hex digits, with or
    /// without the four hyphens, optionally in braces or parentheses.
    fn from_str(s: &str) -> PsResult<Self> {
        let bad = || invalid(format!("{s:?} is not a GUID"));
        let trimmed = s.trim();
        let inner = match (trimmed.as_bytes().first(), trimmed.as_bytes().last()) {
            (Some(b'{'), Some(b'}')) | (Some(b'('), Some(b')')) => &trimmed[1..trimmed.len() - 1],
            (Some(_open), Some(_close)) => trimmed,
            (None, _) | (_, None) => return Err(bad()),
        };
        let mut out = [0u8; 16];
        let mut digits = 0usize;
        let mut after_hyphen = false;
        for ch in inner.chars() {
            if ch == '-' {
                if after_hyphen || !matches!(digits, 8 | 12 | 16 | 20) {
                    return Err(bad());
                }
                after_hyphen = true;
                continue;
            }
            after_hyphen = false;
            let v = ch.to_digit(16).ok_or_else(bad)? as u8;
            if digits == 32 {
                return Err(bad());
            }
            let byte = &mut out[digits / 2];
            *byte = if digits.is_multiple_of(2) { v << 4 } else { *byte | v };
            digits += 1;
        }
        if digits != 32 || after_hyphen {
            return Err(bad());
        }
        Ok(PsGuid::from_rfc4122(out))
    }
}

impl PsTyped for PsGuid {
    const CLR_NAME: &'static str = "System.Guid";
    const VALUE_TYPE: bool = true;
}

impl FromPs for PsGuid {
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut bytes = [0u8; 16];
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().guid_read)(obj.as_raw(), bytes.as_mut_ptr(), &mut err) };
        if s == PS_OK { Ok(PsGuid { bytes }) } else { Err(conv_err("Guid", err)) }
    }
}

impl IntoPs for PsGuid {
    const TYPE_TAG: PsTypeTag = PS_TYPE_GUID;
    fn into_ps(self) -> PsResult<PsObject> {
        Ok(unsafe { PsObject::from_raw((vtable().guid_new)(self.bytes.as_ptr())) })
    }
}

impl PsTyped for char {
    const CLR_NAME: &'static str = "char";
    const VALUE_TYPE: bool = true;
}

impl FromPs for char {
    /// A CLR `char` is one UTF-16 unit; a lone surrogate is an error.
    fn from_ps(obj: &PsObject) -> PsResult<Self> {
        let mut unit = 0u16;
        let mut err = PsHandle::NULL;
        let s = unsafe { (vtable().char_read)(obj.as_raw(), &mut unit, &mut err) };
        if s != PS_OK {
            return Err(conv_err("Char", err));
        }
        match char::from_u32(unit as u32) {
            Some(c) => Ok(c),
            None => Err(invalid(format!("U+{unit:04X} is a surrogate, not a character"))),
        }
    }
}

impl IntoPs for char {
    const TYPE_TAG: PsTypeTag = PS_TYPE_CHAR;
    /// Fails for a character outside the Basic Multilingual Plane,
    /// which needs two UTF-16 units.
    fn into_ps(self) -> PsResult<PsObject> {
        let unit = u16::try_from(self as u32).map_err(|overflow| invalid(format!("{self:?} does not fit one UTF-16 unit: {overflow}")))?;
        Ok(unsafe { PsObject::from_raw((vtable().char_new)(unit)) })
    }
}

/// A value of a CLR enum this module did not declare, from its
/// underlying number: `enum_value("System.ConsoleColor", 12)` is
/// `[System.ConsoleColor]::Red`.
///
/// The name is resolved the way the engine resolves a type literal,
/// so anything `[System.ConsoleColor]` reaches in script is reachable
/// here, and a name it cannot resolve is the error. A module's own
/// `#[psenum]` types do not come through here: they have a class id
/// and go through the factory, which needs no name.
///
/// `#[psenum(clr = "...")]` calls this, so a mirror enum needs no
/// hand-written `IntoPs`.
pub fn enum_value(type_name: &str, value: i64) -> PsResult<PsObject> {
    let name = crate::pipeline::utf16(type_name);
    let mut out = PsHandle::NULL;
    let mut err = PsHandle::NULL;
    let s = unsafe { (vtable().enum_new)(crate::pipeline::str16(&name), value, &mut out, &mut err) };
    if s == PS_OK {
        Ok(unsafe { PsObject::from_raw(out) })
    } else {
        Err(conv_err(type_name, err))
    }
}
