//! Toml, the toml crate ported to PowerShell: ConvertFrom-Toml reads TOML
//! text into objects, and ConvertTo-Toml writes objects as TOML text.

use pwrs::object::{add_note, new_psobject};
use pwrs::prelude::*;
use pwrs::sys::{
    PS_TYPE_BOOL, PS_TYPE_CHAR, PS_TYPE_DATETIME, PS_TYPE_DECIMAL, PS_TYPE_F32, PS_TYPE_F64, PS_TYPE_GUID, PS_TYPE_I16, PS_TYPE_I32, PS_TYPE_I64, PS_TYPE_I8,
    PS_TYPE_OBJECT, PS_TYPE_STRING, PS_TYPE_TIMESPAN, PS_TYPE_U16, PS_TYPE_U32, PS_TYPE_U64, PS_TYPE_U8,
};
use pwrs::values::{MAX_DATETIME_TICKS, TICKS_PER_MINUTE, TICKS_PER_SECOND};
use std::collections::HashMap;
use toml::value::{Date, Datetime, Offset, Time};
use toml::{Table, Value};

/// Ticks in one day; a tick is 100 ns.
const TICKS_PER_DAY: i64 = 86_400 * TICKS_PER_SECOND;

/// The largest offset from UTC that a DateTimeOffset allows, in minutes.
const MAX_OFFSET_MINUTES: u16 = 14 * 60;

/// How deep ConvertTo-Toml nests tables and arrays when -Depth is absent.
const DEFAULT_DEPTH: i64 = 32;

/// Reads TOML text into objects.
///
/// Each table becomes a PSCustomObject whose properties are its keys, in
/// the order the text gives them, or an ordered hashtable under
/// -AsHashtable. A string becomes a String, a Boolean a Boolean, an
/// integer an Int64, a float a Double and an array an object array. An
/// offset date-time becomes a DateTimeOffset, a local date-time a
/// DateTime, a local date a DateTime at midnight and a local time a
/// TimeSpan.
///
/// Each string it is given is one line of a single document, so a file
/// read line by line with Get-Content is read as the whole file. When it
/// is given no text at all, it writes nothing. Text that is not TOML ends
/// the command with an error giving the line and column.
///
/// # Examples
/// Get-Content Cargo.toml | ConvertFrom-Toml
/// (Get-Content Cargo.toml -Raw | ConvertFrom-Toml).package.version
/// ConvertFrom-Toml "name = 'demo'" -AsHashtable
#[cmdlet(verb = "ConvertFrom", noun = "Toml", output = ["System.Management.Automation.PSCustomObject", "System.Collections.Specialized.OrderedDictionary"])]
#[derive(Default)]
pub struct ConvertFromToml {
    /// TOML text, or one line of it.
    #[param(mandatory, position = 0, value_from_pipeline, allow_empty_string)]
    pub input_object: String,
    /// Returns each table as an ordered hashtable rather than a
    /// PSCustomObject. Its keys keep their case, so a table with two keys
    /// that differ only in case can be read this way.
    #[param]
    pub as_hashtable: bool,
    /// The lines given so far, read as one document at the end.
    lines: Vec<String>,
}

impl Cmdlet for ConvertFromToml {
    fn process(&mut self, _ps: &Pipeline<'_>) -> PsResult<()> {
        self.lines.push(std::mem::take(&mut self.input_object));
        Ok(())
    }

    fn end(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        if self.lines.is_empty() {
            return Ok(());
        }
        let text = self.lines.join("\n");
        let table: Table = text.parse().map_err(|e| parse_error(&text, &e))?;
        let reader = Reader { as_hashtable: self.as_hashtable };
        ps.write(reader.table(table, "").map_err(PsError::terminating)?)
    }
}

/// The error for text the toml crate cannot read, which ends the command:
/// the line and column where it stopped and what it found there. The
/// crate's own report, which draws the line and marks the place, is the
/// error's details.
fn parse_error(text: &str, error: &toml::de::Error) -> PsError {
    let place = match error.span() {
        Some(span) => {
            let (line, column) = line_and_column(text, span.start);
            format!(" at line {line}, column {column}")
        }
        None => String::new(),
    };
    PsError::new(ErrorCategory::ParserError, "TomlParseError", format!("TOML parse error{place}: {}", error.message()))
        .with_details(error.to_string())
        .terminating()
}

/// The line and column of byte `at` of `text`, both counted from 1.
/// Columns count characters, not bytes.
fn line_and_column(text: &str, at: usize) -> (usize, usize) {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let before = &text[..at];
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    (before.matches('\n').count() + 1, before[line_start..].chars().count() + 1)
}

/// How ConvertFrom-Toml returns a table: as a PSCustomObject, or as an
/// ordered hashtable under -AsHashtable.
struct Reader {
    as_hashtable: bool,
}

impl Reader {
    /// The object or ordered hashtable `table` becomes. `path` names the
    /// table in an error, and is empty for the document's own table.
    fn table(&self, table: Table, path: &str) -> PsResult<PsObject> {
        if self.as_hashtable {
            let hashtable = PsHashtable(PsType::from_name("System.Collections.Specialized.OrderedDictionary").new(&[])?);
            for (key, value) in table {
                let item = self.value(value, &key_path(path, &key))?;
                hashtable.set(&key, item)?;
            }
            return Ok(hashtable.0);
        }
        let object = new_psobject("");
        // PowerShell compares property names without regard to case, so
        // each key is stored here in lower case, to find two keys that
        // differ only in case.
        let mut seen = HashMap::new();
        for (key, value) in table {
            let here = key_path(path, &key);
            if key.is_empty() {
                return Err(PsError::new(ErrorCategory::InvalidData, "TomlEmptyKey", format!("{here} is an empty key, and a property's name cannot be empty; use -AsHashtable")));
            }
            if let Some(earlier) = seen.insert(key.to_lowercase(), key.clone()) {
                let earlier = key_path(path, &earlier);
                return Err(PsError::new(ErrorCategory::InvalidData, "TomlKeysDifferInCase", format!("{earlier} and {here} differ only in case, but property names must differ by more than case; use -AsHashtable")));
            }
            add_note(&object, &key, self.value(value, &here)?)?;
        }
        Ok(object)
    }

    /// The PowerShell value of `value`, found at `path`.
    fn value(&self, value: Value, path: &str) -> PsResult<PsObject> {
        match value {
            Value::String(text) => text.into_ps(),
            Value::Integer(number) => number.into_ps(),
            Value::Float(number) => number.into_ps(),
            Value::Boolean(flag) => flag.into_ps(),
            Value::Datetime(datetime) => datetime_object(&datetime, path),
            Value::Array(items) => {
                let mut objects = Vec::with_capacity(items.len());
                for (index, item) in items.into_iter().enumerate() {
                    objects.push(self.value(item, &format!("{path}[{index}]"))?);
                }
                objects.into_ps()
            }
            Value::Table(table) => self.table(table, path),
        }
    }
}

/// The .NET value of a TOML date-time: a DateTimeOffset for an offset
/// date-time, a DateTime for a local date-time or a local date, which is
/// at midnight, and a TimeSpan for a local time.
fn datetime_object(datetime: &Datetime, path: &str) -> PsResult<PsObject> {
    let time = match &datetime.time {
        Some(time) => time_ticks(time, path)?,
        None => 0,
    };
    let Some(date) = &datetime.date else {
        return PsTimeSpan::from_ticks(time).into_ps();
    };
    let ticks = date_ticks(date, path)? + time;
    let Some(offset) = datetime.offset else {
        return PsDateTime::new(ticks, DateTimeKind::Unspecified).into_ps();
    };
    let minutes = match offset {
        Offset::Z => 0,
        Offset::Custom { minutes } => minutes,
    };
    let utc = ticks - i64::from(minutes) * TICKS_PER_MINUTE;
    if minutes.unsigned_abs() > MAX_OFFSET_MINUTES || !(0..=MAX_DATETIME_TICKS).contains(&utc) {
        return Err(PsError::new(
            ErrorCategory::InvalidData,
            "TomlDateOutOfRange",
            format!("{path} is {datetime}, and a DateTimeOffset allows an offset of at most 14 hours and a time in UTC from year 1 to year 9999"),
        ));
    }
    PsDateTimeOffset::new(ticks, minutes).into_ps()
}

/// Ticks from midnight to a TOML time. A tick is 100 ns, so digits past
/// the seventh decimal place of a second are dropped, as the TOML
/// specification requires when a reader stores less precision.
fn time_ticks(time: &Time, path: &str) -> PsResult<i64> {
    let second = time.second.unwrap_or(0);
    if second == 60 {
        return Err(PsError::new(ErrorCategory::InvalidData, "TomlDateOutOfRange", format!("{path} is {time}, a leap second, which .NET dates and times cannot store")));
    }
    let seconds = (i64::from(time.hour) * 60 + i64::from(time.minute)) * 60 + i64::from(second);
    Ok(seconds * TICKS_PER_SECOND + i64::from(time.nanosecond.unwrap_or(0) / 100))
}

/// Ticks from the start of year 1 to the start of a TOML date.
fn date_ticks(date: &Date, path: &str) -> PsResult<i64> {
    if date.year == 0 {
        return Err(PsError::new(ErrorCategory::InvalidData, "TomlDateOutOfRange", format!("{path} is {date}, in year 0, and a DateTime starts at year 1")));
    }
    Ok(days_from_civil(i64::from(date.year), date.month, date.day) * TICKS_PER_DAY)
}

/// Days from January 1 of year 1 to a date of the Gregorian calendar,
/// which DateTime extends back to year 1. The year is counted from March,
/// so a leap day is the last day of a year, and every 400 years have
/// 146,097 days.
/// Day 0 of that count is March 1 of year 0, 306 days before January 1 of
/// year 1.
fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * ((i64::from(month) + 9) % 12) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 306
}

/// The date `days` after January 1 of year 1, the inverse of
/// `days_from_civil`, for a day DateTime holds.
fn civil_from_days(days: i64) -> (u16, u8, u8) {
    let days = days + 306;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 { month_from_march + 3 } else { month_from_march - 9 };
    let year = era * 400 + year_of_era + i64::from(month <= 2);
    (year as u16, month as u8, day as u8)
}

/// Writes objects as TOML text.
///
/// Each object it is given becomes one TOML document, written as one
/// string. A document is a table, so the object must be a hashtable or
/// other dictionary, whose keys must be strings, or an object with
/// properties. Its keys or properties become the document's keys in their
/// order, and the tables, objects and arrays inside it become TOML tables
/// and arrays.
///
/// Strings and characters are written as strings, Booleans as Booleans,
/// integers as integers, and Single, Double and Decimal values as floats.
/// A DateTimeOffset is written as an offset date-time, a DateTime whose
/// Kind is Utc as an offset date-time in UTC, any other DateTime as a
/// local date-time, and a TimeSpan of less than a day as a local time. An
/// enum is written as its name and a GUID as text.
///
/// TOML has no null, so a key whose value is $null is an error naming the
/// key, unless -SkipNull is given, in which case such keys are left out.
/// A $null in an array is an error either way. A table or array nested
/// deeper than -Depth is an error naming where it is; nothing is cut
/// short.
///
/// # Examples
/// [ordered]@{ name = 'demo'; version = '1.0.0' } | ConvertTo-Toml
/// [ordered]@{ package = [ordered]@{ name = 'demo'; edition = '2024' } } | ConvertTo-Toml
/// Get-Content Cargo.toml -Raw | ConvertFrom-Toml | ConvertTo-Toml
#[cmdlet(verb = "ConvertTo", noun = "Toml", output = ["System.String"])]
#[derive(Default)]
pub struct ConvertToToml {
    /// The object to write: a hashtable or other dictionary, or an object
    /// with properties.
    #[param(mandatory, position = 0, value_from_pipeline, allow_null)]
    pub input_object: PsObject,
    /// How many levels of tables and arrays may be nested inside the
    /// document's own table, from 0 to 100; 32 when absent.
    #[param(validate_range(0, 100))]
    pub depth: Option<i64>,
    /// Leaves out each key whose value is $null, which is otherwise an
    /// error.
    #[param]
    pub skip_null: bool,
}

impl Cmdlet for ConvertToToml {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let writer = Writer { depth: self.depth.unwrap_or(DEFAULT_DEPTH), skip_null: self.skip_null };
        let table = writer.document(&self.input_object)?;
        let text = toml::to_string(&table).map_err(|e| PsError::new(ErrorCategory::InvalidData, "TomlWriteError", e.to_string()))?;
        ps.write(text.trim_end_matches('\n'))
    }
}

/// How ConvertTo-Toml reads a PowerShell value: how deep tables and
/// arrays may nest, and whether a key whose value is $null is left out.
struct Writer {
    depth: i64,
    skip_null: bool,
}

/// What an object with no type tag is to ConvertTo-Toml.
enum Kind {
    /// A DateTimeOffset, written as an offset date-time.
    DateTimeOffset,
    /// A BigInteger, written as an integer when it fits one.
    BigInteger,
    /// An enum, written as its name.
    Enum,
    /// A dictionary, written as a table of its keys.
    Dictionary,
    /// Any other collection, written as an array.
    Collection,
    /// Any other object, written as a table of its properties.
    Properties,
}

/// What `value`, an object with no type tag, is to ConvertTo-Toml.
fn kind_of(value: &PsObject) -> PsResult<Kind> {
    let class = value.call("GetType", &[])?;
    Ok(match String::from_ps(&class.get("FullName")?)?.as_str() {
        "System.DateTimeOffset" => Kind::DateTimeOffset,
        "System.Numerics.BigInteger" => Kind::BigInteger,
        _ if bool::from_ps(&class.get("IsEnum")?)? => Kind::Enum,
        _ if implements(&class, "System.Collections.IDictionary")? => Kind::Dictionary,
        _ if implements(&class, "System.Collections.IEnumerable")? => Kind::Collection,
        _ => Kind::Properties,
    })
}

/// Whether `class`, a System.Type, implements the interface named `name`.
fn implements(class: &PsObject, name: &str) -> PsResult<bool> {
    Ok(!class.call("GetInterface", &[name.into_ps()?])?.is_null())
}

impl Writer {
    /// The document `input` makes, which must be a table.
    fn document(&self, input: &PsObject) -> PsResult<Table> {
        if !input.is_null() && input.type_tag()? == PS_TYPE_OBJECT {
            match kind_of(input)? {
                Kind::Dictionary => return self.dictionary(input, "", 0),
                Kind::Properties => return self.properties(input, "", 0),
                Kind::DateTimeOffset | Kind::BigInteger | Kind::Enum | Kind::Collection => {}
            }
        }
        let what = if input.is_null() { "$null".to_string() } else { format!("a {}", input.type_name()?) };
        Err(PsError::new(ErrorCategory::InvalidArgument, "TomlNotATable", format!("a TOML document is a table, and {what} is not one; give a hashtable or an object with properties")))
    }

    /// The TOML value of `value`, found at `path`, `level` tables or arrays
    /// below the document's own table.
    fn value(&self, value: &PsObject, path: &str, level: i64) -> PsResult<Value> {
        Ok(match value.type_tag()? {
            PS_TYPE_STRING => Value::String(String::from_ps(value)?),
            PS_TYPE_CHAR => Value::String(char::from_ps(value)?.to_string()),
            PS_TYPE_BOOL => Value::Boolean(bool::from_ps(value)?),
            PS_TYPE_I8 | PS_TYPE_I16 | PS_TYPE_I32 | PS_TYPE_I64 | PS_TYPE_U8 | PS_TYPE_U16 | PS_TYPE_U32 => Value::Integer(i64::from_ps(value)?),
            PS_TYPE_U64 => {
                let number = u64::from_ps(value)?;
                match i64::try_from(number) {
                    Ok(number) => Value::Integer(number),
                    Err(too_big) => return Err(beyond_integers(path, number).with_details(too_big.to_string())),
                }
            }
            PS_TYPE_F32 => Value::Float(single_as_double(f32::from_ps(value)?)?),
            PS_TYPE_F64 | PS_TYPE_DECIMAL => Value::Float(f64::from_ps(value)?),
            PS_TYPE_DATETIME => datetime_value(PsDateTime::from_ps(value)?),
            PS_TYPE_TIMESPAN => time_value(value, path)?,
            PS_TYPE_GUID => Value::String(PsGuid::from_ps(value)?.to_string()),
            _ => match kind_of(value)? {
                Kind::DateTimeOffset => offset_value(PsDateTimeOffset::from_ps(value)?),
                Kind::BigInteger => match i64::from_ps(value) {
                    Ok(number) => Value::Integer(number),
                    Err(too_big) => return Err(beyond_integers(path, String::from_ps(value)?).with_details(too_big.message)),
                },
                Kind::Enum => Value::String(String::from_ps(value)?),
                Kind::Dictionary => Value::Table(self.dictionary(value, path, level)?),
                Kind::Collection => Value::Array(self.array(value, path, level)?),
                Kind::Properties => Value::Table(self.properties(value, path, level)?),
            },
        })
    }

    /// The table a dictionary makes, with its keys in the dictionary's
    /// order. Each key must be a string.
    fn dictionary(&self, value: &PsObject, path: &str, level: i64) -> PsResult<Table> {
        self.check_depth(path, level)?;
        let dictionary = PsHashtable::from_ps(value)?;
        let mut table = Table::new();
        for key in Vec::<PsObject>::from_ps(&value.get("Keys")?)? {
            if key.type_tag()? != PS_TYPE_STRING {
                let place = if path.is_empty() { "the document" } else { path };
                return Err(PsError::new(
                    ErrorCategory::InvalidData,
                    "TomlKeyNotText",
                    format!("{place} has the key {}, a {}, and a TOML key is a string", String::from_ps(&key)?, key.type_name()?),
                ));
            }
            let name = String::from_ps(&key)?;
            let item = dictionary.get(&name)?;
            self.add(&mut table, name, &item, path, level)?;
        }
        Ok(table)
    }

    /// The table an object's properties make, in the order PowerShell
    /// lists them.
    fn properties(&self, value: &PsObject, path: &str, level: i64) -> PsResult<Table> {
        self.check_depth(path, level)?;
        let mut table = Table::new();
        for property in Vec::<PsObject>::from_ps(&value.get("PSObject")?.get("Properties")?)? {
            let name = String::from_ps(&property.get("Name")?)?;
            self.add(&mut table, name, &property.get("Value")?, path, level)?;
        }
        Ok(table)
    }

    /// The array a collection makes, with its items in order.
    fn array(&self, value: &PsObject, path: &str, level: i64) -> PsResult<Vec<Value>> {
        self.check_depth(path, level)?;
        let items = Vec::<PsObject>::from_ps(value)?;
        let mut array = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let here = format!("{path}[{index}]");
            if item.is_null() {
                return Err(PsError::new(ErrorCategory::InvalidData, "TomlNullValue", format!("{here} is $null, and TOML has no null to put in an array")));
            }
            array.push(self.value(item, &here, level + 1)?);
        }
        Ok(array)
    }

    /// Adds `key` to `table` with the TOML value of `item`. A $null item is
    /// left out under -SkipNull and is an error otherwise.
    fn add(&self, table: &mut Table, key: String, item: &PsObject, path: &str, level: i64) -> PsResult<()> {
        let here = key_path(path, &key);
        if item.is_null() {
            if self.skip_null {
                return Ok(());
            }
            return Err(PsError::new(ErrorCategory::InvalidData, "TomlNullValue", format!("{here} is $null, and TOML has no null; -SkipNull leaves such keys out")));
        }
        let value = self.value(item, &here, level + 1)?;
        table.insert(key, value);
        Ok(())
    }

    /// Refuses a table or array `level` levels below the document's own
    /// table when -Depth allows fewer.
    fn check_depth(&self, path: &str, level: i64) -> PsResult<()> {
        if level <= self.depth {
            return Ok(());
        }
        Err(PsError::new(ErrorCategory::LimitsExceeded, "TomlDepthExceeded", format!("{path} is nested {level} levels deep, and -Depth {} allows fewer", self.depth)))
    }
}

/// The error for an integer outside the range of a TOML integer, which is
/// a signed 64-bit number.
fn beyond_integers(path: &str, number: impl std::fmt::Display) -> PsError {
    PsError::new(ErrorCategory::InvalidData, "TomlIntegerRange", format!("{path} is {number}, and a TOML integer must be from -9223372036854775808 to 9223372036854775807"))
}

/// A Single as the Double with the same shortest decimal form, so 0.1 is
/// written as 0.1 rather than as the Double nearest the Single, which is
/// 0.10000000149011612.
fn single_as_double(number: f32) -> PsResult<f64> {
    let text = number.to_string();
    text.parse().map_err(|e| PsError::new(ErrorCategory::InvalidData, "TomlFloat", format!("{text} does not read back as a float: {e}")))
}

/// A DateTime as a TOML date-time: an offset date-time in UTC when its
/// Kind is Utc, and a local date-time otherwise.
fn datetime_value(datetime: PsDateTime) -> Value {
    let (date, time) = date_and_time(datetime.ticks);
    let offset = (datetime.kind == DateTimeKind::Utc).then_some(Offset::Z);
    Value::Datetime(Datetime { date: Some(date), time: Some(time), offset })
}

/// A DateTimeOffset as a TOML offset date-time; an offset of zero is
/// written as Z.
fn offset_value(datetime: PsDateTimeOffset) -> Value {
    let (date, time) = date_and_time(datetime.ticks);
    let offset = match datetime.offset_minutes {
        0 => Offset::Z,
        minutes => Offset::Custom { minutes },
    };
    Value::Datetime(Datetime { date: Some(date), time: Some(time), offset: Some(offset) })
}

/// A TimeSpan as a TOML local time, which is a time of day.
fn time_value(value: &PsObject, path: &str) -> PsResult<Value> {
    let span = PsTimeSpan::from_ps(value)?;
    if !(0..TICKS_PER_DAY).contains(&span.ticks) {
        return Err(PsError::new(
            ErrorCategory::InvalidData,
            "TomlTimeOutOfRange",
            format!("{path} is {}, and a TOML local time is a time of day, at least 00:00:00 and less than 24 hours", String::from_ps(value)?),
        ));
    }
    Ok(Value::Datetime(Datetime { date: None, time: Some(time_of_day(span.ticks)), offset: None }))
}

/// The date and time of day `ticks` after the start of year 1.
fn date_and_time(ticks: i64) -> (Date, Time) {
    let (year, month, day) = civil_from_days(ticks.div_euclid(TICKS_PER_DAY));
    (Date { year, month, day }, time_of_day(ticks.rem_euclid(TICKS_PER_DAY)))
}

/// The time `ticks` after midnight, less than a day. The seconds are
/// always written, and a fraction only when there is one.
fn time_of_day(ticks: i64) -> Time {
    let seconds = ticks / TICKS_PER_SECOND;
    let fraction = ticks % TICKS_PER_SECOND;
    Time {
        hour: (seconds / 3600) as u8,
        minute: (seconds / 60 % 60) as u8,
        second: Some((seconds % 60) as u8),
        nanosecond: (fraction != 0).then_some((fraction * 100) as u32),
    }
}

/// `path` followed by `key`, as a dotted TOML key. A key with any
/// character other than a letter, digit, underscore or hyphen is quoted.
fn key_path(path: &str, key: &str) -> String {
    let bare = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let key = if bare { key.to_string() } else { format!("{key:?}") };
    if path.is_empty() { key } else { format!("{path}.{key}") }
}

pwrs::export_module! {
    name: "Toml",
    cmdlets: [ConvertFromToml, ConvertToToml],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_count_from_the_first_day_of_year_one() {
        assert_eq!(days_from_civil(1, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 1) * TICKS_PER_DAY, pwrs::values::UNIX_EPOCH_TICKS);
        assert_eq!(days_from_civil(9999, 12, 31), MAX_DATETIME_TICKS / TICKS_PER_DAY);
        assert_eq!(days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28), 2, "2000 is a leap year");
        assert_eq!(days_from_civil(1900, 3, 1) - days_from_civil(1900, 2, 28), 1, "1900 is not");
    }

    #[test]
    fn every_day_from_year_1_to_9999_reads_back() {
        for days in 0..=MAX_DATETIME_TICKS / TICKS_PER_DAY {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(i64::from(year), month, day), days, "{year}-{month}-{day}");
        }
    }

    #[test]
    fn a_time_is_ticks_from_midnight_and_reads_back() {
        let time = Time { hour: 7, minute: 32, second: Some(0), nanosecond: Some(999_999_999) };
        let ticks = time_ticks(&time, "t").expect("a time of day");
        assert_eq!(ticks, (7 * 60 + 32) * 60 * TICKS_PER_SECOND + 9_999_999);
        assert_eq!(time_of_day(ticks).to_string(), "07:32:00.9999999");
        assert_eq!(time_of_day(0).to_string(), "00:00:00");
    }

    #[test]
    fn a_leap_second_and_year_0_have_no_dotnet_value() {
        let leap = Time { hour: 23, minute: 59, second: Some(60), nanosecond: None };
        assert_eq!(time_ticks(&leap, "t").expect_err("a leap second").error_id, "TomlDateOutOfRange");
        let year_0 = Date { year: 0, month: 1, day: 1 };
        assert_eq!(date_ticks(&year_0, "d").expect_err("year 0").error_id, "TomlDateOutOfRange");
    }

    #[test]
    fn an_offset_past_14_hours_or_a_utc_time_before_year_1_has_no_dotnet_value() {
        for text in ["2000-01-01T00:00:00+15:00", "0001-01-01T00:00:00+01:00"] {
            let datetime: Datetime = text.parse().expect("an offset date-time");
            let refused = datetime_object(&datetime, "t").expect_err(text);
            assert_eq!(refused.error_id, "TomlDateOutOfRange", "{text}");
            assert!(refused.message.contains(text), "{text}: {}", refused.message);
        }
    }

    #[test]
    fn a_parse_error_gives_the_line_and_column_the_crate_reports() {
        let text = "[package]\nname = demo\n";
        let error = text.parse::<Table>().expect_err("demo is not quoted");
        let report = error.to_string();
        let first = report.lines().next().expect("a report");
        assert_eq!(first, "TOML parse error at line 2, column 8", "{report}");
        let ours = parse_error(text, &error);
        assert!(ours.message.starts_with(&format!("{first}: ")), "{} against {report}", ours.message);
        assert_eq!(ours.details.as_deref(), Some(report.as_str()));
        assert!(ours.terminating);
    }

    #[test]
    fn a_column_counts_characters() {
        // Byte 10 of the text is the =, after b, an é of two bytes, and a
        // space on line 2.
        assert_eq!(line_and_column("a = 1\nbé = 2", 10), (2, 4));
        assert_eq!(line_and_column("a = 1", 0), (1, 1));
        assert_eq!(line_and_column("a = 1\n", 99), (2, 1));
    }

    #[test]
    fn a_key_path_quotes_what_a_bare_key_cannot_hold() {
        assert_eq!(key_path("", "name"), "name");
        assert_eq!(key_path("server", "port"), "server.port");
        assert_eq!(key_path("server", "a b"), "server.\"a b\"");
        assert_eq!(key_path("", ""), "\"\"");
    }

    #[test]
    fn a_single_is_written_in_its_shortest_form() {
        assert_eq!(single_as_double(0.1).expect("a float"), 0.1);
        assert!(single_as_double(f32::NAN).expect("a float").is_nan());
        assert_eq!(single_as_double(f32::INFINITY).expect("a float"), f64::INFINITY);
    }
}
