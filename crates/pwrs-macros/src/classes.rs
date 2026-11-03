//! `#[psclass]` expansion: three output modes over one field list.
//!
//! * `copied`: a generated CLR class; `IntoPs` packs a field block and
//!   calls the module's factory once per object.
//! * `proxy`: the Rust value is boxed and the generated CLR class reads
//!   fields through `pwrs_proxy_get` until it is disposed.
//! * `psobject`: no CLR type; `IntoPs` builds a `PSObject` with note
//!   properties and a `PSTypeName`.

use crate::json::{self, Obj};
use crate::params::{key_of, lit_str, mask_bit, mask_words, str_list, MetaList};
use crate::types::{check_reserved, doc_lines, lower, output_clr, pascal, Customs, Lowered, Slot, OBJECT_MEMBERS, PROXY_MEMBERS};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{punctuated::Punctuated, Attribute, Ident, ItemStruct, Meta, Result, Token};

/// `#[psfield(...)]` on a field. `skip` leaves the field out of the
/// PowerShell surface: not a property, not packed, not read back.
#[derive(Default)]
struct FieldArgs {
    skip: bool,
    /// The .NET attributes `attribute(...)` puts on the property.
    attributes: Vec<crate::passthrough::Passed>,
}

fn parse_field_args(attr: &Attribute) -> Result<FieldArgs> {
    let mut a = FieldArgs::default();
    let list: Punctuated<Meta, Token![,]> = match &attr.meta {
        Meta::Path(bare) if bare.is_ident("psfield") => return Ok(a),
        Meta::Path(other) => return Err(syn::Error::new_spanned(other, "expected #[psfield] or #[psfield(skip)]")),
        Meta::List(l) => l.parse_args_with(Punctuated::parse_terminated)?,
        Meta::NameValue(nv) => return Err(syn::Error::new_spanned(nv, "#[psfield] takes a parenthesized list")),
    };
    for m in list {
        match m {
            Meta::Path(p) if p.is_ident("skip") => a.skip = true,
            Meta::List(l) if l.path.is_ident("attribute") => a.attributes.push(crate::passthrough::parse(&l)?),
            other => return Err(syn::Error::new_spanned(other, "unknown #[psfield] argument; use skip or attribute(...)")),
        }
    }
    if a.skip && !a.attributes.is_empty() {
        return Err(syn::Error::new_spanned(attr, "a skipped field is no property, so it has nothing to put an attribute on"));
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
    /// The path of a `fn(&Self) -> usize` returning how many bytes of
    /// native memory a proxy value owns.
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
    /// The `#[psmethods]` methods behind the class's operators.
    operators: Operators,
    /// The `#[psmethods]` methods behind the class's conversions.
    conversions: Conversions,
    /// The `inheritable` flag: script classes may derive from the class.
    inheritable: Option<syn::Path>,
    /// The .NET class a proxy class derives from, by its C# name, with
    /// the expression it came from for errors.
    base: Option<(String, syn::Expr)>,
    /// The .NET interfaces a proxy class implements beside the ones its
    /// roles make, by their C# names, with the expression for errors.
    implements: Option<(Vec<String>, syn::Expr)>,
    /// The events a proxy class declares of its own, each its name and its
    /// delegate type's C# name, with the expression for errors.
    events: Option<(Vec<(String, String)>, syn::Expr)>,
    /// The abstract members of a proxy class's base and interfaces that
    /// throw NotSupportedException when no `#[member]` method backs them,
    /// each named as `#[member]` names one, with the expression for errors.
    stubs: Option<(Vec<String>, syn::Expr)>,
    /// The .NET attributes `attribute(...)` puts on the class, with the
    /// first entry's tokens for errors.
    attributes: (Vec<crate::passthrough::Passed>, Option<syn::MetaList>),
}

/// The members a `stubs` list names, each as `#[member]` names one: its
/// name, or its declaring type's C# name, a dot and its name. Refuses an
/// empty list, an empty name or one with white space, and a name given
/// twice.
fn parse_stubs(value: &syn::Expr) -> Result<Vec<String>> {
    let names = str_list(value)?;
    if names.is_empty() {
        return Err(syn::Error::new_spanned(value, "stubs names no member; give each abstract member left without a #[member] method by its name, as [\"Insert\", \"RemoveAt\"]"));
    }
    for (i, name) in names.iter().enumerate() {
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(syn::Error::new_spanned(value, format!("stubs names {name:?}, which is not a member's name; give its name, or its declaring type's C# name, a dot and its name, as #[member] does")));
        }
        if names[..i].contains(name) {
            return Err(syn::Error::new_spanned(value, format!("stubs names {name} twice")));
        }
    }
    Ok(names)
}

/// The delegate type of an event the `events` key names without one.
const DEFAULT_EVENT_TYPE: &str = "System.EventHandler";

/// The events an `events` list declares: each item a name, with its
/// delegate type's full C# name after a colon, [`DEFAULT_EVENT_TYPE`]
/// when it gives none. Refuses an empty list, a name that is not a C#
/// identifier or hides an inherited member, a name given twice, and a type
/// that is not a full C# name.
fn parse_events(value: &syn::Expr) -> Result<Vec<(String, String)>> {
    let items = str_list(value)?;
    if items.is_empty() {
        return Err(syn::Error::new_spanned(
            value,
            "events names no event; give each one's name, with its delegate type after a colon when it is not System.EventHandler, as [\"Changed\", \"Progress: System.EventHandler<Hello.ProgressArgs>\"]",
        ));
    }
    let mut events: Vec<(String, String)> = Vec::new();
    for item in &items {
        let (name, ty) = match item.split_once(':') {
            Some((name, ty)) => (name.trim(), ty.trim()),
            None => (item.trim(), DEFAULT_EVENT_TYPE),
        };
        let identifier = name.chars().next().is_some_and(|ch| ch.is_alphabetic() || ch == '_') && name.chars().all(|ch| ch.is_alphanumeric() || ch == '_');
        if !identifier {
            return Err(syn::Error::new_spanned(value, format!("events names {name:?}, which is not an event name; an event's name is a C# identifier, as Changed")));
        }
        check_type_name("events", ty).map_err(|message| syn::Error::new_spanned(value, format!("{name}: {message}")))?;
        check_reserved(value, "an event", name, &[OBJECT_MEMBERS, PROXY_MEMBERS])?;
        if events.iter().any(|(earlier, _)| earlier == name) {
            return Err(syn::Error::new_spanned(value, format!("events names {name} twice")));
        }
        events.push((name.to_string(), ty.to_string()));
    }
    Ok(events)
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
            return Err(syn::Error::new_spanned(method, format!("{key} applies to a proxy class; its interface calls methods on the object, and a copied or psobject class has no Rust value to run one against")));
        }
        match (&self.count, &self.item) {
            (Some(count), None) => return Err(syn::Error::new_spanned(count, "count makes a list with item: name the method that reads one element, item = method")),
            (None, Some(item)) => return Err(syn::Error::new_spanned(item, "item makes a list with count: name the method that returns the length, count = method")),
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
            (None, Some(hash)) => Err(syn::Error::new_spanned(hash, "hash goes with equals: name the method that compares for equality too, equals = method")),
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

/// The methods behind a class's arithmetic operators, each by its Rust
/// name. The shell declares a C# operator for each, which PowerShell's
/// `+`, `-`, `*`, `/` and `%` call. On a proxy class the method takes
/// `&self` or `&Self` as the left operand; on a copied class it is a
/// static method taking `Self`.
#[derive(Default)]
struct Operators {
    /// `+`, as `op_Addition`.
    add: Option<Ident>,
    /// `-`, as `op_Subtraction`.
    sub: Option<Ident>,
    /// `*`, as `op_Multiply`.
    mul: Option<Ident>,
    /// `/`, as `op_Division`.
    div: Option<Ident>,
    /// `%`, as `op_Modulus`.
    rem: Option<Ident>,
}

impl Operators {
    /// Each operator named, with its key.
    fn named(&self) -> Vec<(&'static str, &Ident)> {
        let all = [("add", &self.add), ("sub", &self.sub), ("mul", &self.mul), ("div", &self.div), ("rem", &self.rem)];
        all.into_iter().filter_map(|(key, method)| method.as_ref().map(|m| (key, m))).collect()
    }

    /// Refuses an operator on a psobject class, which has no methods.
    fn check(&self, mode: Mode) -> Result<()> {
        if let Some((key, method)) = self.named().first()
            && mode == Mode::PsObject
        {
            return Err(syn::Error::new_spanned(method, format!("{key} calls a method of the class, and a psobject class has none; make it mode = proxy or mode = copied")));
        }
        Ok(())
    }

    /// A constant block that fails to compile unless each named method is
    /// a `#[psmethods]` method taking the class as its left operand: by
    /// reference on a proxy class, by value on a copied one.
    fn signature_checks(&self, class: &Ident, mode: Mode) -> TokenStream {
        let named = self.named();
        if named.is_empty() {
            return quote!();
        }
        let checks = named.into_iter().map(|(_key, method)| {
            let marker = format_ident!("__pwrs_psmethod_{}", method, span = method.span());
            let left = if mode == Mode::Proxy { quote!(&#class) } else { quote!(#class) };
            quote! {
                let _: () = #class::#marker;
                let _: fn(#left, _) -> ::pwrs::PsResult<_> = #class::#method;
            }
        });
        quote! {
            const _: () = {
                #(#checks)*
            };
        }
    }
}

/// The methods behind a class's conversions, each by its Rust name.
/// `implicit` and `explicit` name the methods behind C# conversion
/// operators, which PowerShell's casts and its binder call: a static
/// method taking another type and returning the class converts from that
/// type, and one taking the class converts to the type it returns, which
/// `cargo pwrs build` tells apart from the descriptor. `converter` names
/// the two functions behind the PSTypeConverter the class names with
/// `[TypeConverter]`: one converting any value to the class, one
/// converting the class to any type.
#[derive(Default)]
struct Conversions {
    implicit: Vec<Ident>,
    explicit: Vec<Ident>,
    /// `fn(PsObject) -> PsResult<Option<Self>>`, then a method
    /// `fn(&self, PsType) -> PsResult<Option<PsObject>>`, or on a copied
    /// class a static taking `Self` first.
    converter: Option<(Ident, Ident)>,
}

impl Conversions {
    /// Each method named, with its key.
    fn named(&self) -> Vec<(&'static str, &Ident)> {
        let mut all: Vec<(&'static str, &Ident)> = Vec::new();
        all.extend(self.implicit.iter().map(|m| ("implicit", m)));
        all.extend(self.explicit.iter().map(|m| ("explicit", m)));
        if let Some((from, to)) = &self.converter {
            all.extend([("converter", from), ("converter", to)]);
        }
        all
    }

    /// Refuses a conversion on a psobject class, which has no methods, and a
    /// method behind two conversion operators.
    fn check(&self, mode: Mode) -> Result<()> {
        let named = self.named();
        if let Some((key, method)) = named.first()
            && mode == Mode::PsObject
        {
            return Err(syn::Error::new_spanned(method, format!("{key} calls a method of the class, and a psobject class has none; make it mode = proxy or mode = copied")));
        }
        let operators: Vec<&Ident> = self.implicit.iter().chain(&self.explicit).collect();
        for (i, method) in operators.iter().enumerate() {
            if operators[..i].contains(method) {
                return Err(syn::Error::new_spanned(method, format!("{method} is named twice in implicit and explicit; a method is behind one conversion")));
            }
        }
        if let Some((from, to)) = &self.converter
            && from == to
        {
            return Err(syn::Error::new_spanned(to, "converter names one function converting from any value and another converting to any type"));
        }
        Ok(())
    }

    /// A constant block that fails to compile unless each named method is
    /// a `#[psmethods]` method, and the converter's two functions have
    /// their shapes: by reference to the class on a proxy, by value on a
    /// copied class.
    fn signature_checks(&self, class: &Ident, mode: Mode) -> TokenStream {
        let named = self.named();
        if named.is_empty() {
            return quote!();
        }
        let markers = named.into_iter().map(|(_key, method)| {
            let marker = format_ident!("__pwrs_psmethod_{}", method, span = method.span());
            quote!(let _: () = #class::#marker;)
        });
        let converter = match &self.converter {
            Some((from, to)) => {
                let left = if mode == Mode::Proxy { quote!(&#class) } else { quote!(#class) };
                quote! {
                    let _: fn(::pwrs::PsObject) -> ::pwrs::PsResult<::core::option::Option<#class>> = #class::#from;
                    let _: fn(#left, ::pwrs::PsType) -> ::pwrs::PsResult<::core::option::Option<::pwrs::PsObject>> = #class::#to;
                }
            }
            None => quote!(),
        };
        quote! {
            const _: () = {
                #(#markers)*
                #converter
            };
        }
    }
}

/// Parses `key = [method, ...]`, the bare names of `#[psmethods]`
/// methods, refusing an empty list.
fn method_list(key: &str, value: &syn::Expr) -> Result<Vec<Ident>> {
    let shape = || syn::Error::new_spanned(value, format!("{key} takes a list of the names of #[psmethods] methods of the class, as [to_double]"));
    let syn::Expr::Array(list) = value else { return Err(shape()) };
    if list.elems.is_empty() {
        return Err(shape());
    }
    list.elems.iter().map(|e| role_method(key, e)).collect()
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

/// Refuses a name that `base` or `implements` gives when it is not a
/// type's full C# name: a dotted name such as System.IO.Stream, with any
/// type arguments in angle brackets, as System.IComparable<long>.
fn check_type_name(key: &str, name: &str) -> std::result::Result<(), String> {
    let bad = || format!("{key} takes a type's full C# name, such as System.IO.Stream or System.Collections.Generic.IComparer<long>, and {name:?} is not one");
    let mut depth = 0usize;
    for ch in name.chars() {
        match ch {
            '<' | '[' => depth += 1,
            '>' | ']' => depth = depth.checked_sub(1).ok_or_else(bad)?,
            c if c.is_alphanumeric() || matches!(c, '_' | '.' | ',' | ' ') => {}
            _other => return Err(bad()),
        }
    }
    let outer = match name.find('<') {
        Some(at) => &name[..at],
        None => name,
    };
    let dotted = outer.split('.').count() > 1 && outer.split('.').all(|part| !part.is_empty() && !part.contains(' '));
    if depth != 0 || !dotted || name.trim() != name {
        return Err(bad());
    }
    Ok(())
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
    let mut operators = Operators::default();
    let mut conversions = Conversions::default();
    let mut inheritable = None;
    let mut base = None;
    let mut implements = None;
    let mut events = None;
    let mut stubs = None;
    let mut attributes: (Vec<crate::passthrough::Passed>, Option<syn::MetaList>) = (Vec::new(), None);
    for m in list.0 {
        match m {
            Meta::Path(p) if p.is_ident("copied") => mode = Mode::Copied,
            Meta::Path(p) if p.is_ident("proxy") => mode = Mode::Proxy,
            Meta::Path(p) if p.is_ident("psobject") => mode = Mode::PsObject,
            Meta::Path(p) if p.is_ident("inheritable") => inheritable = Some(p),
            Meta::Path(p) => return Err(syn::Error::new_spanned(p, "unknown #[psclass] flag; use copied, proxy, psobject or inheritable")),
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
                    "add" => operators.add = Some(role_method("add", &nv.value)?),
                    "sub" => operators.sub = Some(role_method("sub", &nv.value)?),
                    "mul" => operators.mul = Some(role_method("mul", &nv.value)?),
                    "div" => operators.div = Some(role_method("div", &nv.value)?),
                    "rem" => operators.rem = Some(role_method("rem", &nv.value)?),
                    "implicit" => conversions.implicit = method_list("implicit", &nv.value)?,
                    "explicit" => conversions.explicit = method_list("explicit", &nv.value)?,
                    "converter" => match method_list("converter", &nv.value)?.as_slice() {
                        [from, to] => conversions.converter = Some((from.clone(), to.clone())),
                        _other => {
                            return Err(syn::Error::new_spanned(
                                &nv.value,
                                "converter names two functions, as [from_any, to_any]: one converting any value to the class, and one converting the class to any type",
                            ));
                        }
                    },
                    "base" => {
                        let name = lit_str(&nv.value)?;
                        check_type_name("base", &name).map_err(|message| syn::Error::new_spanned(&nv.value, message))?;
                        base = Some((name, nv.value.clone()));
                    }
                    "implements" => {
                        let names = str_list(&nv.value)?;
                        if names.is_empty() {
                            return Err(syn::Error::new_spanned(&nv.value, "implements names no interface; give it each one's full C# name, as [\"System.IFormattable\"]"));
                        }
                        for (i, name) in names.iter().enumerate() {
                            check_type_name("implements", name).map_err(|message| syn::Error::new_spanned(&nv.value, message))?;
                            if names[..i].contains(name) {
                                return Err(syn::Error::new_spanned(&nv.value, format!("implements names {name} twice")));
                            }
                        }
                        implements = Some((names, nv.value.clone()));
                    }
                    "events" => events = Some((parse_events(&nv.value)?, nv.value.clone())),
                    "stubs" => stubs = Some((parse_stubs(&nv.value)?, nv.value.clone())),
                    unknown => return Err(syn::Error::new_spanned(nv.path, format!("unknown #[psclass] key `{unknown}`"))),
                }
            }
            Meta::List(l) if l.path.is_ident("attribute") => {
                attributes.0.push(crate::passthrough::parse(&l)?);
                if attributes.1.is_none() {
                    attributes.1 = Some(l);
                }
            }
            Meta::List(l) => return Err(syn::Error::new_spanned(l, "unknown #[psclass] argument")),
        }
    }
    if let Some(first) = &attributes.1
        && mode == Mode::PsObject
    {
        return Err(syn::Error::new_spanned(first, "attribute applies to a proxy or copied class; a psobject class has no type of its own to put it on"));
    }
    if let Some(path) = &native_bytes
        && mode != Mode::Proxy
    {
        return Err(syn::Error::new_spanned(path, "native_bytes applies to a proxy class; a copied or psobject class has no Rust value behind its object"));
    }
    if let Some(method) = &view
        && mode != Mode::Proxy
    {
        return Err(syn::Error::new_spanned(
            method,
            "view applies to a proxy class; the view calls a method on the object, and a copied or psobject class has no Rust value behind its object to run one against",
        ));
    }
    if let Some((_, expr)) = &show
        && mode != Mode::Copied
    {
        return Err(syn::Error::new_spanned(
            expr,
            "show applies to a copied class, whose object has its values; a proxy names a view method with view, and a psobject class has no type to give a ToString",
        ));
    }
    for (key, expr) in [
        ("base", base.as_ref().map(|(_, e)| e)),
        ("implements", implements.as_ref().map(|(_, e)| e)),
        ("events", events.as_ref().map(|(_, e)| e)),
        ("stubs", stubs.as_ref().map(|(_, e)| e)),
    ] {
        if let Some(expr) = expr
            && mode != Mode::Proxy
        {
            return Err(syn::Error::new_spanned(
                expr,
                format!("{key} applies to a proxy class, whose members a Rust value backs; a copied or psobject class has no Rust value behind its object"),
            ));
        }
    }
    if let Some((_, expr)) = &stubs
        && base.is_none()
        && implements.is_none()
    {
        return Err(syn::Error::new_spanned(expr, "stubs names abstract members of the class's base and interfaces, and the class names neither a base nor implements"));
    }
    if let Some(flag) = &inheritable
        && mode == Mode::PsObject
    {
        return Err(syn::Error::new_spanned(flag, "inheritable applies to a proxy or copied class; a psobject class has no type of its own to derive from"));
    }
    roles.check(mode)?;
    operators.check(mode)?;
    conversions.check(mode)?;
    Ok(ClassArgs { name, mode, native_bytes, view, show, columns, roles, operators, conversions, inheritable, base, implements, events, stubs, attributes })
}

struct FieldSpec {
    field: Ident,
    ps_name: String,
    lowered: Lowered,
    help: String,
    /// The .NET attributes `attribute(...)` puts on the property.
    attributes: Vec<crate::passthrough::Passed>,
}

fn field_json(i: usize, f: &FieldSpec, customs: &mut Customs) -> String {
    let mut o = Obj::new();
    o.str("name", &f.ps_name).str("rust", &f.field.to_string()).num("index", i as i64);
    customs.clr_keys(&mut o, &output_clr(&f.lowered));
    o.str("slot", f.lowered.slot.name())
        .bool("optional", f.lowered.optional)
        .str("help", &f.help)
        .raw("attributes", &crate::passthrough::json(&f.attributes));
    o.finish()
}

/// Statements that place field `i` of `self` into `block`, with any
/// buffers stored in the `keep_*` vectors until the factory returns.
fn pack_stmt(i: usize, f: &FieldSpec) -> TokenStream {
    let slot = format_ident!("p{}", i);
    let field = &f.field;
    let (word, bit) = mask_bit(i);
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
        // Only a method argument is lowered to a peer, a task, the object
        // itself or a member's type arguments.
        Slot::Peer => quote!(compile_error!("a field cannot be a reference to another object");),
        Slot::Task => quote!(compile_error!("a field cannot be a PsTask");),
        Slot::This => quote!(compile_error!("a field cannot be a PsThis");),
        Slot::Types => quote!(compile_error!("a field cannot be a member's type arguments");),
    };
    if f.lowered.optional {
        quote!(if let ::core::option::Option::Some(__value) = self.#field { block.mask[#word] |= 1u64 << #bit; #store } else { block.mask[#word] &= !(1u64 << #bit); })
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
                let parsed = parse_field_args(&attr)?;
                if mode == Mode::PsObject && !parsed.attributes.is_empty() {
                    return Err(syn::Error::new_spanned(&attr, "attribute applies to a property of a proxy or copied class; a psobject class's note properties have no member to put it on"));
                }
                parsed
            }
            None => FieldArgs::default(),
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
            fields.push(FieldSpec { ps_name, field: ident, lowered, help, attributes: args.attributes });
        }
    }
    let docs = doc_lines(&input.attrs);
    let mut customs = Customs::default();
    let fields_json: Vec<String> = fields.iter().enumerate().map(|(i, f)| field_json(i, f, &mut customs)).collect();
    let events_json: Vec<String> = match &args.events {
        Some((events, _)) => events
            .iter()
            .map(|(name, ty)| {
                let mut e = Obj::new();
                e.str("name", name).str("type", ty);
                e.finish()
            })
            .collect(),
        None => Vec::new(),
    };
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
        .opt_str("base", args.base.as_ref().map(|(name, _)| name.as_str()))
        .strs("implements", args.implements.as_ref().map_or(&[][..], |(names, _)| names.as_slice()))
        .raw("events", &json::array(&events_json))
        .strs("stubs", args.stubs.as_ref().map_or(&[][..], |(names, _)| names.as_slice()))
        .bool("inheritable", args.inheritable.is_some())
        .raw("attributes", &crate::passthrough::json(&args.attributes.0))
        .raw("fields", &json::array(&fields_json))
        .raw("methods", &json::methods_sentinel());
    // An event and a property of one name would collide in the class.
    if let Some((events, expr)) = &args.events
        && let Some(f) = fields.iter().find(|f| events.iter().any(|(name, _)| name == &f.ps_name))
    {
        return Err(syn::Error::new_spanned(expr, format!("events declares {}, which the field {} is already the property of; rename one", f.ps_name, f.field)));
    }
    // A list declares Count and an indexer, which a property of the same
    // name would collide with.
    if args.roles.item.is_some()
        && let Some(f) = fields.iter().find(|f| f.ps_name == "Count" || f.ps_name == "Item")
    {
        return Err(syn::Error::new_spanned(&f.field, format!("a list declares the property {} for count and item; rename this field or mark it #[psfield(skip)]", f.ps_name)));
    }
    for (key, method) in args.roles.named().into_iter().chain(args.operators.named()) {
        o.str(key, &pascal(&method.to_string()));
    }
    let names = |methods: Vec<&Ident>| methods.into_iter().map(|m| pascal(&m.to_string())).collect::<Vec<String>>();
    o.strs("implicit", &names(args.conversions.implicit.iter().collect()))
        .strs("explicit", &names(args.conversions.explicit.iter().collect()))
        .strs("converter", &names(args.conversions.converter.as_ref().map_or(Vec::new(), |(from, to)| vec![from, to])));
    let descriptor_fn = customs.descriptor_fn(&o.finish(), Some(&name));
    let role_checks = args.roles.signature_checks(&name);
    let operator_checks = args.operators.signature_checks(&name, args.mode);
    let conversion_checks = args.conversions.signature_checks(&name, args.mode);

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
    // On an inheritable proxy class, a call to its protected constructor's
    // id returns its Default value.
    let default_constructor = if args.inheritable.is_some() && args.mode == Mode::Proxy {
        quote! {
            if method_id == ::pwrs::class::DEFAULT_CONSTRUCTOR {
                return ::pwrs::class::inheritable_default::<#name>();
            }
        }
    } else {
        quote!()
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
                #default_constructor
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
    // A field stored only in Rust has no property to read it from, so
    // such a class is not reconstructible by value.
    let from_ps_body = if skipped.is_empty() {
        quote!(Ok(#name { #(#from_fields,)* }))
    } else {
        let kept = skipped.join(", ");
        quote!(Err(::pwrs::PsError::new(
            ::pwrs::ErrorCategory::InvalidType,
            "PwrsOpaqueClass",
            format!("{} has Rust-only state ({}) and cannot be read back by value", #clr_name, #kept),
        )))
    };
    let field_words = mask_words(fields.len());

    Ok(quote! {
        #input

        #[repr(C)]
        #[allow(non_camel_case_types, dead_code)]
        #vis struct #block_ident {
            #(#block_fields,)*
            pub mask: [u64; #field_words],
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

        #operator_checks

        #conversion_checks
    })
}

#[cfg(test)]
mod tests {
    use super::{check_columns, parse_class_args};
    use crate::params::squeezed;

    #[test]
    fn an_operator_names_a_method_of_a_copied_or_proxy_class() {
        for mode in ["copied", "proxy"] {
            let args = parse_class_args(format!("mode = {mode}, add = plus, sub = minus, mul = times, div = over, rem = left").parse().expect("tokens"))
                .unwrap_or_else(|e| panic!("{mode}: {e}"));
            let keys: Vec<&str> = args.operators.named().into_iter().map(|(key, _)| key).collect();
            assert_eq!(keys, ["add", "sub", "mul", "div", "rem"], "{mode}");
        }
        for key in ["add", "sub", "mul", "div", "rem"] {
            match parse_class_args(format!("mode = psobject, {key} = plus").parse().expect("tokens")) {
                Ok(_) => panic!("{key} was accepted on a psobject class"),
                Err(e) => assert!(e.to_string().contains("a psobject class has none"), "{key}: {e}"),
            }
        }
    }

    /// `inheritable` goes on a proxy or copied class, and a call to a
    /// proxy's protected constructor's id returns its Default value.
    #[test]
    fn inheritable_marks_a_proxy_or_copied_class_and_a_proxy_builds_its_default_value() {
        for mode in ["copied", "proxy"] {
            match parse_class_args(format!("mode = {mode}, inheritable").parse().expect("tokens")) {
                Ok(args) => assert!(args.inheritable.is_some(), "{mode}"),
                Err(e) => panic!("{mode}: {e}"),
            }
        }
        match parse_class_args("mode = psobject, inheritable".parse().expect("tokens")) {
            Ok(_) => panic!("inheritable was accepted on a psobject class"),
            Err(e) => assert!(e.to_string().contains("inheritable applies to a proxy or copied class"), "{e}"),
        }
        let item = syn::parse_str::<syn::ItemStruct>("pub struct Shape { pub name: String }").expect("a struct");
        let tokens = match super::expand_psclass("name = \"Demo.Shape\", mode = proxy, inheritable".parse().expect("tokens"), item) {
            Ok(tokens) => squeezed(&tokens.to_string()),
            Err(e) => panic!("the class was refused: {e}"),
        };
        assert!(tokens.contains(&squeezed("if method_id == ::pwrs::class::DEFAULT_CONSTRUCTOR { return ::pwrs::class::inheritable_default::<Shape>(); }")), "{tokens}");
        assert!(tokens.contains(r#"\"inheritable\":true"#), "{tokens}");
    }

    /// `implicit` and `explicit` list methods behind conversion operators,
    /// and `converter` the two functions behind the class's converter.
    #[test]
    fn conversions_list_methods_and_a_converter_names_two_functions() {
        for mode in ["copied", "proxy"] {
            let args = match parse_class_args(format!("mode = {mode}, implicit = [from_int, from_text], explicit = [to_double], converter = [from_any, to_any]").parse().expect("tokens")) {
                Ok(args) => args,
                Err(e) => panic!("{mode}: {e}"),
            };
            let named: Vec<String> = args.conversions.named().into_iter().map(|(key, method)| format!("{key}:{method}")).collect();
            assert_eq!(named, ["implicit:from_int", "implicit:from_text", "explicit:to_double", "converter:from_any", "converter:to_any"], "{mode}");
        }
        let refused = [
            ("mode = psobject, implicit = [from_int]", "a psobject class has none"),
            ("mode = proxy, implicit = [from_int], explicit = [from_int]", "from_int is named twice in implicit and explicit"),
            ("mode = proxy, implicit = []", "implicit takes a list of the names of #[psmethods] methods"),
            ("mode = proxy, explicit = to_double", "explicit takes a list of the names of #[psmethods] methods"),
            ("mode = proxy, converter = [from_any]", "converter names two functions"),
            ("mode = proxy, converter = [any, any]", "converter names one function converting from any value and another converting to any type"),
        ];
        for (args, message) in refused {
            match parse_class_args(args.parse().expect("tokens")) {
                Ok(_) => panic!("{args} was accepted"),
                Err(e) => assert!(e.to_string().contains(message), "{args}: {e}"),
            }
        }
    }

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

    #[test]
    fn a_base_and_interfaces_are_full_type_names_on_a_proxy_class_only() {
        let parse = |args: &str| super::parse_class_args(args.parse().expect("tokens"));
        for accepted in [
            r#"mode = proxy, base = "System.IO.Stream""#,
            r#"mode = proxy, implements = ["System.IFormattable", "System.Collections.Generic.IComparer<long>"]"#,
            r#"mode = proxy, base = "System.Exception", implements = "System.IFormattable""#,
            r#"mode = proxy, implements = ["System.Collections.Generic.IDictionary<string, byte[]>"]"#,
        ] {
            if let Err(e) = parse(accepted) {
                panic!("{accepted} was refused: {e}");
            }
        }
        let refused = [
            (r#"base = "System.IO.Stream""#, "base applies to a proxy class"),
            (r#"mode = psobject, implements = ["System.IFormattable"]"#, "implements applies to a proxy class"),
            (r#"mode = proxy, base = "Stream""#, "takes a type's full C# name"),
            (r#"mode = proxy, base = "global::System.IO.Stream""#, "takes a type's full C# name"),
            (r#"mode = proxy, base = "System.Collections.Generic.List<int""#, "takes a type's full C# name"),
            (r#"mode = proxy, base = " System.IO.Stream""#, "takes a type's full C# name"),
            ("mode = proxy, implements = []", "implements names no interface"),
            (r#"mode = proxy, implements = ["System.IFormattable", "System.IFormattable"]"#, "names System.IFormattable twice"),
        ];
        for (args, message) in refused {
            match parse(args) {
                Ok(_) => panic!("{args} was accepted"),
                Err(e) => assert!(e.to_string().contains(message), "{args}: {e}"),
            }
        }
    }

    /// Each event the key names has its delegate type after a colon, and
    /// System.EventHandler without one, on a proxy class only.
    #[test]
    fn events_declare_names_and_delegate_types_on_a_proxy_class_only() {
        let parse = |args: &str| super::parse_class_args(args.parse().expect("tokens"));
        let args = match parse(r#"mode = proxy, events = ["Changed", "Progress: System.EventHandler<Hello.ProgressArgs>", "Ticked :System.Action<long>"]"#) {
            Ok(args) => args,
            Err(e) => panic!("the events were refused: {e}"),
        };
        let declared: Vec<(String, String)> = match args.events {
            Some((events, _)) => events,
            None => panic!("the events key was parsed into no events"),
        };
        let expected: Vec<(String, String)> = [("Changed", "System.EventHandler"), ("Progress", "System.EventHandler<Hello.ProgressArgs>"), ("Ticked", "System.Action<long>")]
            .iter()
            .map(|(name, ty)| (name.to_string(), ty.to_string()))
            .collect();
        assert_eq!(declared, expected);
        let refused = [
            (r#"events = ["Changed"]"#, "events applies to a proxy class"),
            ("mode = proxy, events = []", "events names no event"),
            (r#"mode = proxy, events = ["Changed", "Changed: System.Action"]"#, "names Changed twice"),
            (r#"mode = proxy, events = ["On Changed"]"#, "not an event name"),
            (r#"mode = proxy, events = ["1st"]"#, "not an event name"),
            (r#"mode = proxy, events = ["Changed: Action"]"#, "takes a type's full C# name"),
            (r#"mode = proxy, events = ["Dispose"]"#, "hides a member the generated class inherits"),
            (r#"mode = proxy, events = ["ToString"]"#, "hides a member the generated class inherits"),
        ];
        for (args, message) in refused {
            match parse(args) {
                Ok(_) => panic!("{args} was accepted"),
                Err(e) => assert!(e.to_string().contains(message), "{args}: {e}"),
            }
        }
    }

    /// The members stubs names are left as given, on a proxy class that
    /// names a base or interfaces only.
    #[test]
    fn stubs_name_members_of_a_proxy_class_s_base_or_interfaces() {
        let parse = |args: &str| super::parse_class_args(args.parse().expect("tokens"));
        let args = match parse(r#"mode = proxy, base = "System.Reflection.Binder", stubs = ["ChangeType", "System.Reflection.Binder.SelectMethod"]"#) {
            Ok(args) => args,
            Err(e) => panic!("the stubs were refused: {e}"),
        };
        match args.stubs {
            Some((names, _)) => assert_eq!(names, ["ChangeType", "System.Reflection.Binder.SelectMethod"]),
            None => panic!("the stubs key was parsed into no names"),
        }
        let refused = [
            (r#"stubs = ["Insert"]"#, "stubs applies to a proxy class"),
            (r#"mode = proxy, stubs = ["Insert"]"#, "the class names neither a base nor implements"),
            (r#"mode = proxy, implements = ["System.Collections.IList"], stubs = []"#, "stubs names no member"),
            (r#"mode = proxy, implements = ["System.Collections.IList"], stubs = ["Insert", "Insert"]"#, "stubs names Insert twice"),
            (r#"mode = proxy, implements = ["System.Collections.IList"], stubs = ["Remove At"]"#, "which is not a member's name"),
        ];
        for (args, message) in refused {
            match parse(args) {
                Ok(_) => panic!("{args} was accepted"),
                Err(e) => assert!(e.to_string().contains(message), "{args}: {e}"),
            }
        }
    }

    /// `#[psclass]` on a copied class of `n` optional fields, `a0` to
    /// `a{n-1}`, printed without whitespace.
    fn copied_class_of(n: usize) -> String {
        let fields: Vec<String> = (0..n).map(|i| format!("pub a{i}: Option<i64>")).collect();
        let item = syn::parse_str::<syn::ItemStruct>(&format!("pub struct Wide {{ {} }}", fields.join(", "))).expect("a struct");
        match super::expand_psclass(proc_macro2::TokenStream::new(), item) {
            Ok(tokens) => squeezed(&tokens.to_string()),
            Err(e) => panic!("{n} fields were refused: {e}"),
        }
    }

    #[test]
    fn the_field_mask_of_a_copied_class_takes_one_word_per_64_fields() {
        for (n, words) in [(0, 1), (1, 1), (64, 1), (65, 2), (128, 2), (129, 3)] {
            let mask = format!("pub mask: [u64; {words}usize],");
            assert!(copied_class_of(n).contains(&squeezed(&mask)), "{n} fields: no `{mask}`");
        }
    }

    #[test]
    fn an_optional_field_past_the_first_64_is_marked_in_its_own_word() {
        let out = copied_class_of(130);
        for (i, word, bit) in [(0, 0, 0), (63, 0, 63), (64, 1, 0), (65, 1, 1), (127, 1, 63), (128, 2, 0), (129, 2, 1)] {
            let pack = format!(
                "if let ::core::option::Option::Some(__value) = self.a{i} {{ block.mask[{word}usize] |= 1u64 << {bit}u64; block.p{i} = __value; }} else {{ block.mask[{word}usize] &= !(1u64 << {bit}u64); }}"
            );
            assert!(out.contains(&squeezed(&pack)), "field {i}: no `{pack}` in {out}");
        }
    }
}
