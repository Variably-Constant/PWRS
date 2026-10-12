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
use crate::types::{check_reserved, doc_lines, lower, output_clr, pascal, path_ident, ClrName, Customs, Lowered, Slot, OBJECT_MEMBERS, PROXY_MEMBERS};
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

/// Whether `t` is `&Self` or `&Ty`: another object of the class, which
/// the shell enters with the receiver and hands over by reference.
fn is_peer(t: &Type, ty: &Ident) -> Result<bool> {
    let Type::Reference(r) = t else { return Ok(false) };
    let names_the_class = matches!(&*r.elem, Type::Path(p) if p.qself.is_none() && (p.path.is_ident("Self") || p.path.is_ident(ty)));
    if !names_the_class {
        return Err(syn::Error::new_spanned(t, "a #[psmethods] argument by reference is another object of the class, &Self; any other type is taken by value"));
    }
    if r.mutability.is_some() {
        return Err(syn::Error::new_spanned(t, "another object of the class is lent shared, as &Self; &mut Self is not supported"));
    }
    Ok(true)
}

/// The lowering of a `&Self` argument: declared as the class's own
/// CLR type, read from a pointer the shell fills.
fn peer_lowered(ty: &Ident) -> Lowered {
    let class: Type = syn::parse_quote!(#ty);
    Lowered { clr: ClrName::Custom { ty: Box::new(class.clone()), suffix: String::new() }, slot: Slot::Peer, optional: false, inner: class, path: false }
}

/// `T` of a `PsTask<T>` argument type.
fn task_of(t: &Type) -> Option<&Type> {
    match path_ident(t)? {
        (ident, value) if ident == "PsTask" => value,
        _other => None,
    }
}

/// The lowering of a `PsTask<T>` argument: declared as `T`'s CLR type,
/// `void` for `()`, from which the shell makes the `Task` it returns.
fn task_lowered(task: &Type, value: &Type) -> Result<Lowered> {
    let (clr, optional) = match value {
        Type::Tuple(unit) if unit.elems.is_empty() => (ClrName::Literal("void".to_string()), false),
        other => {
            let l = lower(other)?;
            (output_clr(&l), l.optional)
        }
    };
    Ok(Lowered { clr, slot: Slot::Task, optional, inner: task.clone(), path: false })
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
                let resolved = resolve_self(&t.ty, ty);
                let lowered = if is_peer(&t.ty, ty)? {
                    peer_lowered(ty)
                } else if let Some(value) = task_of(&resolved) {
                    task_lowered(&resolved, value)?
                } else {
                    lower(&resolved)?
                };
                args.push(Arg { cs_name: lower_camel(&ident.to_string()), ident, lowered });
            }
        }
    }
    // The shell enters a peer's gate beside the receiver's, in one order
    // for every pair of objects, so a method takes one peer at most and
    // only with a receiver to enter beside it.
    let peers = args.iter().filter(|a| a.lowered.slot == Slot::Peer).count();
    if peers > 1 {
        return Err(syn::Error::new_spanned(&f.sig, "a #[psmethods] method takes at most one &Self argument"));
    }
    if peers == 1 && !has_receiver {
        return Err(syn::Error::new_spanned(&f.sig, "a #[psmethods] method taking &Self takes &self or &mut self too"));
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
    // The shell returns the task it hands the method, so a method taking
    // one returns nothing of its own.
    let tasks = args.iter().filter(|a| a.lowered.slot == Slot::Task).count();
    if tasks > 1 {
        return Err(syn::Error::new_spanned(&f.sig, "a #[psmethods] method takes at most one PsTask<T>"));
    }
    if tasks == 1 && (ret.is_some() || constructor) {
        return Err(syn::Error::new_spanned(&f.sig, "a #[psmethods] method taking PsTask<T> returns PsResult<()>: the task it settles is what the script receives"));
    }
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

    // One hidden constant per instance method, which a class's `view`
    // names to show at compile time that the method is one PowerShell
    // can call.
    let markers = methods.iter().filter(|m| m.receiver).map(|m| {
        let marker = format_ident!("__pwrs_psmethod_{}", m.ident);
        quote! {
            #[doc(hidden)]
            #[allow(non_upper_case_globals)]
            pub const #marker: () = ();
        }
    });

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

        impl #ty {
            #(#markers)*
        }

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

#[cfg(test)]
mod tests {
    use super::expand_psmethods;

    fn expand(source: &str) -> syn::Result<proc_macro2::TokenStream> {
        expand_psmethods(proc_macro2::TokenStream::new(), syn::parse_str(source).expect("an impl block"))
    }

    #[test]
    fn a_method_takes_one_peer_and_one_task_and_a_peer_only_beside_a_receiver() {
        for accepted in [
            "impl Row { pub fn order(&self, other: &Self) -> PsResult<i32> { Ok(0) } }",
            "impl Row { pub fn order(&mut self, other: &Row) -> PsResult<i32> { Ok(0) } }",
            "impl Row { pub fn wait(&self, task: PsTask<i64>) -> PsResult<()> { Ok(()) } }",
            "impl Row { pub fn sum(a: i64, task: PsTask<()>) -> PsResult<()> { Ok(()) } }",
        ] {
            if let Err(e) = expand(accepted) {
                panic!("{accepted} was refused: {e}");
            }
        }
        let refused = [
            ("impl Row { pub fn two(&self, a: &Self, b: &Self) -> PsResult<i32> { Ok(0) } }", "at most one &Self"),
            ("impl Row { pub fn lone(other: &Self) -> PsResult<i32> { Ok(0) } }", "taking &Self takes &self or &mut self too"),
            ("impl Row { pub fn change(&self, other: &mut Self) -> PsResult<()> { Ok(()) } }", "&mut Self is not supported"),
            ("impl Row { pub fn text(&self, text: &str) -> PsResult<()> { Ok(()) } }", "any other type is taken by value"),
            ("impl Row { pub fn both(&self, a: PsTask<i64>, b: PsTask<i64>) -> PsResult<()> { Ok(()) } }", "at most one PsTask"),
            ("impl Row { pub fn answers(&self, task: PsTask<i64>) -> PsResult<i64> { Ok(0) } }", "returns PsResult<()>"),
        ];
        for (source, message) in refused {
            match expand(source) {
                Ok(_) => panic!("{source} was accepted"),
                Err(e) => assert!(e.to_string().contains(message), "{source}: {e}"),
            }
        }
    }

    #[test]
    fn a_task_of_an_option_is_read_as_the_task_and_declares_a_nullable_result() {
        use super::{method_json, method_spec, public_fns, read_expr, self_ident, Customs};
        for (source, value) in [
            ("impl Row { pub fn last(&self, task: PsTask<Option<i64>>) -> PsResult<()> { Ok(()) } }", "i64"),
            ("impl Row { pub fn copy(&self, task: PsTask<Option<Self>>) -> PsResult<()> { Ok(()) } }", "Row"),
        ] {
            if let Err(e) = expand(source) {
                panic!("{source} was refused: {e}");
            }
            let input: syn::ItemImpl = syn::parse_str(source).expect("an impl block");
            let ty = self_ident(&input).expect("a plain type name");
            let m = method_spec(&ty, public_fns(&input).expect("its methods")[0]).expect("a method");
            let read: String = read_expr(0, &m.args[0].lowered, &quote::format_ident!("block")).to_string().split_whitespace().collect();
            assert_eq!(read, format!("<PsTask<Option<{value}>>>::from_slot(block.p0)"), "{source}");
            let json = method_json(0, &m, &mut Customs::default());
            assert!(json.contains(r#""slot":"task","optional":true"#), "{source}: {json}");
        }
    }
}
