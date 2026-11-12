---
title: Attribute Reference
weight: 1
---

Every key each attribute accepts, as parsed by `crates/pwrs-macros/src/params.rs`, `classes.rs`, `methods.rs`, `enums.rs`, `completers.rs` (which also has `#[transform]`), `passthrough.rs` (the `attribute(...)` key) and `tiered.rs`, and the `export_module!` grammar in `crates/pwrs/src/runtime.rs`. An unknown key is a compile error naming it.

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
| `supports_paging` | flag | `SupportsPaging = true`: the engine adds `-First`, `-Skip` and `-IncludeTotalCount`, which the cmdlet reads with `ps.paging()` and applies itself, and `ps.write_total_count` writes the total count the caller requests with `-IncludeTotalCount`. See [How To Write A Cmdlet](../how-to/How-To-Write-A-Cmdlet.md#paging) |
| `supports_transactions` | flag | `SupportsTransactions = true`: Windows PowerShell 5.1 adds `-UseTransaction`, and `ps.transaction()` returns the transaction a call given it inside `Start-Transaction` runs in, which work is enlisted in. PowerShell 7 adds no `-UseTransaction`, so there `ps.transaction()` is always `None`. See [How To Write A Cmdlet](../how-to/How-To-Write-A-Cmdlet.md#transactions) |
| `help_uri` | `help_uri = "https://example.org/help"` | `HelpUri`, the page `Get-Help -Online` opens. Anything but an http or https address is a compile error |
| `remoting_capability` | `remoting_capability = "None"` | `RemotingCapability`: `None`, `PowerShell` (the engine's default), `SupportedByCommand` or `OwnedByCommand`, what `Get-Command` reports for running the cmdlet remotely. Another name is a compile error |
| `as_job` | flag, or `as_job = "runtime"` | an `-AsJob` switch, which help lists. As a flag the cmdlet reads it with `ps.as_job()` and starts its job itself. With `"runtime"` the runtime runs `-AsJob`: the cmdlet is `Clone` and implements `CmdletJob`, whose phases one job runs for the whole invocation, and the job is the command's output. A field that is the parameter `AsJob` is a compile error. See [How To Run Work As A Job](../how-to/How-To-Run-Work-As-A-Job.md) |
| `wraps` | `wraps = "Get-ChildItem"`, `wraps = "Microsoft.PowerShell.Archive\\Compress-Archive"` | makes the cmdlet a proxy of the command: it declares the command's parameters in the PowerShell running it, its `[Cmdlet]` settings and output types, runs it as a steppable pipeline, and implements `Cmdlet` through the struct's `Wrapper` steps. Another module's command is read where the module is installed and recorded in `wrapped.lock.json`. A name not Verb-Noun or Module\Verb-Noun is a compile error, and so is any of `supports_should_process`, `confirm_impact`, `default_parameter_set`, `supports_paging`, `supports_transactions`, `help_uri`, `remoting_capability`, `output` and `as_job` beside it. See [How To Write A Cmdlet](../how-to/How-To-Write-A-Cmdlet.md#declare-a-proxy-of-a-command) |
| `attribute` | `attribute(type = "System.ObsoleteAttribute", args = ["Use Get-Greeting."])`, any number | the .NET attribute on the generated cmdlet class, as [The `attribute(...)` key](#the-attribute-key) says: `[Obsolete]` makes PowerShell warn on each call, and `[Experimental]` leaves the cmdlet out of PowerShell 7 while its feature is off |

There is no `positional_binding` key. `PositionalBinding` belongs to a script function's `[CmdletBinding()]`, where it stops PowerShell numbering the parameters that declare no position; a compiled cmdlet's parameters are never numbered that way, so a PWRS parameter is positional only with `position`, which is what `PositionalBinding = $false` gives a function.

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
| `validate_script` | `validate_script = "$_ % 2 -eq 0"` | `[Pwrs.ValidateScriptText]`, the shell's form of `[ValidateScript]`, since an attribute argument cannot be a script block. It runs the script for each element, in a scope of its own, with the element as `$_`, and refuses an element the script does not return a true value for, with the message PowerShell's `ValidateScript` gives. A `$null` element is refused before the script runs, with the message PowerShell's gives for one. A blank script is a compile error |
| `validate_not_null` | flag | `[ValidateNotNull]`: `$null` is refused. Only for a parameter whose type can be null: a string, a `PsObject`, a `Vec<T>` or a class. A number or `bool` is a compile error, even inside an `Option`, and an enum or another value type stops `cargo pwrs build`, because the shell declares each as a value type. Not with `allow_null` |
| `allow_null` | flag | `[AllowNull]`: a mandatory parameter takes `$null`, which the engine otherwise refuses with `ParameterArgumentValidationErrorNullNotAllowed`. Only for a parameter whose type can be null, as for `validate_not_null`. An `Option<String>` given `$null` is `None`, as is an `Option` of a type that can be null, such as `Option<PsObject>` or `Option<PsCredential>`, and a `String` is empty. An `Option` of a number, `bool` or enum is declared as that value type, so the binder refuses `$null` for it with `ParameterBindingFailed`; left out, it is `None` |
| `supports_wildcards` | flag | `[SupportsWildcards]`, and the help says the parameter accepts wildcard characters. The cmdlet does the matching itself, for example through `System.Management.Automation.WildcardPattern` |
| `credential` | flag | `[Credential()]`: a user name given in place of a credential binds by prompting for the password through the host, as `Get-Credential -Credential` does, and a host that cannot prompt refuses it; Windows PowerShell's console host prompts in a dialog, even under `-NonInteractive`. Only for a `PsCredential` or `Option<PsCredential>` parameter |
| `validate_with` | `validate_with = check_size` | the shell's `PwrsValidateWith`, a `ValidateArgumentsAttribute` that runs the named function, `fn(&PsObject) -> PsResult<()>`, over the argument after the binder converts it to the parameter's type and before the cmdlet runs. An `Err` refuses the argument as a validation failure naming the parameter, `ParameterArgumentValidationError`, with the error's message alone after the binder's sentence. The error's id and category are in the `Data` of its exception inside the binder's. A piped value it refuses fails that record alone. A function of another signature is a compile error at its path |
| `clr` | `clr = "byte[]"`, `clr = "System.IO.FileInfo"` | on a `PsObject` or `Option<PsObject>` parameter: the shell declares the property as that CLR type, so the binder converts to it and binds a piped value to the parameter whose type it matches, and the value is still passed as a handle, not copied. Not with `raw`, and not a type a Rust field spells itself (`int`, `string`, `object` and the other C# type keywords), which is a compile error; a string kept as the object rather than as its text is a `PsString` field, declared `string` |
| `attribute` | `attribute(type = "System.Management.Automation.PSDefaultValueAttribute", named(Help = "Hello"))`, any number | the .NET attribute on the parameter's property, as [The `attribute(...)` key](#the-attribute-key) says: `[Obsolete]` makes PowerShell warn when the parameter binds, a validation attribute such as `[ValidateDrive]` checks the argument, and `[PSDefaultValue]`'s `Help`, or else its `Value`, is the default value the help gives |

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
| `implicit`, `explicit` | `implicit = [from_int]`, `explicit = [to_double]`, each a list of `#[psmethods]` methods | the class declares a C# conversion operator for each, `op_Implicit` or `op_Explicit`, which a cast such as `[double]$value` and the parameter binder call. A static `fn(T) -> PsResult<Self>` converts from `T`; a method `fn(&self) -> PsResult<T>`, or a static taking `&Self` (proxy) or `Self` (copied) first, converts to `T`. PowerShell finds an operator by the value's exact type, with no widening, so a conversion meant for a script's integer literals takes `i32`. `cargo pwrs build` refuses a method of another shape, a conversion to or from `object`, an interface or the class's base, which C# allows no operator for, and two conversions the same way between the class and one type. A compile error on a psobject class, for an empty list, and for a method named twice |
| `converter` | `converter = [from_any, to_any]`: a static `fn(PsObject) -> PsResult<Option<Self>>`, then a method `fn(&self, PsType) -> PsResult<Option<PsObject>>`, or on a copied class a static taking `Self` first | the class declares a `PSTypeConverter` nested in it, `PwrsConverter`, and names it with `[TypeConverter]`, so PowerShell converts any value to the class through `from_any` and the class to any type through `to_any`, in casts and in parameter binding, before trying a constructor. `None` means it cannot convert. `to_any` is given the type as a script names it, as `System.Double`. Each `CanConvertFrom` and `CanConvertTo` stores its result for the conversion that follows on its thread, so a conversion calls the function once. The module's import registers the converter as type data too, which PowerShell 7 needs, since it does not look a `[TypeConverter]`'s type up in the module's load context. A compile error on a psobject class, for other than two functions, and for one named twice |
| `inheritable` | the bare flag, on a proxy or copied class | the class is not sealed, so a PowerShell class derives from it (`class Square : Hello.Shape`), and its public instance methods and public interface implementations are virtual, so a derived class's override is what a call through the object reaches; a cast to the base type, `([Hello.Shape]$square).Area()`, is PowerShell's call of the base's own method. A proxy class gets a protected parameterless constructor whose Rust value is the type's `Default`, which it must implement, unless its `new` takes nothing, whose public constructor serves instead. A derived object passes back to Rust as the class, through `PsProxy<T>` or an argument of the class, with the Rust value its base constructor made. A class without the flag stays sealed. A compile error on a psobject class |
| `base` | `base = "System.IO.Stream"`, a .NET class's full C# name, any type arguments in angle brackets | proxy mode only: the generated class derives from that class instead of `Pwrs.ProxyBase`, and its methods marked `#[member]` override the class's members. `cargo pwrs build` reads the class from the reference assemblies each shell compiles against, and refuses a name it does not find, a sealed or static class, an interface, a struct, and a class C# lets no class derive from. Every abstract method and property needs a method, unless `stubs` names it, and the class declares each abstract event itself, as an override. A field or method whose name the base already has is refused. The class's two constructors call a constructor of the base, as `#[base]` under `#[psmethods]` says. A compile error on a copied or psobject class, and for a name that is not dotted |
| `implements` | `implements = ["System.IFormattable"]`, interfaces' full C# names | proxy mode only: the class also implements each interface, and every abstract method and property of it needs a method marked `#[member]`, unless `stubs` names it. The class declares each of its events itself, with the interface's delegate type, public under its name unless the class has that name already, and then explicitly, so `INotifyPropertyChanged`, `INotifyCollectionChanged` and `ICommand` need methods for their methods alone. `cargo pwrs build` refuses an interface the base already implements, one a role makes or extends (as `compare` makes `IComparable`), and one with a static abstract member. A compile error on a copied or psobject class, for a name given twice, and for an empty list |
| `events` | `events = ["Changed", "Progress: System.EventHandler<Hello.ProgressArgs>"]`, each an event's name and, after a colon, its delegate type's full C# name | proxy mode only: the class declares each event, of `System.EventHandler` when the item gives no type; `PsThis::raise` and `PsProxy::raise` raise it, as [How To Raise And Handle Object Events](../how-to/How-To-Raise-And-Handle-Object-Events.md) says. `cargo pwrs build` reads each delegate type from the reference assemblies of both compiles, with any of the module's own classes in its type arguments read as `System.Object`, and refuses one that is not a delegate, a name the class or its base has already, and an event an interface or the base declares for the class already. A compile error on a copied or psobject class, for an empty list, a name that is not a C# identifier, a name given twice, one that hides an inherited member, one a field is the property of, and a type that is not a full C# name |
| `stubs` | `stubs = ["Insert", "RemoveAt"]`, abstract members of the base and interfaces, each named as `#[member]` names one | proxy mode only: each abstract member a name gives that no method marked `#[member]` backs throws `NotSupportedException` naming the class and the member, a property's getter and setter each, and `cargo pwrs build` lists them in a note. Every other abstract member still needs a method. A name that leaves no member to throw on either target framework is refused. A compile error on a copied or psobject class, on one naming neither `base` nor `implements`, for an empty list, a name given twice, and an empty name or one with white space |
| `attribute` | `attribute(type = "System.ComponentModel.DescriptionAttribute", args = ["A badge with text on it."])`, any number | proxy or copied mode: the .NET attribute on the generated class, as [The `attribute(...)` key](#the-attribute-key) says. A compile error on a psobject class, which has no class to put it on |

Generates `PsClassMeta`, `IntoPs`, `PsTyped` and `FromPs`. Field types are the parameter types, with `bool` becoming a `bool` property; a field may itself be a `#[psclass]` type, an `Option` or a `Vec` of one. Proxy mode reads fields through `pwrs_proxy_get`, so the struct's fields are cloned on each read, and the field types must be `Clone` in that mode. Proxy mode also requires the type to be `Send`, since the value is read, called and dropped on whatever thread uses the .NET object (the finalizer thread for a value never disposed). A non-`Send` type is a compile error at the attribute. Calls on one object from different threads take turns, so `Sync` is not needed. A call that uses the object again from inside a call on the same thread is refused when either of the two is exclusive, a `&mut self` method or a `with_mut` borrow, rather than getting a second reference to the value. A method call refused this way throws a `PwrsException`, and a property read gives `$null`, which is what PowerShell's property adapter makes of an exception a getter throws. Shared entries nest: a script reading a property while a cmdlet has the object borrowed through `PsProxy::with` reads it.

With `PsTyped` and `FromPs` the class is also a parameter type and a field type. The shell declares the class (`PSObject` for psobject mode), and `FromPs` reads the object back one field per property name, so a copied object, a proxy, or a PSObject with the note properties all convert. A class used as a cmdlet parameter type needs `Default`, like every parameter type.

### `#[psfield(...)]`

On a field of a `#[psclass]` struct.

| Key | Form | Effect |
|---|---|---|
| `skip` | flag | the field stays in Rust: no property, not packed, not read back, no type requirement. The field is still constructed by the module's own code and is reachable from `#[psmethods]` methods |
| `attribute` | `attribute(type = "System.ComponentModel.DisplayNameAttribute", args = ["Badge text"])`, any number | the .NET attribute on the field's property, as [The `attribute(...)` key](#the-attribute-key) says. A compile error beside `skip`, which leaves no property, and on a field of a psobject class |

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

A method with a receiver may take one argument `this: &PsThis`: the object the method runs on, which the generated method passes and its .NET signature does not have. `this.raise("Changed", args)` raises an event of the object, and the object stays alive as long as a clone of `this` does, for a thread that raises later; see [How To Raise And Handle Object Events](../how-to/How-To-Raise-And-Handle-Object-Events.md). A raise made during the call is delivered once the call returns. `#[member]` matching leaves the argument out, so a property's setter `fn(&mut self, this: &PsThis, value: T)` backs the property. A static, a constructor, a second `&PsThis` and an operator's method taking one are refused.

A method may take one argument `PsTask<T>`, and then returns `PsResult<()>`. The generated method returns `Task<T>`, with `T` declared as an argument of `T` would be: `Task` for `PsTask<()>`, and `Task<long?>` for `PsTask<Option<i64>>`, which `complete(None)` completes with `$null`. It takes no parameter for the `PsTask` itself, and gains a last parameter `CancellationToken cancellationToken = default`. The method moves the `PsTask` to the thread that does the work, which finishes it with `complete`, `fail` or `cancel`. The token sets a flag that `is_cancelled` reads, and the task ends canceled only when the module calls `cancel`, so a cancellation never discards a result the work has already set. A task dropped without being finished faults with `PwrsTaskDropped`. An `Err` from the method itself is thrown by the call, as for any method. Another argument named `cancellation_token` is refused by `cargo pwrs build`, and a constructor takes no `PsTask`.

A method with a receiver may take one argument `&Self` (or `&Type`): another object of the class, borrowed as shared for the call. The generated method declares the class as the parameter type and throws `ArgumentNullException` for `$null`. It enters both objects' gates before the call, the receiver as the method takes it and the other as shared, always in the order the two objects were made. So two threads calling across the same pair the opposite way round never wait on each other. The same object as both is entered once, and refused for a `&mut self` method. A static takes no `&Self`, nor a method two; any other reference type is a compile error.

`#[member = "Name"]` on a method makes it the member of that name of the class's `base`, or of an interface `implements` names. The method is declared under no name of its own; the member is. `cargo pwrs build` reads the member from the reference assemblies of both target frameworks and declares it with its own signature, as an override of the base's member or an implementation of the interface's. Each argument the member receives must convert to the method's by C#'s implicit conversions: the same type, a wider number, `object`, or a nullable value. The method's result must convert to the member's type the same way, or be a `PsObject`, which reaches any reference type, converted as PowerShell converts it when the call runs. A property's getter is a method taking `&self` and returning the value, and its setter takes the value and returns `PsResult<()>`; an indexer's take its index arguments first.

A member's out arguments come back after its result:

- `R M(A a, out B b)` is backed by `fn m(&self, a: A) -> PsResult<(R, B)>`.
- A member returning nothing is backed by a method returning its one out argument, or a tuple of them.
- `bool M(.., out V v)` can also be backed by one returning `PsResult<Option<V>>`, whose `None` gives `false` and `v`'s default.

Each value converts to its out argument's type as a result does. An `Option<PsObject>` cannot tell `None` from a null object, so it is refused there; return `(bool, PsObject)` instead. A `ref` argument is a `&mut T` argument: the member passes its value in and copies back what the method leaves there, which converts as a result does. An `in` argument is passed by value. A generic member's method takes the member's type arguments first, as `types: &[PsType]`, each named as a script writes it, as `System.Int64` or `System.Collections.Generic.List[System.String]`. It takes and returns each value whose type names a type parameter of the member as a `PsObject`, which the member converts as PowerShell converts a value, `null` giving the type's default. An interface's generic method with a constraint on a type parameter is implemented explicitly, so the interface's constraints still apply. A tuple result, a `&mut` argument and a `&[PsType]` argument are compile errors on a method not marked `#[member]`, and a `&[PsType]` argument after an argument the member has.

A name two members share is written with its declaring type, as `#[member = "System.Collections.IComparer.Compare"]`, or with the last part of that name, as `#[member = "IComparer<long>.Compare"]`, whose type arguments may then be named without their namespaces, as `#[member = "IEnumerable<KeyValuePair<string,long>>.GetEnumerator"]`. An interface's member is declared public under its name, unless the class has another member of that name, and is then implemented explicitly. A member the method fits on one target framework only is declared for that one alone, and the build says so in a note. `cargo pwrs build` refuses each of these, naming the class, the method and the member:

- a name no member has
- members of the name, none of which the method fits
- two members the method fits
- two methods backing one member
- every abstract member left without a method that `stubs` does not name

A member that is not virtual or abstract, returns by reference, or has a type parameter allowing a ref struct fits no method. `Dispose` frees the class's Rust value, so a method marked `#[member = "Dispose"]` is refused; the value's `Drop` frees what it owns. A compile error for a method with no receiver, one taking a `PsTask`, a private one, a second `#[member]` on one method, and a `#[member]` with no name.

`#[base(arg, ...)]` on `new`, in a class with a `base`, names the arguments of `new` it passes to the base class's constructor, in that constructor's order, and the build calls the constructor they fit. Without it, `new` calls the base constructor that takes no arguments. The base constructor runs first and Rust's `new` second, so a `new` that returns `Err` leaves an object with no Rust value behind it, and so nothing to free. `#[base]` alone, on a method `fn(&self) -> PsResult<T>`, or `PsResult<(A, B, ...)>` for several, gives the base constructor's arguments for an object Rust makes outside `new`, as a cmdlet writing one does. A class whose base has no constructor taking no arguments needs one, and a class has at most one. The generated class does not declare it, and no role may name it. A compile error for `#[base(...)]` on a method other than `new`, a name `new` has no argument for, a name given twice, and a `#[base]` method that takes more than `&self` or returns nothing.

`#[attribute(type = "System.ObsoleteAttribute", args = ["Read Text."])]` on a method, any number of them, puts the .NET attribute on the method of the generated class, or on `new`'s public constructor, as [The `attribute(...)` key](#the-attribute-key) says. A compile error on a private fn, and on a method marked `#[member]` or a bare `#[base]`, which the class declares as a private helper or not at all.

A constructor has no pipeline, so it cannot write a warning or an error record: an `Err` is the exception the script's `::new` call throws. A condition a cmdlet would warn about is either refused in a constructor or left silent there, and a cmdlet that must warn does its own check.

**On a proxy class** the value `new` returns is the one behind the new object.

**On a copied class** there is no Rust value behind the object, so only statics can be declared, and a method taking `&self` is refused by `cargo pwrs build` with the reason. `new` builds the value in Rust and the new object is made from it field by field, so its properties are ordinary settable fields like any other copied object's. What a script can construct depends on whether the class declares a `new`:

| The class declares | Public constructors |
|---|---|
| no `new` | the parameterless one C# supplies, which leaves every field at zero or null |
| a `new` | only that one; the parameterless constructor is not emitted, so a script cannot make an object with every field at zero or null |

For a parameterless constructor that starts from the type's `Default` rather than zeros, give `new` all-`Option` arguments and start from `Self::default()`. The generated factory builds objects through a constructor of its own, selected by the runtime's `Pwrs.FromFields` marker, which no Rust signature produces. So it never calls a constructor the module declared, and a `new()` that returns `Self` cannot call itself through the factory. Arguments are of the parameter types: `Option<T>` makes the C# parameter optional with a `null` default, `bool` is a `bool` and not a switch, and a `#[psclass]` type is read back through its properties. A `PsType` argument is declared `string`, the type's name as a script writes it, which the method reads as a `PsType`. A method returns `PsResult<T>` for any of those types, including a `#[psclass]` type (a proxy class returned this way is a new proxy object), or `PsResult<()>` for nothing. An `Option<String>` or `Option<PathBuf>` argument is declared as `object?` rather than `string?`. The engine's method binder turns `$null` and an omitted argument into an empty string for a `string` parameter, so `None` would never arrive. The generated method converts a supplied value to a string itself. Async and generic methods are rejected, and a method takes any number of arguments. The method's doc comment becomes its `<summary>`.

Generates a `PsMethods<Type>` impl on `MethodsCollector<Type>` with the method descriptors and the dispatch; `#[psclass]` finds it through method resolution, so a class without `#[psmethods]` needs nothing. `cargo pwrs build` refuses methods on a psobject or enum class, and a method taking `&self` on a copied one. Each call is one `pwrs_proxy_call` with a packed argument block. Calls and property reads on one proxy take turns at the object's `Pwrs.ProxyCore`, and an `Err` from the method becomes a `PwrsException` in the script, or the exception the error names with [`PsError::with_exception`](Pipeline-Reference.md#pserror-and-errorcategory-errorrs). A static or a constructor is the same call with a null instance, through the runtime's `Pwrs.StaticCall.Invoke`, which serializes nothing and checks no generation because there is no object to guard. A method with a receiver reached with a null instance is refused with `PwrsNoInstance` rather than run. The constructor's call returns, for a proxy, the new value's pointer, which the public constructor passes to the internal one; for a copied class, the object the class's factory built, whose fields the public constructor copies into itself.

## The `attribute(...)` key

`#[cmdlet]`, `#[param]`, `#[psclass]` and `#[psfield]` take `attribute(...)`, and a `#[psmethods]` method takes `#[attribute(...)]`, each any number of times, to put a .NET attribute on what the shell generates for them. [How To Write A Cmdlet](../how-to/How-To-Write-A-Cmdlet.md#net-attributes) shows each in hello.

| Part | Form | Meaning |
|---|---|---|
| `type` | `type = "System.ObsoleteAttribute"` | required; the attribute class's full name, its `Attribute` suffix included |
| `args` | `args = ["Use Next.", false]`, or one value, `args = "Use Next."` | the constructor's arguments, in order |
| `named` | `named(Help = "Hello", Value = 3)` | properties and fields set by name, matched without regard to case |

A value is a string, an integer or a float literal with an optional sign, `true` or `false`, a char, `null`, or a list `[...]` of values. `cargo pwrs build` reads the class from the reference assemblies of both shells and writes the attribute through the public constructor the arguments fit, one whose parameters take the values' own types before one taking `object`. A constructor's `params` array takes its elements as separate arguments, or as a list. Where a parameter is an enum, a string names a member, or several joined by commas, matched without regard to case; where it is a `System.Type`, a string is a type's full name, written as `typeof`. An integer must fit its parameter's type.

An attribute class that only one shell's references have is written for that shell alone, under `#if NET` or `#if !NET`, and the build warns, naming it and the shell that leaves it off. The build refuses each of these, naming where the key was written and the shell whose references refused it:

- a type in neither shell's references as a public attribute class
- an abstract one
- one whose `[AttributeUsage]` does not allow it on a class, a constructor, a property or a method, whichever the key's place is
- one given twice that its `[AttributeUsage]` allows once
- arguments that fit no public constructor, which the refusal lists, or that fit two equally
- a named member that is no settable property or field
- `System.ObsoleteAttribute` with `true` for its error argument, since PowerShell ignores the error flag and warns either way, and C# would refuse the shell's own uses of the member

The shell is compiled with C#'s warnings for using an obsolete member off, since it reads and sets every member it declares.

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

The function becomes a unit struct of the same name implementing `TransformFn`; list that name under `transforms`. It generates an `ArgumentTransformationAttribute` on the parameter, which the engine runs before converting the argument to the parameter's declared type and before validation. So a `long` parameter can accept `2MB`, and a refusal is a binding failure naming the parameter rather than an error the cmdlet wrote. The refusal's text is the error's message alone after the binder's sentence, and the error's id and category are in the `Data` of its exception inside the binder's.

The value arrives as the caller wrote it, so a transform checks it carefully and returns anything it does not recognize unchanged, for the binder to convert or reject; returning the value unchanged is always correct. No instance exists yet, so it takes no `Pipeline` and cannot write to a stream.

## `#[dynamic_params(...)]`

On a `fn(bound: &PsHashtable) -> PsResult<Vec<DynamicParam>>`.

| Key | Form | Effect |
|---|---|---|
| `cmdlet` | `cmdlet = GetThing` | the cmdlet type (a path, not a string) the function is for |

Implements `DynamicParams` for that type; list the cmdlet type under `dynamic_params`.

## `#[on_load]`, `#[on_import]`, `#[on_remove]` and `#[on_unload]`

Each on a `fn() -> PsResult<()>`, and `#[on_remove]` also on a `fn(removal: &Removal) -> PsResult<()>`. None takes a key. A key is a compile error, and so is any other argument.

The function becomes a unit struct of the same name implementing `OnLoad`, `OnImport`, `OnRemove` or `OnUnload`. Name it under `on_load`, `on_import`, `on_remove` or `on_unload` in `export_module!`.

- `on_import` runs at every import of the module, in any runspace.
- `on_remove` runs once for each import. It runs at `Remove-Module`, or when the runspace that imported the module closes without removing it. A runspace closes when it is disposed, when a thread job or a `ForEach-Object -Parallel` run ends, when its pool is disposed, and when PowerShell exits.
- `on_load` runs when the module goes from imported in no runspace of the process to imported in one, before that import's `on_import`.
- `on_unload` runs after the last `on_remove` in the process.

An `on_remove` that takes a `&Removal` gets the reason it runs and the runspace it runs for. `removal.cause` is `RemovalCause::RemoveModule` when `Remove-Module` ran it, and `RemovalCause::RunspaceClosed` when the runspace that imported the module closed or broke with the module still imported. `removal.runspace` is the `InstanceId` of the runspace whose import it removes, as a `PsGuid`, under either cause; it is `None` only for an import made on a thread with no default runspace. A command reads the same id with [`Pipeline::runspace_id`](Pipeline-Reference.md#pipelineps-cratespwrssrcpipeliners-workerrs), so a module can record which runspace made something when the command runs, and release that runspace's things, and only those, when the runspace removes the module or closes. A process-wide resource belongs in `on_unload` instead, which runs once no runspace has the module. Hello's removal hook records the last removal, and the [Hello tour](Hello-Tour.md#get-rustlifecycle-what-runs-at-load-import-removal-and-unload) shows what it recorded in each case. A unit test gives the hook a removal with `pwrs::testing::run_removal_hook`; see [How To Test A Module](../how-to/How-To-Test-A-Module.md).

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

The struct's doc comment is the provider description in the descriptor. `Filter` and `Credentials` let `-Filter` and `-Credential` reach the provider, which PowerShell refuses otherwise, and `ExpandWildcards` sends a path with wildcards to `expand_path`. `Transactions` gives the provider's commands `-UseTransaction` in Windows PowerShell 5.1, and the call's transaction reaches the provider as `pwrs::provider::call().transaction`; PowerShell 7 gives a provider's commands no `-UseTransaction`. `Include` and `Exclude` are accepted, and change nothing: PowerShell applies `-Include` and `-Exclude` itself whatever a provider declares. [How To Write A Provider](../../how-to/how-to-write-a-provider/#the-calls-filter-patterns--force-and-credential) says what each one does.

## `#[tiered]`

On a free function. Takes no keys. The body is compiled for the baseline, for each x86-64 psABI level above it with that level's extensions and the lower levels' (`x86-64-v2`, `x86-64-v3`, `x86-64-v4`), and for each AArch64 feature tier above `neon` with its features and the lower tiers' (`dotprod`, `i8mm`, `sve`, `sve2`). The function runs the copy for `pwrs::cpu::level()` on x86-64 and `pwrs::cpu::arm::tier()` on AArch64, each computed once per process and lowered by `PWRS_CPU_MAX`, and the baseline copy on any other architecture. The copies are the same Rust, so they give the same results; [How To Use Instruction Sets](../how-to/How-To-Use-Instruction-Sets.md#one-body-every-tier) has the test that compares them.

Each argument is a name, `mut` or not, and the function may be generic over types and constants. An async, const or extern function, a method, a pattern argument, and an `impl Trait` argument or return type are compile errors naming the reason. An `unsafe fn` stays unsafe to call.

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
