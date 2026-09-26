---
title: Conversions Reference
weight: 2
---

Every Rust type that crosses the boundary and what it becomes. Parameter and field lowering is `crates/pwrs-macros/src/types.rs`; the conversion traits are `crates/pwrs/src/convert.rs`, `types.rs`, `values.rs`, `pinned.rs` and the impls `#[psclass]` and `#[psenum]` generate.

## As a parameter, a class field, or a method argument

The same lowering serves `#[psmethods]` arguments and, with `bool` as `bool`, their return types; an optional string argument is declared `object?` (see [Attribute Reference](Attribute-Reference.md)).

| Rust type | CLR type declared | Slot in the block |
|---|---|---|
| `bool` | `SwitchParameter` (parameter), `bool` (field) | `u8` |
| `i8`, `i16`, `i32`, `i64` | `sbyte`, `short`, `int`, `long` | same width, inline |
| `u8`, `u16`, `u32`, `u64` | `byte`, `ushort`, `uint`, `ulong` | same width, inline |
| `f32`, `f64` | `float`, `double` | inline |
| `String` | `string` | a borrowed UTF-16 `(ptr, len)` |
| `PathBuf` | `string` | as `String`; the Rust side wraps it in a `PathBuf` |
| `PsObject` | `object`, or the type `#[param(clr = "...")]` names | a handle |
| `PsScriptBlock` | `System.Management.Automation.ScriptBlock` | a handle |
| `PsHashtable` | `System.Collections.Hashtable` | a handle |
| `PsReadOnlyTable` | `Pwrs.ReadOnlyTable` | a handle; `over` wraps an existing `IDictionary` without copying it, so order survives and a change through the source shows through. Both `$t.key` and `$t['key']` read; neither writes. A nested dictionary is wrapped on the way out, from the indexer, from `Values` and from enumeration |
| `PsSecureString` | `System.Security.SecureString` | a handle |
| `PsCredential` | `System.Management.Automation.PSCredential` | a handle |
| `PsDateTime` | `System.DateTime` | a handle to the boxed value |
| `PsDateTimeOffset` | `System.DateTimeOffset` | a handle to the boxed value |
| `PsDecimal` | `System.Decimal` | a handle to the boxed value |
| `PsTimeSpan` | `System.TimeSpan` | a handle to the boxed value |
| `PsGuid` | `System.Guid` | a handle to the boxed value |
| `char` | `char` | a handle to the boxed value |
| a `#[psenum]` type | its declared enum name, or the existing CLR enum a `clr` mirror names | a handle to the boxed value |
| a `#[psclass]` type | its declared class name; `System.Management.Automation.PSObject` for psobject mode | a handle |
| `PsProxy<T>`, `T` a proxy `#[psclass]` | `T`'s declared class name | a handle; the value is not read at binding |
| `Option<T>` | as `T` | as `T`; the bound bit says whether it was supplied |
| `Vec<T>` | `T[]` (`bool[]` for `Vec<bool>`) | a handle to the array |

A parameter the engine has not assigned keeps its field's value; its slot is not read, so a `FromPs` never sees a `$null` for an unbound parameter. A class used as a cmdlet parameter type needs `Default`, as every parameter type does.

The CLR type in this table is what the engine's binder coerces the argument to before the module runs, which for a large array is the dominant cost of the call. `#[param(raw)]` declares such a parameter as `object` instead and leaves the conversion to Rust; see [Attribute Reference](Attribute-Reference.md) for what that trades away.

Not allowed as a parameter or field: `Option<Option<T>>`, `Vec<Option<T>>`, any generic type other than `Option`, `Vec` and `PsProxy`, and any type that does not implement `PsTyped` (every unknown bare identifier is treated as a `PsTyped` type; `#[psclass]` and `#[psenum]` types implement it, and a plain struct fails at the bound with a clear error).

## `FromPs` (reading a `PsObject`)

| Type | How |
|---|---|
| `PsObject` | clone of the handle |
| `PathBuf` | the display string, as a path |
| `i64` | `i64_read`: `LanguagePrimitives.ConvertTo<long>` |
| `i8`, `i16`, `i32`, `u8`, `u16`, `u32` | through `i64` with a range check; out of range is an `InvalidData` error |
| `u64` | `u64_read`: `LanguagePrimitives.ConvertTo<ulong>` |
| `usize`, `isize` | through `u64` and `i64` with a range check |
| `f64` | `f64_read`; `f32` narrows from it |
| `bool` | `bool_read`: `LanguagePrimitives.ConvertTo<bool>` |
| `String` | `string_read`: `LanguagePrimitives.ConvertTo<string>`, so any object's display string |
| `Option<T>` | `None` for `$null`, else `T` |
| `Vec<T>` | `array_len` and `array_get` over any array, `IList` or `IEnumerable`; `$null` is empty |
| `HashMap<String, V>` | keys as strings, values through `V: FromPs` |
| `PsScriptBlock`, `PsHashtable`, `PsSecureString` | wrap the handle |
| `PsCredential` | `UserName` and `Password` read through the dynamic surface |
| `PsBigInt` | `ToByteArray` through a pinned borrow |
| `PsDateTime` | `datetime_read`: `LanguagePrimitives.ConvertTo<DateTime>`, then ticks and kind |
| `PsDateTimeOffset` | `datetimeoffset_read`: ticks and the UTC offset in whole minutes |
| `PsDecimal` | `decimal_read`: the four `Decimal.GetBits` words, lo, mid, hi, flags |
| `PsDecimalBits` | the same four words in memory order, through `PsDecimal` |
| `PsTimeSpan` | `timespan_read`: `LanguagePrimitives.ConvertTo<TimeSpan>`, then ticks |
| `PsGuid` | `guid_read`: `LanguagePrimitives.ConvertTo<Guid>`, then `ToByteArray` |
| `PsErrorRecord` | four member reads on the record: `CategoryInfo.Category`, `FullyQualifiedErrorId`, `Exception.Message`, `TargetObject`. An object that is not a record fails on the first of them |
| `char` | `char_read`: `LanguagePrimitives.ConvertTo<char>`; a surrogate unit is an `InvalidData` error |
| a `#[psenum]` type | the underlying integer, matched to a variant; an unmatched value is an `InvalidData` error |
| a CLR enum this module never declared | `i64` reads its number and `String` its name, both through `LanguagePrimitives.ConvertTo`, with no declaration needed. `PsErrorRecord` reads `CategoryInfo.Category`, a `System.Management.Automation.ErrorCategory`, exactly this way. `#[psenum(clr = "...")]` reads it as a Rust enum instead, and types the parameter as the CLR enum so the binder validates and completes it |
| a `#[psclass]` type | every field by its property name through `dyn_get`, from a copied object, a proxy, or a PSObject with the notes; `$null` is an `InvalidData` error |
| `PsProxy<T>` | a clone of the handle, with no call into the value; `$null` is an `InvalidData` error, and a `T` that is not a proxy class an `InvalidType` one. The value is reached later through `with` and `with_mut`, which check the object's class, module and load |

## `IntoPs` (producing a `PsObject`) and `ps.write`

| Type | Object produced | `write` path |
|---|---|---|
| `PsObject` | itself | handle |
| `&str`, `String` | a `string` | direct `write_string` |
| `PathBuf` | a `string` (lossy UTF-8 of the path) | direct `write_string` |
| `i64` | a `long` | direct `write_i64` |
| `i8`, `i16`, `i32`, `u8`, `u16`, `u32` | a `long` (widened) | direct `write_i64` |
| `u64` | a `ulong` | handle |
| `usize` | a `ulong` | handle |
| `isize` | a `long` | direct `write_i64` |
| `f64` | a `double` | direct `write_f64` |
| `f32` | a `double` (widened) | direct `write_f64` |
| `bool` | a `bool` | direct `write_bool` |
| `Option<T>` | `T`'s object, or `$null` | `Some` as `T`; `None` a null handle |
| `Vec<T>` | a typed array (`long[]`, `string[]`, `double[]`, `object[]` for handles) | one handle to that array, written enumerated, so the pipeline sees the elements |
| `PsArray<T>` | the same typed array | one object |
| `PsMemory<T>` | a `Memory<T>` over the Rust allocation on .NET; a `T[]` copy on .NET Framework | handle |
| `HashMap<String, V>` | a `Hashtable` | handle |
| `PsScriptBlock`, `PsHashtable`, `PsSecureString` | the wrapped handle | handle |
| `PsCredential` | a `PSCredential` through its constructor, which rejects an empty user name | handle |
| `PsBigInt` | a `System.Numerics.BigInteger` from its bytes | handle |
| `PsDateTime` | a `DateTime` with the ticks and kind; ticks past `DateTime.MaxValue` are an error | handle |
| `PsDateTimeOffset` | a `DateTimeOffset` from the ticks and offset | handle |
| `PsDecimal`, `PsDecimalBits` | a `Decimal` through its four-word constructor, which rejects a scale above 28 | handle |
| `PsTimeSpan` | a `TimeSpan` | handle |
| `PsGuid` | a `Guid` | handle |
| `char` | a `char`; a character outside the Basic Multilingual Plane is an `InvalidData` error | handle |
| a `#[psclass]` type | the class's object in its mode | handle |
| `PsProxy<T>` | the same object it holds | handle |
| a `#[psenum]` type | the enum value, through the class factory for a declared enum and through `enum_new` for a `clr` mirror | handle |

The element type of a `Vec<T>` array is `T::TYPE_TAG`: `bool[]`, `sbyte[]` .. `ulong[]`, `float[]`, `double[]`, `string[]`, `char[]`, `DateTime[]`, `TimeSpan[]`, `Guid[]`, else `object[]`. A class field declared as an array of any other type (an enum, a credential) receives that `object[]` converted by the engine to the declared element type.

## Scalars keep their width

`into_ps` builds the CLR type of the Rust type it was given: an `i32` arrives as a `System.Int32`, a `u16` as a `System.UInt16`, an `f32` as a `System.Single`. The engine types every operator's answer by its operands' widths, so `[int]::MaxValue + 1` is a `Double` while the same sum over `Int64` is an `Int64`, and a value that left script as an `Int32` and came back an `Int64` would change what the caller's next operator does with it.

The width is bought with one handle. `i64`, `f64` and `bool` are what the direct write entries build, so those still write without one; every other scalar width costs one crossing more per written value than a widened write would. `PsObject::type_tag()` is the cheapest way to see what a value actually is.

## The object type tag

`PsObject::type_tag()` answers the `PS_TYPE_*` tag of an object's own type in one crossing, and `PS_TYPE_OBJECT` for anything outside the vocabulary. It is the fast half of dynamic dispatch: `type_name()` is `GetType`, then `FullName`, then a string read, and each of the first two goes through the engine's member invocation. Measured, the tag is 12 to 13 ns and the name 937 to 1046 ns, with `$o.GetType().FullName` in script at 345 to 1203 ns depending on the object; see [Benchmarks](Benchmarks.md).

A caller that does not know its input's type at compile time matches on the tag first and falls back to `type_name()` only for `PS_TYPE_OBJECT`. The tag sees through the `PSObject` the engine wraps a value in, so a wrapped `Int32` answers `PS_TYPE_I32` rather than `PS_TYPE_OBJECT`.

## Decimals, and two orders for the same four words

A `System.Decimal` is four 32-bit words. They have two orders and PWRS names both, because confusing them silently produces plausible wrong numbers:

- **`PsDecimal`** is `Decimal.GetBits` order, `lo`, `mid`, `hi`, `flags`, which is documented and stable. `flags` carries the sign in bit 31 and the scale, 0 to 28, in bits 16 to 23. This is what crosses as a scalar.
- **`PsDecimalBits`** is memory order, `flags`, `hi`, `lo`, `mid`, which is what a pinned `Decimal[]` element actually is. It is `PsDecimal` permuted `[3], [2], [0], [1]`, and `From` converts either way.

Memory order is internal to the runtime and documented nowhere. It was measured as `flags, hi, lo, mid` on x64 Windows under .NET 10 and .NET Framework 4.8, by two independent methods. The proof described next has agreed with it on x64 Linux under .NET 10.0.11 and x64 FreeBSD under .NET 9.0.14, where the hello suite's block sums pass, and no reading exists for arm64. So the first `pin::<PsDecimalBits>()` in a process proves the order against `Decimal.GetBits` and caches the verdict; a host that disagrees fails with `PwrsDecimalLayout` rather than returning reinterpreted words. Later pins read the cached answer and cost nothing.

A scale is not a value. `1.10` and `1.1` are equal and not identical, and a round trip that dropped the trailing zero would still compare equal, so the scale is carried rather than normalized.

## Where the two hosts disagree

PWRS ships to Windows PowerShell 5.1 on .NET Framework and to PowerShell 7 on Core, and a few conversions are not the same on both. These are the engine's own behavior, not PWRS's, and PWRS carries through whatever the host produced rather than picking one:

| | Windows PowerShell 5.1 | PowerShell 7 |
|---|---|---|
| `[decimal]'1.10'` | scale 2, the trailing zero kept | scale 1, normalized away |
| `-eq`, `-lt`, `-like` on text | invariant culture through NLS | invariant culture through ICU |

`[decimal]::Parse('1.10')` and the `1.10d` literal are scale 2 on both, so either is the way to write a decimal whose scale you mean.

The collation row matters more than it looks. The two implementations disagree on punctuation and ligatures and treat control characters as nothing, so `-eq` is not ordinal equality on either host and the two hosts do not agree with each other. Rust's `==` and `cmp` on a `String` are ordinal, so a module that compares text in Rust and a script that compares the same text with `-eq` can reach different answers, and the difference moves between hosts. Where the answer has to match the engine's, ask the engine: `String.Compare` and `WildcardPattern` through `PsType::call_static`. Ordinal comparison in Rust is safe for printable ASCII and for text you control on both sides, such as your own keys and tags. This divergence was reported by a consumer measuring it on their own surface; PWRS documents it rather than having measured the ligature and control-character cases itself.

## Dates, spans and GUIDs (`values.rs`)

`PsDateTime { ticks: i64, kind: DateTimeKind }` holds ticks of 100 ns from the start of year 1 and `DateTimeKind::{Unspecified, Utc, Local}`. `TryFrom<SystemTime>` gives a `Utc` value (an error past the year 9999); `TryFrom<PsDateTime> for SystemTime` accepts only a `Utc` value, and `to_utc()` asks the engine's `ToUniversalTime()` for the others (an `Unspecified` value is treated as local, as the engine does). The constants `TICKS_PER_SECOND`, `UNIX_EPOCH_TICKS` and `MAX_DATETIME_TICKS` are public.

`PsTimeSpan { ticks: i64 }` may be negative. `TryFrom<Duration>` fails past `TimeSpan.MaxValue`; `TryFrom<PsTimeSpan> for Duration` fails for a negative span. Precision below a tick is dropped in both directions.

`PsGuid { bytes: [u8; 16] }` holds the `Guid.ToByteArray` order. `Display` writes the hyphenated lowercase form; `FromStr` accepts 32 hex digits with or without the four hyphens, optionally in braces or parentheses. `to_rfc4122()` and `from_rfc4122(bytes)` swap the first three fields to and from big-endian, the order of the textual form and of the `uuid` crate. `PsGuid::NIL` is all zeros.

`char` crosses as one UTF-16 unit.

## Error records (`values.rs`)

`PsErrorRecord { category: ErrorCategory, error_id: String, message: String, target: PsObject }` reads a `System.Management.Automation.ErrorRecord`: the category as the typed `ErrorCategory` rather than its number, the fully qualified error id, the exception's message, and the target object, which is `$null` when the record carries none. A record handed down the pipeline and one caught in script and passed as an argument read the same way.

It is `FromPs` only. A cmdlet raises an error by returning `Err(PsError)`, which the engine turns into a record of its own; there is nothing a cmdlet does with a record it built itself.

The four member reads are what reading a record costs anywhere, typed. A category outside the enum is an `InvalidData` error rather than a silent `NotSpecified`, and `ErrorCategory::from_code(u32) -> Option<ErrorCategory>` is the mapping on its own.

## Secure strings and credentials (`types.rs`)

`PsSecureString(pub PsObject)`: `new(text)` builds a read-only `SecureString` (an error past 65536 characters); `len()` and `is_empty()` read the length without decrypting; `reveal()` decrypts into a UTF-16 copy that is zeroed before it is freed and returns the `String`. Its `Debug` form is the type name only.

`PsCredential { user_name: String, password: PsSecureString }`: `new(user_name, password)` builds the secure string; `FromPs` reads a `PSCredential`'s `UserName` and `Password`; `IntoPs` calls the `PSCredential` constructor.

## Pinned arrays

`PsObject::pin::<T>()` borrows a managed `T[]` for `T` in `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `f32`, `f64`, and `PsDecimalBits` for a `Decimal[]`; it fails for a non-array, a non-primitive element type, an element size that differs from `T`, or an element type that is not `T`. The type is checked and not only the width, so an `Int64[]` does not pin as `f64` and a `UInt32[]` does not pin as `i32`: those pairs are the same size, and a pin that took one for the other would reinterpret the array rather than read it, with no error raised. The refusal is `PwrsPinElementType` and names both tags. `PsObject::from_slice(&[T])` builds a new array of `T::TYPE_TAG` and copies through one pin.

A `Vec<T>` of those same primitive types uses the pin in both directions on its own: reading asks the array for its element tag and, when it matches `T`, copies the whole array through one pin, falling back to the element-by-element path for any other collection (an `object[]`, an `IList`, a mixed array); writing fills a typed array through one pin. Every other element type keeps the element-by-element path, which is one `array_get` and one read entry per element.

`PsMemory<T>` goes the other way without a copy on .NET: `zeroed(len)` allocates a buffer that derefs to `[T]` for filling in place (`from_slice` and `TryFrom<Vec<T>>` copy once), and `into_ps` hands the allocation to `memory_view_new`, which wraps it as a `Memory<T>` freed when the managed owner is collected; on .NET Framework the entry copies into a `T[]` and frees the buffer before returning. A `PsMemory` never handed over frees its buffer on drop.

## Text

Strings cross as UTF-16. `pwrs::text::to_utf16`, `from_utf16` and `from_str16` do the conversions with a single-pass ASCII path and std for the rest; unpaired surrogates become U+FFFD. `try_from_utf16` and `try_from_str16` are the same readers with every allocation reserved first, so a string too large for the allocator is a `PwrsOutOfMemory` error record rather than the end of the host process; the string, array, hashtable, `BigInteger` and `SecureString` conversions reserve the same way.
