//! `#[psenum]` expansion: a fieldless Rust enum that the shell
//! declares as a CLR enum with `long` as its underlying type.
//!
//! The Rust enum gains `PsTyped` (so it can be a parameter or field
//! type), `PsClassMeta` (so `export_module!` can register it and the
//! generator can emit the CLR type), `IntoPs` (the discriminant
//! crosses as an `i64` block through the class factory) and `FromPs`
//! (the CLR value reads back as its underlying integer).

use crate::json::Obj;
use crate::params::{key_of, lit_int, lit_str, MetaList};
use crate::types::doc_lines;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Fields, ItemEnum, Meta, Result};

struct EnumArgs {
    name: Option<String>,
    clr: Option<String>,
}

fn parse_enum_args(attr: TokenStream) -> Result<EnumArgs> {
    let list: MetaList = syn::parse2(attr)?;
    let mut name = None;
    let mut clr = None;
    let mut clr_span = None;
    for m in list.0 {
        match m {
            Meta::NameValue(nv) => {
                let key = key_of(&nv.path)?;
                match key.as_str() {
                    "name" => name = Some(lit_str(&nv.value)?),
                    "clr" => {
                        clr_span = Some(nv.path.clone());
                        clr = Some(lit_str(&nv.value)?);
                    }
                    unknown => return Err(syn::Error::new_spanned(nv.path, format!("unknown #[psenum] key `{unknown}`"))),
                }
            }
            other => return Err(syn::Error::new_spanned(other, "#[psenum] takes name = \"Namespace.Type\" or clr = \"Namespace.Type\"")),
        }
    }
    if name.is_some() {
        if let Some(path) = clr_span {
            return Err(syn::Error::new_spanned(
                path,
                "#[psenum] takes name or clr, not both: name declares a CLR enum this module owns, clr mirrors one that already exists",
            ));
        }
    }
    Ok(EnumArgs { name, clr })
}

pub fn expand_psenum(attr: TokenStream, input: ItemEnum) -> Result<TokenStream> {
    let args = parse_enum_args(attr)?;
    let name = input.ident.clone();
    let mirrors = args.clr.is_some();
    let clr_name = match (args.name, args.clr) {
        (_, Some(existing)) => existing,
        (Some(n), None) => n,
        (None, None) => name.to_string(),
    };
    let mut variants = Vec::new();
    let mut next: i64 = 0;
    for v in &input.variants {
        if !matches!(v.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(v, "#[psenum] needs a fieldless enum"));
        }
        let value = match &v.discriminant {
            Some((_, expr)) => lit_int(expr)?,
            None => next,
        };
        next = value + 1;
        let help = doc_lines(&v.attrs).join(" ").trim().to_string();
        variants.push((v.ident.clone(), value, help));
    }
    if variants.is_empty() {
        return Err(syn::Error::new_spanned(&input, "#[psenum] needs at least one variant"));
    }

    let docs = doc_lines(&input.attrs);
    let variants_json: Vec<String> = variants
        .iter()
        .map(|(ident, value, help)| {
            let mut o = Obj::new();
            o.str("name", &ident.to_string()).num("value", *value).str("help", help);
            o.finish()
        })
        .collect();
    let mut o = Obj::new();
    o.str("name", &clr_name)
        .str("rust", &name.to_string())
        .str("mode", "enum")
        .str("description", docs.join("\n").trim())
        .raw("fields", "[]")
        .raw("variants", &crate::json::array(&variants_json));
    let descriptor = o.finish();

    let to_i64_arms = variants.iter().map(|(ident, value, _)| quote!(#name::#ident => #value,));
    let from_i64_arms = variants.iter().map(|(ident, value, _)| quote!(#value => ::core::result::Result::Ok(#name::#ident),));

    // A mirror has no class of its own: the CLR type already exists,
    // nothing is generated for it, and it takes no class id. Without
    // `PsClassMeta` it also cannot be listed under `enums:`, which is
    // the guardrail rather than a silent second declaration of a type
    // the shell does not own.
    let class_meta = if mirrors { quote!() } else { class_meta_impl(&name, &clr_name, &descriptor) };
    let into_ps_value = if mirrors {
        quote!(::pwrs::values::enum_value(#clr_name, value))
    } else {
        quote!({
            let class_id = <#name as ::pwrs::class::PsClassMeta>::class_id()?;
            unsafe { ::pwrs::runtime::factory_new(class_id, &value as *const i64 as *const ::core::ffi::c_void) }
        })
    };

    Ok(quote! {
        #input

        impl ::pwrs::class::PsTyped for #name {
            const CLR_NAME: &'static str = #clr_name;
            const VALUE_TYPE: bool = true;
        }

        #class_meta

        impl ::pwrs::IntoPs for #name {
            fn into_ps(self) -> ::pwrs::PsResult<::pwrs::PsObject> {
                let value: i64 = match self {
                    #(#to_i64_arms)*
                };
                #into_ps_value
            }
        }

        impl ::pwrs::FromPs for #name {
            fn from_ps(obj: &::pwrs::PsObject) -> ::pwrs::PsResult<Self> {
                let value = <i64 as ::pwrs::FromPs>::from_ps(obj)?;
                match value {
                    #(#from_i64_arms)*
                    other => Err(::pwrs::PsError::new(
                        ::pwrs::ErrorCategory::InvalidData,
                        "PwrsEnumValue",
                        format!("{} has no variant with value {}", #clr_name, other),
                    )),
                }
            }
        }
    })
}

/// The `PsClassMeta` impl a module-owned enum gets and a mirror of an
/// existing CLR enum does not.
fn class_meta_impl(name: &syn::Ident, clr_name: &str, descriptor: &str) -> TokenStream {
    quote! {
        impl ::pwrs::class::PsClassMeta for #name {
            const NAME: &'static str = #clr_name;
            const MODE: &'static str = "enum";
            fn descriptor() -> ::std::string::String {
                ::std::string::String::from(#descriptor)
            }
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
            unsafe fn proxy_get(_instance: *mut ::core::ffi::c_void, _field_id: u32) -> ::pwrs::PsResult<::pwrs::PsObject> {
                Err(::pwrs::PsError::new(::pwrs::ErrorCategory::InvalidOperation, "PwrsNotAProxy", format!("{} is an enum", #clr_name)))
            }
            unsafe fn proxy_call(_instance: *mut ::core::ffi::c_void, _method_id: u32, _args: *const ::core::ffi::c_void) -> ::pwrs::PsResult<::pwrs::PsObject> {
                Err(::pwrs::PsError::new(::pwrs::ErrorCategory::InvalidOperation, "PwrsNotAProxy", format!("{} is an enum", #clr_name)))
            }
            unsafe fn proxy_drop(_instance: *mut ::core::ffi::c_void) {}
        }
    }
}
