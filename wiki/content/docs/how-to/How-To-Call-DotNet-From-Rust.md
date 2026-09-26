---
title: How To Call .NET From Rust
weight: 5
---

Reaching any .NET object, type or script block from Rust, and borrowing managed arrays without copying. Source: `crates/pwrs/src/dynamic.rs`, `types.rs`, `pinned.rs`, `object.rs`, and the vtable entries in `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs` they call.

## Objects

`PsObject` is an owned `GCHandle` to any managed object. It is `Send` and `Sync`; only the stream API is thread-affine. Clone it to get a second handle to the same object; drop it to free the handle.

```rust
let text: PsObject = "shout".into_ps()?;
let upper = text.call("ToUpper", &[])?;            // instance method by name
let host = obj.get("Host")?;                       // property or field
obj.set("Timeout", &30i64.into_ps()?)?;            // settable property
let name = obj.type_name()?;                       // "System.Uri"
let s = String::from_ps(&upper)?;                  // any object to string, via LanguagePrimitives
```

`call` on a table (any `IDictionary`, wrapped or not) answers `get_Item`, `set_Item`, `ContainsKey`, `Contains` and `Remove` through the dictionary itself, which is what `PsHashtable` and `PsReadOnlyTable` reads go through; a missing key reads as `$null` whatever the table's own indexer does. Every other `call` goes through PowerShell's member binder first (`PSObject.Methods`), so ETS methods and overload resolution work the way they do in script; when the adapter has no such method it falls back to the CLR binder on the base object, which is what reaches the accessors the adapter hides. `get` reads `PSObject.Members` first and then `IDictionary` keys, so hashtable entries are readable by name.

## Types, statics, constructors

```rust
let math = PsType::from_name("System.Math");
let abs = i64::from_ps(&math.call_static("Abs", &[(-5i64).into_ps()?])?)?;
let uri = PsType::from_name("System.Uri").new(&["https://example.com/x".into_ps()?])?;
```

Type names are resolved by the engine's type resolver, so `int`, `System.IO.File` and the names PowerShell accepts inside `[...]` all work. Statics and constructors use the .NET default binder for overload resolution.

## Script blocks

```rust
#[param(mandatory)] pub script: PsScriptBlock,
// ...
for out in self.script.call(ps, &args)? {
    ps.write_object(&out)?;
}
```

`PsScriptBlock::call` needs the `Pipeline` token because a script block runs on the pipeline thread; it returns every object the block emitted. `PsScriptBlock` is also a parameter type (`System.Management.Automation.ScriptBlock` in the shell).

## Commands by name

```rust
let found = ps.invoke("Get-ChildItem", &[("Path", root.into_ps()?), ("File", true.into_ps()?)])?;
let sorted = ps.invoke_with_input("Sort-Object", &[("Property", "Length".into_ps()?)], Some(&found.into_ps()?))?;
```

`Pipeline::invoke` runs a cmdlet, function or alias the session can see, with the parameters bound by name, and returns everything it wrote; `invoke_with_input` pipes a value in first, unrolling a collection into records. The name is resolved to the command and run through a nested pipeline in the current runspace, so no script text is built and nothing is parsed, which is what a script block costs when all you want is to call something.

What the command raises is handled the way it would be had the user run it. A non-terminating error is written to your cmdlet's error stream and the output still comes back, so the caller's `-ErrorAction` decides what it means; a terminating error is the `Err`. A name the session cannot resolve is a terminating error naming it.

## Hashtables

`PsHashtable` wraps any `IDictionary`: `new()` makes an empty `Hashtable`; `get`, `set`, `contains`, `len`, `is_empty`, `keys`. `HashMap<String, V>` converts both ways.

## Big integers

`PsBigInt` carries a `System.Numerics.BigInteger` as its little-endian two's-complement bytes, the form `ToByteArray` and the byte-array constructor share. `from_ps` reads one; `into_ps` constructs one; `is_negative` and `doubled` operate on the bytes.

## Secure strings and credentials

```rust
#[param(mandatory)] pub credential: PsCredential,
// ...
let password = self.credential.password.reveal()?;   // decrypts; the UTF-16 copy is zeroed afterwards
let fresh = PsCredential::new("ada", "hunter2")?;     // builds a read-only SecureString and a PSCredential
```

`PsSecureString` wraps a `System.Security.SecureString`; `new` builds one from Rust text, `len` reads its length without decrypting, `reveal` decrypts through an unmanaged buffer the runtime zeroes before returning. `PsCredential` is the `UserName` and `Password` of a `PSCredential`; its `IntoPs` runs the engine's constructor, which rejects an empty user name.

## Dates, spans, GUIDs, chars

`PsDateTime`, `PsTimeSpan`, `PsGuid` and `char` cross through their own vtable entries as ticks, bytes and code units; `PsDateTime::to_utc()` is the one engine call among them (`ToUniversalTime()`). The types and their `std` conversions are in [Conversions Reference](Conversions-Reference.md).

## Pinned arrays

```rust
let bytes = self.data.pin::<u8>()?;       // &[u8] over the managed byte[]
let sum: u64 = bytes.iter().map(|&b| b as u64).sum();
drop(bytes);                              // the pin is released here, or at the end of the phase

let out = PsObject::from_slice(&[1u32, 2, 3])?;   // a new uint[] filled through one pin
```

`pin::<T>()` pins a managed array of primitives and borrows it as `&[T]` (and `&mut [T]`). It fails when the object is not an array, when the element type is not pinnable, when the element size differs from `T`, or when the element type is not `T`. The type is checked and not only the width, so an `Int64[]` does not pin as `f64`: those are the same size, and a pin that took one for the other would reinterpret the array rather than read it. The GC cannot move the array while the borrow lives; the borrow must not outlive the phase.

`T` is any of the `Primitive` types: `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `f32`, `f64`, and `PsDecimalBits` for a `Decimal[]`. That last one is the four words of a `System.Decimal` in memory order rather than the order `Decimal.GetBits` reports, and the first decimal pin in a process proves that order against `GetBits` before any element is read.

## Rust memory as `Memory<T>`

```rust
let mut buf = PsMemory::<u8>::zeroed(len)?;   // one allocation, zero-filled
fill(&mut buf);                                // &mut [u8], in place
ps.write(buf)?;                                // a Memory<byte> over that allocation
```

`PsMemory<T>` is the other direction of zero copy: on .NET the engine receives a `Memory<T>` that views the Rust allocation, and the allocation is freed when the managed owner is collected; on .NET Framework the runtime copies the bytes into a `T[]` and frees the allocation before the write returns. `from_slice` and `TryFrom<Vec<T>>` make one from existing data with one copy. Scripts use the value as any `Memory<T>`: `.Length`, `.ToArray()`, or hand it to a .NET API taking a `ReadOnlyMemory<T>`.

## Building PSObjects by hand

```rust
let obj = pwrs::object::new_psobject("My.Type");            // PSObject with a PSTypeName
pwrs::object::add_note(&obj, "Name", "x".into_ps()?)?;      // a note property
let name = pwrs::object::property(&obj, "Name")?;           // and back out by name
ps.write_object(&obj)?;
```

This is what `#[psclass(mode = psobject)]` generates; use it directly when the shape is decided at run time.

## Threads

Every call on this page except `PsScriptBlock::call` and the stream API is legal on any thread: `PsObject::get`, `call`, `PsType`, pinning, and conversions do not need the token. See [How To Use Threads](How-To-Use-Threads.md).
