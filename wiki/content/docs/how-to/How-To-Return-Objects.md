---
title: How To Return Objects
weight: 2
---

Scalars, collections, and structured objects on the output stream. Source: `crates/pwrs-macros/src/classes.rs` and `enums.rs` (what `#[psclass]` and `#[psenum]` generate), `crates/pwrs/src/convert.rs` (`IntoPs`), `crates/cargo-pwrs/src/generate.rs` (the CLR types and factories the shell gets), `formats.rs` (default views).

## Scalars and collections

`ps.write(value)` takes any `IntoPs`. Strings, `i64`, `f64`, `bool`, `isize` and `Option` of those go through direct vtable entries, with no handle allocated.

Every other numeric width arrives as its own CLR type, not widened: an `i32` is a `System.Int32`, a `u16` a `System.UInt16`, an `f32` a `System.Single`. Writing each such value uses one handle. The width matters because PowerShell picks the type of an operator's result from the types of its operands: `[int]::MaxValue + 1` is a `Double`, while the same sum over `Int64` is an `Int64`. So a value that left script as an `Int32` and came back an `Int64` would change what the caller's next operator does with it. `u64` and `usize` have no direct entry and take a handle regardless. `Vec<T>` enumerates into the pipeline, one object per element; wrap it in `PsArray(vec)` to write one array object of the element's CLR type (`long[]`, `string[]`, `object[]`). `HashMap<String, V>` becomes a `Hashtable`. `Option<T>` writes `Some` as `T` and `$null` for `None`.

## Copied classes (the default)

```rust
#[psclass(name = "Inventory.Item")]
#[derive(Default, Clone)]
pub struct Item {
    /// Display name.
    pub name: String,
    pub quantity: i64,
    pub tags: Vec<String>,
    pub price: Option<f64>,
    pub state: State,          // a #[psenum]
}
```

The shell declares `namespace Inventory { public sealed class Item { public string? Name { get; set; } public long Quantity { get; set; } public string[]? Tags { get; set; } public double? Price { get; set; } public global::Inventory.State State { get; set; } } }`. Writing an `Item` packs its fields into a block and makes one call into .NET, through `factory_new`, where the registered factory builds the CLR object. Field doc comments become `<summary>` on the properties. A class takes any number of fields, of the types parameters take, with `bool` fields as `bool` rather than `SwitchParameter`.

Pick copied mode when the object is data: it prints, formats, serializes and pipes like any .NET object, and nothing of it stays in Rust after the write.

`name` defaults to the Rust type name. A dotted name places the type in that namespace; a bare name places it in the module's own namespace (`Pwrs.Modules.<Module>`).

### Constructing a copied object from script

```rust
#[psmethods]
impl Item {
    /// Every argument optional, so [Inventory.Item]::new() starts from Default.
    pub fn new(name: Option<String>, quantity: Option<i64>) -> PsResult<Self> {
        let mut item = Item::default();
        if let Some(name) = name { item.name = name; }
        if let Some(quantity) = quantity {
            if quantity < 0 {
                return Err(PsError::new(ErrorCategory::InvalidArgument, "Quantity", "a quantity is never negative"));
            }
            item.quantity = quantity;
        }
        Ok(item)
    }
}
```

A copied class takes `#[psmethods]` statics, and `new` is its constructor: a script writes `[Inventory.Item]::new('widget', 3)`, the value is built in Rust, and the object is made from it field by field. It stays a copied object, so its properties are plain fields a script can set, and reading them makes no call into Rust. Methods taking `&self` work only on proxy classes, because a copied object has no Rust value to run them on.

Declaring `new` removes the public parameterless constructor C# otherwise supplies. Without a `new`, `[Inventory.Item]::new()` gives an object whose fields are all zeros and nulls, which may not be a valid value of the type, and PowerShell accepts it wherever the type is taken. With one, a script can create the object only through it. Rust has no overloading, so to let one `new` take any number of arguments, including none, make every argument an `Option` and start from `Default`.

A constructor has no pipeline, so where a cmdlet would warn it refuses instead: an `Err` is the exception `::new` throws.

## Proxy classes

```rust
#[psclass(name = "Inventory.Cursor", mode = proxy)]
pub struct Cursor {
    pub label: String,
    pub position: i64,
    #[psfield(skip)]
    handle: RingHandle,      // Rust-only state: no property, no type requirement
}
```

A `#[psfield(skip)]` field stores Rust-only state in a proxy: a mapping, a lock, a connection, anything with no PowerShell type. It is left out of the descriptor and the property list, and it is never read back from an object, and its type needs no `Clone`, `FromPs` or `PsTyped`; `#[psmethods]` methods use it freely. A class with such a field cannot be read back by value, so as a plain parameter or method argument type it fails with `PwrsOpaqueClass` at bind time, while it remains an output, a method return type and a field type. A cmdlet takes it as `PsProxy<Cursor>` instead, which reads the value where it is; see [How To Pass Native Data Between Cmdlets](How-To-Pass-Native-Data.md).

The Rust value is boxed and stays in Rust. The shell declares a sealed class deriving `Pwrs.ProxyBase`, or the .NET class `base` names (see [A .NET base class and .NET interfaces](#a-net-base-class-and-net-interfaces)), whose property getters each call `pwrs_proxy_get(class_id, field_id, ptr)` and convert the result. `Dispose()` frees the boxed value exactly once; a finalizer covers the case where nobody disposed it. `IsDisposed` reports the state. A property read after disposal throws `ObjectDisposedException` from the getter; PowerShell's property adapter turns that into `$null`, so check `IsDisposed` rather than the value.

Pick proxy mode when the value is large or expensive to copy and the reader will read only a few fields, or when the value must stay in Rust for its whole life. A value with a large allocation should name a `native_bytes` function (`#[psclass(mode = proxy, native_bytes = Cursor::bytes)]`, a `fn(&Self) -> usize`). The .NET object is small whatever the Rust value contains, so without that number the garbage collector does not count the Rust memory, and the Rust value stays until other .NET allocations cause a collection. The type must be `Send`: PowerShell may pass the object to any thread, and the value is dropped on the finalizer thread when a script never disposes it. The runtime makes reads and calls on one object take turns, so `Sync` is not required. A value that must stay on the thread that made it (a guard released by its taker, for instance) does not belong in a proxy; do such work inside one phase or one method call. A proxy that stores a `PsObject` referring back to itself forms a cycle between Rust and .NET that is never freed, and nothing detects it.

### Methods on a proxy

```rust
#[psmethods]
impl Cursor {
    /// Moves by `by` and returns the new position.
    pub fn advance(&mut self, by: i64) -> PsResult<i64> {
        self.position += by;
        Ok(self.position)
    }

    /// The label, or `prefix` when given.
    pub fn describe(&self, prefix: Option<String>) -> PsResult<String> { /* ... */ }

    pub fn reset(&mut self) -> PsResult<()> { /* ... */ }
}
```

A script calls `$cursor.Advance(5)`, `$cursor.Describe()` or `$cursor.Describe('n')`, and `$cursor.Reset()`. A method may be named `Get`, `Set`, `Call` or any other name a collection uses. The generated property getters and method bodies call the runtime through `base.PwrsGet` and `base.PwrsCall`, or on a class with a base of its own through its `__pwrsCore` field, so a module's own method never replaces them. The handful of names that would hide an inherited member (`Dispose`, `IsDisposed`, `ToString` and the rest of the `System.Object` set) are a compile error instead; see [Attribute Reference](Attribute-Reference.md).

Every `pub fn` taking `&self` or `&mut self` becomes a method of the generated class. Its arguments use the parameter types (a `#[psclass]` argument is read back through its properties), and an `Option<T>` argument is a C# optional parameter. It returns `PsResult<T>` for any of those types, or `PsResult<()>` for a `void` method; a returned proxy class is a new proxy object, as `Counter::split` in the hello example shows. An `Err` is thrown to the script as a `PwrsException`, or as the .NET exception it names with [`PsError::with_exception`](../reference/Pipeline-Reference.md#pserror-and-errorcategory-errorrs), so a catch of that type catches it. `Hello.Ledger`'s `Add` throws `ArgumentException` for a name already there and its indexer `KeyNotFoundException` for a missing one, as a .NET dictionary does. A call after `Dispose()` throws `ObjectDisposedException`. Calls and property reads on one object run one at a time, so a `&mut self` method never runs at the same time as a read. On the thread inside a call, a property read, a `&self` method and a `with` borrow are shared entries that nest, so a `&self` method may take its own receiver as a by-value argument. A `&mut self` method and a `with_mut` borrow are exclusive. Each is refused with a `PwrsException` while anything is inside the object, and every other entry is refused while it runs, so a `&mut self` method cannot take its own receiver by value. Methods need proxy mode: a copied object has no Rust state to call into, and `cargo pwrs build` reports that.

A `pub fn` in the block with no receiver is a static method of the class, and a script calls it on the type: `[Ns.Type]::Parse('label=7')`. A static that returns the class returns a new proxy object, so a parser or a factory is called the way one on a .NET type is. The static named `new` that returns `PsResult<Self>` is the constructor, and a script writes `[Ns.Type]::new(args)` to make an object without going through a cmdlet. The value it returns is the one behind the object, released on `Dispose()` or garbage collection like any other. So there are three ways to make an object of the type, each for a different kind of script: a cmdlet for pipelines, a constructor for code that names the type, and a static for code that has the text.

### Lists, streams and comparisons

PowerShell indexes, counts, enumerates, orders and compares an object only through .NET interfaces, so a proxy class names the `#[psmethods]` methods that do each, and the class implements the interface through them:

```rust
#[psclass(name = "Ring.View", mode = proxy, count = element_count, item = at)]
pub struct View { /* ... */ }

#[psmethods]
impl View {
    pub fn element_count(&self) -> PsResult<i32> { /* ... */ }
    pub fn at(&self, index: i32) -> PsResult<u8> { /* ... */ }
}

#[psclass(name = "Ring.Key", mode = proxy, compare = order, equals = same_as, hash = hash_code)]
pub struct Key { /* ... */ }

#[psmethods]
impl Key {
    pub fn order(&self, other: &Self) -> PsResult<i32> { /* ... */ }
    pub fn same_as(&self, other: &Self) -> PsResult<bool> { /* ... */ }
    pub fn hash_code(&self) -> PsResult<i64> { /* ... */ }
}
```

`count` and `item` make the class an `IReadOnlyList<T>`: `$view.Count`, `$view[3]`, `foreach ($b in $view)`, `$view | Measure-Object` and `$view -contains 7` all work, in both hosts. `set_item`, a `fn(&mut self, i32, T) -> PsResult<()>`, adds `IList<T>` and makes `$view[3] = 7` write through. `next`, a `fn(&mut self) -> PsResult<Option<T>>`, makes a stream instead: an `IEnumerable<T>` that gives items until the method returns `None`, and can be read once. `compare` gives `IComparable` and `IComparable<T>`, so `-lt`, `-gt` and `Sort-Object` order by value, and `equals` with `hash` give `IEquatable<T>` and the `Equals` and `GetHashCode` overrides, so `-eq`, a hashtable key, `Group-Object` and `Select-Object -Unique` go by value. The two methods comparing objects take the other one as `&Self`. The generated method takes both objects' gates before the call, always in the order the objects were made, so two threads comparing the same pair the opposite way round cannot wait on each other.

### Methods that return tasks

```rust
#[psmethods]
impl Ring {
    /// Pops the next item, waiting on a thread of its own.
    pub fn pop_async(&self, timeout_ms: i64, task: PsTask<i64>) -> PsResult<()> {
        let ring = self.ring.clone();
        std::thread::spawn(move || {
            while !task.is_cancelled() {
                if let Some(item) = ring.try_pop() {
                    return task.complete(item);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            task.cancel();
        });
        Ok(())
    }
}
```

A method that takes a `PsTask<T>` returns `PsResult<()>`, and the generated method returns `Task<T>` instead: `$ring.PopAsync(100)` returns the task at once, and a script waits on it with `.Wait()`, `.Result` or `.GetAwaiter().GetResult()`, or passes it to anything that awaits a task. The generated method also takes the caller's `CancellationToken` as its last, optional parameter, `$ring.PopAsync(100, $source.Token)`, which sets the flag that `is_cancelled` reads without a call into .NET. The work finishes the task once, from any thread: `complete(value)`, `fail(error)`, which fails it with the error the method would otherwise have thrown, or `cancel()`. Canceling the token only sets that flag, and the task ends canceled only when the module's code calls `cancel()`, so a pop that has already taken an item completes with it instead of losing it. The task above ends canceled only when it took nothing. A task the module drops without finishing it, after a panic for example, fails with `PwrsTaskDropped` instead of leaving the caller waiting. `PsTask<()>` is a `Task` of no value, and `PsTask<Option<T>>` is a task whose result may be `$null`: `Task<long?>` for `Option<i64>`, completed with `$null` by `complete(None)`. Both hosts run it the same way. The task's source is made with `RunContinuationsAsynchronously`, so no continuation runs inline on the Rust thread that finishes it.

A stop does not reach a script blocked inside `.Wait()`. Stopping a pipeline 500 ms into `.Wait()` on a four-second task returned when the task ended, 3.5 seconds later, in pwsh 7.6.6 and Windows PowerShell 5.1. The same pipeline waiting with `.Wait(250)` in a loop stopped within 35 ms. Wait in short slices, or pass the method a token the script can cancel.

### A list's operators

A class that is a list is a collection to PowerShell in every way, so its operators filter: `$view -eq 20` returns the elements equal to 20, as it does for an array, instead of comparing the object. Unlike an empty array, an empty list object is still `$true` as a condition, so `if ($view.Count)` is the test for elements.

### A .NET base class and .NET interfaces

```rust
#[psclass(name = "Hello.ByteStream", mode = proxy, base = "System.IO.Stream")]
pub struct ByteStream {
    #[psfield(skip)]
    bytes: Vec<u8>,
    #[psfield(skip)]
    at: usize,
}

#[psmethods]
impl ByteStream {
    pub fn new(text: String) -> PsResult<ByteStream> { /* ... */ }

    #[member = "Length"]
    pub fn length(&self) -> PsResult<i64> {
        Ok(self.bytes.len() as i64)
    }

    #[member = "Position"]
    pub fn position(&self) -> PsResult<i64> { /* ... */ }

    #[member = "Position"]
    pub fn set_position(&mut self, value: i64) -> PsResult<()> { /* ... */ }

    /// Reads up to `count` bytes into the caller's own `buffer`.
    #[member = "Read"]
    pub fn read(&mut self, mut buffer: PsArrayMut<u8>, offset: i32, count: i32) -> PsResult<i32> {
        let mut view = buffer.pin()?;
        // copy from self.bytes into view[offset..], and move self.at
    }

    // CanRead, CanSeek, CanWrite, Flush, Write, Seek and SetLength are
    // backed the same way.
}
```

`base` makes the generated class derive from a .NET class instead of `Pwrs.ProxyBase`, and `implements` adds .NET interfaces. A method marked `#[member = "Name"]` is that member of the base or of an interface. .NET's own code then takes the object as it takes any object of the type:

```powershell
$s = [Hello.ByteStream]::new("one`ntwo")
[System.IO.StreamReader]::new($s).ReadToEnd()
```

`cargo pwrs build` reads the base and each interface from the reference assemblies the shell compiles against, for both target frameworks. It declares each member with the member's own signature, as `public override int Read(byte[] buffer, int offset, int count)`. A method marked as a member is declared only as that member, not under a name of its own. An interface's member is public under its name, so a script calls it as any method, unless the class has another member of that name. It is then implemented explicitly. Every abstract member of the base and of each interface needs a method, and the build names each one that has none.

Each argument the member receives must convert to the method's argument by C#'s own implicit conversions: the same type, a wider number, `object`, or a nullable value. The method's result must convert to the member's type the same way. A `PsObject` result also reaches any reference type, converted as PowerShell converts it when the call runs. A `PsArrayMut<u8>` argument is the caller's own `byte[]`, which `pin` reads and writes in place, so `Read` fills the buffer a `StreamReader` passes it.

A property's getter is a method taking `&self` and returning the value, and its setter takes the value and returns nothing; both are marked with the property's name. A name two members share is written with its declaring type, as `#[member = "System.Collections.IComparer.Compare"]`. A member that one target framework has and the other cannot be overridden on, such as `Stream.CopyTo(Stream, int)`, which is not virtual in .NET Standard 2.0, is declared for the first alone, and the build says so in a note.

The base class's constructor runs before Rust's `new`. Without `#[base(...)]`, `new` calls the base constructor that takes no arguments. `#[base(message)]` on `new` passes the arguments it names to the constructor they fit:

```rust
#[psclass(name = "Hello.Failure", mode = proxy, base = "System.Exception")]
pub struct Failure {
    pub code: i32,
    #[psfield(skip)]
    message: String,
}

#[psmethods]
impl Failure {
    #[base(message)]
    pub fn new(message: String, code: i32) -> PsResult<Failure> {
        Ok(Failure { code, message })
    }

    /// The message, for a failure Rust makes outside `new`.
    #[base]
    pub fn base_args(&self) -> PsResult<String> {
        Ok(self.message.clone())
    }
}
```

`throw [Hello.Failure]::new('disk full', 28)` throws an exception whose `Message` is `disk full` and whose `Code` is 28. An object Rust makes outside `new`, as a cmdlet writing one does, gets the base constructor's arguments from the method marked `#[base]`. That method takes `&self` and returns one value, or a tuple of them. A class whose base has no constructor taking no arguments needs one.

`Dispose()` frees the Rust value, as on any proxy. On a base with a `Dispose(bool)` to override, such as `Stream`, the class overrides it; on any other base, the class implements `IDisposable`. `IsDisposed` reports the state, unless the base has a member of that name. `Bases.Tests.ps1` in the hello example checks in both hosts that a `StreamReader` reads `Hello.ByteStream` and `CopyTo` copies it, that `-f` formats `Hello.Fraction` through `IFormattable`, that `[Array]::Sort` orders through the `IComparer` `Hello.Descending`, and that `Hello.Failure` is thrown and caught.

### Out, ref and generic members, and stubs

```rust
#[psclass(
    name = "Hello.Ledger",
    mode = proxy,
    implements = ["System.Collections.Generic.IDictionary<string, long>"],
    stubs = ["Add", "Remove", "Contains", "CopyTo"]
)]
pub struct Ledger {
    #[psfield(skip)]
    entries: BTreeMap<String, i64>,
}

#[psmethods]
impl Ledger {
    /// TryGetValue: Some gives true and the value, None false and 0.
    #[member = "TryGetValue"]
    pub fn try_get(&self, key: String) -> PsResult<Option<i64>> {
        Ok(self.entries.get(&key).copied())
    }

    // Item, Keys, Values, Add(key, value), ContainsKey, Remove(key), Count,
    // IsReadOnly, Clear and both GetEnumerators are backed as above.
}
```

A member's out arguments come back after its result, so `bool TryGetValue(string key, out long value)` is backed by a method returning `(bool, i64)`, or by this one returning `Option<i64>`, whose `None` gives `false` and 0. A member returning nothing is backed by a method returning its out argument, or a tuple of them. A `ref` argument is a `&mut` one, whose value the member copies back to the caller:

```rust
#[member = "ReorderArgumentArray"]
pub fn reorder(&mut self, args: &mut PsObject, _state: PsObject) -> PsResult<()> {
    let mut values = Vec::<PsObject>::from_ps(args)?;
    values.reverse();
    *args = PsArray(values).into_ps()?;
    Ok(())
}
```

```powershell
$ledger = [Hello.Ledger]::new()
$ledger['seven'] = 7
$value = 0
$ledger.TryGetValue('seven', [ref]$value)   # True, and $value is 7
```

A generic member's method takes the member's type arguments first, by name, and each value whose type names one of them as a `PsObject`. `Hello.ConstantProvider` backs `IQueryProvider.Execute<TResult>` with `fn execute_as(&mut self, types: &[PsType], expression: PsObject) -> PsResult<PsObject>`, and the member converts what it returns to `TResult` as PowerShell converts a value.

`stubs` names the abstract members a class leaves without a method: each throws `NotSupportedException` naming the class and the member, and `cargo pwrs build` lists them in a note. `Hello.Ledger` leaves the four members taking a `KeyValuePair<string, long>` to its stubs. A name in `stubs` that leaves nothing to throw is refused, so a stub cannot outlive the member it stood for. The [Attribute Reference](../reference/Attribute-Reference.md#psmethods) gives each rule. `Bases.Tests.ps1` checks in both hosts `Hello.Ledger`'s members, `TryGetValue` through a script and through .NET reflection, `Hello.ReverseBinder`'s ref and out arguments of `System.Reflection.Binder.BindToMethod`, `Hello.ConstantProvider`'s `Execute<TResult>`, and each stub's `NotSupportedException`.

## Conversions to and from other types

A cast and the parameter binder convert a value to a class, or a class to another type, through the class's conversion operators and its type converter. `implicit` and `explicit` list the methods behind C# conversion operators:

```rust
#[psclass(name = "Hello.Fraction", mode = proxy, implicit = [from_integer], explicit = [to_double])]
pub struct Fraction {
    pub numerator: i64,
    pub denominator: i64,
}

#[psmethods]
impl Fraction {
    /// From an int: a static taking another type and returning the class.
    pub fn from_integer(value: i32) -> PsResult<Fraction> {
        Ok(Fraction { numerator: i64::from(value), denominator: 1 })
    }

    /// To a double: a method taking nothing and returning another type.
    pub fn to_double(&self) -> PsResult<f64> {
        Ok(self.numerator as f64 / self.denominator as f64)
    }
}
```

```powershell
[double][Hello.Fraction]::new(1, 2)   # 0.5
([Hello.Fraction]5).Denominator       # 1
Get-RustFractionValue -Fraction 3     # the binder converts 3 to a Fraction
```

PowerShell finds an operator by the exact type of the value, with no widening: `5` is an `Int32`, which an operator taking a `long` does not reach, so `from_integer` takes an `i32`.

`converter` names two functions behind a `PSTypeConverter`, for conversions an operator cannot express: from any value, and to any type.

```rust
#[psclass(name = "Hello.Temperature", mode = proxy, converter = [from_any, to_any])]
pub struct Temperature {
    pub celsius: f64,
}

#[psmethods]
impl Temperature {
    /// `21.5C`, `70F` or a number of degrees; None for anything else.
    pub fn from_any(value: PsObject) -> PsResult<Option<Temperature>> { /* ... */ }

    /// A double, an int or text; None for any other type.
    pub fn to_any(&self, to: PsType) -> PsResult<Option<PsObject>> {
        match to.name() {
            "System.Double" => Ok(Some(self.celsius.into_ps()?)),
            "System.Int32" => Ok(Some((self.celsius.round() as i32).into_ps()?)),
            _other => Ok(None),
        }
    }
}
```

```powershell
([Hello.Temperature]'70F').Celsius    # 21.1111111111111
[int][Hello.Temperature]'21.5C'       # 22
Get-RustCelsius -Temperature 212F     # 100
```

`None` means the class cannot convert that value, or cannot convert to that type, and PowerShell goes on to its other conversions, or reports that it cannot convert. PowerShell tries the converter before a constructor. Each `CanConvertFrom` or `CanConvertTo` call stores its result for the conversion that follows, so a conversion calls the function once. `Operators.Tests.ps1` in the hello example checks each conversion in both hosts, through casts and through a cmdlet's parameter, and that one conversion calls `from_any` once. The [Attribute Reference](../reference/Attribute-Reference.md#psclass) gives each rule.

## Classes a script class derives from

A class is sealed unless it is marked `inheritable`. An inheritable proxy class has a protected constructor whose Rust value is its `Default`, and its methods are virtual:

```rust
#[psclass(name = "Hello.Shape", mode = proxy, inheritable)]
pub struct Shape {
    pub name: String,
}

impl Default for Shape {
    fn default() -> Self {
        Shape { name: "shape".to_string() }
    }
}

#[psmethods]
impl Shape {
    pub fn area(&self) -> PsResult<f64> {
        Ok(0.0)
    }
}
```

```powershell
class Square : Hello.Shape {
    [double] $Side = 3
    [double] Area() { return $this.Side * $this.Side }
}
$square = [Square]::new()
$square.Name                       # shape, the Default's
Measure-RustShape -Shape $square   # shape 9: Rust calls Area through the object
```

The script class must be defined once the module is imported, since PowerShell resolves the base type when it parses the class: in a script that imports the module, define it in a script block made after the import, or in a file dot-sourced after it. A call through the object reaches the override, from a script, from .NET code and from Rust's `object().call(...)`; Rust's own `with(|s| s.area())` runs the Rust method. A cast to the base type, `([Hello.Shape]$square).Area()`, is PowerShell's call of the base's own method. `Bases.Tests.ps1` checks the construction, the override and passing the object back to Rust in both hosts.

## PSObject classes

```rust
#[psclass(name = "Inventory.Summary", mode = psobject)]
pub struct Summary {
    pub count: i64,
    pub total: f64,
}
```

No CLR type is generated. Writing a `Summary` builds a `PSObject`, inserts `Inventory.Summary` as its `PSTypeName`, and adds one note property per field. Pick this mode when a type name for formatting is all you need, or when the shape must stay loose.

## Enums

```rust
#[psenum(name = "Inventory.State")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Draft,
    Active = 10,
    Retired,
}
```

The shell declares `namespace Inventory { public enum State : long { Draft = 0, Active = 10, Retired = 11 } }`. Discriminants follow Rust's rules: an explicit value, else the previous value plus one. The enum must be fieldless. List it under `enums` in `export_module!`.

An enum can be used in each place a type can:

- as a parameter (`pub state: State`, `Option<State>`, `Vec<State>`), where the binder converts member names case-insensitively and completes them;
- as a class field, in any of the three modes;
- as an output, `ps.write(State::Active)`, which arrives as an `Inventory.State` value.

`FromPs` reads the value back as its underlying integer and returns an `InvalidData` error for a value that matches no variant.

## Accepting objects back

```rust
#[psclass(name = "Inventory.Order")]
#[derive(Default, Clone)]
pub struct Order {
    pub item: Item,             // a class as a field
    pub extras: Vec<Item>,      // or a list of them
    pub summary: Option<Summary>,
}

#[cmdlet(verb = "Get", noun = "OrderTotal")]
#[derive(Default)]
pub struct GetOrderTotal {
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub order: Order,           // a class as a parameter
}
```

Every `#[psclass]` type is a parameter type and a field type. The shell declares the class (`PSObject` for psobject mode), and the engine binds only an object of that type. Rust reads it back one field per property name, which is the same path for a copied object, a proxy (each read is one native call), and a PSObject with the notes. A class used as a parameter type needs `Default`.

## Default views

When a module declares classes, `cargo pwrs build` writes `<Module>.Format.ps1xml` with one view per copied, proxy or psobject class: a table when the class has at most five fields, a list otherwise. The manifest's `FormatsToProcess` loads it. Enums get no view.

A class can name the columns of its table with `columns`, a list of its own property names, in any mode. The format file then gets a view `<Type>.Columns`, a table of exactly those columns in that order, ahead of the class's table or list, so a class with any number of fields can default to a short table:

```rust
#[psclass(name = "Hello.Job", columns = ["Name", "Done", "Failed"])]
pub struct Job {
    pub name: String,
    pub items: i64,
    pub done: i64,
    pub failed: i64,
    pub elapsed_ms: i64,
    pub workers: i32,
    pub queue: String,
}
```

```powershell
New-RustJob nightly 10 -Failed 2
```

```text
Name    Done Failed
----    ---- ------
nightly    8      2
```

![PowerShell 7 showing the job as a table of its three named columns, the header in green and the numbers right-aligned](/images/how-to-return-objects-columns.png)

`Format-Table` shows the same columns, and `Format-List` still shows all seven properties. The view of every field stays in the file as `<Type>`, so `Format-List -View Hello.Job` names it. An empty list, a name that is not a property of the class, and a name given twice are compile errors.

A proxy class can instead be shown by text it renders. `view` names a `#[psmethods]` method taking `&self` and returning `PsResult<String>`, and the format file gets a custom view for the class, ahead of its table or list, that writes what the method returns:

```rust
#[psclass(name = "Hello.Frame", mode = proxy, view = draw)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub color: bool,
}

#[psmethods]
impl Frame {
    /// The frame drawn in text.
    pub fn draw(&self) -> PsResult<String> {
        // ...
    }
}
```

```powershell
New-RustFrame 3 1
```

```text
+---+
|   |
+---+
```

The view is the default, so the object shows as its text at the console and through `Out-String`, while `Format-Table` and `Format-List` still show its properties. The text is written as it is: ANSI escape sequences in it reach the host, and PowerShell 7's `$PSStyle.OutputRendering` sets, as for any output, whether they remain when the output is redirected. A `view` on a copied or psobject class, a name that is not a method of the class, a method with another signature, and a method outside `#[psmethods]` are compile errors. `Views.Tests.ps1` in the hello example checks the view, the table and list, and the escape sequences in both hosts.

A copied class can name the text its object shows with `show`, a format over its own property names. The generated class's `ToString()` returns it, so an object nested in another prints as that text rather than its type name:

```rust
#[psclass(name = "Hello.Point", show = "({X}, {Y})")]
pub struct Point {
    pub x: i64,
    pub y: i64,
}
```

```powershell
New-RustSegment 1 2 3 4 | Format-List
```

```text
Start : (1, 2)
End   : (3, 4)
```

![PowerShell 7 showing the segment as a list, its labels in green and each point as the text its show format makes](/images/how-to-return-objects-show.png)

`{{` and `}}` stand for braces. The text is built in C# from the object's own properties, with no call into Rust, on both hosts. A property of a string or class type may be null, and the text shows a null one as nothing. `Hello.Light` declares `show = "{Name}: {State}"`, so `New-RustLight corner -State Green` shows as `corner: Green`, and as `: Green` once its `Name` is set to `$null`. `show` on a proxy or psobject class, a name that is not a property of the class, and a lone or unclosed brace are compile errors. `Views.Tests.ps1` checks `ToString()`, the text inside another object, the properties, and a null string property, in both hosts.

## Registration

```rust
pwrs::export_module! {
    name: "Inventory",
    cmdlets: [GetItem],
    classes: [Item, Cursor, Summary],
    enums: [State],
}
```

Classes and enums share one id space, classes first in list order, then enums. Each type finds its id by its Rust type, not by its name. A class the library writes but the list leaves out is named in a warning by `cargo pwrs build`, and its write fails at run time with `PwrsUnknownClass`. Two types under one name, compared without regard to case, stop `cargo pwrs build`, which names both, whether the list has both or one of them and the library writes the other.
