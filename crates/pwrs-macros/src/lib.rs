//! Attribute macros that turn Rust items into cmdlet, class, and enum
//! descriptors. `#[cmdlet]` emits, for one struct: the struct without
//! its `#[param]` attributes, a `#[repr(C)]` parameter block whose
//! layout the C# generator mirrors, a `CmdletMeta` impl carrying the
//! JSON descriptor, and a `CmdletBind` impl that reads the block.
//! `#[psclass]` emits the `PsClassMeta` and `IntoPs` impls for one of
//! three output modes; `#[psmethods]` adds callable methods to a proxy
//! class.

mod classes;
mod completers;
mod enums;
mod json;
mod methods;
mod params;
mod types;

use proc_macro::TokenStream;
use syn::{parse_macro_input, ItemEnum, ItemFn, ItemImpl, ItemStruct};

/// `#[cmdlet(verb = "Get", noun = "Thing", supports_should_process,
/// confirm_impact = "Medium", default_parameter_set = "X",
/// alias = ["gt"], output = ["System.String"])]` on a struct whose
/// `#[param]` fields are the parameters.
#[proc_macro_attribute]
pub fn cmdlet(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    match params::expand_cmdlet(attr.into(), input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// `#[param(...)]` is consumed by `#[cmdlet]`; on its own it is an error.
#[proc_macro_attribute]
pub fn param(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let err = syn::Error::new(proc_macro2::Span::call_site(), "#[param] is only valid on fields of a #[cmdlet] struct");
    let mut out: TokenStream = err.to_compile_error().into();
    out.extend(item);
    out
}

/// `#[psclass(name = "My.Type", mode = copied)]` on an output struct.
/// Modes: `copied` (default), `proxy`, `psobject`. A field marked
/// `#[psfield(skip)]` stays in Rust: not a property, not packed, not
/// read back.
#[proc_macro_attribute]
pub fn psclass(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    match classes::expand_psclass(attr.into(), input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// `#[psfield(skip)]` is consumed by `#[psclass]`; on its own it is an
/// error.
#[proc_macro_attribute]
pub fn psfield(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let err = syn::Error::new(proc_macro2::Span::call_site(), "#[psfield] is only valid on fields of a #[psclass] struct");
    let mut out: TokenStream = err.to_compile_error().into();
    out.extend(item);
    out
}

/// `#[completer(cmdlet = "Get-Thing", parameter = "Name")]` on a
/// `fn(ctx: &CompletionContext) -> PsResult<Vec<Completion>>`. The
/// function becomes a unit struct implementing `CompleterFn`, so its
/// name is what `export_module!` lists under `completers`.
#[proc_macro_attribute]
pub fn completer(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    match completers::expand_completer(attr.into(), input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// `#[transform(cmdlet = "Get-Thing", parameter = "Size")]` on a
/// `fn(value: &PsObject) -> PsResult<PsObject>`. The function becomes
/// a unit struct implementing `TransformFn`, so its name is what
/// `export_module!` lists under `transforms`. The engine runs it
/// before the argument is coerced to the parameter's type, so a
/// refusal is a binding failure naming the parameter.
#[proc_macro_attribute]
pub fn transform(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    match completers::expand_transform(attr.into(), input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// `#[dynamic_params(cmdlet = GetThing)]` on a free
/// `fn(bound: &PsHashtable) -> PsResult<Vec<DynamicParam>>`; the
/// function becomes the cmdlet type's `DynamicParams` impl.
#[proc_macro_attribute]
pub fn dynamic_params(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    match completers::expand_dynamic_params(attr.into(), input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// `#[on_import]` on a `fn() -> PsResult<()>`. The function becomes a
/// unit struct implementing `OnImport`, so its name is what
/// `export_module!` names under `on_import`. It runs on every import
/// of the module, including one after a removal in the same session,
/// and an `Err` fails the import.
#[proc_macro_attribute]
pub fn on_import(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    match completers::expand_on_import(attr.into(), input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// `#[on_remove]` on a `fn() -> PsResult<()>`. The function becomes a
/// unit struct implementing `OnRemove`, so its name is what
/// `export_module!` names under `on_remove`. It runs when the module
/// is removed, which unloads nothing, so it is where an import's
/// resources are released.
#[proc_macro_attribute]
pub fn on_remove(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    match completers::expand_on_remove(attr.into(), input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// `#[provider(name = "Rust")]` on a struct implementing `Provider`;
/// generates the `ProviderMeta` descriptor. The drives it serves come
/// from `Provider::default_drives` and `New-PSDrive`, each of which
/// hands back the instance serving that drive.
#[proc_macro_attribute]
pub fn provider(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    match completers::expand_provider(attr.into(), input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// `#[psenum(name = "My.Kind")]` on a fieldless enum, emitted as a
/// CLR enum with `long` underneath. The enum is then a parameter
/// type, a class field type, and an output value; list it under
/// `enums` in `export_module!`.
#[proc_macro_attribute]
pub fn psenum(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemEnum);
    match enums::expand_psenum(attr.into(), input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// `#[psmethods]` on `impl Type { ... }` for a proxy `#[psclass]`
/// type: every `pub fn` taking `&self` or `&mut self` and returning
/// `PsResult<T>` becomes a method of the generated proxy class, with
/// the parameter types of a cmdlet and `PsResult<()>` for a method
/// returning nothing.
#[proc_macro_attribute]
pub fn psmethods(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemImpl);
    match methods::expand_psmethods(attr.into(), input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}
