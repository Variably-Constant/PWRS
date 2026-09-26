//! `#[completer]`, `#[transform]` and `#[dynamic_params]` expansion.

use crate::json::Obj;
use crate::params::{key_of, lit_str, str_list, MetaList};
use crate::types::doc_lines;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Expr, ItemFn, ItemStruct, LitStr, Meta, Result};

/// `#[completer(cmdlet = "Get-Thing", parameter = "Name")]` turns the
/// annotated function into a unit struct implementing `CompleterFn`.
pub fn expand_completer(attr: TokenStream, input: ItemFn) -> Result<TokenStream> {
    let (cmdlet, parameter) = target_keys(attr, "completer")?;
    let target = format!("{cmdlet}/{parameter}");

    let vis = &input.vis;
    let name = &input.sig.ident;
    let block = &input.block;
    let arg = match input.sig.inputs.first() {
        Some(a) => a,
        None => return Err(syn::Error::new_spanned(&input.sig, "a completer takes &CompletionContext")),
    };
    Ok(quote! {
        #[allow(non_camel_case_types)]
        #vis struct #name;

        impl ::pwrs::completer::CompleterFn for #name {
            const TARGET: &'static str = #target;
            fn complete(#arg) -> ::pwrs::PsResult<::std::vec::Vec<::pwrs::Completion>> #block
        }
    })
}

/// `#[transform(cmdlet = "Get-Thing", parameter = "Size")]` turns the
/// annotated function into a unit struct implementing `TransformFn`.
pub fn expand_transform(attr: TokenStream, input: ItemFn) -> Result<TokenStream> {
    let (cmdlet, parameter) = target_keys(attr, "transform")?;
    let target = format!("{cmdlet}/{parameter}");
    let vis = &input.vis;
    let name = &input.sig.ident;
    let block = &input.block;
    let arg = match input.sig.inputs.first() {
        Some(a) => a,
        None => return Err(syn::Error::new_spanned(&input.sig, "a transform takes &PsObject")),
    };
    Ok(quote! {
        #[allow(non_camel_case_types)]
        #vis struct #name;

        impl ::pwrs::transform::TransformFn for #name {
            const TARGET: &'static str = #target;
            fn transform(#arg) -> ::pwrs::PsResult<::pwrs::PsObject> #block
        }
    })
}

/// The `cmdlet` and `parameter` keys both `#[completer]` and
/// `#[transform]` take, named in the errors by the attribute asking.
fn target_keys(attr: TokenStream, attribute: &str) -> Result<(String, String)> {
    let list: MetaList = syn::parse2(attr)?;
    let mut cmdlet = None;
    let mut parameter = None;
    for m in list.0 {
        match m {
            Meta::NameValue(nv) => {
                let key = key_of(&nv.path)?;
                match key.as_str() {
                    "cmdlet" => cmdlet = Some(lit_str(&nv.value)?),
                    "parameter" => parameter = Some(lit_str(&nv.value)?),
                    unknown => return Err(syn::Error::new_spanned(nv.path, format!("unknown #[{attribute}] key `{unknown}`"))),
                }
            }
            other => return Err(syn::Error::new_spanned(other, format!("#[{attribute}] takes cmdlet = \"...\", parameter = \"...\""))),
        }
    }
    let span = proc_macro2::Span::call_site();
    let cmdlet = cmdlet.ok_or_else(|| syn::Error::new(span, format!("#[{attribute}] needs cmdlet = \"Verb-Noun\"")))?;
    let parameter = parameter.ok_or_else(|| syn::Error::new(span, format!("#[{attribute}] needs parameter = \"Name\"")))?;
    Ok((cmdlet, parameter))
}

/// `#[dynamic_params(cmdlet = CmdletType)]` on a free function turns it
/// into a `DynamicParams` impl for that cmdlet type.
pub fn expand_dynamic_params(attr: TokenStream, input: ItemFn) -> Result<TokenStream> {
    let list: MetaList = syn::parse2(attr)?;
    let mut cmdlet_ty = None;
    for m in list.0 {
        match m {
            Meta::NameValue(nv) if nv.path.is_ident("cmdlet") => {
                cmdlet_ty = match &nv.value {
                    Expr::Path(p) => Some(p.path.clone()),
                    other => return Err(syn::Error::new_spanned(other, "cmdlet takes a type path, e.g. cmdlet = GetThing")),
                };
            }
            other => return Err(syn::Error::new_spanned(other, "#[dynamic_params] takes cmdlet = <CmdletType>")),
        }
    }
    let cmdlet_ty = cmdlet_ty.ok_or_else(|| syn::Error::new(proc_macro2::Span::call_site(), "#[dynamic_params] needs cmdlet = <CmdletType>"))?;
    let block = &input.block;
    let arg = match input.sig.inputs.first() {
        Some(a) => a,
        None => return Err(syn::Error::new_spanned(&input.sig, "dynamic parameters take &PsHashtable")),
    };
    Ok(quote! {
        impl ::pwrs::completer::DynamicParams for #cmdlet_ty {
            fn dynamic_parameters(#arg) -> ::pwrs::PsResult<::std::vec::Vec<::pwrs::DynamicParam>> #block
        }
    })
}

/// `#[on_import]` turns a `fn() -> PsResult<()>` into a unit struct
/// implementing `OnImport`.
pub fn expand_on_import(attr: TokenStream, input: ItemFn) -> Result<TokenStream> {
    expand_hook(attr, input, quote!(::pwrs::lifecycle::OnImport), "#[on_import]")
}

/// `#[on_remove]` turns a `fn() -> PsResult<()>` into a unit struct
/// implementing `OnRemove`.
pub fn expand_on_remove(attr: TokenStream, input: ItemFn) -> Result<TokenStream> {
    expand_hook(attr, input, quote!(::pwrs::lifecycle::OnRemove), "#[on_remove]")
}

fn expand_hook(attr: TokenStream, input: ItemFn, trait_path: TokenStream, what: &str) -> Result<TokenStream> {
    if !attr.is_empty() {
        return Err(syn::Error::new_spanned(attr, format!("{what} takes no arguments")));
    }
    if !input.sig.inputs.is_empty() {
        return Err(syn::Error::new_spanned(&input.sig, format!("{what} goes on a fn with no arguments returning PsResult<()>")));
    }
    let vis = &input.vis;
    let name = &input.sig.ident;
    let block = &input.block;
    Ok(quote! {
        #[allow(non_camel_case_types)]
        #vis struct #name;

        impl #trait_path for #name {
            fn run() -> ::pwrs::PsResult<()> #block
        }
    })
}

/// `#[provider(name = "Rust", capabilities = [...])]` emits the
/// `ProviderMeta` descriptor for a `Provider` struct.
///
/// The drives a provider serves come from `Provider::default_drives`
/// and `New-PSDrive`, both of which hand back the instance that serves
/// the drive.
pub fn expand_provider(attr: TokenStream, input: ItemStruct) -> Result<TokenStream> {
    let list: MetaList = syn::parse2(attr)?;
    let mut name = None;
    let mut capabilities = Vec::new();
    for m in list.0 {
        match m {
            Meta::NameValue(nv) => {
                let key = key_of(&nv.path)?;
                match key.as_str() {
                    "name" => name = Some(lit_str(&nv.value)?),
                    "drive" => {
                        return Err(syn::Error::new_spanned(
                            nv.path,
                            "#[provider] has no `drive` key: a drive carries the instance that serves it, so it comes from Provider::default_drives or New-PSDrive",
                        ))
                    }
                    "capabilities" => capabilities = str_list(&nv.value)?,
                    unknown => return Err(syn::Error::new_spanned(nv.path, format!("unknown #[provider] key `{unknown}`"))),
                }
            }
            other => return Err(syn::Error::new_spanned(other, "#[provider] takes name = \"...\", capabilities = [...]")),
        }
    }
    let ty = input.ident.clone();
    let name = name.unwrap_or_else(|| ty.to_string());
    let docs = doc_lines(&input.attrs);
    let mut o = Obj::new();
    o.str("name", &name)
        .str("rust", &ty.to_string())
        .strs("capabilities", &capabilities)
        .str("description", docs.join("\n").trim());
    let descriptor = LitStr::new(&o.finish(), proc_macro2::Span::call_site());
    Ok(quote! {
        #input

        impl ::pwrs::provider::ProviderMeta for #ty {
            const NAME: &'static str = #name;
            const DESCRIPTOR: &'static str = #descriptor;
        }
    })
}
