//! `#[psmethods]` expansion: the public methods of an `impl` block
//! become methods of the class's generated proxy type.
//!
//! Each method's arguments are lowered like cmdlet parameters into a
//! `#[repr(C)]` block the generated shell packs (a `bound` word, then
//! one slot per argument); the return type is `PsResult<T>` for any
//! output type, or `PsResult<()>` for nothing. The impl block itself
//! is emitted unchanged, so the methods stay callable from Rust.

use crate::json::{self, Obj};
use crate::params::{lit_str, mask_words, read_expr};
use crate::types::{check_reserved, doc_lines, lower, output_clr, pascal, path_ident, ClrName, Customs, Lowered, Slot, OBJECT_MEMBERS, PROXY_MEMBERS};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{Attribute, FnArg, GenericArgument, Ident, ImplItem, ImplItemFn, ItemImpl, Meta, Pat, PathArguments, ReceiverKind, Result, ReturnType, Token, Type, Visibility};

/// What `#[member]` and `#[base]` say of one method. Both are taken off
/// the method before the impl block is emitted, since neither is an
/// attribute the compiler defines.
#[derive(Default)]
struct Marks {
    /// The base or interface member the method backs, with the attribute
    /// for errors.
    member: Option<(String, Attribute)>,
    /// A bare `#[base]`: the method returns the base class constructor's
    /// arguments for an object Rust makes outside `new`.
    base_provider: Option<Attribute>,
    /// `#[base(...)]` on `new`: the arguments of `new` that go to the base
    /// class's constructor, in its order.
    base_args: Option<(Vec<Ident>, Attribute)>,
}

impl Marks {
    /// Any of the attributes, for an error about where they stand.
    fn any(&self) -> Option<&Attribute> {
        match (&self.member, &self.base_provider, &self.base_args) {
            (Some((_, attr)), _, _) | (None, Some(attr), _) | (None, None, Some((_, attr))) => Some(attr),
            (None, None, None) => None,
        }
    }
}

/// Takes `#[member]` and `#[base]` off `f`, leaving its other attributes.
fn take_marks(f: &mut ImplItemFn) -> Result<Marks> {
    let mut marks = Marks::default();
    let mut rest = Vec::new();
    for attr in std::mem::take(&mut f.attrs) {
        if attr.path().is_ident("member") {
            let shape = || syn::Error::new_spanned(&attr, "#[member] names the base or interface member the method backs, as #[member = \"Read\"]");
            if marks.member.is_some() {
                return Err(syn::Error::new_spanned(&attr, "a method backs one member, so it takes one #[member]"));
            }
            let name = match &attr.meta {
                Meta::NameValue(nv) => lit_str(&nv.value)?,
                Meta::Path(..) | Meta::List(..) => return Err(shape()),
            };
            if name.is_empty() || name.contains(char::is_whitespace) {
                return Err(shape());
            }
            marks.member = Some((name, attr));
        } else if attr.path().is_ident("base") {
            if marks.base_provider.is_some() || marks.base_args.is_some() {
                return Err(syn::Error::new_spanned(&attr, "a method takes one #[base]"));
            }
            match &attr.meta {
                Meta::Path(..) => marks.base_provider = Some(attr),
                Meta::List(list) => {
                    let names: Punctuated<Ident, Token![,]> = list.parse_args_with(Punctuated::parse_terminated)?;
                    if names.is_empty() {
                        return Err(syn::Error::new_spanned(&attr, "#[base(...)] names the arguments of new that go to the base class's constructor"));
                    }
                    marks.base_args = Some((names.into_iter().collect(), attr));
                }
                Meta::NameValue(..) => {
                    return Err(syn::Error::new_spanned(
                        &attr,
                        "#[base] takes the names of new's arguments, as #[base(message)], or stands alone on the method that returns them for an object Rust makes outside new",
                    ));
                }
            }
        } else {
            rest.push(attr);
        }
    }
    f.attrs = rest;
    if let (Some((_, attr)), true) = (&marks.member, marks.base_provider.is_some() || marks.base_args.is_some()) {
        return Err(syn::Error::new_spanned(attr, "a method backs a member or gives the base class constructor's arguments, not both"));
    }
    Ok(marks)
}

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
    /// The base or interface member the method backs, by the name its
    /// `#[member]` gives.
    member: Option<String>,
    /// The arguments of `new` that its `#[base(...)]` passes to the base
    /// class's constructor, by index, in the constructor's order.
    base_args: Option<Vec<usize>>,
    /// For a `#[base]` method, one lowering for each argument of the base
    /// class's constructor it returns, and whether it returns a tuple.
    base_ret: Option<(Vec<Lowered>, bool)>,
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
/// the shell enters with the receiver and passes by reference.
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

fn method_spec(ty: &Ident, f: &ImplItemFn, marks: &Marks) -> Result<MethodSpec> {
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
    // A #[base] method's return is the base constructor's arguments,
    // lowered one by one below rather than as one output.
    let ret = match return_inner(&f.sig)? {
        Some(_arguments) if marks.base_provider.is_some() => None,
        Some(t) => Some(lower(&resolve_self(&t, ty))?),
        None => None,
    };
    // The shell returns the task it passes to the method, so a method
    // taking one returns nothing of its own.
    let tasks = args.iter().filter(|a| a.lowered.slot == Slot::Task).count();
    if tasks > 1 {
        return Err(syn::Error::new_spanned(&f.sig, "a #[psmethods] method takes at most one PsTask<T>"));
    }
    if tasks == 1 && (ret.is_some() || constructor) {
        return Err(syn::Error::new_spanned(&f.sig, "a #[psmethods] method taking PsTask<T> returns PsResult<()>: the task it finishes is what the script receives"));
    }
    let member = match &marks.member {
        Some((name, attr)) => {
            if !has_receiver {
                return Err(syn::Error::new_spanned(attr, "a method backing a member takes &self or &mut self: the member is called on the object"));
            }
            if tasks == 1 {
                return Err(syn::Error::new_spanned(attr, "a method taking a PsTask cannot back a member, whose own signature says what it returns"));
            }
            Some(name.clone())
        }
        None => None,
    };
    let base_args = match &marks.base_args {
        Some((names, attr)) => {
            if !constructor {
                return Err(syn::Error::new_spanned(attr, "#[base(...)] goes on new, the class's constructor, and names its arguments for the base class's constructor"));
            }
            let mut at: Vec<usize> = Vec::new();
            for name in names {
                let i = match args.iter().position(|a| &a.ident == name) {
                    Some(i) => i,
                    None => return Err(syn::Error::new_spanned(name, format!("new takes no argument named {name}"))),
                };
                if at.contains(&i) {
                    return Err(syn::Error::new_spanned(name, format!("#[base(...)] names {name} twice")));
                }
                at.push(i);
            }
            Some(at)
        }
        None => None,
    };
    let base_ret = match &marks.base_provider {
        Some(attr) => {
            if !has_receiver || mutable || !args.is_empty() {
                return Err(syn::Error::new_spanned(attr, "a #[base] method takes &self and nothing else: it reads the value Rust made for the object being built"));
            }
            let shape = || syn::Error::new_spanned(attr, "a #[base] method returns the base class constructor's arguments: PsResult<T> for one, or PsResult<(A, B, ...)> for several");
            match return_inner(&f.sig)? {
                Some(Type::Tuple(parts)) => Some((parts.elems.iter().map(|e| lower(&resolve_self(e, ty))).collect::<Result<Vec<Lowered>>>()?, true)),
                Some(one) => Some((vec![lower(&resolve_self(&one, ty))?], false)),
                None => return Err(shape()),
            }
        }
        None => None,
    };
    let help = doc_lines(&f.attrs).join(" ").trim().to_string();
    let ps_name = pascal(&f.sig.ident.to_string());
    // A method backing a member is declared by the member's name, and a
    // #[base] method is not declared at all, so neither name can hide an
    // inherited member.
    if member.is_none() && base_ret.is_none() {
        check_reserved(&f.sig.ident, "a method", &ps_name, &[OBJECT_MEMBERS, PROXY_MEMBERS])?;
    }
    Ok(MethodSpec { ps_name, ident: f.sig.ident.clone(), help, args, ret, receiver: has_receiver, mutable, constructor, member, base_args, base_ret })
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
    let base_args = match &m.base_args {
        Some(at) => format!("[{}]", at.iter().map(|i| i.to_string()).collect::<Vec<String>>().join(",")),
        None => "null".to_string(),
    };
    let base_ret = match &m.base_ret {
        Some((parts, _tuple)) => {
            let parts: Vec<String> = parts
                .iter()
                .map(|l| {
                    let mut o = Obj::new();
                    customs.clr_keys(&mut o, &output_clr(l));
                    o.bool("optional", l.optional);
                    o.finish()
                })
                .collect();
            json::array(&parts)
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
        .bool("constructor", m.constructor)
        .opt_str("member", m.member.as_deref())
        .raw("base_args", &base_args)
        .bool("base", m.base_ret.is_some())
        .raw("base_ret", &base_ret);
    o.finish()
}

pub fn expand_psmethods(attr: TokenStream, mut input: ItemImpl) -> Result<TokenStream> {
    if !attr.is_empty() {
        return Err(syn::Error::new_spanned(attr, "#[psmethods] takes no arguments"));
    }
    let ty = self_ident(&input)?;
    let mut marks: Vec<(Ident, Marks)> = Vec::new();
    for item in input.items.iter_mut() {
        if let ImplItem::Fn(f) = item {
            let taken = take_marks(f)?;
            if let Some(attr) = taken.any()
                && !matches!(f.vis, Visibility::Public(..))
            {
                return Err(syn::Error::new_spanned(attr, "#[member] and #[base] go on a pub fn of the block, one PowerShell reaches"));
            }
            marks.push((f.sig.ident.clone(), taken));
        }
    }
    let unmarked = Marks::default();
    let marks_of = |f: &ImplItemFn| match marks.iter().find(|(ident, _)| *ident == f.sig.ident) {
        Some((_, taken)) => taken,
        None => &unmarked,
    };
    let methods = public_fns(&input)?.into_iter().map(|f| method_spec(&ty, f, marks_of(f))).collect::<Result<Vec<MethodSpec>>>()?;
    if let Some(second) = methods.iter().filter(|m| m.base_ret.is_some()).nth(1) {
        return Err(syn::Error::new_spanned(&second.ident, "a class has one #[base] method, which returns the base class constructor's arguments"));
    }
    let mut customs = Customs::default();
    let json: Vec<String> = methods.iter().enumerate().map(|(i, m)| method_json(i, m, &mut customs)).collect();
    let stmts = customs.descriptor_stmts(&json::array(&json), None);

    // One hidden constant per method other than a constructor and a
    // #[base] method, which a class's `view`, roles and operators name to
    // show at compile time that the method is one PowerShell can call.
    let markers = methods.iter().filter(|m| !m.constructor && m.base_ret.is_none()).map(|m| {
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
        let words = mask_words(m.args.len());
        quote! {
            #[repr(C)]
            #[allow(non_camel_case_types, dead_code)]
            struct #block {
                /// Bit `i % 64` of word `i / 64` set when argument `i` was
                /// supplied.
                pub bound: [u64; #words],
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
        // `&self` method is given a shared reference, and the gate lets
        // a property read nest inside it, such as one converting the
        // receiver passed back as an argument; a `&mut self` method is
        // given the only reference, and the gate refuses every other
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
        // A #[base] method's arguments go back as one object[], which the
        // shell reads by position into the base constructor's parameters.
        let result = if m.constructor {
            quote!(::pwrs::class::construct(#call))
        } else if let Some((parts, tuple)) = &m.base_ret {
            if *tuple {
                let names: Vec<Ident> = (0..parts.len()).map(|i| format_ident!("__part{}", i)).collect();
                quote!({
                    let (#(#names,)*) = #call;
                    ::pwrs::IntoPs::into_ps(::pwrs::PsArray(::std::vec![#(::pwrs::IntoPs::into_ps(#names)?),*]))
                })
            } else {
                quote!(::pwrs::IntoPs::into_ps(::pwrs::PsArray(::std::vec![::pwrs::IntoPs::into_ps(#call)?])))
            }
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
    use crate::params::squeezed;

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
            ("impl Row { pub fn valued(&self, task: PsTask<i64>) -> PsResult<i64> { Ok(0) } }", "returns PsResult<()>"),
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
        use super::{method_json, method_spec, public_fns, read_expr, self_ident, Customs, Marks};
        for (source, value) in [
            ("impl Row { pub fn last(&self, task: PsTask<Option<i64>>) -> PsResult<()> { Ok(()) } }", "i64"),
            ("impl Row { pub fn copy(&self, task: PsTask<Option<Self>>) -> PsResult<()> { Ok(()) } }", "Row"),
        ] {
            if let Err(e) = expand(source) {
                panic!("{source} was refused: {e}");
            }
            let input: syn::ItemImpl = syn::parse_str(source).expect("an impl block");
            let ty = self_ident(&input).expect("a plain type name");
            let m = method_spec(&ty, public_fns(&input).expect("its methods")[0], &Marks::default()).expect("a method");
            let read: String = read_expr(0, &m.args[0].lowered, &quote::format_ident!("block")).to_string().split_whitespace().collect();
            assert_eq!(read, format!("<PsTask<Option<{value}>>>::from_slot(block.p0)"), "{source}");
            let json = method_json(0, &m, &mut Customs::default());
            assert!(json.contains(r#""slot":"task","optional":true"#), "{source}: {json}");
        }
    }

    /// A raw identifier names its method or argument without the `r#`, and
    /// a C# keyword stays as written, for the shell to write with an `@`.
    #[test]
    fn a_raw_identifier_gives_its_name_without_the_r_hash() {
        use super::{method_json, method_spec, public_fns, self_ident, Customs, Marks};
        let source = "impl Row { pub fn r#match(&self, r#type: i64, operator: String) -> PsResult<i64> { Ok(0) } }";
        if let Err(e) = expand(source) {
            panic!("{source} was refused: {e}");
        }
        let input: syn::ItemImpl = syn::parse_str(source).expect("an impl block");
        let ty = self_ident(&input).expect("a plain type name");
        let m = method_spec(&ty, public_fns(&input).expect("its methods")[0], &Marks::default()).expect("a method");
        assert_eq!(m.ps_name, "Match");
        let names: Vec<&str> = m.args.iter().map(|a| a.cs_name.as_str()).collect();
        assert_eq!(names, ["type", "operator"]);
        let json = method_json(0, &m, &mut Customs::default());
        assert!(json.contains(r#""name":"type""#) && json.contains(r#""name":"operator""#), "{json}");
    }

    #[test]
    fn an_array_taken_in_place_is_declared_as_an_array_of_its_primitive() {
        use super::{method_json, method_spec, public_fns, self_ident, Customs, Marks};
        for (elem, clr) in [("u8", "byte[]"), ("i64", "long[]"), ("f32", "float[]"), ("PsDecimalBits", "decimal[]")] {
            let source = format!("impl Row {{ pub fn fill(&self, buffer: PsArrayMut<{elem}>, rows: Option<PsArrayMut<{elem}>>) -> PsResult<()> {{ Ok(()) }} }}");
            let input: syn::ItemImpl = syn::parse_str(&source).expect("an impl block");
            let ty = self_ident(&input).expect("a plain type name");
            let m = method_spec(&ty, public_fns(&input).expect("its methods")[0], &Marks::default()).expect("a method");
            let json = method_json(0, &m, &mut Customs::default());
            assert!(json.contains(&format!(r#""clr":"{clr}","value_type":false,"slot":"handle","optional":false"#)), "{source}: {json}");
            assert!(json.contains(&format!(r#""clr":"{clr}","value_type":false,"slot":"handle","optional":true"#)), "{source}: {json}");
        }
        for refused in ["String", "bool", "Vec<u8>", "PsObject"] {
            let source = format!("impl Row {{ pub fn fill(&self, buffer: PsArrayMut<{refused}>) -> PsResult<()> {{ Ok(()) }} }}");
            match expand(&source) {
                Ok(_) => panic!("{source} was accepted"),
                Err(e) => assert!(e.to_string().contains("PsArrayMut takes a primitive element type"), "{source}: {e}"),
            }
        }
    }

    /// Each method's descriptor entry, with its marks read as the
    /// expansion reads them.
    fn method_entries(source: &str) -> Vec<String> {
        use super::{method_json, method_spec, self_ident, take_marks, Customs};
        let mut input: syn::ItemImpl = syn::parse_str(source).expect("an impl block");
        let ty = self_ident(&input).expect("a plain type name");
        let mut entries = Vec::new();
        for (i, item) in input.items.iter_mut().enumerate() {
            if let syn::ImplItem::Fn(f) = item {
                let marks = take_marks(f).expect("the marks");
                let m = method_spec(&ty, f, &marks).expect("a method");
                entries.push(method_json(i, &m, &mut Customs::default()));
            }
        }
        entries
    }

    #[test]
    fn member_and_base_marks_reach_the_descriptor_and_leave_the_emitted_block() {
        let source = r#"impl Echo {
            #[base(message)]
            pub fn new(message: String, code: i64) -> PsResult<Self> { Ok(Echo) }
            #[member = "Read"]
            pub fn read(&mut self, buffer: PsArrayMut<u8>, offset: i32, count: i32) -> PsResult<i32> { Ok(0) }
            #[base]
            pub fn base_args(&self) -> PsResult<(String, i64)> { Ok((String::new(), 0)) }
            #[member = "ToString"]
            pub fn to_string(&self) -> PsResult<String> { Ok(String::new()) }
        }"#;
        let emitted: String = match expand(source) {
            Ok(tokens) => tokens.to_string().split_whitespace().collect(),
            Err(e) => panic!("the block was refused: {e}"),
        };
        assert!(!emitted.contains("#[member") && !emitted.contains("#[base"), "a mark was emitted: {emitted}");
        assert!(!emitted.contains("__pwrs_psmethod_base_args"), "the #[base] method has a marker");
        let entries = method_entries(source);
        assert!(entries[0].contains(r#""constructor":true"#) && entries[0].contains(r#""base_args":[0]"#), "{}", entries[0]);
        assert!(entries[1].contains(r#""member":"Read""#) && entries[1].contains(r#""base":false,"base_ret":null"#), "{}", entries[1]);
        assert!(
            entries[2].contains(r#""ret":null"#)
                && entries[2].contains(r#""base":true,"base_ret":[{"clr":"string","value_type":false,"optional":false},{"clr":"long","value_type":false,"optional":false}]"#),
            "{}",
            entries[2]
        );
        assert!(entries[3].contains(r#""member":"ToString""#), "{}", entries[3]);
        if let Err(e) = expand("impl Echo { #[base] pub fn base_args(&self) -> PsResult<(String,)> { Ok((String::new(),)) } }") {
            panic!("a one-element tuple was refused: {e}");
        }
    }

    #[test]
    fn member_and_base_marks_stand_where_they_can_be_honored() {
        let refused = [
            (r#"impl E { #[member = "Read"] pub fn read(count: i32) -> PsResult<i32> { Ok(0) } }"#, "the member is called on the object"),
            ("impl E { #[member] pub fn read(&self) -> PsResult<i32> { Ok(0) } }", "#[member] names the base or interface member"),
            (r#"impl E { #[member = "A"] #[member = "B"] pub fn read(&self) -> PsResult<i32> { Ok(0) } }"#, "takes one #[member]"),
            (r#"impl E { #[member = "Read"] #[base] pub fn read(&self) -> PsResult<i32> { Ok(0) } }"#, "not both"),
            (r#"impl E { #[member = "Read"] fn read(&self) -> PsResult<i32> { Ok(0) } }"#, "go on a pub fn"),
            (r#"impl E { #[member = "Wait"] pub fn wait(&self, task: PsTask<i64>) -> PsResult<()> { Ok(()) } }"#, "taking a PsTask cannot back a member"),
            ("impl E { #[base(message)] pub fn make(message: String) -> PsResult<i32> { Ok(0) } }", "goes on new"),
            ("impl E { #[base(missing)] pub fn new(message: String) -> PsResult<Self> { Ok(E) } }", "new takes no argument named missing"),
            ("impl E { #[base(message, message)] pub fn new(message: String) -> PsResult<Self> { Ok(E) } }", "names message twice"),
            (r#"impl E { #[base = "message"] pub fn new(message: String) -> PsResult<Self> { Ok(E) } }"#, "#[base] takes the names of new's arguments"),
            ("impl E { #[base] pub fn args(&mut self) -> PsResult<String> { Ok(String::new()) } }", "takes &self and nothing else"),
            ("impl E { #[base] pub fn args(&self, extra: i32) -> PsResult<String> { Ok(String::new()) } }", "takes &self and nothing else"),
            ("impl E { #[base] pub fn args(&self) -> PsResult<()> { Ok(()) } }", "returns the base class constructor's arguments"),
            (
                "impl E { #[base] pub fn one(&self) -> PsResult<String> { Ok(String::new()) } #[base] pub fn two(&self) -> PsResult<String> { Ok(String::new()) } }",
                "a class has one #[base] method",
            ),
        ];
        for (source, message) in refused {
            match expand(source) {
                Ok(_) => panic!("{source} was accepted"),
                Err(e) => assert!(e.to_string().contains(message), "{source}: {e}"),
            }
        }
    }

    /// `#[psmethods]` on one method of `n` optional arguments, `a0` to
    /// `a{n-1}`, printed without whitespace.
    fn method_of(n: usize) -> String {
        let args: Vec<String> = (0..n).map(|i| format!(", a{i}: Option<i64>")).collect();
        let source = format!("impl Row {{ pub fn wide(&self{}) -> PsResult<i64> {{ Ok(0) }} }}", args.concat());
        match expand(&source) {
            Ok(tokens) => squeezed(&tokens.to_string()),
            Err(e) => panic!("{n} arguments were refused: {e}"),
        }
    }

    #[test]
    fn the_bound_mask_of_a_method_takes_one_word_per_64_arguments() {
        for (n, words) in [(0, 1), (1, 1), (64, 1), (65, 2), (128, 2), (129, 3)] {
            let block = format!("pub bound: [u64; {words}usize],");
            assert!(method_of(n).contains(&squeezed(&block)), "{n} arguments: no `{block}`");
        }
    }

    #[test]
    fn an_argument_past_the_first_64_is_read_from_its_own_word() {
        let out = method_of(130);
        for (i, word, bit) in [(0, 0, 0), (63, 0, 63), (64, 1, 0), (65, 1, 1), (127, 1, 63), (128, 2, 0), (129, 2, 1)] {
            let read = format!(
                "let a{i} = if block.bound[{word}usize] & (1u64 << {bit}u64) != 0 {{ ::core::option::Option::Some(block.p{i}) }} else {{ ::core::option::Option::None }};"
            );
            assert!(out.contains(&squeezed(&read)), "argument {i}: no `{read}` in {out}");
        }
    }
}
