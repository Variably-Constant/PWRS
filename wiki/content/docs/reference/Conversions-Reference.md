---
title: Conversions Reference
weight: 2
---

Every Rust type PWRS converts and the .NET type it becomes. The mapping for parameters and fields is in `crates/pwrs-macros/src/types.rs`; the conversion traits are `crates/pwrs/src/convert.rs`, `types.rs`, `values.rs`, `pinned.rs` and the impls `#[psclass]` and `#[psenum]` generate.

## As a parameter, a class field, or a method argument

The same mapping applies to `#[psmethods]` arguments and, with `bool` as `bool`, their return types; an optional string argument is declared `object?` (see [Attribute Reference](Attribute-Reference.md)).

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
| `PsHashtable` | `System.Collections.IDictionary` | a handle to the caller's own dictionary, unconverted: a hashtable, an `[ordered]` table, whose keys stay in the order written, or any other `IDictionary` |
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
| `PsArrayMut<T>`, `T` a type a pinned array takes | `T[]` (`decimal[]` for `PsDecimalBits`) | a handle to the caller's own array, which `pin` borrows in place; see [Pinned arrays](#pinned-arrays) |

The field of a parameter the engine has not assigned has the value it started with; its slot is not read, so a `FromPs` never sees a `$null` for an unbound parameter. A class used as a cmdlet parameter type needs `Default`, as every parameter type does.

The CLR type in this table is what the engine's binder converts the argument to before the module runs, which for a large array is the dominant cost of the call. `#[param(raw)]` declares such a parameter as `object` instead and leaves the conversion to Rust; see [Attribute Reference](Attribute-Reference.md) for what that trades away.

Not allowed as a parameter or field: `Option<Option<T>>`, `Vec<Option<T>>`, any generic type other than `Option`, `Vec`, `PsArrayMut` and `PsProxy`, and any type that does not implement `PsTyped`. Every unknown bare identifier is treated as a `PsTyped` type: `#[psclass]` and `#[psenum]` types implement it, and a plain struct fails at the bound with a clear error.

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
| `String` | `LanguagePrimitives.ConvertTo<string>`, so any object's display string: up to 256 UTF-16 units copied by `string_read`, a longer string decoded where it is, through `string_pin` |
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
| `i8`, `i16`, `i32`, `u8`, `u16`, `u32` | an `sbyte`, `short`, `int`, `byte`, `ushort` or `uint`, the type's own width | handle |
| `u64` | a `ulong` | handle |
| `usize` | a `ulong` | handle |
| `isize` | a `long` | direct `write_i64` |
| `f64` | a `double` | direct `write_f64` |
| `f32` | a `float` | handle |
| `bool` | a `bool` | direct `write_bool` |
| `Option<T>` | `T`'s object, or `$null` | `Some` as `T`; `None` a null handle |
| `Vec<T>` | an array of `T`'s element type, listed below the table | one handle to that array, written enumerated, so the pipeline sees the elements |
| `PsArray<T>` | the same typed array | one object |
| `PsMemory<T>` | a `Memory<T>`, or after `read_only()` a `ReadOnlyMemory<T>`, over the Rust allocation on .NET; a `T[]` copy on .NET Framework | handle |
| `PsMemoryView<T>` | a `Memory<T>` (`writable`) or `ReadOnlyMemory<T>` (`read_only`) over the module's memory, valid while its owner lives, on .NET; a `T[]` copy on .NET Framework | handle |
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
| `PsProxy<T>` | the same object it refers to | handle |
| a `#[psenum]` type | the enum value, through the class factory for a declared enum and through `enum_new` for a `clr` mirror | handle |

The element type of a `Vec<T>` array is `T::TYPE_TAG`: `bool[]`, `sbyte[]` .. `ulong[]`, `float[]`, `double[]`, `string[]`, `char[]`, `DateTime[]`, `TimeSpan[]`, `Guid[]`, `decimal[]`, else `object[]`. A class field declared as an array of any other type (an enum, a credential) receives that `object[]` converted by the engine to the declared element type.

## Scalars are not widened

`into_ps` builds the CLR type of the Rust type it was given: an `i32` arrives as a `System.Int32`, a `u16` as a `System.UInt16`, an `f32` as a `System.Single`. PowerShell picks the type of an operator's result from the types of its operands, so `[int]::MaxValue + 1` is a `Double`, while the same sum over `Int64` is an `Int64`. A value that left script as an `Int32` and came back an `Int64` would change what the caller's next operator does with it.

Writing a scalar at its own width costs a handle. `i64`, `f64` and `bool` are what the direct write entries build, so each of those is written with one call into .NET. Every other scalar is built by its own constructor entry, written as a handle, and the handle freed: two calls more per written value than a widened write would cost. `PsObject::type_tag()` is the cheapest way to find a value's type.

## The object type tag

`PsObject::type_tag()` returns the `PS_TYPE_*` tag of an object's own type with one call into .NET, and `PS_TYPE_OBJECT` for any type PWRS has no tag for. It is the fast way to find an object's type: `type_name()` is `GetType`, then `FullName`, then a string read, and each of the first two goes through the engine's member invocation. Measured, the tag is 12 to 13 ns and the name 937 to 1046 ns, with `$o.GetType().FullName` in script at 345 to 1203 ns depending on the object; see [Benchmarks](Benchmarks.md).

Code that handles input whose type is not fixed at compile time matches on the tag first and falls back to `type_name()` only for `PS_TYPE_OBJECT`. For a value wrapped in a `PSObject` the tag is the value's, so a wrapped `Int32` returns `PS_TYPE_I32`, not `PS_TYPE_OBJECT`.

## Decimals, and two orders for the same four words

A `System.Decimal` is four 32-bit words in two orders, and PWRS has a type for each. Reading one order as the other gives plausible wrong numbers and no error:

- **`PsDecimal`** is `Decimal.GetBits` order, `lo`, `mid`, `hi`, `flags`, which is documented and stable. `flags` has the sign in bit 31 and the scale, 0 to 28, in bits 16 to 23. This is the form a single decimal value is passed in.
- **`PsDecimalBits`** is memory order, `flags`, `hi`, `lo`, `mid`, which is what a pinned `Decimal[]` element actually is. It is `PsDecimal` permuted `[3], [2], [0], [1]`, and `From` converts either way.

Memory order is internal to the runtime and documented nowhere. It was measured as `flags, hi, lo, mid` on x64 Windows under .NET 10 and .NET Framework 4.8, by two independent methods. The check described next has agreed with it wherever the hello suite's block sums ran: x64 Linux under .NET 10.0.12, x64 FreeBSD under .NET 9.0.14, and arm64 macOS under .NET 10.0.12 and 10.0.13 in CI. So the first `pin::<PsDecimalBits>()` in a process checks the order against `Decimal.GetBits` and stores the result; where the order differs, the pin fails with `PwrsDecimalLayout` rather than returning reinterpreted words. Later pins read the stored result and cost nothing.

A scale is not a value: `1.10` and `1.1` are equal and not identical. The scale is left as it is, not normalized, so `1.10d` comes back from Rust with scale 2 and `1.1d` with scale 1. A test comparing with `-eq` cannot see that, since a value that lost its trailing zero still compares equal.

## Where the two hosts differ

PWRS ships to Windows PowerShell 5.1 on .NET Framework and to PowerShell 7 on Core, and a few conversions are not the same on both. These are the engine's own behavior, not PWRS's, and PWRS passes on whatever the host produced instead of picking one:

| | Windows PowerShell 5.1 | PowerShell 7 |
|---|---|---|
| `[decimal]'1.10'` | scale 2, the trailing zero still there | scale 1, normalized away |
| `-eq`, `-lt`, `-like` on text | invariant culture through NLS | invariant culture through ICU |

`[decimal]::Parse('1.10')` and the `1.10d` literal are scale 2 on both, so either is the way to write a decimal whose scale you mean.

The collation row matters more than it looks. Measured on Windows in pwsh 7.6.7 and Windows PowerShell 5.1.26100.9711:

| | Windows PowerShell 5.1 | PowerShell 7 |
|---|---|---|
| `'encyclopaedia' -eq 'encyclopædia'` | `True` | `False` |
| `'co-op' -lt 'coop'` | `False`, it sorts after | `True` |
| `"a$([char]1)b" -eq 'ab'` | `False` | `True`, the control character counts as nothing |
| `"a$([char]0)b" -eq 'ab'` | `True` | `True` |

So `-eq` is not ordinal equality on either host, and the two hosts give different results. Rust's `==` and `cmp` on a `String` are ordinal and case-sensitive, so a module that compares text in Rust and a script that compares the same text with `-eq` can reach different results, and the difference moves between hosts. Where the result has to match PowerShell's, call the comparisons PowerShell calls: `String.Compare` through `PsType::call_static`, and a `WildcardPattern` made through `PsType::new`, whose `IsMatch` matches as `-like` does when it is made with the text `"IgnoreCase"`. Ordinal comparison in Rust matches the engine only where neither case nor culture can matter, such as keys and tags written in one form on both sides.

## Dates, spans and GUIDs (`values.rs`)

`PsDateTime { ticks: i64, kind: DateTimeKind }` stores ticks of 100 ns from the start of year 1 and `DateTimeKind::{Unspecified, Utc, Local}`. `TryFrom<SystemTime>` gives a `Utc` value (an error past the year 9999); `TryFrom<PsDateTime> for SystemTime` accepts only a `Utc` value, and `to_utc()` calls .NET's `ToUniversalTime()` for the others (an `Unspecified` value is treated as local, as the engine does). The constants `TICKS_PER_SECOND`, `UNIX_EPOCH_TICKS` and `MAX_DATETIME_TICKS` are public.

`PsTimeSpan { ticks: i64 }` may be negative. `TryFrom<Duration>` fails past `TimeSpan.MaxValue`; `TryFrom<PsTimeSpan> for Duration` fails for a negative span. Precision below a tick is dropped in both directions.

`PsGuid { bytes: [u8; 16] }` stores the bytes in `Guid.ToByteArray` order. `Display` writes the hyphenated lowercase form; `FromStr` accepts 32 hex digits with or without the four hyphens, optionally in braces or parentheses. `to_rfc4122()` and `from_rfc4122(bytes)` swap the first three fields to and from big-endian, the order of the textual form and of the `uuid` crate. `PsGuid::NIL` is all zeros.

`char` is passed as one UTF-16 unit.

## Error records (`values.rs`)

`PsErrorRecord { category: ErrorCategory, error_id: String, message: String, target: PsObject }` reads a `System.Management.Automation.ErrorRecord`: the category as the typed `ErrorCategory` rather than its number, the fully qualified error id, the exception's message, and the target object, which is `$null` when the record has none. A record passed down the pipeline and one caught in script and passed as an argument are read the same way.

It is `FromPs` only. A cmdlet raises an error by returning `Err(PsError)`, which the engine turns into a record of its own; there is nothing a cmdlet does with a record it built itself.

Reading a record takes four member reads, as it does anywhere else, and gives typed values. A category outside the enum is an `InvalidData` error rather than a silent `NotSpecified`, and `ErrorCategory::from_code(u32) -> Option<ErrorCategory>` is the mapping on its own.

## Secure strings and credentials (`types.rs`)

`PsSecureString(pub PsObject)`: `new(text)` builds a read-only `SecureString` (an error past 65536 characters); `len()` and `is_empty()` read the length without decrypting; `reveal()` decrypts into a UTF-16 copy that is zeroed before it is freed and returns the `String`. Its `Debug` form is the type name only.

`PsCredential { user_name: String, password: PsSecureString }`: `new(user_name, password)` builds the secure string; `FromPs` reads a `PSCredential`'s `UserName` and `Password`; `IntoPs` calls the `PSCredential` constructor.

## Pinned arrays

`PsObject::pin::<T>()` borrows a managed `T[]` for `T` in `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `f32`, `f64`, and `PsDecimalBits` for a `Decimal[]`. It fails for a non-array, a non-primitive element type, an element size that differs from `T`, or an element type that is not `T`. The type is checked and not only the width, so an `Int64[]` does not pin as `f64` and a `UInt32[]` does not pin as `i32`. Those pairs are the same size, and a pin that took one for the other would reinterpret the array rather than read it, with no error raised. The refusal is `PwrsPinElementType` and names both tags. `PsObject::from_slice(&[T])` builds a new array of `T::TYPE_TAG` and copies through one pin.

A view can write, and Rust forbids two writable views of the same memory, so every pin of an array is recorded on its thread until the view drops. A second pin of the same array on that thread, while the first view lives, is refused with `PwrsArrayPinned`, as when one array is passed for two arguments. An empty array is not recorded, since no view of it reaches any memory. Another runspace on another thread can still write the array while a view lives, as any .NET code with a reference to the array can.

`PsArrayMut<T>` is a parameter or method argument the shell declares as `T[]`, for the same element types. An argument that is already a `T[]` reaches Rust as the caller's own array, so a change through it shows in the caller's variable; the binder converts any other value to a new `T[]` first. Its `FromPs` checks the element type and takes the array without pinning it, and refuses `$null` with `PwrsNullObject`. `pin(&mut self)` borrows it as `PsObject::pin` does, so a value has one view at a time. `object()` is the array itself, and writing a `PsArrayMut` writes the same array as one object. `Hello.ByteStream` in the hello example fills a `StreamReader`'s buffer this way, and `Set-RustBytes` writes the caller's array.

A `Vec<T>` of those same primitive types uses the pin in both directions on its own. Reading gets the array's element tag and, when it matches `T`, copies the whole array through one pin, falling back to the element-by-element path for any other collection (an `object[]`, an `IList`, a mixed array). Writing fills a typed array through one pin. Every other element type takes the element-by-element path, which is one `array_get` and one read entry per element.

`PsMemory<T>` goes the other way without a copy on .NET. `zeroed(len)` allocates a buffer that derefs to `[T]` for filling in place; `from_slice` and `TryFrom<Vec<T>>` copy once. `into_ps` passes the allocation to `memory_view_borrow`, which wraps it as a `Memory<T>` freed when the view's manager is disposed or garbage collected. On .NET Framework the entry copies into a `T[]` and frees the buffer before returning. A `PsMemory` that is never written frees its buffer when it is dropped.

`PsMemoryView<T>` does the same for memory the module did not allocate itself, such as a mapped file. `writable(ptr, len, owner)` and `read_only(ptr, len, owner)` are `unsafe`: the caller must make sure the memory stays valid, and is not moved, while `owner` lives. The owner lives until the view is released, then dropped on whatever thread releases it, the finalizer's among them; on .NET Framework that is before the copy's write returns. Both kinds of buffer take `revocable(&PsRevocation)`: once the `PsRevocation` is revoked, writing a view is refused, and a view already written throws `ObjectDisposedException` from every later `GetSpan` and `Pin`, so `ToArray()`, `CopyTo` and every other access through it fail. A view whose manager has been disposed throws it as well.

## Text

Strings are passed as UTF-16. `pwrs::text::to_utf16`, `from_utf16` and `from_str16` do the conversions with a single-pass ASCII path and std for the rest; unpaired surrogates become U+FFFD. `try_from_utf16` and `try_from_str16` are the same readers with every allocation reserved first, so a string too large for the allocator is a `PwrsOutOfMemory` error record rather than the end of the host process. The string, array, hashtable, `BigInteger` and `SecureString` conversions reserve the same way.
