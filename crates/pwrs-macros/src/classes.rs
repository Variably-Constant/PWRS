//! `#[psclass]` expansion: three output modes over one field list.
//!
//! * `copied`: a generated CLR class; `IntoPs` packs a field block and
//!   calls the module's factory once per object.
//! * `proxy`: the Rust value is boxed and the generated CLR class reads
//!   fields through `pwrs_proxy_get` until it is disposed.
//! * `psobject`: no CLR type; `IntoPs` builds a `PSObject` with note
//!   properties and a `PSTypeName`.

use crate::json::{self, Obj};
use crate::params::{key_of, lit_str, MetaList};
use crate::types::{check_reserved, doc_lines, lower, output_clr, pascal, Customs, Lowered, Slot, OBJECT_MEMBERS, PROXY_MEMBERS};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{punctuated::Punctuated, Attribute, Ident, ItemStruct, Meta, Result, Token};

/// `#[psfield(...)]` on a field. `skip` keeps the field out of the
/// PowerShell surface: not a property, not packed, not read back.
struct FieldArgs {
    skip: bool,
}

fn parse_field_args(attr: &Attribute) -> Result<FieldArgs> {
    let mut a = FieldArgs { skip: false };
    let list: Punctuated<Meta, Token![,]> = match &attr.meta {
        Meta::Path(bare) if bare.is_ident("psfield") => return Ok(a),
        Meta::Path(other) => return Err(syn::Error::new_spanned(other, "expected #[psfield] or #[psfield(skip)]")),
        Meta::List(l) => l.parse_args_with(Punctuated::parse_terminated)?,
        Meta::NameValue(nv) => return Err(syn::Error::new_spanned(nv, "#[psfield] takes a parenthesized list")),
    };
    for m in list {
        match m {
            Meta::Path(p) if p.is_ident("skip") => a.skip = true,
            other => return Err(syn::Error::new_spanned(other, "unknown #[psfield] argument; use skip")),
        }
    }
    Ok(a)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Copied,
    Proxy,
    PsObject,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Copied => "copied",
            Mode::Proxy => "proxy",
            Mode::PsObject => "psobject",
        }
    }
}

struct ClassArgs {
    name: Option<String>,
    mode: Mode,
    /// The path of a `fn(&Self) -> usize` answering the native bytes a
    /// proxy value holds.
    native_bytes: Option<syn::ExprPath>,
}

fn parse_mode(expr: &syn::Expr) -> Result<Mode> {
    let word = match expr {
        syn::Expr::Path(p) => match p.path.get_ident() {
            Some(i) => i.to_string(),
            None => return Err(syn::Error::new_spanned(expr, "mode takes copied, proxy, or psobject")),
        },
        other => lit_str(other)?,
    };
    match word.as_str() {
        "copied" => Ok(Mode::Copied),
        "proxy" => Ok(Mode::Proxy),
        "psobject" => Ok(Mode::PsObject),
        other => Err(syn::Error::new_spanned(expr, format!("unknown mode `{other}`; use copied, proxy, or psobject"))),
    }
}

fn parse_class_args(attr: TokenStream) -> Result<ClassArgs> {
    let list: MetaList = syn::parse2(attr)?;
    let mut name = None;
    let mut mode = Mode::Copied;
    let mut native_bytes = None;
    for m in list.0 {
        match m {
            Meta::Path(p) if p.is_ident("copied") => mode = Mode::Copied,
            Meta::Path(p) if p.is_ident("proxy") => mode = Mode::Proxy,
            Meta::Path(p) if p.is_ident("psobject") => mode = Mode::PsObject,
            Meta::Path(p) => return Err(syn::Error::new_spanned(p, "unknown #[psclass] flag; use copied, proxy, or psobject")),
            Meta::NameValue(nv) => {
                let key = key_of(&nv.path)?;
                match key.as_str() {
                    "name" => name = Some(lit_str(&nv.value)?),
                    "mode" => mode = parse_mode(&nv.value)?,
                    "native_bytes" => match nv.value {
                        syn::Expr::Path(p) => native_bytes = Some(p),
                        other => return Err(syn::Error::new_spanned(other, "native_bytes takes the path of a fn(&Self) -> usize")),
                    },
                    unknown => return Err(syn::Error::new_spanned(nv.path, format!("unknown #[psclass] key `{unknown}`"))),
                }
            }
            Meta::List(l) => return Err(syn::Error::new_spanned(l, "unknown #[psclass] argument")),
        }
    }
    if let Some(path) = &native_bytes {
        if mode != Mode::Proxy {
            return Err(syn::Error::new_spanned(path, "native_bytes applies to a proxy class; a copied or psobject class keeps no Rust value behind its object"));
        }
    }
    Ok(ClassArgs { name, mode, native_bytes })
}

struct FieldSpec {
    field: Ident,
    ps_name: String,
    lowered: Lowered,
    help: String,
}

fn field_json(i: usize, f: &FieldSpec, customs: &mut Customs) -> String {
    let mut o = Obj::new();
    o.str("name", &f.ps_name).str("rust", &f.field.to_string()).num("index", i as i64);
    customs.clr_keys(&mut o, &output_clr(&f.lowered));
    o.str("slot", f.lowered.slot.name()).bool("optional", f.lowered.optional).str("help", &f.help);
    o.finish()
}

/// Statements that place field `i` of `self` into `block`, keeping any
/// buffers alive in the `keep_*` vectors until the factory returns.
fn pack_stmt(i: usize, f: &FieldSpec) -> TokenStream {
    let slot = format_ident!("p{}", i);
    let field = &f.field;
    let bit = i as u64;
    let value = if f.lowered.optional { quote!(__value) } else { quote!(self.#field) };
    let store = match f.lowered.slot {
        Slot::Bool => quote!(block.#slot = if #value { 1 } else { 0 };),
        Slot::Str16 => {
            let to_string = if f.lowered.path { quote!(#value.to_string_lossy().into_owned()) } else { quote!(#value) };
            quote!({
                let s: ::std::string::String = #to_string;
                let u: ::std::vec::Vec<u16> = ::pwrs::text::to_utf16(&s);
                block.#slot = ::pwrs::sys::PsStr16 { ptr: u.as_ptr(), len: u.len() };
                keep_strings.push(u);
            })
        }
        Slot::Handle => quote!({
            let obj = ::pwrs::IntoPs::into_ps(#value)?;
            block.#slot = obj.as_raw();
            keep_handles.push(obj);
        }),
        Slot::I8 | Slot::I16 | Slot::I32 | Slot::I64 | Slot::U8 | Slot::U16 | Slot::U32 | Slot::U64 | Slot::F32 | Slot::F64 => {
            quote!(block.#slot = #value;)
        }
    };
    if f.lowered.optional {
        quote!(if let ::core::option::Option::Some(__value) = self.#field { block.mask |= 1u64 << #bit; #store } else { block.mask &= !(1u64 << #bit); })
    } else {
        store
    }
}

pub fn expand_psclass(attr: TokenStream, mut input: ItemStruct) -> Result<TokenStream> {
    let args = parse_class_args(attr)?;
    let mode = args.mode;
    let name = input.ident.clone();
    let clr_name = match args.name {
        Some(n) => n,
        None => name.to_string(),
    };
    let mut fields = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for field in input.fields.iter_mut() {
        let ident = match &field.ident {
            Some(i) => i.clone(),
            None => return Err(syn::Error::new_spanned(&*field, "#[psclass] needs named fields")),
        };
        let args = match field.attrs.iter().position(|a| a.path().is_ident("psfield")) {
            Some(i) => {
                let attr = field.attrs.remove(i);
                parse_field_args(&attr)?
            }
            None => FieldArgs { skip: false },
        };
        if args.skip {
            skipped.push(ident.to_string());
        } else {
            let ps_name = pascal(&ident.to_string());
            // A psobject class generates no CLR type, so its note
            // properties collide with nothing.
            let reserved: &[&[&str]] = match mode {
                Mode::Proxy => &[OBJECT_MEMBERS, PROXY_MEMBERS],
                Mode::Copied => &[OBJECT_MEMBERS],
                Mode::PsObject => &[],
            };
            check_reserved(&ident, "a field", &ps_name, reserved)?;
            let lowered = lower(&field.ty)?;
            let help = doc_lines(&field.attrs).join(" ").trim().to_string();
            fields.push(FieldSpec { ps_name, field: ident, lowered, help });
        }
    }
    if fields.len() > 64 {
        return Err(syn::Error::new_spanned(&input.fields, "a class may declare at most 64 fields"));
    }
    let docs = doc_lines(&input.attrs);
    let mut customs = Customs::default();
    let fields_json: Vec<String> = fields.iter().enumerate().map(|(i, f)| field_json(i, f, &mut customs)).collect();
    let mut o = Obj::new();
    o.str("name", &clr_name)
        .str("rust", &name.to_string())
        .str("mode", args.mode.name())
        .bool("native_bytes", args.native_bytes.is_some())
        .str("description", docs.join("\n").trim())
        .raw("fields", &json::array(&fields_json))
        .raw("methods", &json::methods_sentinel());
    let descriptor_fn = customs.descriptor_fn(&o.finish(), Some(&name));

    let block_ident = format_ident!("__PwrsFields{}", name);
    let block_fields = fields.iter().enumerate().map(|(i, f)| {
        let slot = format_ident!("p{}", i);
        let ty = f.lowered.slot.rust_ty();
        quote!(pub #slot: #ty)
    });
    let vis = &input.vis;

    let into_ps = match args.mode {
        Mode::Copied => {
            let packs = fields.iter().enumerate().map(|(i, f)| pack_stmt(i, f));
            quote! {
                impl ::pwrs::IntoPs for #name {
                    fn into_ps(self) -> ::pwrs::PsResult<::pwrs::PsObject> {
                        let class_id = <#name as ::pwrs::class::PsClassMeta>::class_id()?;
                        let mut block: #block_ident = unsafe { ::core::mem::zeroed() };
                        let mut keep_strings: ::std::vec::Vec<::std::vec::Vec<u16>> = ::std::vec::Vec::new();
                        let mut keep_handles: ::std::vec::Vec<::pwrs::PsObject> = ::std::vec::Vec::new();
                        #(#packs)*
                        let out = unsafe { ::pwrs::runtime::factory_new(class_id, &block as *const #block_ident as *const ::core::ffi::c_void) };
                        drop(keep_strings);
                        drop(keep_handles);
                        out
                    }
                }
            }
        }
        Mode::Proxy => quote! {
            const _: () = ::pwrs::runtime::assert_send::<#name>();

            impl ::pwrs::IntoPs for #name {
                fn into_ps(self) -> ::pwrs::PsResult<::pwrs::PsObject> {
                    let class_id = <#name as ::pwrs::class::PsClassMeta>::class_id()?;
                    let raw = ::std::boxed::Box::into_raw(::std::boxed::Box::new(self)) as *mut ::core::ffi::c_void;
                    match unsafe { ::pwrs::runtime::factory_new(class_id, raw) } {
                        Ok(obj) => Ok(obj),
                        Err(e) => {
                            drop(unsafe { ::std::boxed::Box::from_raw(raw as *mut #name) });
                            Err(e)
                        }
                    }
                }
            }
        },
        Mode::PsObject => {
            let notes = fields.iter().map(|f| {
                let field = &f.field;
                let ps = &f.ps_name;
                let value = if f.lowered.path { quote!(self.#field.to_string_lossy().into_owned()) } else { quote!(self.#field) };
                quote!(::pwrs::object::add_note(&obj, #ps, ::pwrs::IntoPs::into_ps(#value)?)?;)
            });
            quote! {
                impl ::pwrs::IntoPs for #name {
                    fn into_ps(self) -> ::pwrs::PsResult<::pwrs::PsObject> {
                        let obj = ::pwrs::object::new_psobject(#clr_name);
                        #(#notes)*
                        Ok(obj)
                    }
                }
            }
        }
    };

    let proxy_get_arms = fields.iter().enumerate().map(|(i, f)| {
        let idx = i as u32;
        let field = &f.field;
        let value = if f.lowered.path { quote!(this.#field.to_string_lossy().into_owned()) } else { quote!(this.#field.clone()) };
        quote!(#idx => ::pwrs::IntoPs::into_ps(#value),)
    });
    let proxy_bytes = match &args.native_bytes {
        Some(path) => quote! {
            unsafe fn proxy_bytes(instance: *mut ::core::ffi::c_void) -> u64 {
                let this = &*(instance as *const #name);
                let bytes: usize = (#path)(this);
                bytes as u64
            }
        },
        None => quote!(),
    };
    let proxy_impl = if args.mode == Mode::Proxy {
        quote! {
            #proxy_bytes
            unsafe fn proxy_get(instance: *mut ::core::ffi::c_void, field_id: u32) -> ::pwrs::PsResult<::pwrs::PsObject> {
                let this = &*(instance as *const #name);
                match field_id {
                    #(#proxy_get_arms)*
                    other => Err(::pwrs::PsError::new(
                        ::pwrs::ErrorCategory::InvalidArgument,
                        "PwrsProxyField",
                        format!("{} has no field {}", #clr_name, other),
                    )),
                }
            }
            unsafe fn proxy_call(instance: *mut ::core::ffi::c_void, method_id: u32, args: *const ::core::ffi::c_void) -> ::pwrs::PsResult<::pwrs::PsObject> {
                #[allow(unused_imports)]
                use ::pwrs::class::{NoPsMethods as _, PsMethods as _};
                (&::pwrs::class::MethodsCollector::<#name>::new()).proxy_call(instance, method_id, args)
            }
            unsafe fn proxy_drop(instance: *mut ::core::ffi::c_void) {
                drop(::std::boxed::Box::from_raw(instance as *mut #name));
            }
        }
    } else {
        // A copied class reaches its statics, its constructor among
        // them, through the same table as a proxy's methods, always with
        // a null instance: there is no Rust value behind a copied object
        // to run anything else against.
        let proxy_call = if args.mode == Mode::Copied {
            quote! {
                unsafe fn proxy_call(instance: *mut ::core::ffi::c_void, method_id: u32, args: *const ::core::ffi::c_void) -> ::pwrs::PsResult<::pwrs::PsObject> {
                    #[allow(unused_imports)]
                    use ::pwrs::class::{NoPsMethods as _, PsMethods as _};
                    (&::pwrs::class::MethodsCollector::<#name>::new()).proxy_call(instance, method_id, args)
                }
            }
        } else {
            quote! {
                unsafe fn proxy_call(_instance: *mut ::core::ffi::c_void, _method_id: u32, _args: *const ::core::ffi::c_void) -> ::pwrs::PsResult<::pwrs::PsObject> {
                    Err(::pwrs::PsError::new(
                        ::pwrs::ErrorCategory::InvalidOperation,
                        "PwrsNotAProxy",
                        format!("{} is not a proxy class", #clr_name),
                    ))
                }
            }
        };
        quote! {
            unsafe fn proxy_get(_instance: *mut ::core::ffi::c_void, _field_id: u32) -> ::pwrs::PsResult<::pwrs::PsObject> {
                Err(::pwrs::PsError::new(
                    ::pwrs::ErrorCategory::InvalidOperation,
                    "PwrsNotAProxy",
                    format!("{} is not a proxy class", #clr_name),
                ))
            }
            #proxy_call
            unsafe fn proxy_drop(_instance: *mut ::core::ffi::c_void) {}
        }
    };
    let mode_name = args.mode.name();

    // As a parameter or field type the class is declared by its CLR
    // name; a psobject-mode class has none, so it is declared as
    // PSObject and read back through its note properties.
    let typed_name = match args.mode {
        Mode::PsObject => "System.Management.Automation.PSObject".to_string(),
        Mode::Copied | Mode::Proxy => clr_name.clone(),
    };
    let from_fields = fields.iter().map(|f| {
        let field = &f.field;
        let ps = &f.ps_name;
        let inner = &f.lowered.inner;
        let read_ty = if f.lowered.optional { quote!(::core::option::Option<#inner>) } else { quote!(#inner) };
        quote!(#field: <#read_ty as ::pwrs::FromPs>::from_ps(&obj.get(#ps)?)?)
    });
    // A field kept in Rust only has no property to read it from, so
    // such a class is not reconstructible by value.
    let from_ps_body = if skipped.is_empty() {
        quote!(Ok(#name { #(#from_fields,)* }))
    } else {
        let kept = skipped.join(", ");
        quote!(Err(::pwrs::PsError::new(
            ::pwrs::ErrorCategory::InvalidType,
            "PwrsOpaqueClass",
            format!("{} keeps Rust-only state ({}) and cannot be read back by value", #clr_name, #kept),
        )))
    };

    Ok(quote! {
        #input

        #[repr(C)]
        #[allow(non_camel_case_types, dead_code)]
        #vis struct #block_ident {
            #(#block_fields,)*
            pub mask: u64,
        }

        impl ::pwrs::class::PsTyped for #name {
            const CLR_NAME: &'static str = #typed_name;
            const VALUE_TYPE: bool = false;
        }

        impl ::pwrs::FromPs for #name {
            /// Reads every field back by its property name, from a
            /// copied object, a proxy, or a PSObject with the notes.
            fn from_ps(obj: &::pwrs::PsObject) -> ::pwrs::PsResult<Self> {
                if obj.is_null() {
                    return Err(::pwrs::PsError::new(
                        ::pwrs::ErrorCategory::InvalidData,
                        "PwrsNullObject",
                        format!("a {} cannot be read from $null", #clr_name),
                    ));
                }
                #from_ps_body
            }
        }

        impl ::pwrs::class::PsClassMeta for #name {
            const NAME: &'static str = #clr_name;
            const MODE: &'static str = #mode_name;
            #descriptor_fn
            fn class_id() -> ::pwrs::PsResult<u32> {
                static ID: ::std::sync::OnceLock<u32> = ::std::sync::OnceLock::new();
                match ID.get() {
                    ::core::option::Option::Some(id) => Ok(*id),
                    ::core::option::Option::None => {
                        let id = ::pwrs::runtime::class_id(#clr_name)?;
                        Ok(*ID.get_or_init(|| id))
                    }
                }
            }
            #proxy_impl
        }

        #into_ps
    })
}
