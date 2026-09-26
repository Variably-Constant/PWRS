---
title: Attribute Reference
weight: 1
---

Every key each attribute accepts, as parsed by `crates/pwrs-macros/src/params.rs`, `classes.rs`, `methods.rs`, `enums.rs` and `completers.rs` (which also holds `#[transform]`), and the `export_module!` grammar in `crates/pwrs/src/runtime.rs`. An unknown key is a compile error naming it.

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

On a field of a `#[cmdlet]` struct. `#[param]` alone is valid. At most 64 parameters per cmdlet. The parameter name is the field name in PascalCase.

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
| `validate_pattern` | `validate_pattern = "^[a-z]+$"` | `[ValidatePattern]`, which the engine applies with `RegexOptions.IgnoreCase`, so `^[a-z]+$` admits `ABC` |
| `validate_not_null_or_empty` | flag | `[ValidateNotNullOrEmpty]` |
| `dont_show` | flag | `DontShow = true` |
| `literal_path` | flag | declares the parameter the way PowerShell declares a literal path everywhere else: `[Alias("PSPath", "LP")]` and `ValueFromPipelineByPropertyName = true`, so it binds from a piped object's `PSPath`. An alias the author named themselves is not added twice. The parameter's own resolution is still the cmdlet's to choose, through `ps.resolve_path(path, literal)` |
| `raw` | flag | the shell declares the property as `object` rather than the parameter's own CLR type, so the engine's binder hands the argument over without coercing it; Rust still converts to the field's type. Only for a parameter that crosses as a handle (`Vec<T>`, `PsObject`, a class, an enum) |
| `allow_empty_collection` | flag | `[AllowEmptyCollection]`: a mandatory collection parameter takes an empty array, which the engine otherwise refuses with `ParameterArgumentValidationErrorEmptyArrayNotAllowed`. Only for a parameter that crosses as a handle: a `Vec<T>`, or a `PsObject` declared as an array with `clr` |
| `clr` | `clr = "byte[]"`, `clr = "System.IO.FileInfo"` | on a `PsObject` or `Option<PsObject>` parameter: the shell declares the property as that CLR type, so the binder coerces to it and chooses between parameters by it, and the value still crosses as a handle, uncopied. Not with `raw`, and not a type a Rust field spells itself (`int`, `string`, `object` and the other C# type keywords), which is a compile error |

Field types are listed in [Conversions Reference](Conversions-Reference.md). `Option<Option<T>>` and `Vec<Option<T>>` are rejected.

`raw` is for bulk data. A parameter declared as an array type is the engine's binder's to check before the module is entered: on Windows PowerShell 5.1 it walks the array, 165 ms for a 4 MB `byte[]` and 1.3 s for 32 MB whether the array arrives bare or wrapped in a `PSObject`, against 0.9 ms and 5 ms for the same parameter marked `raw`; on pwsh 7.6.6 both cost the copy into the `Vec<u8>`, about 0.5 ms and 5 ms. An `object[]` of the values boxed, bound to a parameter declaring the array type, is converted one by one on both hosts, 786 ms and 2.2 s for 4 MB. `docs/PERF.md` carries the table. What `raw` costs is the binder's own work: the parameter shows as `object` in `Get-Help` and completion, a wrong-typed argument is not refused at bind time, and a value the conversion cannot take is a `PwrsConversionError` from Rust instead. A string reaching a `Vec<u8>` this way enumerates as its characters rather than being rejected.

`clr` is the other way round: the binder does its work and Rust reads the result in place. A `PsObject` parameter is declared `object`, and an `object` parameter taking pipeline input accepts every piped record. Declared `byte[]` with `clr`, it accepts a byte array whole and a lone byte as an array of one, while a piped `FileInfo`, which does not convert, goes on to a `literal_path` parameter bound from its `PSPath`. The module then reads the bytes through `pin`, without a copy. `Measure-RustInput` in the hello example does both, and `Bytes.Tests.ps1` checks each case in both hosts, including that the cmdlet can change the caller's own array.

## `#[psclass(...)]`

On a struct with named fields, at most 64. Fields need no attribute; each field's doc comment becomes its property summary.

| Key | Form | Effect |
|---|---|---|
| `name` | `name = "Ns.Type"` | the CLR type name (or the `PSTypeName` in psobject mode); defaults to the Rust name. A dotted name places the type in that namespace; a bare name lands in `Pwrs.Modules.<Module>` |
| `mode` | `mode = copied`, `mode = proxy`, `mode = psobject` (also as strings) | the output mode; default `copied` |
| `copied`, `proxy`, `psobject` | flags | the same as `mode = ...` |
| `native_bytes` | `native_bytes = path::to_fn`, a `fn(&Self) -> usize` | proxy mode only: the bytes of native memory a value keeps alive. The object reports them with `GC.AddMemoryPressure` when it is made, asks again after each method call and each `PsProxy::with_mut`, and withdraws them when the value is freed. A compile error on a copied or psobject class |

Generates `PsClassMeta`, `IntoPs`, `PsTyped` and `FromPs`. Field types are the parameter types, with `bool` becoming a `bool` property; a field may itself be a `#[psclass]` type, an `Option` or a `Vec` of one. Proxy mode reads fields through `pwrs_proxy_get`, so the struct's fields are cloned on each read (`Clone` on the field types is required in that mode), and requires the type to be `Send`: the value is read, called and dropped on whatever thread holds the managed object (the finalizer thread for a value never disposed), and a non-`Send` type is a compile error at the attribute. Calls on one object never overlap, so `Sync` is not needed, and one that reaches the object again from inside a call on the same thread, such as a script reading a property while a cmdlet holds the object through `PsProxy`, is refused with a `PwrsException` rather than given a second reference to the value.

With `PsTyped` and `FromPs` the class is also a parameter type and a field type: the shell declares the class (`PSObject` for psobject mode), and `FromPs` reads the object back one field per property name, so a copied object, a proxy, or a PSObject carrying the notes all convert. A class used as a cmdlet parameter type needs `Default`, like every parameter type.

### `#[psfield(...)]`

On a field of a `#[psclass]` struct.

| Key | Form | Effect |
|---|---|---|
| `skip` | flag | the field stays in Rust: no property, not packed, not read back, no type requirement. The field is still constructed by the module's own code and is reachable from `#[psmethods]` methods |

A class with a skipped field keeps state no property shows, so it cannot be reconstructed by value: its `FromPs` returns a `PwrsOpaqueClass` error naming the fields, which is what a cmdlet parameter or method argument of that class reports when bound. Such a class still works as an output, a method return type, and a field type of another class, and a proxy one is taken as a parameter with `PsProxy<T>`, which reads the value in place; see [How To Pass Native Data Between Cmdlets](../how-to/How-To-Pass-Native-Data.md).

### Reserved member names

A field or method whose PascalCase name would hide a member the generated class inherits is a compile error naming the set. `Equals`, `GetHashCode`, `GetType` and `ToString` come from `System.Object` and are reserved for the fields of a copied or proxy class and for every method; `Dispose`, `IsDisposed`, `PwrsGet` and `PwrsCall` come from `Pwrs.ProxyBase` and are reserved as well for a proxy class's fields and for every method. A psobject class generates no CLR type, so its fields are unrestricted.

Every other name is free, `Get`, `Set`, `Call` and `Contains` included: the generated property getters and method bodies reach the base class as `base.PwrsGet(...)` and `base.PwrsCall(...)`, so a module method of any name is the one a script calls and never displaces the runtime's own.

## `#[psenum(...)]`

On a fieldless enum with at least one variant.

| Key | Form | Effect |
|---|---|---|
| `name` | `name = "Ns.Kind"` | the CLR enum name this module declares; defaults to the Rust name |
| `clr` | `clr = "System.ConsoleColor"` | mirrors a CLR enum that already exists instead of declaring one |

Discriminants: an explicit `= n` is used as given; otherwise the previous value plus one, starting at 0. Variant doc comments become member summaries. Generates `PsTyped`, `PsClassMeta` (mode `enum`), `IntoPs` and `FromPs`. The enum may then be a parameter type, a class field type, and an output.

`name` and `clr` are mutually exclusive, and giving both is a compile error. A `clr` enum mirrors a type the runtime already has: nothing is generated for it, it takes no class id, and it is not listed under `enums:` in `export_module!` (without `PsClassMeta` it cannot be, which is the guardrail rather than a second declaration of a type the shell does not own). What it buys is the parameter being declared as that CLR type, so the binder converts its member names, completes them, and rejects the rest before the body runs. A variant list narrower than the CLR enum's members is allowed and useful: a member the Rust enum does not name binds and then fails in `FromPs` with `PwrsEnumValue`. `IntoPs` builds the value through the `enum_new` entry, which resolves the name the way a type literal does.

## `#[psmethods]`

On `impl Type { ... }` where `Type` is a `#[psclass]` struct in proxy or copied mode. Takes no keys. Every `pub fn` in the block becomes a method of the generated class, named in PascalCase; private functions, constants, associated types and macro invocations are left alone. The impl block is emitted unchanged, so the methods stay callable from Rust. `Self` may be written anywhere a type is, `PsResult<Self>` and `other: Self` included, and names the class.

A method with a receiver, `&self` or `&mut self`, runs against the object, and so exists only on a proxy class. A `pub fn` with no receiver is a `static` method of the generated class, called on the type as `[Ns.Type]::Name(...)`: it runs against no value, and a static that returns the class returns a new object of it. The static named `new` that returns `PsResult<Self>` is the class's constructor, reached as `[Ns.Type]::new(...)`, and a `new` returning anything else is a compile error. Rust has no overloading, so a class has one `new`; `Option<T>` arguments make it callable with fewer arguments, and one `new(a: Option<i64>, b: Option<i64>)` answers `[T]::new()`, `[T]::new(1)` and `[T]::new(1, 2)`.

A constructor has no pipeline, so it cannot write a warning or an error record: an `Err` is the exception the script's `::new` call throws. A condition a cmdlet would warn about is either refused in a constructor or left silent there, and a cmdlet that must warn keeps its own check.

**On a proxy class** the value `new` returns is the one the new object holds.

**On a copied class** there is no Rust value behind the object, so only statics can be declared, and a method taking `&self` is refused by `cargo pwrs build` with the reason. `new` builds the value in Rust and the new object is made from it field by field, so its properties are ordinary settable fields like any other copied object's. What a script can construct depends on whether the class declares a `new`:

| The class declares | Public constructors |
|---|---|
| no `new` | the parameterless one C# supplies, which fills CLR zeros, as it always has |
| a `new` | only that one; the parameterless constructor is not emitted, so a script cannot make an object of zeros the class never meant to exist |

To keep a parameterless constructor that starts from the type's `Default` rather than zeros, give `new` all-`Option` arguments and start from `Self::default()`. The generated factory builds objects through a constructor of its own, selected by the runtime's `Pwrs.FromFields` marker, which no Rust signature lowers to; so it never calls a constructor the module declared, and a `new()` that returns `Self` cannot call itself through the factory. Arguments are of the parameter types (`Option<T>` makes the C# parameter optional with a `null` default; `bool` is a `bool`, not a switch; a `#[psclass]` type is read back through its properties), and returns `PsResult<T>` for any of those types, including a `#[psclass]` type (a proxy class returned this way is a new proxy object), or `PsResult<()>` for nothing. An `Option<String>` or `Option<PathBuf>` argument is declared as `object?` rather than `string?`: the engine's method binder turns `$null` and an omitted argument into an empty string for a `string` parameter, so `None` would never arrive; the generated method converts a supplied value to a string itself. Async and generic methods are rejected; at most 64 arguments. The method's doc comment becomes its `<summary>`.

Generates a `PsMethods<Type>` impl on `MethodsCollector<Type>` carrying the method descriptors and the dispatch; `#[psclass]` finds it through method resolution, so a class without `#[psmethods]` needs nothing. `cargo pwrs build` refuses methods on a psobject or enum class, and a method taking `&self` on a copied one. Each call is one `pwrs_proxy_call` with a packed argument block; calls and property reads on one proxy are serialized by the managed base; an `Err` from the method becomes a `PwrsException` in the script. A static or a constructor is the same call with a null instance, through the runtime's `Pwrs.StaticCall.Invoke`, which serializes nothing and checks no generation because there is no object to guard. A method with a receiver reached with a null instance is refused with `PwrsNoInstance` rather than run. The constructor's call answers, for a proxy, the new value's pointer, which the public constructor hands to the internal one; for a copied class, the object the class's factory built, whose fields the public constructor copies into itself.

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

The function becomes a unit struct of the same name implementing `TransformFn`; list that name under `transforms`. It generates an `ArgumentTransformationAttribute` on the parameter, which the engine runs before coercing the argument to the parameter's declared type and before validation, so a `long` parameter can accept `2MB` and a refusal is a binding failure naming the parameter rather than an error the cmdlet wrote.

The value arrives as the caller wrote it, so a transform reads it defensively and hands back anything it does not recognize for the binder to coerce or reject; returning the value unchanged is always correct. No instance exists yet, so it takes no `Pipeline` and cannot write to a stream.

## `#[dynamic_params(...)]`

On a `fn(bound: &PsHashtable) -> PsResult<Vec<DynamicParam>>`.

| Key | Form | Effect |
|---|---|---|
| `cmdlet` | `cmdlet = GetThing` | the cmdlet type (a path, not a string) the function serves |

Implements `DynamicParams` for that type; list the cmdlet type under `dynamic_params`.

## `#[on_import]` and `#[on_remove]`

Each on a `fn() -> PsResult<()>`. Neither takes a key; a key or an argument to the function is a compile error.

The function becomes a unit struct of the same name implementing `OnImport` or `OnRemove`; name it under `on_import` or `on_remove`. The generated shell implements `IModuleAssemblyInitializer` for a declared `on_import` and `IModuleAssemblyCleanup` for a declared `on_remove`, and neither interface otherwise, so a module without hooks is not called at import or removal. A hook runs on the thread executing `Import-Module` or `Remove-Module`, with no cmdlet and no pipeline.

A removal unloads nothing: the native library stays mapped and its statics keep their values. So `on_import` runs on every import, including one that follows a removal in the same session, and `on_remove` is the only place an import's resources are released. An `Err` from `on_import` fails the import; an `Err` from `on_remove` is reported by `Remove-Module` and the removal goes ahead. On a reload the old library's `on_remove` runs before the new library's `on_import`; see [How To Reload A Module](../how-to/How-To-Reload-A-Module.md).

## `#[provider(...)]`

On a struct implementing `Provider`; one value of it serves one drive.

| Key | Form | Effect |
|---|---|---|
| `name` | `name = "MemFs"` | the provider name; defaults to the Rust name |
| `capabilities` | `capabilities = ["Filter", "Include"]` | added to `[CmdletProvider]` as `ProviderCapabilities` flags by name; `ShouldProcess` is always set |

The struct's doc comment is the provider description in the descriptor.

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
    on_import: count_import,
    on_remove: count_remove,
}
```

`name` and `cmdlets` are required (`cmdlets: []` is allowed); the other entries are optional and must appear in this order when present. `on_import` and `on_remove` each name one unit struct, not a list. The position of each list entry is its id: cmdlets from 0; classes then enums share one id space in that order; completers, transforms and providers each from 0. The macro emits the `pwrs_module_init`, `pwrs_module_descriptor`, `pwrs_cmdlet_*`, `pwrs_proxy_*`, `pwrs_completer_invoke`, `pwrs_transform_invoke`, `pwrs_dynparams_invoke`, `pwrs_provider_invoke` and `pwrs_module_lifecycle` exports. One `export_module!` per crate.
