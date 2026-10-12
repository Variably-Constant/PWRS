//! `#[psclass]` expansion: three output modes over one field list.
//!
//! * `copied`: a generated CLR class; `IntoPs` packs a field block and
//!   calls the module's factory once per object.
//! * `proxy`: the Rust value is boxed and the generated CLR class reads
//!   fields through `pwrs_proxy_get` until it is disposed.
//! * `psobject`: no CLR type; `IntoPs` builds a `PSObject` with note
//!   properties and a `PSTypeName`.

use crate::json::{self, Obj};
use crate::params::{key_of, lit_str, str_list, MetaList};
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
    /// The `#[psmethods]` method of a proxy class whose text is the
    /// class's default view.
    view: Option<Ident>,
    /// The text a copied class's `ToString()` shows: a format over its
    /// own property names, with the expression it came from for errors.
    show: Option<(String, syn::Expr)>,
    /// The property names a table of the class shows, in order, with the
    /// expression they came from for errors.
    columns: Option<(Vec<String>, syn::Expr)>,
    /// The `#[psmethods]` methods behind the .NET interfaces a proxy class
    /// implements.
    roles: Roles,
}

/// The methods behind a proxy class's interfaces, each by its Rust name.
#[derive(Default)]
struct Roles {
    /// `fn(&self) -> PsResult<i32>`: the list's `Count`.
    count: Option<Ident>,
    /// `fn(&self, i32) -> PsResult<T>`: the list's indexer.
    item: Option<Ident>,
    /// `fn(&mut self, i32, T) -> PsResult<()>`: the indexer's setter.
    set_item: Option<Ident>,
    /// `fn(&mut self) -> PsResult<Option<T>>`: the next element of a
    /// stream, `None` at its end.
    next: Option<Ident>,
    /// `fn(&self, &Self) -> PsResult<i32>`: the order of two objects.
    compare: Option<Ident>,
    /// `fn(&self, &Self) -> PsResult<bool>`: whether two objects are equal.
    equals: Option<Ident>,
    /// `fn(&self) -> PsResult<i64>`: a hash that equal objects share.
    hash: Option<Ident>,
}

impl Roles {
    /// Each role named, with its key.
    fn named(&self) -> Vec<(&'static str, &Ident)> {
        let all = [
            ("count", &self.count),
            ("item", &self.item),
            ("set_item", &self.set_item),
            ("next", &self.next),
            ("compare", &self.compare),
            ("equals", &self.equals),
            ("hash", &self.hash),
        ];
        all.into_iter().filter_map(|(key, method)| method.as_ref().map(|m| (key, m))).collect()
    }

    /// Refuses roles named on a class that is not a proxy, and the
    /// combinations that give no complete interface.
    fn check(&self, mode: Mode) -> Result<()> {
        if let Some((key, method)) = self.named().first()
            && mode != Mode::Proxy
        {
            return Err(syn::Error::new_spanned(method, format!("{key} applies to a proxy class; its interface calls methods on the object, and a copied or psobject class keeps no Rust value to run one against")));
        }
        match (&self.count, &self.item) {
            (Some(count), None) => return Err(syn::Error::new_spanned(count, "count makes a list with item: name the method that reads one element, item = method")),
            (None, Some(item)) => return Err(syn::Error::new_spanned(item, "item makes a list with count: name the method that answers the length, count = method")),
            _list_or_none => {}
        }
        if let Some(set) = &self.set_item
            && self.item.is_none()
        {
            return Err(syn::Error::new_spanned(set, "set_item writes an element of the list that count and item make; name those too"));
        }
        if let (Some(next), Some(_)) = (&self.next, &self.item) {
            return Err(syn::Error::new_spanned(next, "next makes a stream, which a list already is through count and item; name one or the other"));
        }
        match (&self.equals, &self.hash) {
            (Some(equals), None) => Err(syn::Error::new_spanned(equals, "equals goes with hash: equal objects must hash alike, so name a method for GetHashCode too, hash = method")),
            (None, Some(hash)) => Err(syn::Error::new_spanned(hash, "hash goes with equals: name the method that decides equality too, equals = method")),
            _both_or_neither => Ok(()),
        }
    }

    /// A constant block that fails to compile unless each named method is
    /// a `#[psmethods]` method of the shape its role calls for.
    fn signature_checks(&self, class: &Ident) -> TokenStream {
        let exported = self.named().into_iter().map(|(_key, method)| {
            let marker = format_ident!("__pwrs_psmethod_{}", method, span = method.span());
            quote!(let _: () = #class::#marker;)
        });
        let mut shapes = Vec::new();
        if let Some(m) = &self.count {
            shapes.push(quote!(let _count: fn(&#class) -> ::pwrs::PsResult<i32> = #class::#m;));
        }
        match (&self.item, &self.set_item) {
            (Some(get), Some(set)) => shapes.push(quote!(::pwrs::class::assert_list_shape::<#class, _>(#class::#get, #class::#set);)),
            (Some(get), None) => shapes.push(quote!(let _item: fn(&#class, i32) -> ::pwrs::PsResult<_> = #class::#get;)),
            _no_list => {}
        }
        if let Some(m) = &self.next {
            shapes.push(quote!(let _next: fn(&mut #class) -> ::pwrs::PsResult<::core::option::Option<_>> = #class::#m;));
        }
        if let Some(m) = &self.compare {
            shapes.push(quote!(let _compare: fn(&#class, &#class) -> ::pwrs::PsResult<i32> = #class::#m;));
        }
        if let Some(m) = &self.equals {
            shapes.push(quote!(let _equals: fn(&#class, &#class) -> ::pwrs::PsResult<bool> = #class::#m;));
        }
        if let Some(m) = &self.hash {
            shapes.push(quote!(let _hash: fn(&#class) -> ::pwrs::PsResult<i64> = #class::#m;));
        }
        if shapes.is_empty() {
            return quote!();
        }
        quote! {
            const _: () = {
                #(#exported)*
                #(#shapes)*
            };
        }
    }
}

/// Parses `key = method`, the bare name of a `#[psmethods]` method.
fn role_method(key: &str, value: &syn::Expr) -> Result<Ident> {
    if let syn::Expr::Path(p) = value
        && let Some(ident) = p.path.get_ident()
    {
        return Ok(ident.clone());
    }
    Err(syn::Error::new_spanned(value, format!("{key} takes the name of a #[psmethods] method of the class")))
}

/// Refuses a `columns` list that is empty, names a property twice, or
/// names one the class does not have.
fn check_columns(columns: &[String], properties: &[&str]) -> std::result::Result<(), String> {
    if columns.is_empty() {
        return Err("columns names no property; give it the properties the table shows, as [\"Name\", ...]".to_string());
    }
    for (i, column) in columns.iter().enumerate() {
        if !properties.contains(&column.as_str()) {
            return Err(format!("columns names {column}, which is not a property of the class; its properties are {}", properties.join(", ")));
        }
        if columns[..i].contains(column) {
            return Err(format!("columns names {column} twice"));
        }
    }
    Ok(())
}

/// The pieces of a `show` format: `Ok(text)` for literal text, `Err(name)`
/// for a `{Name}` placeholder, with `{{` and `}}` standing for braces.
/// A brace that neither opens a placeholder nor doubles is refused.
fn show_pieces(format: &str) -> std::result::Result<Vec<std::result::Result<String, String>>, String> {
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    name.push(c);
                }
                if !closed || name.is_empty() {
                    return Err(format!("show has an unclosed or empty placeholder in \"{format}\"; write {{Name}} for a property, {{{{ and }}}} for braces"));
                }
                if !text.is_empty() {
                    pieces.push(Ok(std::mem::take(&mut text)));
                }
                pieces.push(Err(name));
            }
            '}' => return Err(format!("show has a lone }} in \"{format}\"; write }}}} for a brace")),
            other => text.push(other),
        }
    }
    if !text.is_empty() {
        pieces.push(Ok(text));
    }
    Ok(pieces)
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
    let mut view = None;
    let mut show = None;
    let mut columns = None;
    let mut roles = Roles::default();
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
                    "view" => match &nv.value {
                        syn::Expr::Path(p) if p.path.get_ident().is_some() => view = p.path.get_ident().cloned(),
                        other => return Err(syn::Error::new_spanned(other, "view takes the name of a #[psmethods] method of the class, fn(&self) -> PsResult<String>")),
                    },
                    "show" => show = Some((lit_str(&nv.value)?, nv.value.clone())),
                    "columns" => columns = Some((str_list(&nv.value)?, nv.value.clone())),
                    "count" => roles.count = Some(role_method("count", &nv.value)?),
                    "item" => roles.item = Some(role_method("item", &nv.value)?),
                    "set_item" => roles.set_item = Some(role_method("set_item", &nv.value)?),
                    "next" => roles.next = Some(role_method("next", &nv.value)?),
                    "compare" => roles.compare = Some(role_method("compare", &nv.value)?),
                    "equals" => roles.equals = Some(role_method("equals", &nv.value)?),
                    "hash" => roles.hash = Some(role_method("hash", &nv.value)?),
                    unknown => return Err(syn::Error::new_spanned(nv.path, format!("unknown #[psclass] key `{unknown}`"))),
                }
            }
            Meta::List(l) => return Err(syn::Error::new_spanned(l, "unknown #[psclass] argument")),
        }
    }
    if let Some(path) = &native_bytes
        && mode != Mode::Proxy
    {
        return Err(syn::Error::new_spanned(path, "native_bytes applies to a proxy class; a copied or psobject class keeps no Rust value behind its object"));
    }
    if let Some(method) = &view
        && mode != Mode::Proxy
    {
        return Err(syn::Error::new_spanned(
            method,
            "view applies to a proxy class; the view calls a method on the object, and a copied or psobject class keeps no Rust value behind its object to run one against",
        ));
    }
    if let Some((_, expr)) = &show
        && mode != Mode::Copied
    {
        return Err(syn::Error::new_spanned(
            expr,
            "show applies to a copied class, whose values its object holds; a proxy names a view method with view, and a psobject class has no type to give a ToString",
        ));
    }
    roles.check(mode)?;
    Ok(ClassArgs { name, mode, native_bytes, view, show, columns, roles })
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
        // Only a method argument is lowered to a peer or a task.
        Slot::Peer => quote!(compile_error!("a field cannot hold a reference to another object");),
        Slot::Task => quote!(compile_error!("a field cannot hold a PsTask");),
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
    if let Some((format, expr)) = &args.show {
        let pieces = show_pieces(format).map_err(|message| syn::Error::new_spanned(expr, message))?;
        for piece in &pieces {
            if let Err(placeholder) = piece
                && !fields.iter().any(|f| &f.ps_name == placeholder)
            {
                let names: Vec<&str> = fields.iter().map(|f| f.ps_name.as_str()).collect();
                return Err(syn::Error::new_spanned(expr, format!("show names {{{placeholder}}}, which is not a property of the class; its properties are {}", names.join(", "))));
            }
        }
    }
    let view_name = args.view.as_ref().map(|method| pascal(&method.to_string()));
    let columns: Vec<String> = match &args.columns {
        Some((columns, expr)) => {
            let names: Vec<&str> = fields.iter().map(|f| f.ps_name.as_str()).collect();
            check_columns(columns, &names).map_err(|message| syn::Error::new_spanned(expr, message))?;
            // The table of named columns is the view <Type>.Columns, which a
            // view method named columns would also be.
            if let Some(method) = &args.view
                && view_name.as_deref() == Some("Columns")
            {
                return Err(syn::Error::new_spanned(method, "a view method named columns would be the view <Type>.Columns, which is the table that columns makes; give the method another name"));
            }
            columns.clone()
        }
        None => Vec::new(),
    };
    let mut o = Obj::new();
    o.str("name", &clr_name)
        .str("rust", &name.to_string())
        .str("mode", args.mode.name())
        .bool("native_bytes", args.native_bytes.is_some())
        .opt_str("view", view_name.as_deref())
        .opt_str("show", args.show.as_ref().map(|(format, _)| format.as_str()))
        .strs("columns", &columns)
        .str("description", docs.join("\n").trim())
        .raw("fields", &json::array(&fields_json))
        .raw("methods", &json::methods_sentinel());
    // A list declares Count and an indexer, which a property of the same
    // name would collide with.
    if args.roles.item.is_some()
        && let Some(f) = fields.iter().find(|f| f.ps_name == "Count" || f.ps_name == "Item")
    {
        return Err(syn::Error::new_spanned(&f.field, format!("a list declares the property {} for count and item; rename this field or mark it #[psfield(skip)]", f.ps_name)));
    }
    for (key, method) in args.roles.named() {
        o.str(key, &pascal(&method.to_string()));
    }
    let descriptor_fn = customs.descriptor_fn(&o.finish(), Some(&name));
    let role_checks = args.roles.signature_checks(&name);

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
    let rust_name = name.to_string();

    // The view is a #[psmethods] method, which has the marker, taking
    // &self and returning the text; either failing is a compile error at
    // the name the attribute gives.
    let view_check = match &args.view {
        Some(method) => {
            let marker = format_ident!("__pwrs_psmethod_{}", method, span = method.span());
            quote! {
                const _: () = {
                    let _exported: () = #name::#marker;
                    let _renders: fn(&#name) -> ::pwrs::PsResult<::std::string::String> = #name::#method;
                };
            }
        }
        None => quote!(),
    };

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
            const PATH: &'static str = ::core::concat!(::core::module_path!(), "::", #rust_name);
            const MODE: &'static str = #mode_name;
            #descriptor_fn
            fn class_id() -> ::pwrs::PsResult<u32> {
                static ID: ::std::sync::OnceLock<u32> = ::std::sync::OnceLock::new();
                match ID.get() {
                    ::core::option::Option::Some(id) => Ok(*id),
                    ::core::option::Option::None => {
                        ::core::hint::black_box(::core::concat!("PWRS-CLASS/1\t", #clr_name, "\t", ::core::module_path!(), "::", #rust_name, "\0"));
                        let id = ::pwrs::runtime::class_id::<Self>()?;
                        Ok(*ID.get_or_init(|| id))
                    }
                }
            }
            #proxy_impl
        }

        #into_ps

        #view_check

        #role_checks
    })
}

#[cfg(test)]
mod tests {
    use super::check_columns;

    fn columns(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn columns_takes_properties_of_the_class_once_each() {
        let properties = ["Name", "Items", "Committed", "Runner"];
        assert!(check_columns(&columns(&["Items"]), &properties).is_ok());
        assert!(check_columns(&columns(&["Runner", "Name"]), &properties).is_ok(), "any order");
        let refused = [
            (columns(&[]), "names no property"),
            (columns(&["Name", "Items", "Name"]), "names Name twice"),
            (columns(&["Items", "Workers"]), "names Workers, which is not a property of the class; its properties are Name, Items, Committed, Runner"),
            (columns(&["items"]), "names items, which is not a property"),
        ];
        for (list, message) in refused {
            match check_columns(&list, &properties) {
                Ok(()) => panic!("{list:?} was accepted"),
                Err(e) => assert!(e.contains(message), "{list:?}: {e}"),
            }
        }
    }

    #[test]
    fn roles_make_whole_interfaces_on_a_proxy_class_only() {
        let parse = |args: &str| super::parse_class_args(args.parse().expect("tokens"));
        for accepted in [
            "mode = proxy, count = len, item = at",
            "mode = proxy, count = len, item = at, set_item = put",
            "mode = proxy, next = take",
            "mode = proxy, compare = order",
            "mode = proxy, equals = same, hash = hash_code",
        ] {
            if let Err(e) = parse(accepted) {
                panic!("{accepted} was refused: {e}");
            }
        }
        let refused = [
            ("count = len, item = at", "count applies to a proxy class"),
            ("mode = proxy, count = len", "count makes a list with item"),
            ("mode = proxy, item = at", "item makes a list with count"),
            ("mode = proxy, set_item = put", "set_item writes an element of the list"),
            ("mode = proxy, count = len, item = at, next = take", "next makes a stream"),
            ("mode = proxy, equals = same", "equals goes with hash"),
            ("mode = proxy, hash = hash_code", "hash goes with equals"),
            ("mode = proxy, compare = \"order\"", "compare takes the name of a #[psmethods] method"),
        ];
        for (args, message) in refused {
            match parse(args) {
                Ok(_) => panic!("{args} was accepted"),
                Err(e) => assert!(e.to_string().contains(message), "{args}: {e}"),
            }
        }
    }
}
