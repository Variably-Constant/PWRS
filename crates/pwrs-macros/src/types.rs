//! Type lowering shared by `#[cmdlet]` parameters and `#[psclass]`
//! fields, plus small helpers over attributes and names.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, Expr, ExprLit, GenericArgument, Ident, Lit, Meta, PathArguments, Result, Type};

/// How one value crosses the boundary inside a block.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Str16,
    Handle,
}

impl Slot {
    pub fn name(self) -> &'static str {
        match self {
            Slot::Bool => "bool",
            Slot::I8 => "i8",
            Slot::I16 => "i16",
            Slot::I32 => "i32",
            Slot::I64 => "i64",
            Slot::U8 => "u8",
            Slot::U16 => "u16",
            Slot::U32 => "u32",
            Slot::U64 => "u64",
            Slot::F32 => "f32",
            Slot::F64 => "f64",
            Slot::Str16 => "str16",
            Slot::Handle => "handle",
        }
    }

    pub fn rust_ty(self) -> TokenStream {
        match self {
            Slot::Bool => quote!(u8),
            Slot::I8 => quote!(i8),
            Slot::I16 => quote!(i16),
            Slot::I32 => quote!(i32),
            Slot::I64 => quote!(i64),
            Slot::U8 => quote!(u8),
            Slot::U16 => quote!(u16),
            Slot::U32 => quote!(u32),
            Slot::U64 => quote!(u64),
            Slot::F32 => quote!(f32),
            Slot::F64 => quote!(f64),
            Slot::Str16 => quote!(::pwrs::sys::PsStr16),
            Slot::Handle => quote!(::pwrs::sys::PsHandle),
        }
    }
}

/// The CLR type a lowered Rust type declares in the shell.
#[derive(Clone)]
pub enum ClrName {
    /// Known at macro time.
    Literal(String),
    /// A user type implementing `pwrs::class::PsTyped`; its name is
    /// read from the trait when the descriptor is built. `suffix` is
    /// `[]` for an array of it.
    Custom { ty: Box<Type>, suffix: String },
}

/// The lowered shape of a Rust type.
pub struct Lowered {
    /// CLR type the shell declares.
    pub clr: ClrName,
    pub slot: Slot,
    /// `Option<T>`: absent means `None`.
    pub optional: bool,
    /// The type a `FromPs`/`IntoPs` conversion targets when the slot
    /// is a handle.
    pub inner: Type,
    /// The Rust type is `PathBuf`.
    pub path: bool,
}

fn generic_arg(seg: &syn::PathSegment) -> Option<&Type> {
    if let PathArguments::AngleBracketed(a) = &seg.arguments {
        for g in &a.args {
            if let GenericArgument::Type(t) = g {
                return Some(t);
            } else {
                continue;
            }
        }
        None
    } else {
        None
    }
}

pub fn path_ident(ty: &Type) -> Option<(&Ident, Option<&Type>)> {
    let Type::Path(tp) = ty else { return None };
    let seg = tp.path.segments.last()?;
    Some((&seg.ident, generic_arg(seg)))
}

pub fn lower(ty: &Type) -> Result<Lowered> {
    let (ident, arg) = path_ident(ty).ok_or_else(|| syn::Error::new_spanned(ty, "unsupported type"))?;
    let name = ident.to_string();
    let plain = |clr: &str, slot: Slot| -> Result<Lowered> {
        if arg.is_some() {
            return Err(syn::Error::new_spanned(ty, format!("`{name}` takes no type argument here")));
        }
        Ok(Lowered { clr: ClrName::Literal(clr.into()), slot, optional: false, inner: ty.clone(), path: false })
    };
    match name.as_str() {
        "bool" => plain("SwitchParameter", Slot::Bool),
        "i8" => plain("sbyte", Slot::I8),
        "i16" => plain("short", Slot::I16),
        "i32" => plain("int", Slot::I32),
        "i64" => plain("long", Slot::I64),
        "u8" => plain("byte", Slot::U8),
        "u16" => plain("ushort", Slot::U16),
        "u32" => plain("uint", Slot::U32),
        "u64" => plain("ulong", Slot::U64),
        "f32" => plain("float", Slot::F32),
        "f64" => plain("double", Slot::F64),
        "String" => plain("string", Slot::Str16),
        "PathBuf" => Ok(Lowered { clr: ClrName::Literal("string".into()), slot: Slot::Str16, optional: false, inner: ty.clone(), path: true }),
        "PsObject" => plain("object", Slot::Handle),
        "PsScriptBlock" => plain("System.Management.Automation.ScriptBlock", Slot::Handle),
        "PsHashtable" => plain("System.Collections.Hashtable", Slot::Handle),
        "Option" => {
            let inner = arg.ok_or_else(|| syn::Error::new_spanned(ty, "Option needs a type argument"))?;
            let mut l = lower(inner)?;
            if l.optional {
                return Err(syn::Error::new_spanned(ty, "Option<Option<T>> is not supported"));
            }
            l.optional = true;
            Ok(l)
        }
        "Vec" => {
            let inner = arg.ok_or_else(|| syn::Error::new_spanned(ty, "Vec needs a type argument"))?;
            let e = lower(inner)?;
            if e.optional {
                return Err(syn::Error::new_spanned(ty, "Vec<Option<T>> is not supported"));
            }
            let clr = match e.clr {
                ClrName::Literal(elem) => {
                    let elem = if elem == "SwitchParameter" { "bool".to_string() } else { elem };
                    ClrName::Literal(format!("{elem}[]"))
                }
                ClrName::Custom { ty: elem, suffix } => ClrName::Custom { ty: elem, suffix: format!("{suffix}[]") },
            };
            Ok(Lowered { clr, slot: Slot::Handle, optional: false, inner: ty.clone(), path: false })
        }
        // Declared as the class's own CLR type, which `PsProxy<T>`'s
        // `PsTyped` impl answers, so the binder checks the object.
        "PsProxy" => {
            if arg.is_none() {
                return Err(syn::Error::new_spanned(ty, "PsProxy needs a type argument: the #[psclass(proxy)] type it holds"));
            }
            Ok(Lowered {
                clr: ClrName::Custom { ty: Box::new(ty.clone()), suffix: String::new() },
                slot: Slot::Handle,
                optional: false,
                inner: ty.clone(),
                path: false,
            })
        }
        _custom => {
            if arg.is_some() {
                return Err(syn::Error::new_spanned(
                    ty,
                    format!("`{name}` is not a supported type here; use a primitive, String, PathBuf, PsObject, Option<T>, Vec<T>, PsProxy<T>, or a type implementing PsTyped (a #[psenum] type, char, PsDateTime, PsTimeSpan, PsGuid, PsSecureString, PsCredential)"),
                ));
            }
            Ok(Lowered {
                clr: ClrName::Custom { ty: Box::new(ty.clone()), suffix: String::new() },
                slot: Slot::Handle,
                optional: false,
                inner: ty.clone(),
                path: false,
            })
        }
    }
}

/// Doc comment lines of an item, without the leading space rustdoc
/// keeps after `///`. Attributes other than `doc = "..."` are not
/// documentation and are skipped.
pub fn doc_lines(attrs: &[Attribute]) -> Vec<String> {
    let mut out = Vec::new();
    for a in attrs {
        let nv = if let Meta::NameValue(nv) = &a.meta { nv } else { continue };
        if !nv.path.is_ident("doc") {
            continue;
        }
        let text = if let Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) = &nv.value { s.value() } else { continue };
        let line = match text.strip_prefix(' ') {
            Some(rest) => rest.to_string(),
            None => text,
        };
        out.push(line);
    }
    out
}

/// Members every generated class inherits from `System.Object`. A
/// property or method of the same name hides one, which changes what
/// the engine and `$obj.GetType()` see.
pub const OBJECT_MEMBERS: &[&str] = &["Equals", "GetHashCode", "GetType", "ToString"];

/// Members a generated proxy class inherits from `Pwrs.ProxyBase`,
/// public or used by the generated code.
pub const PROXY_MEMBERS: &[&str] = &["Dispose", "IsDisposed", "PwrsGet", "PwrsCall"];

/// An error when `ps_name` would hide an inherited member.
pub fn check_reserved(span: &impl quote::ToTokens, what: &str, ps_name: &str, reserved: &[&[&str]]) -> Result<()> {
    for set in reserved {
        if set.contains(&ps_name) {
            return Err(syn::Error::new_spanned(
                span,
                format!("{what} is declared as `{ps_name}`, which hides a member the generated class inherits; rename it (reserved: {})", set.join(", ")),
            ));
        }
    }
    Ok(())
}

pub fn pascal(snake: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for ch in snake.chars() {
        if ch == '_' {
            up = true;
        } else if up {
            out.extend(ch.to_uppercase());
            up = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Registers custom types met while lowering and hands each its
/// sentinel index; the same type gets the same index.
#[derive(Default)]
pub struct Customs {
    types: Vec<Type>,
}

impl Customs {
    /// Writes the `clr` and `value_type` keys for `l` into `o`.
    pub fn clr_keys(&mut self, o: &mut crate::json::Obj, clr: &ClrName) {
        match clr {
            ClrName::Literal(s) => {
                o.str("clr", s).bool("value_type", false);
            }
            ClrName::Custom { ty, suffix } => {
                let i = self.index(ty);
                o.custom_clr("clr", i, suffix);
                if suffix.is_empty() {
                    o.custom_value_type("value_type", i);
                } else {
                    o.bool("value_type", false);
                }
            }
        }
    }

    fn index(&mut self, ty: &Type) -> usize {
        let existing = self.types.iter().position(|t| quote!(#t).to_string() == quote!(#ty).to_string());
        match existing {
            Some(i) => i,
            None => {
                self.types.push(ty.clone());
                self.types.len() - 1
            }
        }
    }

    /// Statements that assemble `json` into a `String` named `s` at
    /// run time, reading each custom type's CLR name and value-type
    /// flag from its `PsTyped` impl. A methods sentinel reads the
    /// `#[psmethods]` descriptor of `methods_of`, or is `[]` when no
    /// type is given.
    pub fn descriptor_stmts(&self, json: &str, methods_of: Option<&Ident>) -> TokenStream {
        let stmts = crate::json::pieces(json).into_iter().map(|p| match p {
            crate::json::Piece::Text(t) => quote!(s.push_str(#t);),
            crate::json::Piece::ClrName(i) => {
                let ty = self.types.get(i).unwrap_or_else(|| panic!("pwrs-macros referenced custom type {i} of {}", self.types.len()));
                quote!(s.push_str(&::pwrs::runtime::json_body(<#ty as ::pwrs::class::PsTyped>::CLR_NAME));)
            }
            crate::json::Piece::ValueType(i) => {
                let ty = self.types.get(i).unwrap_or_else(|| panic!("pwrs-macros referenced custom type {i} of {}", self.types.len()));
                quote!(s.push_str(if <#ty as ::pwrs::class::PsTyped>::VALUE_TYPE { "true" } else { "false" });)
            }
            crate::json::Piece::Methods => match methods_of {
                Some(ty) => quote!({
                    #[allow(unused_imports)]
                    use ::pwrs::class::{NoPsMethods as _, PsMethods as _};
                    s.push_str(&(&::pwrs::class::MethodsCollector::<#ty>::new()).methods_descriptor());
                }),
                None => quote!(s.push_str("[]");),
            },
        });
        quote!(#(#stmts)*)
    }

    /// A `fn descriptor() -> String` assembling `json` at run time; see
    /// [`Customs::descriptor_stmts`].
    pub fn descriptor_fn(&self, json: &str, methods_of: Option<&Ident>) -> TokenStream {
        let stmts = self.descriptor_stmts(json, methods_of);
        quote! {
            fn descriptor() -> ::std::string::String {
                let mut s = ::std::string::String::new();
                #stmts
                s
            }
        }
    }
}

/// The `bool` slot as a CLR output type is `bool`, not the parameter
/// type `SwitchParameter`.
pub fn output_clr(l: &Lowered) -> ClrName {
    match &l.clr {
        ClrName::Literal(s) if s == "SwitchParameter" => ClrName::Literal("bool".to_string()),
        other => other.clone(),
    }
}
