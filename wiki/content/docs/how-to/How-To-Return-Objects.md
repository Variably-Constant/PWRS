---
title: How To Return Objects
weight: 2
---

Scalars, collections, and structured objects on the output stream. Source: `crates/pwrs-macros/src/classes.rs` and `enums.rs` (what `#[psclass]` and `#[psenum]` generate), `crates/pwrs/src/convert.rs` (`IntoPs`), `crates/cargo-pwrs/src/generate.rs` (the CLR types and factories the shell gets), `formats.rs` (default views).

## Scalars and collections

`ps.write(value)` takes any `IntoPs`. Strings, `i64`, `f64`, `bool`, `isize` and `Option` of those go through direct vtable entries, with no handle allocated.

Every other numeric width arrives as its own CLR type rather than widened: an `i32` is a `System.Int32`, a `u16` a `System.UInt16`, an `f32` a `System.Single`. That is worth one handle per written value, and it is worth paying, because the engine types every operator's answer by its operands' widths. `[int]::MaxValue + 1` is a `Double` while the same sum over `Int64` is an `Int64`, so a value that left script as an `Int32` and came back an `Int64` would change what the caller's next operator does with it. `u64` and `usize` have no direct entry and take a handle regardless. `Vec<T>` enumerates into the pipeline, one object per element; wrap it in `PsArray(vec)` to write one array object of the element's CLR type (`long[]`, `string[]`, `object[]`). `HashMap<String, V>` becomes a `Hashtable`. `Option<T>` writes `Some` as `T` and `$null` for `None`.

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

The shell declares `namespace Inventory { public sealed class Item { public string? Name { get; set; } public long Quantity { get; set; } public string[]? Tags { get; set; } public double? Price { get; set; } public global::Inventory.State State { get; set; } } }`. Writing an `Item` packs its fields into a block, crosses once through `factory_new`, and the registered factory builds the CLR object. Field doc comments become `<summary>` on the properties. Up to 64 fields; the same types as parameters, with `bool` fields as `bool` rather than `SwitchParameter`.

Pick copied mode when the object is data: it prints, formats, serializes and pipes like any .NET object, and Rust holds nothing after the write.

`name` defaults to the Rust type name. A dotted name places the type in that namespace; a bare name lands in the module's own namespace (`Pwrs.Modules.<Module>`).

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

A copied class takes `#[psmethods]` statics, and `new` is its constructor: a script writes `[Inventory.Item]::new('widget', 3)`, the value is built in Rust, and the object is made from it field by field. It stays a copied object, so its properties are plain fields a script can set and nothing crosses the boundary to read them. Methods taking `&self` are proxy-only, because no Rust value sits behind a copied object to run them against.

Declaring `new` removes the public parameterless constructor C# otherwise supplies. Without a `new`, `[Inventory.Item]::new()` gives an object of CLR zeros and nulls, which may be one the type never meant to exist, and it binds as valid wherever the type is taken. With one, a script can construct only through it. Since Rust has no overloading, making every argument an `Option` is how one `new` answers every arity, including none, starting from `Default`.

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

A `#[psfield(skip)]` field is what a proxy exists to hold: a mapping, a lock, a connection, anything with no PowerShell face. It is left out of the descriptor, the property list and the read-back path, and its type needs no `Clone`, `FromPs` or `PsTyped`; `#[psmethods]` methods use it freely. A class with such a field cannot be read back by value, so as a plain parameter or method argument type it fails with `PwrsOpaqueClass` at bind time, while it remains an output, a method return type and a field type. A cmdlet takes it as `PsProxy<Cursor>` instead, which reads the value where it is; see [How To Pass Native Data Between Cmdlets](How-To-Pass-Native-Data.md).

The Rust value is boxed and stays in Rust. The shell declares a sealed class deriving `Pwrs.ProxyBase` whose property getters each call `pwrs_proxy_get(class_id, field_id, ptr)` and convert the result. `Dispose()` frees the box exactly once; a finalizer covers the case where nobody disposed it. `IsDisposed` reports the state. A property read after disposal throws `ObjectDisposedException` from the getter; PowerShell's property adapter turns that into `$null`, so check `IsDisposed` rather than the value.

Pick proxy mode when the value is large or expensive to copy and the reader will touch only a few fields, or when the object must stay owned by Rust for its lifetime. A value holding a large allocation should name a `native_bytes` function (`#[psclass(mode = proxy, native_bytes = Cursor::bytes)]`, a `fn(&Self) -> usize`): the managed wrapper is the same small object whatever the value holds, so without the report nothing tells the garbage collector the object is worth collecting, and the value waits on managed allocation to free it. The type must be `Send`: the engine hands the object to any thread, and the value is dropped on the finalizer thread when a script never disposes it; the runtime serializes reads and calls on one object, so no `Sync` is asked for. A value that must stay on the thread that made it (a guard released by its taker, for instance) does not belong in a proxy; keep such work inside one phase or one method call. A proxy that holds a `PsObject` referring back to itself is a cross-runtime cycle and leaks; nothing detects that.

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

A script calls `$cursor.Advance(5)`, `$cursor.Describe()` or `$cursor.Describe('n')`, and `$cursor.Reset()`. A method may be named `Get`, `Set`, `Call` or anything else a collection wants: the generated property getters and method bodies reach the runtime through `base.PwrsGet` and `base.PwrsCall`, so a module's own method never displaces them. The handful of names that would hide an inherited member (`Dispose`, `IsDisposed`, `ToString` and the rest of the `System.Object` set) are a compile error instead; see [Attribute Reference](Attribute-Reference.md). Every `pub fn` taking `&self` or `&mut self` becomes a method of the generated class; arguments use the parameter types (a `#[psclass]` argument is read back through its properties), an `Option<T>` argument is a C# optional parameter, and the return is `PsResult<T>` for any of those types (a returned proxy class is a new proxy object, as `Counter::split` in the hello example shows) or `PsResult<()>` for a `void` method. An `Err` is thrown to the script as a `PwrsException`; a call after `Dispose()` throws `ObjectDisposedException`. Calls and property reads on one object are serialized, so a `&mut self` method never races a read. On the thread inside a call, a property read, a `&self` method and a `with` borrow are shared entries that nest, so a `&self` method may take its own receiver as a by-value argument; a `&mut self` method and a `with_mut` borrow are exclusive, refused with a `PwrsException` while anything is inside the object and refusing everything while they run, so a `&mut self` method cannot take its own receiver by value. Methods need proxy mode: a copied object has no Rust state to call into, and `cargo pwrs build` says so.

A `pub fn` in the block with no receiver is a static method of the class, and a script calls it on the type: `[Ns.Type]::Parse('label=7')`. A static that returns the class returns a new proxy object, so a parser or a factory reads the way the engine's own types read. The static named `new` that returns `PsResult<Self>` is the constructor, and a script writes `[Ns.Type]::new(args)` to make an object without going through a cmdlet; the value it returns is the one the object holds, released on `Dispose()` or collection like any other. So a type has three ways to come into being, and they answer to three kinds of script: a cmdlet for pipelines, a constructor for code that holds the type, and a static for code that has the text.

### Lists, streams and comparisons

PowerShell indexes, counts, enumerates, orders and compares an object only through .NET interfaces, so a proxy class names the `#[psmethods]` methods that answer each, and the class implements the interface through them:

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

`count` and `item` make the class an `IReadOnlyList<T>`: `$view.Count`, `$view[3]`, `foreach ($b in $view)`, `$view | Measure-Object` and `$view -contains 7` all work, in both hosts. `set_item`, a `fn(&mut self, i32, T) -> PsResult<()>`, adds `IList<T>` and makes `$view[3] = 7` write through. `next`, a `fn(&mut self) -> PsResult<Option<T>>`, makes a stream instead: an `IEnumerable<T>` that answers until the method answers `None`, read once. `compare` gives `IComparable` and `IComparable<T>`, so `-lt`, `-gt` and `Sort-Object` order by value, and `equals` with `hash` give `IEquatable<T>` and the `Equals` and `GetHashCode` overrides, so `-eq`, a hashtable key, `Group-Object` and `Select-Object -Unique` go by value. The two methods comparing objects take the other one as `&Self`: the generated method enters both objects before the call, always in the order they were made, so two threads comparing the same pair the opposite way round cannot wait on each other.

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

A method that takes a `PsTask<T>` returns `PsResult<()>`, and the generated method returns `Task<T>` instead: `$ring.PopAsync(100)` answers at once with the task, and a script waits on it with `.Wait()`, `.Result` or `.GetAwaiter().GetResult()`, or hands it to anything that awaits a task. The generated method also takes the caller's `CancellationToken` as its last, optional parameter, `$ring.PopAsync(100, $source.Token)`, which sets the flag `is_cancelled` reads with no crossing. The work settles the task once, from any thread: `complete(value)`, `fail(error)`, which faults it with the error the method would otherwise have thrown, or `cancel()`. Cancellation is the module's to grant, so a pop that has taken an item completes with it rather than losing it; the task above ends canceled only when it took nothing. A task the module drops unsettled, by a panic among other ways, faults with `PwrsTaskDropped` instead of leaving the caller waiting. `PsTask<()>` is a `Task` of no value. Both hosts run it the same way. The task's source is made with `RunContinuationsAsynchronously`, so no continuation runs inline on the Rust thread that settles it.

A stop does not reach a script blocked inside `.Wait()`: stopping a pipeline 500 ms into `.Wait()` on a four-second task returned when the task ended, 3.5 seconds later, in pwsh 7.6.6 and Windows PowerShell 5.1, while the same pipeline waiting with `.Wait(250)` in a loop stopped within 35 ms. Wait in short slices, or hand the method a token the script can cancel.

### A list's operators

A class that is a list is a collection to PowerShell in every way, so its operators filter: `$view -eq 20` answers the elements equal to 20, as it does for an array, rather than comparing the object. Unlike an empty array, an empty list object is still `$true` as a condition, so `if ($view.Count)` is the test for elements.

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

An enum is usable everywhere a type crosses:

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

Every `#[psclass]` type is a parameter type and a field type. The shell declares the class (`PSObject` for psobject mode) and the engine binds only an object of that type; Rust reads it back one field per property name, which is the same path for a copied object, a proxy (each read is one native call), and a PSObject with the notes. A class used as a parameter type needs `Default`.

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
PS> New-RustJob nightly 10 -Failed 2

Name    Done Failed
----    ---- ------
nightly 8    2
```

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
PS> New-RustFrame 3 1
+---+
|   |
+---+
```

The view is the default, so the object shows as its text at the console and through `Out-String`, while `Format-Table` and `Format-List` still show its properties. The text is written as it is: ANSI escape sequences in it reach the host, and PowerShell 7's `$PSStyle.OutputRendering` decides, as for any output, whether they survive a redirection. A `view` on a copied or psobject class, a name that is not a method of the class, a method with another signature, and a method outside `#[psmethods]` are compile errors. `Views.Tests.ps1` in the hello example checks the view, the table and list, and the escape sequences in both hosts.

A copied class can name the text its object shows with `show`, a format over its own property names. The generated class's `ToString()` returns it, so an object nested in another prints as that text rather than its type name:

```rust
#[psclass(name = "Hello.Point", show = "({X}, {Y})")]
pub struct Point {
    pub x: i64,
    pub y: i64,
}
```

```powershell
PS> New-RustSegment 1 2 3 4 | Format-List

Start : (1, 2)
End   : (3, 4)
```

`{{` and `}}` stand for braces. The text is built in C# from the object's own properties, with no call into Rust, on both hosts. A property of a string or class type may hold null, and the text shows a null one as nothing: `Hello.Light` declares `show = "{Name}: {State}"`, so `New-RustLight corner -State Green` shows as `corner: Green`, and as `: Green` once its `Name` is set to `$null`. `show` on a proxy or psobject class, a name that is not a property of the class, and a lone or unclosed brace are compile errors. `Views.Tests.ps1` checks `ToString()`, the text inside another object, the properties, and a null string property, in both hosts.

## Registration

```rust
pwrs::export_module! {
    name: "Inventory",
    cmdlets: [GetItem],
    classes: [Item, Cursor, Summary],
    enums: [State],
}
```

Classes and enums share one id space, classes first in list order, then enums. Each type finds its id by its Rust type, not by its name. A class the library writes but the list leaves out is named in a warning by `cargo pwrs build`, and its write fails at run time with `PwrsUnknownClass`. Two types under one name, compared without regard to case, stop `cargo pwrs build`, which names both, whether the list carries both or one of them and the library writes the other.
