---
title: How To Call .NET From Rust
weight: 5
---

Reaching any .NET object, type or script block from Rust, and borrowing managed arrays without copying. Source: `crates/pwrs/src/dynamic.rs`, `types.rs`, `pinned.rs`, `object.rs`, and the vtable entries in `crates/cargo-pwrs/dotnet/Pwrs.Runtime/HostVTable.cs` they call.

## Objects

`PsObject` wraps a `GCHandle` to any managed object and frees the handle when it is dropped. It is `Send` and `Sync`, because a handle may be released from any thread, but its methods that call into PowerShell must run on a thread PowerShell called the module on; see [Threads](#threads) below. Clone it to get a second handle to the same object; drop it to free the handle.

```rust
let text: PsObject = "shout".into_ps()?;
let upper = text.call("ToUpper", &[])?;            // instance method by name
let host = obj.get("Host")?;                       // property or field
obj.set("Timeout", &30i64.into_ps()?)?;            // settable property
let name = obj.type_name()?;                       // "System.Uri"
let s = String::from_ps(&upper)?;                  // any object to string, via LanguagePrimitives
```

`call` on a table (any `IDictionary`, wrapped or not) runs `get_Item`, `set_Item`, `ContainsKey`, `Contains` and `Remove` directly on the dictionary, which is what `PsHashtable` and `PsReadOnlyTable` reads go through. A missing key reads as `$null` whatever the table's own indexer does. Every other `call` goes through PowerShell's member binder first (`PSObject.Methods`), so methods added through PowerShell's type data, and overload resolution, work the way they do in script. When the adapter has no such method, it falls back to the CLR binder on the base object, which is how it reaches the accessors the adapter hides. `get` reads `PSObject.Members` first and then `IDictionary` keys, so hashtable entries are readable by name.

## Types, statics, constructors

```rust
let math = PsType::from_name("System.Math");
let abs = i64::from_ps(&math.call_static("Abs", &[(-5i64).into_ps()?])?)?;
let uri = PsType::from_name("System.Uri").new(&["https://example.com/x".into_ps()?])?;
```

Type names are resolved by the engine's type resolver, so `int`, `System.IO.File` and the names PowerShell accepts inside `[...]` all work.

Arguments that already have the parameters' types bind by .NET's own rules. When no overload takes them that way, each overload with as many parameters is tried, every argument converted as PowerShell converts a value, and the one needing the fewest conversions is called. So text works where the parameter is an enum, as in a script, and a static method's name matches without regard to case:

```rust
let pattern = PsType::from_name("System.Management.Automation.WildcardPattern")
    .new(&["*.txt".into_ps()?, "IgnoreCase".into_ps()?])?;            // matches as -like does
let rounded = math.call_static("round", &[2.5f64.into_ps()?, "AwayFromZero".into_ps()?])?;   // 3
```

Where two overloads need as few conversions, the one declared first is called. PowerShell makes the same choice for `[string]::Compare('a', 'A', 'Ordinal')`: the text becomes `$true` for the `bool` overload, declared before the `StringComparison` one, and the call returns 0 in both hosts.

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

`Pipeline::invoke` runs a cmdlet, function or alias the session can see, with the parameters bound by name, and returns everything it wrote; `invoke_with_input` pipes a value in first, unrolling a collection into records. The command is added by name to a nested pipeline in the current runspace, and the engine resolves the name as it does for a typed command. So a module's function runs in its module's scope, and a `dynamicparam` block reading the module's `$script:` state works, as it does in every CDXML function such as `New-ScheduledTaskAction`. No script text is built and nothing is parsed, as it would be for a script block.

What the command raises is handled the way it would be had the user run it. A non-terminating error is written to your cmdlet's error stream and the output still comes back, so the caller's `-ErrorAction` sets what happens to it; a terminating error is the `Err`. A name the session cannot resolve is a terminating error naming it.

## Hashtables

`PsHashtable` wraps any `IDictionary`: `new()` makes an empty `Hashtable`; `get`, `set`, `contains`, `len`, `is_empty`, `keys`. `HashMap<String, V>` converts both ways.

## Big integers

`PsBigInt` stores a `System.Numerics.BigInteger` as its little-endian two's-complement bytes, the form `ToByteArray` returns and the byte-array constructor takes. `from_ps` reads one; `into_ps` constructs one; `is_negative` and `doubled` operate on the bytes.

## Secure strings and credentials

```rust
#[param(mandatory)] pub credential: PsCredential,
// ...
let password = self.credential.password.reveal()?;   // decrypts; the UTF-16 copy is zeroed afterward
let fresh = PsCredential::new("ada", "hunter2")?;     // builds a read-only SecureString and a PSCredential
```

`PsSecureString` wraps a `System.Security.SecureString`; `new` builds one from Rust text, `len` reads its length without decrypting, `reveal` decrypts through an unmanaged buffer the runtime zeroes before returning. `PsCredential` is the `UserName` and `Password` of a `PSCredential`; its `IntoPs` runs the engine's constructor, which rejects an empty user name.

## Dates, spans, GUIDs, chars

`PsDateTime`, `PsTimeSpan`, `PsGuid` and `char` are passed through their own vtable entries as ticks, bytes and code units; `PsDateTime::to_utc()` is the only one of them that calls into .NET (`ToUniversalTime()`). The types and their `std` conversions are in [Conversions Reference](Conversions-Reference.md).

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

`PsMemory<T>` is zero copy in the other direction. On .NET, PowerShell gets a `Memory<T>` over the Rust allocation, and the allocation is freed when the view's manager is disposed or collected. On .NET Framework the runtime copies the bytes into a `T[]` and frees the allocation before the write returns. `from_slice` and `TryFrom<Vec<T>>` make one from existing data with one copy, and after `.read_only()` it is written as a `ReadOnlyMemory<T>`. Scripts use the value as any `Memory<T>`: `.Length`, `.ToArray()`, `.Slice(start, length)`, `CopyTo`, or pass it to a .NET API taking a `ReadOnlyMemory<T>`. PowerShell cannot store a `Span<T>`, so `.Span` reads as `$null` from script.

## The module's own memory as `Memory<T>`

```rust
let mapping: Arc<Mapping> = open_mapping(path)?;              // memory PWRS did not allocate
let view = unsafe { PsMemoryView::<u8>::read_only(mapping.as_ptr(), mapping.len(), Arc::clone(&mapping)) }
    .revocable(&self.revocation);                                // optional
ps.write(view)?;                                                 // a ReadOnlyMemory<byte> over the mapping
```

`PsMemoryView<T>` gives .NET a view of memory whose life the module manages some other way: a mapped file, an arena, a region another library manages. The third argument is the view's owner, any `Send + Sync + 'static` value, which lives until PowerShell releases the view. That is when the view's manager is disposed or collected on .NET, and before the write returns on .NET Framework, where the elements are copied into a `T[]` instead. The owner is dropped on whatever thread releases the view, so with an `Arc` of the mapping as the owner, the memory stays mapped for as long as any view of it lives. `writable` makes a `Memory<T>` that .NET code may write through, and `read_only` a `ReadOnlyMemory<T>`. Both are `unsafe`: the caller must make sure that the memory is valid, and not moved, while the owner lives, and for `writable`, that the module has no Rust reference to it while .NET code may write it.

`revocable(&revocation)` ties a view to a `PsRevocation`. Once `revocation.revoke()` has run, writing a view is refused with an `ObjectDisposedException` error record. A view already written throws `ObjectDisposedException` from every later `GetSpan` and `Pin`, which is every read and write through it: `ToArray()`, `CopyTo`, a .NET API reading it. Use it for a capability that is sealed or a region about to be unmapped. The memory under any span taken before the revoke stays valid while the owner lives. A view whose manager has been disposed refuses the same way. `PsMemory<T>` takes `revocable` too.

## Building PSObjects by hand

```rust
let obj = pwrs::object::new_psobject("My.Type");            // PSObject with a PSTypeName
pwrs::object::add_note(&obj, "Name", "x".into_ps()?)?;      // a note property
let name = pwrs::object::property(&obj, "Name")?;           // and back out by name
ps.write_object(&obj)?;
```

This is what `#[psclass(mode = psobject)]` generates; use it directly when the properties are known only at run time.

## Threads

The calls on this page that call into PowerShell must run on a thread PowerShell called the module on. That is the pipeline thread during a phase, or, for as long as the call lasts, the thread that a proxy's property read or method, a completer, a transform, dynamic parameters, a provider operation, or a lifecycle hook was called on. Only `PsScriptBlock::call` and the stream API need the `Pipeline` token as well. A thread the module started is not one of those threads. `PsObject::get`, `set`, `call` and `pin`, `PsType::call_static` and `new`, and a `PsProxy` borrow made from it return an error with id `PwrsOffThread` in a debug build and under `cargo pwrs test`, rather than running. A worker builds plain Rust values and the phase converts and writes them, and a thread that is meant to call into PowerShell itself says so with `pwrs::attach_current_thread()`. See [How To Use Threads](How-To-Use-Threads.md).
