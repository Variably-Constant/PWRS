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
    /// `[AllowEmptyString]`: a mandatory string takes an empty one.
    allow_empty_string: bool,
    /// `[ValidateLength(min, max)]`: each string has that many characters.
    validate_length: Option<(i64, i64)>,
    /// `[ValidateCount(min, max)]`: the array has that many elements.
    validate_count: Option<(i64, i64)>,
    /// `[Pwrs.ValidateScriptText]`: the script returns true for each element.
    validate_script: Option<String>,
    /// `[ValidateNotNull]`: the argument is not `$null`.
    validate_not_null: bool,
    /// `[AllowNull]`: a mandatory parameter takes `$null`.
    allow_null: bool,
    /// `[SupportsWildcards]`: help says the parameter takes wildcards.
    supports_wildcards: bool,
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
                    "allow_empty_string" => a.allow_empty_string = true,
                    "validate_not_null" => a.validate_not_null = true,
                    "allow_null" => a.allow_null = true,
                    "supports_wildcards" => a.supports_wildcards = true,
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
                    "validate_script" => {
                        let script = lit_str(&nv.value)?;
                        if script.trim().is_empty() {
                            return Err(syn::Error::new_spanned(&nv.value, "validate_script is a script that returns true for a value the parameter takes, so it cannot be blank"));
                        }
                        a.validate_script = Some(script);
                    }
                    "clr" => a.clr = Some(clr_type_name(&nv.value)?),
                    unknown => return Err(syn::Error::new_spanned(nv.path, format!("unknown #[param] key `{unknown}`"))),
                }
            }
            Meta::List(l) => {
                let key = key_of(&l.path)?;
                match key.as_str() {
                    "validate_range" => a.validate_range = Some(int_pair(&l, &key)?),
                    "validate_length" => a.validate_length = Some(count_bounds(&l, &key)?),
                    "validate_count" => a.validate_count = Some(count_bounds(&l, &key)?),
                    _ => return Err(syn::Error::new_spanned(l, "unknown #[param] argument")),
                }
            }
        }
    }
    Ok(a)
}

/// The two integers of `key(min, max)`.
fn int_pair(l: &syn::MetaList, key: &str) -> Result<(i64, i64)> {
    let exprs: Punctuated<Expr, Token![,]> = l.parse_args_with(Punctuated::parse_terminated)?;
    let v: Vec<i64> = exprs.iter().map(lit_int).collect::<Result<_>>()?;
    if v.len() != 2 {
        return Err(syn::Error::new_spanned(l, format!("{key}(min, max) takes two integers")));
    }
    Ok((v[0], v[1]))
}

/// The bounds of `validate_length(min, max)` or `validate_count(min,
/// max)`. PowerShell's attributes take 32-bit integers and refuse a min
/// below 0, a max below 1 and a max below the min.
fn count_bounds(l: &syn::MetaList, key: &str) -> Result<(i64, i64)> {
    let (min, max) = int_pair(l, key)?;
    if min < 0 || max < 1 || max < min || max > i64::from(i32::MAX) {
        return Err(syn::Error::new_spanned(
            l,
            format!("{key}(min, max) needs a min of 0 or more, and a max of at least 1, at least the min and at most {}", i32::MAX),
        ));
    }
    Ok((min, max))
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

/// The CLR type the shell declares for a parameter after `raw` and
/// `clr`, when the macro knows it. A class or enum type's name is read
/// from its trait when the descriptor is built, so it gives `None`.
fn declared_clr<'a>(args: &'a ParamArgs, lowered: &'a Lowered) -> Option<&'a str> {
    if args.raw {
        return Some("object");
    }
    if let Some(clr) = &args.clr {
        return Some(clr);
    }
    match &lowered.clr {
        ClrName::Literal(name) => Some(name),
        ClrName::Custom { .. } => None,
    }
}

/// Whether the shell declares the parameter as an array.
fn declares_array(args: &ParamArgs, lowered: &Lowered) -> bool {
    match declared_clr(args, lowered) {
        Some(clr) => clr.ends_with("[]"),
        None => matches!(&lowered.clr, ClrName::Custom { suffix, .. } if suffix.ends_with("[]")),
    }
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
                "#[param(raw)] needs a parameter that is passed as a handle (Vec<T>, PsObject, a class or an enum), not one passed inline",
            ));
        }
        if args.allow_empty_collection && lowered.slot != Slot::Handle {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "#[param(allow_empty_collection)] needs a collection parameter, one that is passed as a handle (Vec<T>, or a PsObject declared as an array with clr), not one passed inline",
            ));
        }
        if args.allow_empty_string && lowered.slot != Slot::Str16 {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "#[param(allow_empty_string)] needs a string parameter (String, PathBuf, or an Option of either)",
            ));
        }
        if args.validate_length.is_some() && !matches!(declared_clr(&args, &lowered), Some("string" | "string[]" | "System.String" | "System.String[]")) {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "#[param(validate_length(...))] needs a string parameter (String, PathBuf, Vec<String>, or an Option of one), since PowerShell's ValidateLength refuses anything but a string",
            ));
        }
        if args.validate_count.is_some() && !declares_array(&args, &lowered) {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "#[param(validate_count(...))] needs an array parameter (Vec<T>, or a PsObject declared as an array with clr)",
            ));
        }
        // A number or bool is declared as a CLR value type even inside an
        // Option, and the binder cannot convert $null to one; a class or
        // enum type is checked by cargo pwrs build, which reads its kind.
        let inline_value = matches!(
            lowered.slot,
            Slot::Bool | Slot::I8 | Slot::I16 | Slot::I32 | Slot::I64 | Slot::U8 | Slot::U16 | Slot::U32 | Slot::U64 | Slot::F32 | Slot::F64
        );
        if (args.validate_not_null || args.allow_null) && inline_value {
            let key = if args.allow_null { "allow_null" } else { "validate_not_null" };
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("#[param({key})] needs a parameter that can be null, such as a String, a PsObject or a Vec; a number or bool cannot be null, even inside an Option"),
            ));
        }
        if args.validate_not_null && args.allow_null {
            return Err(syn::Error::new_spanned(&field.ty, "#[param(validate_not_null)] refuses $null and #[param(allow_null)] takes it; declare one"));
        }
        // `clr` declares what the binder converts to while the value is
        // still passed as a handle, which only a PsObject takes whatever
        // it is.
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
    Ok(specs)
}

/// The 64-bit words a mask of `n` items takes, one bit each: never fewer
/// than one, so a block of 64 items or fewer has the one word it always
/// had.
pub fn mask_words(n: usize) -> usize {
    n.div_ceil(64).max(1)
}

/// The word of a mask that item `i`'s bit is in, and the bit in that
/// word.
pub fn mask_bit(i: usize) -> (usize, u64) {
    (i / 64, (i % 64) as u64)
}

/// `text` without its whitespace, so a test finds a piece of an expansion
/// however its tokens are spaced when printed.
#[cfg(test)]
pub(crate) fn squeezed(text: &str) -> String {
    text.split_whitespace().collect()
}

/// `[min,max]` as JSON, or `null`.
fn pair_json(pair: Option<(i64, i64)>) -> String {
    match pair {
        Some((a, b)) => format!("[{a},{b}]"),
        None => "null".to_string(),
    }
}

fn param_json(i: usize, p: &ParamSpec, customs: &mut Customs) -> String {
    let mut o = Obj::new();
    o.str("name", &p.ps_name).str("rust", &p.field.to_string()).num("index", i as i64);
    // `raw` declares the property as object, so the engine's binder
    // passes the argument on untouched; the Rust side still converts
    // to the field's own type.
    // `clr` declares a PsObject as the type it names, which the binder
    // then converts to, and the handle points at the converted value as
    // it is.
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
        .raw("validate_range", &pair_json(p.args.validate_range))
        .opt_str("validate_pattern", p.args.validate_pattern.as_deref())
        .bool("not_null_or_empty", p.args.not_null_or_empty)
        .bool("dont_show", p.args.dont_show)
        .bool("literal_path", p.args.literal_path)
        .bool("raw", p.args.raw)
        .bool("allow_empty_collection", p.args.allow_empty_collection)
        .bool("allow_empty_string", p.args.allow_empty_string)
        .raw("validate_length", &pair_json(p.args.validate_length))
        .raw("validate_count", &pair_json(p.args.validate_count))
        .opt_str("validate_script", p.args.validate_script.as_deref())
        .bool("validate_not_null", p.args.validate_not_null)
        .bool("allow_null", p.args.allow_null)
        .bool("supports_wildcards", p.args.supports_wildcards);
    o.finish()
}

/// Expression reading slot `i` of `block` into the Rust type.
pub fn read_expr(i: usize, l: &Lowered, block: &Ident) -> TokenStream {
    let slot = format_ident!("p{}", i);
    let (word, bit) = mask_bit(i);
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
        // The shell enters the peer's gate for the call and refuses a null
        // one before it, so the pointer is a live value of the class.
        Slot::Peer => quote!({
            let p = #block.#slot;
            if p.is_null() {
                return Err(::pwrs::PsError::new(
                    ::pwrs::ErrorCategory::InvalidArgument,
                    "PwrsNullObject",
                    format!("a {} argument cannot be $null", <#inner as ::pwrs::class::PsTyped>::CLR_NAME),
                ));
            }
            &*(p as *const #inner)
        }),
        Slot::Task => quote!(<#inner>::from_slot(#block.#slot)),
    };
    let read = if l.path { quote!(::std::path::PathBuf::from(#read)) } else { read };
    // A task slot always has the task the shell made: there `optional`
    // makes the task's result nullable, which only the CLR declaration
    // reads.
    // A string given as $null comes as a null pointer, which an Option
    // reads as None, as it reads a parameter that was not given.
    let given = if l.slot == Slot::Str16 { quote!(&& !#block.#slot.ptr.is_null()) } else { quote!() };
    if l.optional && l.slot != Slot::Task {
        quote!(if #block.bound[#word] & (1u64 << #bit) != 0 #given { ::core::option::Option::Some(#read) } else { ::core::option::Option::None })
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
    // The field of a parameter the engine never assigned does not change: the
    // slot has nothing worth reading, and a handle type's FromPs
    // is never given a null for it.
    let block_var = format_ident!("block");
    let reads = specs.iter().enumerate().map(|(i, p)| {
        let field = &p.field;
        let (word, bit) = mask_bit(i);
        let expr = read_expr(i, &p.lowered, &block_var);
        if p.lowered.optional {
            quote!(self.#field = #expr;)
        } else {
            quote!(if block.bound[#word] & (1u64 << #bit) != 0 { self.#field = #expr; })
        }
    });
    let words = mask_words(specs.len());

    let vis = &input.vis;
    Ok(quote! {
        #input

        #[repr(C)]
        #[allow(non_camel_case_types, dead_code)]
        #vis struct #block_ident {
            /// Bit `i % 64` of word `i / 64` set when parameter `i` was
            /// assigned since the previous phase. The runtime reads these
            /// words without the block's type, so they stay first.
            pub dirty: [u64; #words],
            /// Bit `i % 64` of word `i / 64` set when parameter `i` was
            /// ever assigned.
            pub bound: [u64; #words],
            #(#block_fields,)*
        }

        impl ::pwrs::CmdletMeta for #name {
            const NAME: &'static str = #ps_name;
            const MASK_WORDS: usize = #words;
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

    /// The parameters of a struct with one field of type `ty` under
    /// `#[param(allow_empty_string)]`.
    fn allow_empty_string_on(ty: &str) -> Result<Vec<ParamSpec>> {
        let text = format!("struct S {{ #[param(mandatory, allow_empty_string)] pub input_object: {ty} }}");
        let mut item = syn::parse_str::<syn::ItemStruct>(&text).expect("a struct");
        collect_params(&mut item.fields)
    }

    #[test]
    fn allow_empty_string_takes_a_string_parameter_and_refuses_any_other() {
        for accepted in ["String", "Option<String>", "std::path::PathBuf"] {
            let specs = allow_empty_string_on(accepted).unwrap_or_else(|e| panic!("{accepted} was refused: {e}"));
            assert!(specs[0].args.allow_empty_string, "{accepted} lost the flag");
        }
        for refused in ["i32", "bool", "Vec<String>", "PsObject"] {
            match allow_empty_string_on(refused) {
                Ok(_) => panic!("{refused} was accepted"),
                Err(e) => assert!(e.to_string().contains("#[param(allow_empty_string)] needs a string parameter"), "{refused}: {e}"),
            }
        }
    }

    /// The parameters of a struct with one field of type `ty` under
    /// `#[param(<args>)]`.
    fn param_on(args: &str, ty: &str) -> Result<Vec<ParamSpec>> {
        let text = format!("struct S {{ #[param({args})] pub input_object: {ty} }}");
        let mut item = syn::parse_str::<syn::ItemStruct>(&text).expect("a struct");
        collect_params(&mut item.fields)
    }

    /// The error a declaration is refused with; a panic when it is
    /// accepted.
    fn refusal(args: &str, ty: &str) -> String {
        match param_on(args, ty) {
            Ok(_) => panic!("#[param({args})] on {ty} was accepted"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn validate_length_takes_the_bounds_powershell_accepts_on_a_string_parameter() {
        for ty in ["String", "Option<String>", "std::path::PathBuf", "Vec<String>"] {
            let specs = param_on("validate_length(1, 8)", ty).unwrap_or_else(|e| panic!("{ty} was refused: {e}"));
            assert_eq!(specs[0].args.validate_length, Some((1, 8)), "{ty}");
        }
        let widest = param_on("validate_length(0, 2147483647)", "String").expect("the widest bounds");
        assert_eq!(widest[0].args.validate_length, Some((0, i64::from(i32::MAX))));
        for bounds in ["(0, 0)", "(-1, 5)", "(3, 2)", "(0, 2147483648)", "(1)"] {
            assert!(refusal(&format!("validate_length{bounds}"), "String").contains("validate_length(min, max)"), "{bounds}");
        }
        for ty in ["i32", "PsObject", "Vec<i64>"] {
            assert!(refusal("validate_length(1, 8)", ty).contains("needs a string parameter"), "{ty}");
        }
    }

    #[test]
    fn validate_count_takes_the_bounds_powershell_accepts_on_an_array_parameter() {
        for (args, ty) in [("validate_count(1, 3)", "Vec<String>"), ("validate_count(1, 3)", "Option<Vec<i64>>"), ("validate_count(1, 3), clr = \"byte[]\"", "PsObject")] {
            let specs = param_on(args, ty).unwrap_or_else(|e| panic!("{args} on {ty} was refused: {e}"));
            assert_eq!(specs[0].args.validate_count, Some((1, 3)), "{args} on {ty}");
        }
        assert!(refusal("validate_count(2, 1)", "Vec<String>").contains("validate_count(min, max)"));
        for ty in ["String", "PsObject", "i64"] {
            assert!(refusal("validate_count(1, 3)", ty).contains("needs an array parameter"), "{ty}");
        }
    }

    #[test]
    fn validate_script_stores_the_script_text_and_refuses_a_blank_one() {
        let specs = param_on("validate_script = \"$_ % 2 -eq 0\"", "i64").expect("a script on a number");
        assert_eq!(specs[0].args.validate_script.as_deref(), Some("$_ % 2 -eq 0"));
        assert!(refusal("validate_script = \" \"", "i64").contains("cannot be blank"));
    }

    #[test]
    fn validate_not_null_and_allow_null_need_a_parameter_that_can_be_null_and_refuse_each_other() {
        for key in ["validate_not_null", "allow_null"] {
            for ty in ["String", "Option<String>", "PsObject", "Option<PsObject>", "Vec<i64>"] {
                let specs = param_on(key, ty).unwrap_or_else(|e| panic!("{key} on {ty} was refused: {e}"));
                assert!(specs[0].args.validate_not_null != specs[0].args.allow_null, "{key} on {ty} lost the flag");
            }
            for ty in ["i32", "Option<i64>", "bool", "f64"] {
                assert!(refusal(key, ty).contains("needs a parameter that can be null"), "{key} on {ty}");
            }
        }
        assert!(refusal("validate_not_null, allow_null", "PsObject").contains("declare one"));
    }

    #[test]
    fn supports_wildcards_is_a_flag_on_any_parameter() {
        for ty in ["String", "Vec<String>", "PsObject"] {
            let specs = param_on("supports_wildcards", ty).unwrap_or_else(|e| panic!("{ty} was refused: {e}"));
            assert!(specs[0].args.supports_wildcards, "{ty}");
        }
    }

    /// `#[cmdlet]` on a struct of `n` parameters, `a0` to `a{n-1}`, the odd
    /// ones optional, printed without whitespace.
    fn cmdlet_of(n: usize) -> String {
        let fields: Vec<String> = (0..n)
            .map(|i| if i % 2 == 0 { format!("#[param] pub a{i}: i64") } else { format!("#[param] pub a{i}: Option<i64>") })
            .collect();
        let item = syn::parse_str::<ItemStruct>(&format!("struct Wide {{ {} }}", fields.join(", "))).expect("a struct");
        let attr: TokenStream = "verb = \"Measure\", noun = \"Wide\"".parse().expect("tokens");
        match expand_cmdlet(attr, item) {
            Ok(tokens) => squeezed(&tokens.to_string()),
            Err(e) => panic!("{n} parameters were refused: {e}"),
        }
    }

    #[test]
    fn each_mask_of_a_parameter_block_takes_one_word_per_64_parameters() {
        for (n, words) in [(0, 1), (1, 1), (64, 1), (65, 2), (128, 2), (129, 3)] {
            let out = cmdlet_of(n);
            for piece in [format!("pub dirty: [u64; {words}usize],"), format!("pub bound: [u64; {words}usize],"), format!("const MASK_WORDS: usize = {words}usize;")] {
                assert!(out.contains(&squeezed(&piece)), "{n} parameters: no `{piece}` in {out}");
            }
        }
    }

    #[test]
    fn a_parameter_past_the_first_64_is_read_from_its_own_word() {
        let out = cmdlet_of(130);
        for (i, word, bit) in [(0, 0, 0), (63, 0, 63), (64, 1, 0), (65, 1, 1), (127, 1, 63), (128, 2, 0), (129, 2, 1)] {
            let given = format!("block.bound[{word}usize] & (1u64 << {bit}u64) != 0");
            let read = if i % 2 == 0 {
                format!("if {given} {{ self.a{i} = block.p{i}; }}")
            } else {
                format!("self.a{i} = if {given} {{ ::core::option::Option::Some(block.p{i}) }} else {{ ::core::option::Option::None }};")
            };
            assert!(out.contains(&squeezed(&read)), "parameter {i}: no `{read}` in {out}");
        }
    }
}
