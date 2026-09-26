//! `#[cmdlet]` expansion: attribute grammar, parameter block layout,
//! descriptor JSON, and the generated `CmdletBind`.

use crate::json::{self, Obj};
use crate::types::{doc_lines, lower, pascal, ClrName, Customs, Lowered, Slot};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Attribute, Expr, ExprLit, Fields, Ident, ItemStruct, Lit, Meta, Path, Result, Token,
};

struct CmdletArgs {
    verb: String,
    noun: String,
    should_process: bool,
    confirm_impact: Option<String>,
    default_set: Option<String>,
    aliases: Vec<String>,
    output: Vec<String>,
}

pub fn lit_str(expr: &Expr) -> Result<String> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => Ok(s.value()),
        other => Err(syn::Error::new_spanned(other, "expected a string literal")),
    }
}

pub fn lit_int(expr: &Expr) -> Result<i64> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Int(i), .. }) => i.base10_parse(),
        Expr::Unary(u) => Ok(-lit_int(&u.expr)?),
        other => Err(syn::Error::new_spanned(other, "expected an integer literal")),
    }
}

pub fn str_list(expr: &Expr) -> Result<Vec<String>> {
    match expr {
        Expr::Array(a) => a.elems.iter().map(lit_str).collect(),
        single => Ok(vec![lit_str(single)?]),
    }
}

pub fn key_of(path: &Path) -> Result<String> {
    match path.get_ident() {
        Some(i) => Ok(i.to_string()),
        None => Err(syn::Error::new_spanned(path, "expected a single identifier key")),
    }
}

pub struct MetaList(pub Punctuated<Meta, Token![,]>);

impl Parse for MetaList {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(MetaList(Punctuated::parse_terminated(input)?))
    }
}

fn parse_cmdlet_args(attr: TokenStream) -> Result<CmdletArgs> {
    let list: MetaList = syn::parse2(attr)?;
    let mut verb = None;
    let mut noun = None;
    let mut should_process = false;
    let mut confirm_impact = None;
    let mut default_set = None;
    let mut aliases = Vec::new();
    let mut output = Vec::new();
    for m in list.0 {
        match m {
            Meta::Path(p) if p.is_ident("supports_should_process") => should_process = true,
            Meta::Path(p) => return Err(syn::Error::new_spanned(p, "unknown #[cmdlet] flag")),
            Meta::NameValue(nv) => {
                let key = key_of(&nv.path)?;
                match key.as_str() {
                    "verb" => verb = Some(lit_str(&nv.value)?),
                    "noun" => noun = Some(lit_str(&nv.value)?),
                    "confirm_impact" => confirm_impact = Some(lit_str(&nv.value)?),
                    "default_parameter_set" => default_set = Some(lit_str(&nv.value)?),
                    "alias" => aliases = str_list(&nv.value)?,
                    "output" => output = str_list(&nv.value)?,
                    unknown => return Err(syn::Error::new_spanned(nv.path, format!("unknown #[cmdlet] key `{unknown}`"))),
                }
            }
            Meta::List(l) => return Err(syn::Error::new_spanned(l, "unknown #[cmdlet] argument")),
        }
    }
    let span = proc_macro2::Span::call_site();
    let verb = verb.ok_or_else(|| syn::Error::new(span, "#[cmdlet] needs verb = \"...\""))?;
    let noun = noun.ok_or_else(|| syn::Error::new(span, "#[cmdlet] needs noun = \"...\""))?;
    Ok(CmdletArgs { verb, noun, should_process, confirm_impact, default_set, aliases, output })
}

#[derive(Default)]
struct ParamArgs {
    mandatory: bool,
    position: Option<i64>,
    /// The parameter sets the parameter belongs to; empty for every set.
    sets: Vec<String>,
    pipeline: bool,
    pipeline_by_name: bool,
    remaining: bool,
    aliases: Vec<String>,
    help: Option<String>,
    validate_set: Vec<String>,
    validate_range: Option<(i64, i64)>,
    validate_pattern: Option<String>,
    not_null_or_empty: bool,
    dont_show: bool,
    literal_path: bool,
    raw: bool,
    /// The CLR type a `PsObject` parameter is declared as, from `clr`.
    clr: Option<String>,
    /// `[AllowEmptyCollection]`: a mandatory collection takes an empty one.
    allow_empty_collection: bool,
}

fn parse_param_args(attr: &Attribute) -> Result<ParamArgs> {
    let mut a = ParamArgs::default();
    let list: Punctuated<Meta, Token![,]> = match &attr.meta {
        Meta::Path(bare) if bare.is_ident("param") => return Ok(a),
        Meta::Path(other) => return Err(syn::Error::new_spanned(other, "expected #[param] or #[param(...)]")),
        Meta::List(l) => l.parse_args_with(Punctuated::parse_terminated)?,
        Meta::NameValue(nv) => return Err(syn::Error::new_spanned(nv, "#[param] takes a parenthesized list")),
    };
    for m in list {
        match m {
            Meta::Path(p) => {
                let key = key_of(&p)?;
                match key.as_str() {
                    "mandatory" => a.mandatory = true,
                    "value_from_pipeline" => a.pipeline = true,
                    "value_from_pipeline_by_property_name" => a.pipeline_by_name = true,
                    "value_from_remaining" => a.remaining = true,
                    "validate_not_null_or_empty" => a.not_null_or_empty = true,
                    "dont_show" => a.dont_show = true,
                    "literal_path" => a.literal_path = true,
                    "raw" => a.raw = true,
                    "allow_empty_collection" => a.allow_empty_collection = true,
                    unknown => return Err(syn::Error::new_spanned(p, format!("unknown #[param] flag `{unknown}`"))),
                }
            }
            Meta::NameValue(nv) => {
                let key = key_of(&nv.path)?;
                match key.as_str() {
                    "position" => a.position = Some(lit_int(&nv.value)?),
                    "set" => a.sets = set_names(&nv.value)?,
                    "alias" => a.aliases = str_list(&nv.value)?,
                    "help" => a.help = Some(lit_str(&nv.value)?),
                    "validate_set" => a.validate_set = str_list(&nv.value)?,
                    "validate_pattern" => a.validate_pattern = Some(lit_str(&nv.value)?),
                    "clr" => a.clr = Some(clr_type_name(&nv.value)?),
                    unknown => return Err(syn::Error::new_spanned(nv.path, format!("unknown #[param] key `{unknown}`"))),
                }
            }
            Meta::List(l) if l.path.is_ident("validate_range") => {
                let exprs: Punctuated<Expr, Token![,]> = l.parse_args_with(Punctuated::parse_terminated)?;
                let v: Vec<i64> = exprs.iter().map(lit_int).collect::<Result<_>>()?;
                if v.len() != 2 {
                    return Err(syn::Error::new_spanned(l, "validate_range(min, max) takes two integers"));
                }
                a.validate_range = Some((v[0], v[1]));
            }
            Meta::List(l) => return Err(syn::Error::new_spanned(l, "unknown #[param] argument")),
        }
    }
    Ok(a)
}

/// The parameter sets `set = "Name"` or `set = ["A", "B"]` names: at
/// least one, none blank, and none twice, whatever the case of each.
fn set_names(expr: &Expr) -> Result<Vec<String>> {
    let names = str_list(expr)?;
    if names.is_empty() {
        return Err(syn::Error::new_spanned(expr, "set names at least one parameter set"));
    }
    for (i, name) in names.iter().enumerate() {
        if name.trim().is_empty() {
            return Err(syn::Error::new_spanned(expr, "a parameter set name cannot be blank"));
        }
        if names[..i].iter().any(|earlier| earlier.eq_ignore_ascii_case(name)) {
            return Err(syn::Error::new_spanned(expr, format!("set names the parameter set {name} twice")));
        }
    }
    Ok(names)
}

/// C# keywords for the types a Rust field spells itself: `clr` names a
/// type PWRS has no Rust spelling for.
const KEYWORD_TYPES: &[&str] = &[
    "bool", "byte", "sbyte", "short", "ushort", "int", "uint", "long", "ulong", "float", "double", "decimal", "char", "string", "object",
];

/// The type `clr = "byte[]"` names, as the shell writes it in C#: a
/// type name with its namespace, generic arguments and array ranks, and
/// not one of the keywords in `KEYWORD_TYPES`.
fn clr_type_name(expr: &Expr) -> Result<String> {
    let name = lit_str(expr)?;
    let well_formed = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "_.[]<>, ".contains(c));
    if !well_formed {
        return Err(syn::Error::new_spanned(expr, format!("clr names a CLR type as C# writes it, such as \"byte[]\" or \"System.IO.FileInfo\", and `{name}` is not one")));
    }
    if KEYWORD_TYPES.contains(&name.as_str()) {
        return Err(syn::Error::new_spanned(expr, format!("clr = \"{name}\" names a type a Rust field declares itself; use the Rust type instead of a PsObject")));
    }
    Ok(name)
}

struct ParamSpec {
    field: Ident,
    ps_name: String,
    lowered: Lowered,
    args: ParamArgs,
    help: String,
}

fn collect_params(fields: &mut Fields) -> Result<Vec<ParamSpec>> {
    let mut specs = Vec::new();
    for field in fields.iter_mut() {
        let attr_index = match field.attrs.iter().position(|a| a.path().is_ident("param")) {
            Some(i) => i,
            None => continue,
        };
        let attr = field.attrs.remove(attr_index);
        let args = parse_param_args(&attr)?;
        let ident = match &field.ident {
            Some(i) => i.clone(),
            None => return Err(syn::Error::new_spanned(&*field, "#[cmdlet] needs named fields")),
        };
        let lowered = lower(&field.ty)?;
        // `raw` only changes the declared CLR type, so the slot must
        // already be a handle: an inline slot has a fixed width the
        // block depends on.
        if args.raw && lowered.slot != Slot::Handle {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "#[param(raw)] needs a parameter that crosses as a handle (Vec<T>, PsObject, a class or an enum), not one that crosses inline",
            ));
        }
        if args.allow_empty_collection && lowered.slot != Slot::Handle {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "#[param(allow_empty_collection)] needs a collection parameter, one that crosses as a handle (Vec<T>, or a PsObject declared as an array with clr), not one that crosses inline",
            ));
        }
        // `clr` declares what the binder coerces to while the value still
        // crosses as a handle, which only a PsObject takes whatever it is.
        if args.clr.is_some() {
            if args.raw {
                return Err(syn::Error::new_spanned(&field.ty, "#[param(raw)] declares the parameter object and clr declares another type; use one"));
            }
            let declares_object = matches!(&lowered.clr, ClrName::Literal(name) if name == "object");
            if lowered.slot != Slot::Handle || !declares_object {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    "#[param(clr = \"...\")] declares the CLR type of a PsObject or Option<PsObject> parameter; this type declares its own",
                ));
            }
        }
        let help = match &args.help {
            Some(h) => h.clone(),
            None => doc_lines(&field.attrs).join(" ").trim().to_string(),
        };
        specs.push(ParamSpec { ps_name: pascal(&ident.to_string()), field: ident, lowered, args, help });
    }
    if specs.len() > 64 {
        return Err(syn::Error::new_spanned(fields, "a cmdlet may declare at most 64 parameters"));
    }
    Ok(specs)
}

fn param_json(i: usize, p: &ParamSpec, customs: &mut Customs) -> String {
    let range = match p.args.validate_range {
        Some((a, b)) => format!("[{a},{b}]"),
        None => "null".to_string(),
    };
    let mut o = Obj::new();
    o.str("name", &p.ps_name).str("rust", &p.field.to_string()).num("index", i as i64);
    // `raw` declares the property as object, so the engine's binder
    // hands the argument over untouched; the Rust side still converts
    // to the field's own type.
    // `clr` declares a PsObject as the type it names, which the binder
    // then coerces to, and the handle carries the coerced value as is.
    if p.args.raw {
        o.str("clr", "object").bool("value_type", false);
    } else if let Some(clr) = &p.args.clr {
        o.str("clr", clr).bool("value_type", false);
    } else {
        customs.clr_keys(&mut o, &p.lowered.clr);
    }
    o.str("slot", p.lowered.slot.name())
        .bool("optional", p.lowered.optional)
        .bool("mandatory", p.args.mandatory)
        .opt_num("position", p.args.position)
        .strs("sets", &p.args.sets)
        .bool("pipeline", p.args.pipeline)
        .bool("pipeline_by_name", p.args.pipeline_by_name)
        .bool("remaining", p.args.remaining)
        .strs("aliases", &p.args.aliases)
        .str("help", &p.help)
        .strs("validate_set", &p.args.validate_set)
        .raw("validate_range", &range)
        .opt_str("validate_pattern", p.args.validate_pattern.as_deref())
        .bool("not_null_or_empty", p.args.not_null_or_empty)
        .bool("dont_show", p.args.dont_show)
        .bool("literal_path", p.args.literal_path)
        .bool("raw", p.args.raw)
        .bool("allow_empty_collection", p.args.allow_empty_collection);
    o.finish()
}

/// Expression reading slot `i` of `block` into the Rust type.
pub fn read_expr(i: usize, l: &Lowered, block: &Ident) -> TokenStream {
    let slot = format_ident!("p{}", i);
    let bit = i as u64;
    let inner = &l.inner;
    let read = match l.slot {
        Slot::Bool => quote!(#block.#slot != 0),
        Slot::Str16 => quote!({
            let s = #block.#slot;
            ::pwrs::text::try_from_str16(s.ptr, s.len)?
        }),
        Slot::Handle => quote!({
            let h = #block.#slot;
            let owned = if h.is_null() {
                ::pwrs::PsObject::null()
            } else {
                ::pwrs::PsObject::from_raw((::pwrs::host::vtable().clone_handle)(h))
            };
            <#inner as ::pwrs::FromPs>::from_ps(&owned)?
        }),
        Slot::I8 | Slot::I16 | Slot::I32 | Slot::I64 | Slot::U8 | Slot::U16 | Slot::U32 | Slot::U64 | Slot::F32 | Slot::F64 => {
            quote!(#block.#slot)
        }
    };
    let read = if l.path { quote!(::std::path::PathBuf::from(#read)) } else { read };
    if l.optional {
        quote!(if #block.bound & (1u64 << #bit) != 0 { ::core::option::Option::Some(#read) } else { ::core::option::Option::None })
    } else {
        read
    }
}

pub fn expand_cmdlet(attr: TokenStream, mut input: ItemStruct) -> Result<TokenStream> {
    let args = parse_cmdlet_args(attr)?;
    let specs = collect_params(&mut input.fields)?;
    let name = input.ident.clone();
    let block_ident = format_ident!("__PwrsParams{}", name);
    let ps_name = format!("{}-{}", args.verb, args.noun);

    // Rustdoc's own split: the summary is the first paragraph, so the
    // synopsis is every line up to the first blank one joined into a
    // sentence, and the description is the rest as written.
    let docs = doc_lines(&input.attrs);
    let first_blank = docs.iter().position(|l| l.trim().is_empty()).unwrap_or(docs.len());
    let synopsis = docs[..first_blank].iter().map(|s| s.trim()).collect::<Vec<_>>().join(" ").trim().to_string();
    let description = match docs.get(first_blank + 1..) {
        Some(rest) => rest.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n").trim().to_string(),
        None => String::new(),
    };

    let mut customs = Customs::default();
    let params_json: Vec<String> = specs.iter().enumerate().map(|(i, p)| param_json(i, p, &mut customs)).collect();
    let mut o = Obj::new();
    o.str("verb", &args.verb)
        .str("noun", &args.noun)
        .str("name", &ps_name)
        .str("rust", &name.to_string())
        .bool("should_process", args.should_process)
        .opt_str("confirm_impact", args.confirm_impact.as_deref())
        .opt_str("default_set", args.default_set.as_deref())
        .strs("aliases", &args.aliases)
        .strs("output_types", &args.output)
        .str("synopsis", &synopsis)
        .str("description", &description)
        .raw("params", &json::array(&params_json));
    let descriptor_fn = customs.descriptor_fn(&o.finish(), None);

    let block_fields = specs.iter().enumerate().map(|(i, p)| {
        let slot = format_ident!("p{}", i);
        let ty = p.lowered.slot.rust_ty();
        quote!(pub #slot: #ty)
    });
    // A parameter the engine never assigned keeps its value: the
    // slot holds nothing worth reading, and a handle type's FromPs
    // is never handed a null for it.
    let block_var = format_ident!("block");
    let reads = specs.iter().enumerate().map(|(i, p)| {
        let field = &p.field;
        let bit = i as u64;
        let expr = read_expr(i, &p.lowered, &block_var);
        if p.lowered.optional {
            quote!(self.#field = #expr;)
        } else {
            quote!(if block.bound & (1u64 << #bit) != 0 { self.#field = #expr; })
        }
    });

    let vis = &input.vis;
    Ok(quote! {
        #input

        #[repr(C)]
        #[allow(non_camel_case_types, dead_code)]
        #vis struct #block_ident {
            /// Bit `i` set when parameter `i` was assigned since the
            /// previous phase. The runtime reads this word before the
            /// block's type is known, so it stays first.
            pub dirty: u64,
            /// Bit `i` set when parameter `i` was ever assigned.
            pub bound: u64,
            #(#block_fields,)*
        }

        impl ::pwrs::CmdletMeta for #name {
            const NAME: &'static str = #ps_name;
            #descriptor_fn
            fn phase_mask() -> &'static ::core::sync::atomic::AtomicU32 {
                static MASK: ::core::sync::atomic::AtomicU32 = ::core::sync::atomic::AtomicU32::new(::pwrs::sys::PS_PHASE_MASK_ALL);
                &MASK
            }
        }

        impl ::pwrs::cmdlet::CmdletBind for #name {
            unsafe fn bind(&mut self, params: *const ::core::ffi::c_void) -> ::pwrs::PsResult<()> {
                let block = &*(params as *const #block_ident);
                #(#reads)*
                ::pwrs::PsResult::<()>::Ok(())
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expr(text: &str) -> Expr {
        syn::parse_str::<Expr>(text).expect("an expression")
    }

    #[test]
    fn set_takes_one_name_or_several_and_refuses_a_repeat_or_a_blank() {
        assert_eq!(set_names(&expr("\"ByName\"")).expect("one set"), ["ByName"]);
        assert_eq!(set_names(&expr("[\"Path\", \"LiteralPath\"]")).expect("two sets"), ["Path", "LiteralPath"]);
        assert!(set_names(&expr("[\"Path\", \"path\"]")).is_err(), "a set named twice in another case");
        assert!(set_names(&expr("[]")).is_err(), "no set");
        assert!(set_names(&expr("\" \"")).is_err(), "a blank name");
    }

    #[test]
    fn clr_takes_a_type_name_and_refuses_a_keyword_or_stray_text() {
        assert_eq!(clr_type_name(&expr("\"byte[]\"")).expect("byte[]"), "byte[]");
        assert_eq!(clr_type_name(&expr("\"System.IO.FileInfo\"")).expect("FileInfo"), "System.IO.FileInfo");
        assert_eq!(clr_type_name(&expr("\"System.Collections.Generic.List<int>\"")).expect("a generic"), "System.Collections.Generic.List<int>");
        for refused in ["\"int\"", "\"object\"", "\"string\"", "\"byte[]; x\"", "\"\""] {
            assert!(clr_type_name(&expr(refused)).is_err(), "{refused} was accepted");
        }
    }
}
