//! `#[psmethods]` expansion: the public methods of an `impl` block
//! become methods of the class's generated proxy type.
//!
//! Each method's arguments are lowered like cmdlet parameters into a
//! `#[repr(C)]` block the generated shell packs (a `bound` word, then
//! one slot per argument); the return type is `PsResult<T>` for any
//! output type, or `PsResult<()>` for nothing. The impl block itself
//! is emitted unchanged, so the methods stay callable from Rust.

use crate::json::{self, Obj};
use crate::params::read_expr;
use crate::types::{check_reserved, doc_lines, lower, output_clr, pascal, Customs, Lowered, OBJECT_MEMBERS, PROXY_MEMBERS};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, GenericArgument, Ident, ImplItem, ImplItemFn, ItemImpl, Pat, PathArguments, ReceiverKind, Result, ReturnType, Type, Visibility};

struct Arg {
    ident: Ident,
    /// The C# parameter name: the Rust name in lower camel case.
    cs_name: String,
    lowered: Lowered,
}

struct MethodSpec {
    ident: Ident,
    ps_name: String,
    help: String,
    args: Vec<Arg>,
    /// `None` for `PsResult<()>`.
    ret: Option<Lowered>,
    /// Takes `&self` or `&mut self`; a method without one is static.
    receiver: bool,
    /// Takes `&mut self`, so the call enters the object's gate
    /// exclusively; a `&self` method enters it as shared.
    mutable: bool,
    /// The static named `new` returning `Self`: the class's constructor.
    constructor: bool,
}

fn self_ident(input: &ItemImpl) -> Result<Ident> {
    match &*input.self_ty {
        Type::Path(p) => match p.path.get_ident() {
            Some(i) => Ok(i.clone()),
            None => Err(syn::Error::new_spanned(&input.self_ty, "#[psmethods] needs a plain type name: `impl Type { ... }` for a proxy #[psclass] type")),
        },
        other => Err(syn::Error::new_spanned(other, "#[psmethods] goes on `impl Type { ... }` for a proxy #[psclass] type")),
    }
}

fn lower_camel(snake: &str) -> String {
    let p = pascal(snake);
    let mut chars = p.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => p,
    }
}

/// `T` of a `PsResult<T>` return type; `None` for `PsResult<()>`.
fn return_inner(sig: &syn::Signature) -> Result<Option<Type>> {
    let shape = || syn::Error::new_spanned(&sig.ident, "a #[psmethods] method returns PsResult<T>, or PsResult<()> for nothing");
    let ty = match &sig.output {
        ReturnType::Default => return Err(shape()),
        ReturnType::Type(_arrow, ty) => ty,
    };
    let tp = match &**ty {
        Type::Path(tp) => tp,
        other => return Err(syn::Error::new_spanned(other, "a #[psmethods] method returns PsResult<T>, or PsResult<()> for nothing")),
    };
    let seg = tp.path.segments.last().ok_or_else(shape)?;
    if seg.ident != "PsResult" {
        return Err(shape());
    }
    let generics = match &seg.arguments {
        PathArguments::AngleBracketed(a) => a,
        PathArguments::None | PathArguments::Parenthesized(..) => return Err(shape()),
    };
    let mut inner = None;
    for g in &generics.args {
        match g {
            GenericArgument::Type(t) => inner = Some(t),
            other => return Err(syn::Error::new_spanned(other, "PsResult takes one type argument")),
        }
    }
    match inner {
        Some(Type::Tuple(t)) if t.elems.is_empty() => Ok(None),
        Some(other) => Ok(Some(other.clone())),
        None => Err(shape()),
    }
}

/// Whether the signature returns `PsResult<Self>` or `PsResult<Ty>`.
fn returns_self(sig: &syn::Signature, ty: &Ident) -> Result<bool> {
    Ok(match return_inner(sig)? {
        Some(Type::Path(p)) => p.path.is_ident("Self") || p.path.is_ident(ty),
        _ => false,
    })
}

/// The `pub fn` items of the block. Private functions, constants,
/// associated types and macro invocations are not methods; anything
/// else is an error.
fn public_fns(input: &ItemImpl) -> Result<Vec<&ImplItemFn>> {
    let mut fns = Vec::new();
    for item in &input.items {
        if let ImplItem::Fn(f) = item {
            if matches!(f.vis, Visibility::Public(..)) {
                fns.push(f);
            }
        } else if !matches!(item, ImplItem::Const(..) | ImplItem::Type(..) | ImplItem::Macro(..)) {
            return Err(syn::Error::new_spanned(item, "#[psmethods] understands fn, const, type and macro items only"));
        }
    }
    Ok(fns)
}

/// `t` with every `Self` in it named as the impl's own type. The
/// lowered types are spelled inside the generated `impl ... for
/// MethodsCollector<T>`, where `Self` names the collector and not the
/// class, so `PsResult<Self>` and `other: Self` are resolved here.
fn resolve_self(t: &Type, ty: &Ident) -> Type {
    let mut t = t.clone();
    replace_self(&mut t, ty);
    t
}

fn replace_self(t: &mut Type, ty: &Ident) {
    match t {
        Type::Path(tp) if tp.qself.is_none() && tp.path.is_ident("Self") => *t = syn::parse_quote!(#ty),
        Type::Path(tp) => {
            for seg in tp.path.segments.iter_mut() {
                if let PathArguments::AngleBracketed(a) = &mut seg.arguments {
                    for g in a.args.iter_mut() {
                        if let GenericArgument::Type(inner) = g {
                            replace_self(inner, ty);
                        }
                    }
                }
            }
        }
        Type::Reference(r) => replace_self(&mut r.elem, ty),
        Type::Slice(s) => replace_self(&mut s.elem, ty),
        Type::Array(a) => replace_self(&mut a.elem, ty),
        Type::Paren(p) => replace_self(&mut p.elem, ty),
        Type::Group(g) => replace_self(&mut g.elem, ty),
        Type::Tuple(tuple) => {
            for e in tuple.elems.iter_mut() {
                replace_self(e, ty);
            }
        }
        _holds_no_type => {}
    }
}

fn method_spec(ty: &Ident, f: &ImplItemFn) -> Result<MethodSpec> {
    if f.sig.asyncness.is_some() || !f.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&f.sig, "a #[psmethods] method is neither async nor generic"));
    }
    let mut args = Vec::new();
    let mut has_receiver = false;
    let mut mutable = false;
    for a in &f.sig.inputs {
        match a {
            FnArg::Receiver(r) => match &r.kind {
                ReceiverKind::Reference(_, _, m) => {
                    has_receiver = true;
                    mutable = m.is_some();
                }
                _value_or_typed => return Err(syn::Error::new_spanned(r, "a #[psmethods] method takes &self or &mut self")),
            },
            FnArg::Typed(t) => {
                let ident = match &*t.pat {
                    Pat::Ident(p) => p.ident.clone(),
                    other => return Err(syn::Error::new_spanned(other, "#[psmethods] arguments need plain names")),
                };
                let lowered = lower(&resolve_self(&t.ty, ty))?;
                args.push(Arg { cs_name: lower_camel(&ident.to_string()), ident, lowered });
            }
        }
    }
    // No receiver makes a static; the static named `new` is the
    // constructor and has to hand back a value of the class.
    let constructor = !has_receiver && f.sig.ident == "new";
    if constructor && !returns_self(&f.sig, ty)? {
        return Err(syn::Error::new_spanned(&f.sig, "a #[psmethods] `new` is the class's constructor and returns PsResult<Self>"));
    }
    if args.len() > 64 {
        return Err(syn::Error::new_spanned(&f.sig, "a #[psmethods] method takes at most 64 arguments"));
    }
    let ret = match return_inner(&f.sig)? {
        Some(t) => Some(lower(&resolve_self(&t, ty))?),
        None => None,
    };
    let help = doc_lines(&f.attrs).join(" ").trim().to_string();
    let ps_name = pascal(&f.sig.ident.to_string());
    check_reserved(&f.sig.ident, "a method", &ps_name, &[OBJECT_MEMBERS, PROXY_MEMBERS])?;
    Ok(MethodSpec { ps_name, ident: f.sig.ident.clone(), help, args, ret, receiver: has_receiver, mutable, constructor })
}

fn method_json(i: usize, m: &MethodSpec, customs: &mut Customs) -> String {
    let params: Vec<String> = m
        .args
        .iter()
        .enumerate()
        .map(|(j, a)| {
            let mut o = Obj::new();
            o.str("name", &a.cs_name).str("rust", &a.ident.to_string()).num("index", j as i64);
            customs.clr_keys(&mut o, &output_clr(&a.lowered));
            o.str("slot", a.lowered.slot.name()).bool("optional", a.lowered.optional);
            o.finish()
        })
        .collect();
    let ret = match &m.ret {
        Some(l) => {
            let mut o = Obj::new();
            customs.clr_keys(&mut o, &output_clr(l));
            o.bool("optional", l.optional);
            o.finish()
        }
        None => "null".to_string(),
    };
    let mut o = Obj::new();
    o.str("name", &m.ps_name)
        .str("rust", &m.ident.to_string())
        .num("index", i as i64)
        .str("help", &m.help)
        .raw("params", &json::array(&params))
        .raw("ret", &ret)
        .bool("static", !m.receiver)
        .bool("mutable", m.mutable)
        .bool("constructor", m.constructor);
    o.finish()
}

pub fn expand_psmethods(attr: TokenStream, input: ItemImpl) -> Result<TokenStream> {
    if !attr.is_empty() {
        return Err(syn::Error::new_spanned(attr, "#[psmethods] takes no arguments"));
    }
    let ty = self_ident(&input)?;
    let methods = public_fns(&input)?.into_iter().map(|f| method_spec(&ty, f)).collect::<Result<Vec<MethodSpec>>>()?;
    let mut customs = Customs::default();
    let json: Vec<String> = methods.iter().enumerate().map(|(i, m)| method_json(i, m, &mut customs)).collect();
    let stmts = customs.descriptor_stmts(&json::array(&json), None);

    let block_ident = |m: &MethodSpec| format_ident!("__PwrsArgs{}{}", ty, m.ps_name);
    let blocks = methods.iter().map(|m| {
        let block = block_ident(m);
        let fields = m.args.iter().enumerate().map(|(j, a)| {
            let slot = format_ident!("p{}", j);
            let t = a.lowered.slot.rust_ty();
            quote!(pub #slot: #t)
        });
        quote! {
            #[repr(C)]
            #[allow(non_camel_case_types, dead_code)]
            struct #block {
                /// Bit `i` set when argument `i` was supplied.
                pub bound: u64,
                #(#fields,)*
            }
        }
    });

    let block_var = format_ident!("block");
    let arms = methods.iter().enumerate().map(|(i, m)| {
        let id = i as u32;
        let block = block_ident(m);
        let bind = if m.args.is_empty() { quote!() } else { quote!(let #block_var = &*(args as *const #block);) };
        let reads = m.args.iter().enumerate().map(|(j, a)| {
            let name = &a.ident;
            let expr = read_expr(j, &a.lowered, &block_var);
            quote!(let #name = #expr;)
        });
        let method = &m.ident;
        let names = m.args.iter().map(|a| &a.ident);
        // Only a method with a receiver has a value to run against; a
        // static is called on the type and never reads `instance`. A
        // copied class has no value behind its object at all, so a
        // receiver reached with none is refused rather than read. A
        // `&self` method is handed a shared reference, and the gate lets
        // a property read nest inside it, such as one converting the
        // receiver passed back as an argument; a `&mut self` method is
        // handed the only reference, and the gate refuses every other
        // entry while it runs.
        let ps_name = &m.ps_name;
        let borrow = if m.mutable { quote!(&mut *(instance as *mut #ty)) } else { quote!(&*(instance as *const #ty)) };
        let this = if m.receiver {
            quote! {
                if instance.is_null() {
                    return Err(::pwrs::PsError::new(
                        ::pwrs::ErrorCategory::InvalidOperation,
                        "PwrsNoInstance",
                        format!("{} has no Rust value to run {} against", <#ty as ::pwrs::class::PsTyped>::CLR_NAME, #ps_name),
                    ));
                }
                let this = #borrow;
            }
        } else {
            quote!()
        };
        let call = if m.receiver { quote!(this.#method(#(#names),*)?) } else { quote!(#ty::#method(#(#names),*)?) };
        let result = if m.constructor {
            quote!(::pwrs::class::construct(#call))
        } else if m.ret.is_some() {
            quote!(::pwrs::IntoPs::into_ps(#call))
        } else {
            quote!({
                #call;
                Ok(::pwrs::PsObject::null())
            })
        };
        quote! {
            #id => {
                #this
                #bind
                #(#reads)*
                #result
            }
        }
    });

    Ok(quote! {
        #input

        #(#blocks)*

        impl ::pwrs::class::PsMethods<#ty> for ::pwrs::class::MethodsCollector<#ty> {
            fn methods_descriptor(&self) -> ::std::string::String {
                let mut s = ::std::string::String::new();
                #stmts
                s
            }

            #[allow(unused_variables)]
            unsafe fn proxy_call(&self, instance: *mut ::core::ffi::c_void, method_id: u32, args: *const ::core::ffi::c_void) -> ::pwrs::PsResult<::pwrs::PsObject> {
                match method_id {
                    #(#arms)*
                    other => Err(::pwrs::PsError::new(
                        ::pwrs::ErrorCategory::InvalidArgument,
                        "PwrsProxyMethod",
                        format!("{} has no method {}", stringify!(#ty), other),
                    )),
                }
            }
        }
    })
}
