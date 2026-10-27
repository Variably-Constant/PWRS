---
title: Attribute Reference
weight: 1
---

Every key each attribute accepts, as parsed by `crates/pwrs-macros/src/params.rs`, `classes.rs`, `methods.rs`, `enums.rs` and `completers.rs` (which also has `#[transform]`), and the `export_module!` grammar in `crates/pwrs/src/runtime.rs`. An unknown key is a compile error naming it.

## `#[cmdlet(...)]`

On a struct with named fields. Requires `verb` and `noun`. The struct must implement `Default` and `Cmdlet`.

| Key | Form | Effect |
|---|---|---|
| `verb` | `verb = "Get"` | required; the `[Cmdlet]` verb |
| `noun` | `noun = "Thing"` | required; the `[Cmdlet]` noun |
| `supports_should_process` | flag | `SupportsShouldProcess = true` |
| `confirm_impact` | `confirm_impact = "High"` | `ConfirmImpact = ConfirmImpact.High` (`None`, `Low`, `Medium`, `High`) |
| `default_parameter_set` | `default_parameter_set = "ByName"` | `DefaultParameterSetName` |
| `alias` | `alias = ["gt", "getit"]` or `alias = "gt"` | `[Alias]` on the cmdlet; also exported by the manifest |
| `output` | `output = ["System.String"]` | `[OutputType]` and the help's return values |

The doc comment on the struct follows rustdoc's own split: the first paragraph, every line up to the first blank one, is the synopsis, joined into a sentence; what follows is the description. Lines under a `# Examples` (or `# Example`) heading, one command per line, become help examples.

## `#[param(...)]`

On a field of a `#[cmdlet]` struct. `#[param]` alone is valid, and a cmdlet takes any number of parameters. The parameter name is the field name in PascalCase.

| Key | Form | Effect |
|---|---|---|
| `mandatory` | flag | `Mandatory = true` |
| `position` | `position = 0` | `Position` |
| `set` | `set = "ByName"`, or `set = ["Path", "LiteralPath"]` for a parameter in several sets | one `[Parameter]` per set, each with `ParameterSetName` and the same other arguments; the parameter is in no other set, and help writes a syntax line per set. Without `set` the parameter is in every set. A set named twice, in any case, or a blank name is a compile error |
| `value_from_pipeline` | flag | `ValueFromPipeline = true` |
| `value_from_pipeline_by_property_name` | flag | `ValueFromPipelineByPropertyName = true` |
| `value_from_remaining` | flag | `ValueFromRemainingArguments = true` |
| `alias` | `alias = ["n"]` | `[Alias]` on the parameter |
| `help` | `help = "..."` | `HelpMessage`; defaults to the field's doc comment |
| `validate_set` | `validate_set = ["a", "b"]` | `[ValidateSet]` |
| `validate_range` | `validate_range(1, 100)` | `[ValidateRange(1L, 100L)]`; two integer literals, negatives allowed |
| `validate_pattern` | `validate_pattern = "^[a-z]+$"` | `[ValidatePattern]`, which the engine applies with `RegexOptions.IgnoreCase`, so `^[a-z]+$` accepts `ABC` |
| `validate_not_null_or_empty` | flag | `[ValidateNotNullOrEmpty]` |
| `dont_show` | flag | `DontShow = true` |
| `literal_path` | flag | declares the parameter the way PowerShell declares a literal path everywhere else: `[Alias("PSPath", "LP")]` and `ValueFromPipelineByPropertyName = true`, so it binds from a piped object's `PSPath`. An alias the author named themselves is not added twice. The cmdlet still resolves the path itself, through `ps.resolve_path(path, literal)` |
| `raw` | flag | the shell declares the property as `object` rather than the parameter's own CLR type, so PowerShell's binder passes the argument through without converting it; Rust still converts to the field's type. Only for a parameter that is passed as a handle (`Vec<T>`, `PsObject`, a class, an enum) |
| `allow_empty_collection` | flag | `[AllowEmptyCollection]`: a mandatory collection parameter takes an empty array, which the engine otherwise refuses with `ParameterArgumentValidationErrorEmptyArrayNotAllowed`. Only for a parameter that is passed as a handle: a `Vec<T>`, or a `PsObject` declared as an array with `clr` |
| `allow_empty_string` | flag | `[AllowEmptyString]`: a mandatory string parameter takes an empty string, such as a blank line piped from `Get-Content`, which the engine otherwise refuses with `ParameterArgumentValidationErrorEmptyStringNotAllowed` before the cmdlet runs. Only for a string parameter: `String`, `PathBuf`, or an `Option` of either |
| `validate_length` | `validate_length(1, 8)` | `[ValidateLength(1, 8)]`: each string has at least min and at most max characters. Only for a string parameter: `String`, `PathBuf`, `Vec<String>`, or an `Option` of one. Bounds PowerShell refuses are a compile error: a min below 0, a max below 1, or a max below the min |
| `validate_count` | `validate_count(1, 3)` | `[ValidateCount(1, 3)]`: the array has at least min and at most max elements, with the same bounds as `validate_length`. Only for an array parameter: a `Vec<T>`, or a `PsObject` declared as an array with `clr` |
| `validate_script` | `validate_script = "$_ % 2 -eq 0"` | `[Pwrs.ValidateScriptText]`, the shell's form of `[ValidateScript]`, since an attribute argument cannot be a script block. It runs the script for each element, in a scope of its own, with the element as `$_`, and refuses an element the script does not return a true value for, with the message PowerShell's `ValidateScript` gives. A blank script is a compile error |
| `validate_not_null` | flag | `[ValidateNotNull]`: `$null` is refused. Only for a parameter whose type can be null: a string, a `PsObject`, a `Vec<T>` or a class. A number or `bool` is a compile error, even inside an `Option`, and an enum or another value type stops `cargo pwrs build`, because the shell declares each as a value type. Not with `allow_null` |
| `allow_null` | flag | `[AllowNull]`: a mandatory parameter takes `$null`, which the engine otherwise refuses with `ParameterArgumentValidationErrorNullNotAllowed`. Only for a parameter whose type can be null, as for `validate_not_null`. An `Option<String>` given `$null` is `None`, and a `String` is empty |
| `supports_wildcards` | flag | `[SupportsWildcards]`, and the help says the parameter accepts wildcard characters. The cmdlet does the matching itself, for example through `System.Management.Automation.WildcardPattern` |
| `clr` | `clr = "byte[]"`, `clr = "System.IO.FileInfo"` | on a `PsObject` or `Option<PsObject>` parameter: the shell declares the property as that CLR type, so the binder converts to it and binds a piped value to the parameter whose type it matches, and the value is still passed as a handle, not copied. Not with `raw`, and not a type a Rust field spells itself (`int`, `string`, `object` and the other C# type keywords), which is a compile error |

Field types are listed in [Conversions Reference](Conversions-Reference.md). `Option<Option<T>>` and `Vec<Option<T>>` are rejected.

`raw` is for bulk data. PowerShell's binder checks a parameter declared as an array type before the module is entered. On Windows PowerShell 5.1 it goes through every element: 159 to 163 ms for a 4 MB `byte[]` and 1.3 s for 32 MB, whether the array arrives bare or wrapped in a `PSObject`. The same parameter marked `raw` costs 0.9 ms and 5 ms. On pwsh 7.6.6 both cost the copy into the `Vec<u8>`, about 0.4 ms and 4.7 ms. An `object[]` of the values boxed, bound to a parameter declaring the array type, is converted one by one on both hosts, 717 ms and 2.2 s for 4 MB. `docs/PERF.md` has the table. What `raw` costs is the binder's own work: the parameter shows as `object` in `Get-Help` and completion, a wrong-typed argument is not refused at bind time, and a value the conversion cannot take is a `PwrsConversionError` from Rust instead. A string reaching a `Vec<u8>` this way enumerates as its characters rather than being rejected.

`clr` is the other way round: the binder does its work and Rust reads the result in place. A `PsObject` parameter is declared `object`, and an `object` parameter taking pipeline input accepts every piped record. Declared `byte[]` with `clr`, it accepts a byte array whole and a lone byte as an array of one, while a piped `FileInfo`, which does not convert, goes on to a `literal_path` parameter bound from its `PSPath`. The module then reads the bytes through `pin`, without a copy. `Measure-RustInput` in the hello example does both, and `Bytes.Tests.ps1` checks each case in both hosts, including that the cmdlet can change the caller's own array.

## `#[psclass(...)]`

On a struct with named fields, any number of them. Fields need no attribute; each field's doc comment becomes its property summary.

| Key | Form | Effect |
|---|---|---|
| `name` | `name = "Ns.Type"` | the CLR type name (or the `PSTypeName` in psobject mode); defaults to the Rust name. A dotted name places the type in that namespace; a bare name places it in `Pwrs.Modules.<Module>`. Two classes or enums of one module under one name, compared without regard to case, stop `cargo pwrs build`, naming both, whether `export_module!` lists both or one of them and the library writes the other |
| `mode` | `mode = copied`, `mode = proxy`, `mode = psobject` (also as strings) | the output mode; default `copied` |
| `copied`, `proxy`, `psobject` | flags | the same as `mode = ...` |
| `native_bytes` | `native_bytes = path::to_fn`, a `fn(&Self) -> usize` | proxy mode only: the bytes of native memory the value owns. The object reports them with `GC.AddMemoryPressure` when it is made, gets them again after each method call and each `PsProxy::with_mut`, and removes them when the value is freed. A compile error on a copied or psobject class |
| `show` | `show = "({X}, {Y})"`, a format over the class's own property names | copied mode only: the generated class's `ToString()` is the format with each `{Name}` replaced by that property's value, `{{` and `}}` standing for braces, so the object prints as that text wherever PowerShell shows it inside another object or turns it into a string. It runs in C# with no call into Rust, on both hosts. A compile error on a proxy or psobject class, for a name that is not a property of the class, and for a lone or unclosed brace |
| `view` | `view = method`, a `#[psmethods]` method `fn(&self) -> PsResult<String>` | proxy mode only: the class's default view is the text the method returns, written as it is, through a custom view in the module's format file ahead of the class's table or list, which `Format-Table` and `Format-List` still show. A compile error on a copied or psobject class, for a name that is not such a method, and for a method outside `#[psmethods]` |
| `columns` | `columns = ["Name", "Done"]`, the class's own property names | any mode: the class's default view is a table of exactly these properties, in this order, whatever its number of fields, through a view `<Type>.Columns` in the module's format file ahead of the table or list of every field the class gets otherwise (the view `<Type>`). `Format-List` still shows every property. With `view` as well, the method's text stays the default and `Format-Table` shows these columns. A compile error for an empty list, a name that is not a property of the class, a name given twice, and a view method named `columns` |
| `count`, `item` | `count = method`, a `#[psmethods]` method `fn(&self) -> PsResult<i32>`; `item = method`, one `fn(&self, i32) -> PsResult<T>` | proxy mode only, the two together: the class implements `IReadOnlyList<T>`, `T` being the item method's return type, with a public `Count` and a public indexer `this[int]` that call the methods, and an enumerator that reads `Count` once and then each index. `$obj.Count`, `$obj[i]`, `foreach` and the pipeline reach the elements in both hosts. A compile error for one without the other, for a method that is not such a `#[psmethods]` method, and for a field whose property would be `Count` or `Item`; `cargo pwrs build` refuses a method whose PowerShell name is `Count` or `Item` |
| `set_item` | `set_item = method`, a `#[psmethods]` method `fn(&mut self, i32, T) -> PsResult<()>` | with `count` and `item`: the class also implements `IList<T>`, and the indexer gains a setter calling the method, so `$obj[i] = value` writes through. The size is fixed: `ICollection<T>.IsReadOnly` is `true`, as an array's is, and `Add`, `Insert`, `Remove`, `RemoveAt` and `Clear` throw `NotSupportedException`; `IndexOf`, `Contains` and `CopyTo` go through the indexer. A compile error without `item`, and when `T` differs from the item method's |
| `next` | `next = method`, a `#[psmethods]` method `fn(&mut self) -> PsResult<Option<T>>` | proxy mode only: the class implements `IEnumerable<T>`, enumerating what the method returns until it returns `None`. The stream is the object's own, so a second enumeration goes on from where the first stopped. A compile error with `item`, which makes the class a list already |
| `compare` | `compare = method`, a `#[psmethods]` method `fn(&self, other: &Self) -> PsResult<i32>`, less than, equal to or greater than 0 | proxy mode only: the class implements `IComparable` and `IComparable<T>` through the method, so `-lt`, `-gt`, `-le`, `-ge` and `Sort-Object` order objects by value. Null comes first, an object compares equal to itself without a call, and another type is an `ArgumentException` |
| `equals`, `hash` | `equals = method`, a `#[psmethods]` method `fn(&self, other: &Self) -> PsResult<bool>`; `hash = method`, one `fn(&self) -> PsResult<i64>` | proxy mode only, the two together: the class implements `IEquatable<T>` and overrides `Equals(object)` and `GetHashCode()` (the 64-bit hash folded to 32), so `-eq`, `-ne`, `-contains`, a hashtable key, `Group-Object`, `Select-Object -Unique` and `Sort-Object -Unique` go by value. A compile error for one without the other |
| `add`, `sub`, `mul`, `div`, `rem` | `add = method` and the others, each a `#[psmethods]` method whose left operand is the class. On a proxy class it is `fn(&self, other: T) -> PsResult<R>`, or a static method taking `&Self` first; on a copied class it is a static method taking `Self` first | the class declares the C# operator for each: `op_Addition`, `op_Subtraction`, `op_Multiply`, `op_Division` and `op_Modulus`, which PowerShell's `+`, `-`, `*`, `/` and `%` call. Each calls the method, which may take any other operand type and return any type. PowerShell runs unary minus as `0 - value`, which needs a number as the left operand, so `-$value` is refused. An operator on a psobject class, or one whose method returns nothing or has another shape, is a compile error, or stops `cargo pwrs build` where only the descriptor can show it |
| `base` | `base = "System.IO.Stream"`, a .NET class's full C# name, any type arguments in angle brackets | proxy mode only: the generated class derives from that class instead of `Pwrs.ProxyBase`, and its methods marked `#[member]` override the class's members. `cargo pwrs build` reads the class from the reference assemblies each shell compiles against, and refuses a name it does not find, a sealed or static class, an interface, a struct, and a class C# lets no class derive from. Every abstract member needs a method. A field or method whose name the base already has is refused. The class's two constructors call a constructor of the base, as `#[base]` under `#[psmethods]` says. A compile error on a copied or psobject class, and for a name that is not dotted |
| `implements` | `implements = ["System.IFormattable"]`, interfaces' full C# names | proxy mode only: the class also implements each interface, and every abstract member of it needs a method marked `#[member]`. `cargo pwrs build` refuses an interface the base already implements, one a role makes or extends (as `compare` makes `IComparable`), and one with a static abstract member. A compile error on a copied or psobject class, for a name given twice, and for an empty list |

Generates `PsClassMeta`, `IntoPs`, `PsTyped` and `FromPs`. Field types are the parameter types, with `bool` becoming a `bool` property; a field may itself be a `#[psclass]` type, an `Option` or a `Vec` of one. Proxy mode reads fields through `pwrs_proxy_get`, so the struct's fields are cloned on each read, and the field types must be `Clone` in that mode. Proxy mode also requires the type to be `Send`, since the value is read, called and dropped on whatever thread uses the .NET object (the finalizer thread for a value never disposed). A non-`Send` type is a compile error at the attribute. Calls on one object from different threads take turns, so `Sync` is not needed. A call that uses the object again from inside a call on the same thread is refused when either of the two is exclusive, a `&mut self` method or a `with_mut` borrow, rather than getting a second reference to the value. A method call refused this way throws a `PwrsException`, and a property read gives `$null`, which is what PowerShell's property adapter makes of an exception a getter throws. Shared entries nest: a script reading a property while a cmdlet has the object borrowed through `PsProxy::with` reads it.

With `PsTyped` and `FromPs` the class is also a parameter type and a field type. The shell declares the class (`PSObject` for psobject mode), and `FromPs` reads the object back one field per property name, so a copied object, a proxy, or a PSObject with the note properties all convert. A class used as a cmdlet parameter type needs `Default`, like every parameter type.

### `#[psfield(...)]`

On a field of a `#[psclass]` struct.

| Key | Form | Effect |
|---|---|---|
| `skip` | flag | the field stays in Rust: no property, not packed, not read back, no type requirement. The field is still constructed by the module's own code and is reachable from `#[psmethods]` methods |

A class with a skipped field has state no property shows, so it cannot be reconstructed by value. Its `FromPs` returns a `PwrsOpaqueClass` error naming the fields, which is what a cmdlet parameter or method argument of that class reports when bound. Such a class still works as an output, a method return type, and a field type of another class. A proxy one is taken as a parameter with `PsProxy<T>`, which reads the value in place; see [How To Pass Native Data Between Cmdlets](../how-to/How-To-Pass-Native-Data.md).

### Reserved member names

A field or method whose PascalCase name would hide a member the generated class inherits is a compile error naming the set. `Equals`, `GetHashCode`, `GetType` and `ToString` come from `System.Object` and are reserved for the fields of a copied or proxy class and for every method. `Dispose`, `IsDisposed`, `PwrsGet`, `PwrsCall` and `PwrsCallWith` come from `Pwrs.ProxyBase`, and are reserved as well for a proxy class's fields and for every method. A psobject class generates no CLR type, so its fields are unrestricted.

Every other name is free, `Get`, `Set`, `Call` and `Contains` included. The generated property getters and method bodies reach the base class as `base.PwrsGet(...)` and `base.PwrsCall(...)`, so a module method of any name is the one a script calls and never displaces the runtime's own. A class with a `base` of its own reaches its value through its own field `__pwrsCore` instead. `cargo pwrs build` also refuses a field or method of such a class whose name the base declares. A method marked `#[member]` or `#[base]` is declared under no name of its own, so none of these names applies to it, and one may back `ToString`.

## `#[psenum(...)]`

On a fieldless enum with at least one variant.

| Key | Form | Effect |
|---|---|---|
| `name` | `name = "Ns.Kind"` | the CLR enum name this module declares; defaults to the Rust name |
| `clr` | `clr = "System.ConsoleColor"` | mirrors a CLR enum that already exists instead of declaring one |

Discriminants: an explicit `= n` is used as given; otherwise the previous value plus one, starting at 0. Variant doc comments become member summaries. Generates `PsTyped`, `PsClassMeta` (mode `enum`), `IntoPs` and `FromPs`. The enum may then be a parameter type, a class field type, and an output.

`name` and `clr` are mutually exclusive, and giving both is a compile error. A `clr` enum mirrors a type the runtime already has: nothing is generated for it, it takes no class id, and it is not listed under `enums:` in `export_module!` (without `PsClassMeta` it cannot be). A parameter of the enum is declared as that CLR type, so the binder converts its member names, completes them, and rejects the rest before the body runs. A variant list narrower than the CLR enum's members is allowed and useful: a member the Rust enum does not name binds and then fails in `FromPs` with `PwrsEnumValue`. `IntoPs` builds the value through the `enum_new` entry, which resolves the name the way a type literal does.

## `#[psmethods]`

On `impl Type { ... }` where `Type` is a `#[psclass]` struct in proxy or copied mode. Takes no keys. Every `pub fn` in the block becomes a method of the generated class, named in PascalCase; private functions, constants, associated types and macro invocations are left alone. The impl block is emitted unchanged, so the methods stay callable from Rust. `Self` may be written anywhere a type is, `PsResult<Self>` and `other: Self` included, and names the class. An argument's name is its Rust name in lower camel case, as `max_count` becomes `maxCount`. A raw identifier's `r#` is left off, so `r#type` is the argument `type` and the method `Type`. An argument named after a word C# reserves, such as `operator`, is written as `@operator` in the shell, and PowerShell shows the name as written.

A method with a receiver, `&self` or `&mut self`, runs against the object, and so exists only on a proxy class. A `pub fn` with no receiver is a `static` method of the generated class, called on the type as `[Ns.Type]::Name(...)`: it runs against no value, and a static that returns the class returns a new object of it. The static named `new` that returns `PsResult<Self>` is the class's constructor, reached as `[Ns.Type]::new(...)`, and a `new` returning anything else is a compile error. Rust has no overloading, so a class has one `new`; `Option<T>` arguments make it callable with fewer arguments, and one `new(a: Option<i64>, b: Option<i64>)` handles `[T]::new()`, `[T]::new(1)` and `[T]::new(1, 2)`.

A method may take one argument `PsTask<T>`, and then returns `PsResult<()>`. The generated method returns `Task<T>`, with `T` declared as an argument of `T` would be: `Task` for `PsTask<()>`, and `Task<long?>` for `PsTask<Option<i64>>`, which `complete(None)` completes with `$null`. It takes no parameter for the `PsTask` itself, and gains a last parameter `CancellationToken cancellationToken = default`. The method moves the `PsTask` to the thread that does the work, which finishes it with `complete`, `fail` or `cancel`. The token sets a flag that `is_cancelled` reads, and the task ends canceled only when the module calls `cancel`, so a cancellation never discards a result the work has already set. A task dropped without being finished faults with `PwrsTaskDropped`. An `Err` from the method itself is thrown by the call, as for any method. Another argument named `cancellation_token` is refused by `cargo pwrs build`, and a constructor takes no `PsTask`.

A method with a receiver may take one argument `&Self` (or `&Type`): another object of the class, borrowed as shared for the call. The generated method declares the class as the parameter type and throws `ArgumentNullException` for `$null`. It enters both objects' gates before the call, the receiver as the method takes it and the other as shared, always in the order the two objects were made. So two threads calling across the same pair the opposite way round never wait on each other. The same object as both is entered once, and refused for a `&mut self` method. A static takes no `&Self`, nor a method two; any other reference type is a compile error.

`#[member = "Name"]` on a method makes it the member of that name of the class's `base`, or of an interface `implements` names. The method is declared under no name of its own; the member is. `cargo pwrs build` reads the member from the reference assemblies of both target frameworks and declares it with its own signature, as an override of the base's member or an implementation of the interface's. Each argument the member receives must convert to the method's by C#'s implicit conversions: the same type, a wider number, `object`, or a nullable value. The method's result must convert to the member's type the same way, or be a `PsObject`, which reaches any reference type, converted as PowerShell converts it when the call runs. A property's getter is a method taking `&self` and returning the value, and its setter takes the value and returns `PsResult<()>`; an indexer's take its index arguments first.

A name two members share is written with its declaring type, as `#[member = "System.Collections.IComparer.Compare"]`, or with the last part of that name, as `#[member = "IComparer<long>.Compare"]`. An interface's member is declared public under its name, unless the class has another member of that name, and is then implemented explicitly. A member the method fits on one target framework only is declared for that one alone, and the build says so in a note. `cargo pwrs build` refuses each of these, naming the class, the method and the member: a name no member has, members of the name none of which the method fits, two members it fits, two methods backing one member, and every abstract member left without a method. A member that is not virtual or abstract, is generic, or takes an argument passed `ref`, `out` or `in` fits no method. `Dispose` frees the class's Rust value, so a method marked `#[member = "Dispose"]` is refused; the value's `Drop` frees what it owns. A compile error for a method with no receiver, one taking a `PsTask`, a private one, a second `#[member]` on one method, and a `#[member]` with no name.

`#[base(arg, ...)]` on `new`, in a class with a `base`, names the arguments of `new` it passes to the base class's constructor, in that constructor's order, and the build calls the constructor they fit. Without it, `new` calls the base constructor that takes no arguments. The base constructor runs first and Rust's `new` second, so a `new` that returns `Err` leaves an object with no Rust value behind it, and so nothing to free. `#[base]` alone, on a method `fn(&self) -> PsResult<T>`, or `PsResult<(A, B, ...)>` for several, gives the base constructor's arguments for an object Rust makes outside `new`, as a cmdlet writing one does. A class whose base has no constructor taking no arguments needs one, and a class has at most one. The generated class does not declare it, and no role may name it. A compile error for `#[base(...)]` on a method other than `new`, a name `new` has no argument for, a name given twice, and a `#[base]` method that takes more than `&self` or returns nothing.

A constructor has no pipeline, so it cannot write a warning or an error record: an `Err` is the exception the script's `::new` call throws. A condition a cmdlet would warn about is either refused in a constructor or left silent there, and a cmdlet that must warn does its own check.

**On a proxy class** the value `new` returns is the one behind the new object.

**On a copied class** there is no Rust value behind the object, so only statics can be declared, and a method taking `&self` is refused by `cargo pwrs build` with the reason. `new` builds the value in Rust and the new object is made from it field by field, so its properties are ordinary settable fields like any other copied object's. What a script can construct depends on whether the class declares a `new`:

| The class declares | Public constructors |
|---|---|
| no `new` | the parameterless one C# supplies, which leaves every field at zero or null |
| a `new` | only that one; the parameterless constructor is not emitted, so a script cannot make an object with every field at zero or null |

For a parameterless constructor that starts from the type's `Default` rather than zeros, give `new` all-`Option` arguments and start from `Self::default()`. The generated factory builds objects through a constructor of its own, selected by the runtime's `Pwrs.FromFields` marker, which no Rust signature produces. So it never calls a constructor the module declared, and a `new()` that returns `Self` cannot call itself through the factory. Arguments are of the parameter types: `Option<T>` makes the C# parameter optional with a `null` default, `bool` is a `bool` and not a switch, and a `#[psclass]` type is read back through its properties. A method returns `PsResult<T>` for any of those types, including a `#[psclass]` type (a proxy class returned this way is a new proxy object), or `PsResult<()>` for nothing. An `Option<String>` or `Option<PathBuf>` argument is declared as `object?` rather than `string?`. The engine's method binder turns `$null` and an omitted argument into an empty string for a `string` parameter, so `None` would never arrive. The generated method converts a supplied value to a string itself. Async and generic methods are rejected, and a method takes any number of arguments. The method's doc comment becomes its `<summary>`.

Generates a `PsMethods<Type>` impl on `MethodsCollector<Type>` with the method descriptors and the dispatch; `#[psclass]` finds it through method resolution, so a class without `#[psmethods]` needs nothing. `cargo pwrs build` refuses methods on a psobject or enum class, and a method taking `&self` on a copied one. Each call is one `pwrs_proxy_call` with a packed argument block. Calls and property reads on one proxy take turns at the object's `Pwrs.ProxyCore`, and an `Err` from the method becomes a `PwrsException` in the script. A static or a constructor is the same call with a null instance, through the runtime's `Pwrs.StaticCall.Invoke`, which serializes nothing and checks no generation because there is no object to guard. A method with a receiver reached with a null instance is refused with `PwrsNoInstance` rather than run. The constructor's call returns, for a proxy, the new value's pointer, which the public constructor passes to the internal one; for a copied class, the object the class's factory built, whose fields the public constructor copies into itself.

## `#[completer(...)]`

On a `fn(ctx: &CompletionContext) -> PsResult<Vec<Completion>>`. Both keys required.

| Key | Form | Effect |
|---|---|---|
| `cmdlet` | `cmdlet = "Get-Thing"` | the cmdlet name the completer attaches to |
| `parameter` | `parameter = "Name"` | the parameter |

The function becomes a unit struct of the same name implementing `CompleterFn`; list that name under `completers`.

## `#[transform(...)]`

On a `fn(value: &PsObject) -> PsResult<PsObject>`. Both keys required, and one transform per parameter.

| Key | Form | Effect |
|---|---|---|
| `cmdlet` | `cmdlet = "Get-Thing"` | the cmdlet name the transform attaches to |
| `parameter` | `parameter = "Size"` | the parameter |

The function becomes a unit struct of the same name implementing `TransformFn`; list that name under `transforms`. It generates an `ArgumentTransformationAttribute` on the parameter, which the engine runs before converting the argument to the parameter's declared type and before validation. So a `long` parameter can accept `2MB`, and a refusal is a binding failure naming the parameter rather than an error the cmdlet wrote.

The value arrives as the caller wrote it, so a transform checks it carefully and returns anything it does not recognize unchanged, for the binder to convert or reject; returning the value unchanged is always correct. No instance exists yet, so it takes no `Pipeline` and cannot write to a stream.

## `#[dynamic_params(...)]`

On a `fn(bound: &PsHashtable) -> PsResult<Vec<DynamicParam>>`.

| Key | Form | Effect |
|---|---|---|
| `cmdlet` | `cmdlet = GetThing` | the cmdlet type (a path, not a string) the function is for |

Implements `DynamicParams` for that type; list the cmdlet type under `dynamic_params`.

## `#[on_load]`, `#[on_import]`, `#[on_remove]` and `#[on_unload]`

Each on a `fn() -> PsResult<()>`. None takes a key; a key or an argument to the function is a compile error.

The function becomes a unit struct of the same name implementing `OnLoad`, `OnImport`, `OnRemove` or `OnUnload`. Name it under `on_load`, `on_import`, `on_remove` or `on_unload` in `export_module!`.

- `on_import` runs at every import of the module, in any runspace.
- `on_remove` runs once for each import. It runs at `Remove-Module`, or when the runspace that imported the module closes without removing it. A runspace closes when it is disposed, when a thread job or a `ForEach-Object -Parallel` run ends, when its pool is disposed, and when PowerShell exits.
- `on_load` runs when the module goes from imported in no runspace of the process to imported in one, before that import's `on_import`.
- `on_unload` runs after the last `on_remove` in the process.

A removal unloads nothing: the native library stays mapped and its statics are not reset. So `on_load` runs again when the module is imported after every runspace has removed it. No destructor runs either, so `on_unload` is where a module releases what `on_load` made. On a reload, the old copy of the library runs `on_unload` and then the new copy runs `on_load`; see [How To Reload A Module](../how-to/How-To-Reload-A-Module.md).

A module's hooks run one at a time. An import, removal or close in another runspace waits until the running hook returns, so the hooks can share state without synchronizing it. No hook runs with a cmdlet or a pipeline. A hook runs on the thread running `Import-Module` or `Remove-Module`, or on the thread closing the runspace.

An `Err` from `on_load` or `on_import` fails the import, and the import counts for nothing; the next import runs `on_load` again. The record has the error's id and category, and its `FullyQualifiedErrorId` is the id followed by `Microsoft.PowerShell.Commands.ImportModuleCommand`.

An `Err` from `on_remove` or `on_unload` is written as an error of `Remove-Module`, and the removal finishes under any error preference. The record's `FullyQualifiedErrorId` is the error's id, and its category is the error's. The module's script writes it with `Write-Error`, so PowerShell shows it under any error preference, `-ErrorAction SilentlyContinue` included, with that command's name before the message. In pwsh 7.6.7, for hello's removal hook:

```text
Write-Error: the remove hook failed because Set-RustLifecycleFailure set it to fail
```

At a reload, an `Err` from the old copy's `on_unload` is written during `Import-Module`, and the import goes on. A closing runspace has no command to report to, so its hooks' errors go to standard error. At the end of a pwsh 7.6.7 process that had hello imported:

```text
pwrs: the unload hook of Hello failed as runspace d34ee67f-5e3e-4880-8b90-91359c5a6754 closed: [HelloLifecycleFailure] the unload hook failed because Set-RustLifecycleFailure set it to fail
```

The generated shell implements `IModuleAssemblyInitializer` when the module declares any of the four, and the module's script runs the removal from its `OnRemove` block. A module that declares none has neither, and is not called at import or removal.

## `#[provider(...)]`

On a struct implementing `Provider`; each drive has its own value of it.

| Key | Form | Effect |
|---|---|---|
| `name` | `name = "MemFs"` | the provider name; defaults to the Rust name |
| `capabilities` | `capabilities = ["Filter", "Credentials"]` | added to `[CmdletProvider]` as `ProviderCapabilities` flags by name; `ShouldProcess` is always set |

The struct's doc comment is the provider description in the descriptor. `Filter` and `Credentials` let `-Filter` and `-Credential` reach the provider, which PowerShell refuses otherwise, and `ExpandWildcards` sends a path with wildcards to `expand_path`. `Include` and `Exclude` are accepted, and change nothing: PowerShell applies `-Include` and `-Exclude` itself whatever a provider declares. [How To Write A Provider](../../how-to/how-to-write-a-provider/#the-calls-filter-patterns--force-and-credential) says what each one does.

## `export_module!`

```rust
pwrs::export_module! {
    name: "Hello",
    cmdlets: [GetGreeting, GetPerson],
    classes: [Person, Counter],
    enums: [Signal],
    completers: [complete_color],
    transforms: [as_bytes],
    dynamic_params: [GetRustReading],
    providers: [MemFs],
    on_load: make_registration,
    on_import: count_import,
    on_remove: count_remove,
    on_unload: release_registration,
}
```

`name` and `cmdlets` are required (`cmdlets: []` is allowed); the other entries are optional and must appear in this order when present. `on_load`, `on_import`, `on_remove` and `on_unload` each name one unit struct, not a list. The position of each list entry is its id: cmdlets from 0; classes then enums share one id space in that order; completers, transforms and providers each from 0. The macro emits the `pwrs_module_init`, `pwrs_module_descriptor`, `pwrs_cmdlet_*`, `pwrs_proxy_*`, `pwrs_completer_invoke`, `pwrs_transform_invoke`, `pwrs_dynparams_invoke`, `pwrs_provider_invoke` and `pwrs_module_lifecycle` exports, and for the import's CPU check the data export `pwrs_cpu_requirements` and, on x86-64, `pwrs_cpuid` and `pwrs_xgetbv`. One `export_module!` per crate.
