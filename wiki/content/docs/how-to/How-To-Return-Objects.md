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

## Registration

```rust
pwrs::export_module! {
    name: "Inventory",
    cmdlets: [GetItem],
    classes: [Item, Cursor, Summary],
    enums: [State],
}
```

Classes and enums share one id space, classes first in list order, then enums. A class used in a write but missing from the list fails at run time with `PwrsUnknownClass`.
