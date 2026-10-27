//! A proxy class's .NET base class and interfaces, for a class that names
//! either or whose methods back members. Each `#[member]` method is
//! matched against what the reference assemblies say of the base and the
//! interfaces on each target framework, and written as the override or
//! the interface implementation the shell declares; so are the base
//! constructors the class's two constructors call, and the way it frees
//! its Rust value on Dispose. What the shell cannot declare is refused
//! here, naming the class, the method and the member, before any C# is
//! compiled.

use std::collections::BTreeMap;

use crate::descriptor::{Class, Method, MethodParam, MethodRet, Module};
use crate::generate::{cs_name, is_reference_type, read_boxed};
use crate::members::{Constructor, Member, Param, Read, TypeInfo};
use crate::Error;

/// The two target frameworks, by their folder names. `#if NET` selects
/// code for the first.
pub const TARGETS: [&str; 2] = ["net10.0", "netstandard2.0"];

/// The class a class without a base of its own derives from, through
/// Pwrs.ProxyBase, and whose virtual members its methods may override.
const OBJECT: &str = "System.Object";

/// The classes C# lets no class derive from.
const SPECIAL: &[&str] = &["System.Array", "System.Delegate", "System.MulticastDelegate", "System.Enum", "System.ValueType"];

/// Whether a class names a base or interfaces, or has a method marked
/// `#[member]` or `#[base]`.
fn names_any(c: &Class) -> bool {
    c.base.is_some() || !c.implements.is_empty() || c.methods.iter().any(|m| m.member.is_some() || m.base || m.base_args.is_some())
}

/// The .NET types the module's classes need read: each proxy class's base
/// and interfaces, and System.Object for a class on Pwrs.ProxyBase whose
/// methods back members. Empty when no class names any.
pub fn types_to_read(module: &Module) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in module.classes.iter().filter(|c| c.mode == "proxy" && names_any(c)) {
        let base = match &c.base {
            Some(base) => base.clone(),
            None => OBJECT.to_string(),
        };
        for t in std::iter::once(base).chain(c.implements.iter().cloned()) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

/// What the shell declares for one class beyond what its descriptor says.
#[derive(Debug, Default)]
pub struct ClassPlan {
    /// The base class's C# name, with the `global::` prefix; `None` for a
    /// class on Pwrs.ProxyBase.
    pub base: Option<String>,
    /// The interfaces the class lists after its base and the ones its
    /// roles make: those `implements` names, then IDisposable where the
    /// class adds it, each with the `global::` prefix.
    pub interfaces: Vec<String>,
    /// The overrides and interface implementations, as C#.
    pub members: String,
    /// For a class with a base: the `base(...)` call of the constructor
    /// `new` makes, for each target, when the class has `new`.
    pub new_base: Option<(String, String)>,
    /// For a class with a base: the internal constructor Rust's factory
    /// calls, with what it needs, as C#.
    pub factory: String,
    /// For a class with a base: Dispose and IsDisposed, as C#.
    pub dispose: String,
}

impl ClassPlan {
    /// Whether the class is declared on its own base, with its
    /// Pwrs.ProxyCore in a field, rather than on Pwrs.ProxyBase.
    pub fn has_base(&self) -> bool {
        self.base.is_some()
    }
}

/// Whether method `m` is declared as a private helper that a member
/// calls, rather than as a method of its own name.
pub fn is_helper(m: &Method) -> bool {
    m.member.is_some()
}

fn refuse(c: &Class, text: impl std::fmt::Display) -> Error {
    Error::msg(format!("class {}: {text}", c.name))
}

/// The plan of each class that names a base, interfaces or members, by
/// class id. `read` is what `members::read` found for
/// [`types_to_read`], and is `None` only when that is empty; `native` is
/// the C# path of the module's `Pwrs.NativeModule`.
pub fn plan(module: &Module, read: Option<&Read>, native: &str) -> Result<BTreeMap<u32, ClassPlan>, Error> {
    let mut plans = BTreeMap::new();
    for c in module.classes.iter().filter(|c| names_any(c)) {
        if c.mode != "proxy" {
            return Err(refuse(c, format!("a base class, interfaces, #[member] and #[base] apply to a proxy class, and this one is {} mode", c.mode)));
        }
        let read = match read {
            Some(read) => read,
            None => return Err(refuse(c, "its base and interfaces were not read")),
        };
        plans.insert(c.id, plan_class(c, read, native)?);
    }
    Ok(plans)
}

/// The class's name without its namespace, as its C# declaration and
/// constructors spell it.
fn short_name(full: &str) -> &str {
    match full.rfind('.') {
        Some(i) => &full[i + 1..],
        None => full,
    }
}

/// The method a role or the view names, by PowerShell name.
fn named<'a>(c: &'a Class, name: &Option<String>) -> Option<&'a Method> {
    let name = name.as_ref()?;
    c.methods.iter().find(|m| &m.name == name)
}

fn plan_class(c: &Class, read: &Read, native: &str) -> Result<ClassPlan, Error> {
    for m in &c.methods {
        if m.base_args.is_some() && c.base.is_none() {
            return Err(refuse(c, format!("{}'s #[base(...)] names arguments for a base class's constructor, and the class names no base", m.rust)));
        }
        if m.base && c.base.is_none() {
            return Err(refuse(c, format!("{} is marked #[base], which returns the arguments of a base class's constructor, and the class names no base", m.rust)));
        }
        if let Some(spec) = &m.member
            && split_spec(spec).1 == "Dispose"
        {
            return Err(refuse(c, format!("{} is marked #[member = \"{spec}\"], and Dispose frees the class's Rust value; free what the value owns in its Drop", m.rust)));
        }
    }
    let uses = [
        ("view", &c.view),
        ("count", &c.count),
        ("item", &c.item),
        ("set_item", &c.set_item),
        ("next", &c.next),
        ("compare", &c.compare),
        ("equals", &c.equals),
        ("hash", &c.hash),
        ("add", &c.add),
        ("sub", &c.sub),
        ("mul", &c.mul),
        ("div", &c.div),
        ("rem", &c.rem),
    ];
    for (key, name) in uses {
        if let Some(m) = named(c, name)
            && (m.member.is_some() || m.base)
        {
            let mark = if m.base { "#[base]" } else { "#[member]" };
            return Err(refuse(c, format!("{key} names {}, which is marked {mark}, so the class declares no method of its name for {key} to call", m.rust)));
        }
    }
    let mut targets = Vec::new();
    for target in TARGETS {
        let types = match read.targets.get(target) {
            Some(types) => types,
            None => return Err(refuse(c, format!("the types it names were not read for {target}"))),
        };
        targets.push(plan_target(c, target, types, native)?);
    }
    let mut plan = merge(c, targets)?;
    let calls = format!("{AS_HELPER}<");
    let in_new = match &plan.new_base {
        Some((net, standard)) => net.contains(&calls) || standard.contains(&calls),
        None => false,
    };
    if in_new || plan.members.contains(&calls) || plan.factory.contains(&calls) {
        plan.members.push('\n');
        plan.members.push_str(&as_declaration());
    }
    Ok(plan)
}

/// A C# type as matching compares it: its name with no `global::` and no
/// spaces, whether it is a value type, and, for a value type, whether it
/// is the nullable `T?`, whose name is then `T`'s.
#[derive(Clone, Debug, PartialEq)]
struct Ty {
    name: String,
    value: bool,
    nullable: bool,
}

impl Ty {
    fn show(&self) -> String {
        if self.nullable { format!("{}?", self.name) } else { self.name.clone() }
    }
}

fn norm(s: &str) -> String {
    s.replace("global::", "").chars().filter(|c| !c.is_whitespace()).collect()
}

/// A type as the reference assemblies name it.
fn ty_of(text: &str, value: bool) -> Ty {
    let n = norm(text);
    match n.strip_suffix('?') {
        Some(inner) if value => Ty { name: inner.to_string(), value: true, nullable: true },
        _plain => Ty { name: n, value, nullable: false },
    }
}

/// The C# type of a Rust method's argument, as its generated method
/// declares it. An optional string is declared `object`.
fn ty_of_param(p: &MethodParam) -> Ty {
    if p.slot == "str16" && p.optional {
        return Ty { name: "object".to_string(), value: false, nullable: false };
    }
    let value = !is_reference_type(&p.clr, p.value_type);
    Ty { name: norm(&p.clr), value, nullable: value && p.optional }
}

/// The C# type of what a Rust method returns.
fn ty_of_ret(r: &MethodRet) -> Ty {
    let value = !is_reference_type(&r.clr, r.value_type);
    Ty { name: norm(&r.clr), value, nullable: value && r.optional }
}

/// C#'s implicit numeric conversions: whether a `from` converts to a `to`
/// with no cast.
fn widens(from: &str, to: &str) -> bool {
    let wider: &[&str] = match from {
        "sbyte" => &["short", "int", "long", "float", "double", "decimal", "nint"],
        "byte" => &["short", "ushort", "int", "uint", "long", "ulong", "float", "double", "decimal", "nint", "nuint"],
        "short" => &["int", "long", "float", "double", "decimal", "nint"],
        "ushort" => &["int", "uint", "long", "ulong", "float", "double", "decimal", "nint", "nuint"],
        "int" => &["long", "float", "double", "decimal", "nint"],
        "uint" => &["long", "ulong", "float", "double", "decimal", "nuint"],
        "long" | "ulong" | "nint" => &["float", "double", "decimal"],
        "nuint" => &["ulong", "float", "double", "decimal"],
        "char" => &["ushort", "int", "uint", "long", "ulong", "float", "double", "decimal", "nint", "nuint"],
        "float" => &["double"],
        _none => &[],
    };
    wider.contains(&to)
}

/// How a value reaches a place of another type.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Conv {
    /// The types are the same.
    Same,
    /// By C#'s implicit conversion: a wider number, an object, a nullable
    /// value.
    Implicit,
    /// An object where a reference type is declared, converted as
    /// PowerShell converts it when the call runs.
    FromObject,
}

/// How a `from` reaches a `to`, or `None` when it does not. `FromObject`
/// is only for a value Rust gives, a result or a base constructor's
/// argument; an argument a member receives must convert implicitly.
fn conv(from: &Ty, to: &Ty) -> Option<Conv> {
    if from.name == to.name {
        return match (from.nullable, to.nullable) {
            (true, false) => None,
            (false, true) => Some(Conv::Implicit),
            _same => Some(Conv::Same),
        };
    }
    if to.name == "object" && !to.nullable {
        return Some(Conv::Implicit);
    }
    if widens(&from.name, &to.name) && (!from.nullable || to.nullable) {
        return Some(Conv::Implicit);
    }
    if from.name == "object" && !to.value {
        return Some(Conv::FromObject);
    }
    None
}

/// A C# type as metadata names it, with the `global::` prefix on every
/// dotted name in it, so it reads the same inside any namespace.
fn global_type(t: &str) -> String {
    fn flush(word: &mut String, out: &mut String) {
        if word.contains('.') {
            out.push_str("global::");
        }
        out.push_str(word);
        word.clear();
    }
    let mut out = String::new();
    let mut word = String::new();
    for ch in t.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '.' {
            word.push(ch);
        } else {
            flush(&mut word, &mut out);
            out.push(ch);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// The member name a `#[member]` gives, and the declaring type before it
/// when it names one: the last dot outside angle brackets splits them.
fn split_spec(spec: &str) -> (Option<String>, String) {
    let mut depth = 0i32;
    let mut at = None;
    for (i, ch) in spec.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth -= 1,
            '.' if depth == 0 => at = Some(i),
            _other => {}
        }
    }
    match at {
        Some(i) => (Some(norm(&spec[..i])), spec[i + 1..].to_string()),
        None => (None, spec.to_string()),
    }
}

/// The C# keyword of a type that has one, from its full name, as the
/// reference assemblies name a member's declaring type.
fn keyword(full: &str) -> &str {
    match full {
        "System.Object" => "object",
        "System.String" => "string",
        "System.Boolean" => "bool",
        "System.Char" => "char",
        "System.SByte" => "sbyte",
        "System.Byte" => "byte",
        "System.Int16" => "short",
        "System.UInt16" => "ushort",
        "System.Int32" => "int",
        "System.UInt32" => "uint",
        "System.Int64" => "long",
        "System.UInt64" => "ulong",
        "System.Single" => "float",
        "System.Double" => "double",
        "System.Decimal" => "decimal",
        other => other,
    }
}

/// `name` with each type in it that C# has a keyword for named by the
/// keyword, as `IComparer<System.Int64>` becomes `IComparer<long>`.
fn keywords_in(name: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    for ch in name.chars().chain(std::iter::once(' ')) {
        if ch.is_alphanumeric() || ch == '_' || ch == '.' {
            word.push(ch);
            continue;
        }
        out.push_str(keyword(&word));
        word.clear();
        if ch != ' ' {
            out.push(ch);
        }
    }
    out
}

/// Whether a member declared by `from` is the one a qualifier names: its
/// whole name, or its last dotted part, with any type arguments.
fn qualifies(from: &str, qualifier: &str) -> bool {
    let from = keywords_in(&norm(from));
    let qualifier = keywords_in(qualifier);
    from == qualifier || from.ends_with(&format!(".{qualifier}"))
}

/// A member a method may back: one of the base chain's, or one of an
/// interface's, with that interface's C# name.
#[derive(Clone, Copy)]
struct Candidate<'a> {
    member: &'a Member,
    interface: Option<&'a str>,
}

impl Candidate<'_> {
    /// The member as messages name it.
    fn describe(&self) -> String {
        let m = self.member;
        let params: Vec<String> = m.params.iter().map(|p| format!("{} {}", p.ty, p.name)).collect();
        match m.kind.as_str() {
            "property" if params.is_empty() => format!("the property {}.{}", m.from, m.name),
            "property" => format!("the indexer {}[{}]", m.from, params.join(", ")),
            "method" => format!("{}.{}({})", m.from, m.name, params.join(", ")),
            other => format!("the {other} {}.{}", m.from, m.name),
        }
    }

    /// What identifies the member, the same on every target that has it.
    fn key(&self) -> String {
        let m = self.member;
        let params: Vec<String> = m.params.iter().map(|p| norm(&p.ty)).collect();
        format!("{}.{}({})", norm(&m.from), m.name, params.join(","))
    }
}

/// What part of a member a method backs.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Method,
    Get,
    Set,
}

impl Part {
    fn word(self) -> &'static str {
        match self {
            Part::Method => "the method",
            Part::Get => "the getter",
            Part::Set => "the setter",
        }
    }
}

/// A method matched to the member it backs on one target.
#[derive(Clone, Copy)]
struct Matched<'a> {
    method: &'a Method,
    candidate: Candidate<'a>,
    part: Part,
    /// How the method's result reaches the member's type, for a method or
    /// a getter that returns one.
    ret: Option<Conv>,
}

/// Refuses a slot no method of a derived class can fill.
fn overridable(slot: &str, interface: bool) -> Result<(), String> {
    match (interface, slot) {
        (true, "abstract" | "default") | (false, "abstract" | "virtual") => Ok(()),
        (_, "static" | "static-abstract") => Err("it is static".to_string()),
        (false, "sealed") => Err("it is sealed, so no class derived from its declaring type can override it".to_string()),
        _other => Err("it is not virtual, so it cannot be overridden".to_string()),
    }
}

/// Refuses parameters a Rust argument cannot stand for, and checks each
/// converts implicitly to the Rust argument at its place.
fn args_fit(params: &[Param], rust: &[Ty], m: &Method) -> Result<(), String> {
    if params.len() != rust.len() {
        return Err(format!("it takes {} argument{}, and {} takes {}", params.len(), if params.len() == 1 { "" } else { "s" }, m.rust, rust.len()));
    }
    for (p, r) in params.iter().zip(rust) {
        if !p.mode.is_empty() {
            return Err(format!("its argument {} is passed as {}, which no Rust argument can be", p.name, p.mode));
        }
        if p.type_unsupported {
            return Err(format!("its argument {} is {}, which no Rust type stands for", p.name, p.ty));
        }
        match conv(&ty_of(&p.ty, p.type_value), r) {
            Some(Conv::Same | Conv::Implicit) => {}
            _none => return Err(format!("its argument {} is {}, which does not convert to {}'s {}", p.name, p.ty, m.rust, r.show())),
        }
    }
    Ok(())
}

/// Whether `m` fits the candidate, and as what part of it.
fn fit(m: &Method, cand: &Candidate<'_>) -> Result<(Part, Option<Conv>), String> {
    let member = cand.member;
    let interface = cand.interface.is_some();
    let rust: Vec<Ty> = m.params.iter().map(ty_of_param).collect();
    let rust_ret = m.ret.as_ref().map(ty_of_ret);
    match member.kind.as_str() {
        "method" => {
            overridable(&member.slot, interface)?;
            if member.generic > 0 {
                return Err("it is generic, which no Rust method can back".to_string());
            }
            if member.returns_ref || member.returns_unsupported {
                return Err(format!("it returns {}, which no Rust type stands for", member.returns));
            }
            args_fit(&member.params, &rust, m)?;
            let ret = match (member.returns.as_str(), &rust_ret) {
                ("void", None) => None,
                ("void", Some(r)) => return Err(format!("it returns nothing, and {} returns {}", m.rust, r.show())),
                (returns, None) => return Err(format!("it returns {returns}, and {} returns nothing", m.rust)),
                (returns, Some(r)) => match conv(r, &ty_of(returns, member.returns_value)) {
                    Some(c) => Some(c),
                    None => return Err(format!("it returns {returns}, and {} returns {}, which does not convert to it", m.rust, r.show())),
                },
            };
            Ok((Part::Method, ret))
        }
        "property" => {
            if member.type_unsupported {
                return Err(format!("its type is {}, which no Rust type stands for", member.ty));
            }
            let index = member.params.len();
            let prop = ty_of(&member.ty, member.type_value);
            if rust.len() == index
                && let Some(r) = &rust_ret
            {
                let get = match &member.get {
                    Some(get) => get,
                    None => return Err("it has no getter the class can see".to_string()),
                };
                overridable(&get.slot, interface)?;
                args_fit(&member.params, &rust, m)?;
                return match conv(r, &prop) {
                    Some(c) => Ok((Part::Get, Some(c))),
                    None => Err(format!("its type is {}, and {} returns {}, which does not convert to it", member.ty, m.rust, r.show())),
                };
            }
            if rust.len() == index + 1 && rust_ret.is_none() {
                let set = match &member.set {
                    Some(set) => set,
                    None => return Err("it has no setter the class can see".to_string()),
                };
                overridable(&set.slot, interface)?;
                args_fit(&member.params, &rust[..index], m)?;
                return match conv(&prop, &rust[index]) {
                    Some(Conv::Same | Conv::Implicit) => Ok((Part::Set, None)),
                    _none => Err(format!("its type is {}, which does not convert to {}'s {}", member.ty, m.rust, rust[index].show())),
                };
            }
            let indexes = if index == 0 { String::new() } else { format!(" its {index} index argument{} and", if index == 1 { "" } else { "s" }) };
            Err(format!(
                "it is a property, which a method backs as its getter, taking{indexes} nothing more and returning its value, or as its setter, taking{indexes} the value and returning nothing"
            ))
        }
        other => Err(format!("it is a{} {other}, which a method cannot back", if other.starts_with(['a', 'e', 'i', 'o', 'u']) { "n" } else { "" })),
    }
}

/// What a method's `#[member]` finds on one target.
enum Fit<'a> {
    /// The one member it backs.
    Hit(Matched<'a>),
    /// No member of the name it gives.
    Absent,
    /// Members of that name, none of which it fits, each with why.
    Misfit(Vec<String>),
}

/// The member `m`'s `#[member]` names on one target. Two members it fits
/// are refused, since the method backs one.
fn match_member<'a>(c: &Class, m: &'a Method, spec: &str, cands: &[Candidate<'a>], target: &str) -> Result<Fit<'a>, Error> {
    let (qualifier, name) = split_spec(spec);
    let named: Vec<&Candidate<'a>> = cands.iter().filter(|cd| cd.member.name == name && qualifier.as_deref().is_none_or(|q| qualifies(&cd.member.from, q))).collect();
    if named.is_empty() {
        return Ok(Fit::Absent);
    }
    let mut fits: Vec<Matched<'a>> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();
    for cd in named {
        match fit(m, cd) {
            Ok((part, ret)) => fits.push(Matched { method: m, candidate: *cd, part, ret }),
            Err(why) => reasons.push(format!("{}: {why}", cd.describe())),
        }
    }
    match fits.len() {
        1 => Ok(Fit::Hit(fits[0])),
        0 => Ok(Fit::Misfit(reasons)),
        _more => {
            let all: Vec<String> = fits.iter().map(|f| f.candidate.describe()).collect();
            Err(refuse(
                c,
                format!(
                    "{} is marked #[member = \"{spec}\"], which fits {} on {target}; name the one it backs with its declaring type, as #[member = \"{}.{name}\"]",
                    m.rust,
                    all.join(" and "),
                    fits[0].candidate.member.from
                ),
            ))
        }
    }
}

/// The C# parameter list of a member, by its own names and types.
fn param_list(params: &[Param]) -> String {
    params.iter().map(|p| format!("{} {}", global_type(&p.ty), cs_name(&p.name))).collect::<Vec<_>>().join(", ")
}

/// The member's own parameters, passed on.
fn arg_list(params: &[Param]) -> Vec<String> {
    params.iter().map(|p| cs_name(&p.name)).collect()
}

/// The call of a method's generated helper.
fn helper_call(m: &Method, args: &[String]) -> String {
    format!("__Pwrs{}({})", m.index, args.join(", "))
}

/// The helper a generated member or constructor converts an object Rust
/// gives through, where .NET declares a narrower reference type: null
/// stays null, and anything else converts as PowerShell converts it, an
/// object a PSObject wraps unwrapped first. Each class that calls it
/// declares its own, so the class needs nothing of Pwrs.Runtime beyond
/// what its base uses. Windows PowerShell runs every module in a process
/// on the runtime the first one loaded.
const AS_HELPER: &str = "__PwrsAs";

/// The declaration of [`AS_HELPER`], the same under either nullable
/// context.
fn as_declaration() -> String {
    format!(
        "#nullable disable\n        private static T {AS_HELPER}<T>(object value) where T : class => value == null ? null : global::System.Management.Automation.LanguagePrimitives.ConvertTo<T>(value);\n#nullable restore\n"
    )
}

/// `value`, converted to the reference type `to` by the class's helper.
fn as_type(to: &str, value: &str) -> String {
    format!("{AS_HELPER}<{}>({value})", global_type(to))
}

/// `call`, converted to `to` as `how` says.
fn result_expr(how: Option<Conv>, to: &str, call: String) -> String {
    match how {
        Some(Conv::FromObject) => as_type(to, &call),
        _same_or_implicit => call,
    }
}

/// The C# of one target's members, each under the key of the member it
/// implements, in the order they were matched.
fn render_members(matched: &[Matched<'_>], base: &TypeInfo, own: &[String]) -> Vec<(String, String)> {
    // An interface method is implemented under its own name while the
    // class has no method of that signature and nothing else of that
    // name, and explicitly otherwise; an interface property, while
    // nothing in the class has its name. The class's own names count for
    // both, whatever their signature.
    let mut signatures: Vec<String> = Vec::new();
    let mut names: Vec<String> = own.to_vec();
    let mut other_names: Vec<String> = own.to_vec();
    for member in &base.members {
        names.push(member.name.clone());
        if member.kind == "method" {
            let params: Vec<String> = member.params.iter().map(|p| norm(&p.ty)).collect();
            signatures.push(format!("{}({})", member.name, params.join(",")));
        } else {
            other_names.push(member.name.clone());
        }
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let mut done: Vec<String> = Vec::new();
    for found in matched {
        let key = found.candidate.key();
        if done.contains(&key) {
            continue;
        }
        done.push(key.clone());
        let member = found.candidate.member;
        let params = param_list(&member.params);
        let args = arg_list(&member.params);
        let interface = found.candidate.interface;
        let text = if member.kind == "method" {
            let ret = global_type(&member.returns);
            let body = result_expr(found.ret, &member.returns, helper_call(found.method, &args));
            let signature: Vec<String> = member.params.iter().map(|p| norm(&p.ty)).collect();
            let sig = format!("{}({})", member.name, signature.join(","));
            match interface {
                None => format!("        {} override {ret} {}({params}) => {body};\n", member.access, cs_name(&member.name)),
                Some(_) if !signatures.contains(&sig) && !other_names.contains(&member.name) => {
                    signatures.push(sig);
                    names.push(member.name.clone());
                    format!("        public {ret} {}({params}) => {body};\n", cs_name(&member.name))
                }
                Some(iface) => format!("        {ret} {}.{}({params}) => {body};\n", global_type(iface), cs_name(&member.name)),
            }
        } else {
            let get = matched.iter().find(|m| m.candidate.key() == key && m.part == Part::Get);
            let set = matched.iter().find(|m| m.candidate.key() == key && m.part == Part::Set);
            let prop = global_type(&member.ty);
            let get_access = member.get.as_ref().map(|a| a.access.as_str());
            let set_access = member.set.as_ref().map(|a| a.access.as_str());
            // The property is declared with the wider of the access of the
            // accessors it overrides, and the narrower accessor says its own.
            let shown = match (get.is_some(), set.is_some(), get_access, set_access) {
                (true, false, Some(a), _) | (false, true, _, Some(a)) => a,
                (_, _, Some("public"), _) | (_, _, _, Some("public")) => "public",
                (_, _, Some(a), _) | (_, _, None, Some(a)) => a,
                _neither => "public",
            };
            let mut accessors = Vec::new();
            if let Some(g) = get {
                let modifier = match get_access {
                    Some(a) if interface.is_none() && a != shown => format!("{a} "),
                    _same => String::new(),
                };
                accessors.push(format!("{modifier}get => {}", result_expr(g.ret, &member.ty, helper_call(g.method, &args))));
            }
            if let Some(s) = set {
                let modifier = match set_access {
                    Some(a) if interface.is_none() && a != shown => format!("{a} "),
                    _same => String::new(),
                };
                let word = if member.set.as_ref().is_some_and(|a| a.init) { "init" } else { "set" };
                let mut with_value = args.clone();
                with_value.push("value".to_string());
                accessors.push(format!("{modifier}{word} => {}", helper_call(s.method, &with_value)));
            }
            let body = format!("{{ {}; }}", accessors.join("; "));
            let declared = if member.params.is_empty() { cs_name(&member.name) } else { format!("this[{params}]") };
            match interface {
                None => format!("        {shown} override {prop} {declared} {body}\n"),
                Some(_) if !names.contains(&member.name) => {
                    names.push(member.name.clone());
                    other_names.push(member.name.clone());
                    format!("        public {prop} {declared} {body}\n")
                }
                Some(iface) => format!("        {prop} {}.{declared} {body}\n", global_type(iface)),
            }
        };
        out.push((key, format!("#nullable disable\n{text}#nullable restore\n")));
    }
    out
}

/// What one target says the class declares.
struct TargetPlan {
    /// The member C#, each piece under the key of what it implements.
    members: Vec<(String, String)>,
    /// The index of each method that backs a member on this target.
    backed: Vec<u32>,
    /// Each method whose member's name this target has and whose
    /// signature fits none of them, with why for each.
    misfits: Vec<(u32, Vec<String>)>,
    new_base: Option<String>,
    factory: String,
    dispose: String,
    /// Interfaces the class lists beside `implements`: IDisposable, when
    /// the class adds it.
    added: Vec<String>,
    /// The interfaces `implements` names, as the references name them.
    interfaces: Vec<String>,
}

/// The generic type a name names, without its type arguments.
fn generic_definition(name: &str) -> String {
    let n = norm(name);
    match n.find('<') {
        Some(at) => n[..at].to_string(),
        None => n,
    }
}

/// The interfaces a class's roles make, as generic definitions, with
/// those they extend.
fn role_interfaces(c: &Class) -> Vec<&'static str> {
    let mut out = Vec::new();
    if c.count.is_some() && c.item.is_some() {
        out.extend([
            "System.Collections.Generic.IReadOnlyList",
            "System.Collections.Generic.IReadOnlyCollection",
            "System.Collections.Generic.IEnumerable",
            "System.Collections.IEnumerable",
        ]);
        if c.set_item.is_some() {
            out.extend(["System.Collections.Generic.IList", "System.Collections.Generic.ICollection"]);
        }
    }
    if c.next.is_some() {
        out.extend(["System.Collections.Generic.IEnumerable", "System.Collections.IEnumerable"]);
    }
    if c.compare.is_some() {
        out.push("System.IComparable");
    }
    if c.equals.is_some() && c.hash.is_some() {
        out.push("System.IEquatable");
    }
    out
}

/// The type `name` found on `target`, refused when it is missing or
/// could not be read whole.
fn found<'a>(c: &Class, types: &'a BTreeMap<String, TypeInfo>, name: &str, key: &str, target: &str) -> Result<&'a TypeInfo, Error> {
    let info = match types.get(name) {
        Some(info) => info,
        None => return Err(refuse(c, format!("{key} names {name}, which was not read for {target}"))),
    };
    if !info.found {
        return match &info.problem {
            Some(why) => Err(refuse(c, format!("{key} names {name}, and on {target} {why}"))),
            None => Err(refuse(c, format!("{key} names {name}, which is not found on {target}"))),
        };
    }
    if let Some(problem) = &info.problem {
        return Err(refuse(c, format!("{key} names {name}, and on {target} {problem}")));
    }
    Ok(info)
}

/// Picks the base constructor that `given`, the types of the values Rust
/// gives it, fits: the one fitting with every type the same, else the one
/// fitting at all. Returns it and how each value reaches its parameter.
fn choose_constructor<'a>(base: &'a TypeInfo, given: &[Ty]) -> Result<(&'a Constructor, Vec<Conv>), String> {
    let mut fitting: Vec<(&Constructor, Vec<Conv>)> = Vec::new();
    for ctor in &base.constructors {
        if ctor.params.len() != given.len() || ctor.params.iter().any(|p| !p.mode.is_empty() || p.type_unsupported) {
            continue;
        }
        let convs: Option<Vec<Conv>> = ctor.params.iter().zip(given).map(|(p, g)| conv(g, &ty_of(&p.ty, p.type_value))).collect();
        if let Some(convs) = convs {
            fitting.push((ctor, convs));
        }
    }
    let exact: Vec<usize> = (0..fitting.len()).filter(|&i| fitting[i].1.iter().all(|c| *c == Conv::Same)).collect();
    let pick = match (exact.len(), fitting.len()) {
        (1, _) => exact[0],
        (0, 1) => 0,
        _none_or_several => {
            let shown: Vec<String> = given.iter().map(Ty::show).collect();
            let all: Vec<String> = base
                .constructors
                .iter()
                .map(|ctor| format!("{} {}({})", ctor.access, base.name, ctor.params.iter().map(|p| format!("{} {}", p.ty, p.name)).collect::<Vec<_>>().join(", ")))
                .collect();
            let what = if fitting.is_empty() { "none of its constructors takes" } else { "more than one of its constructors takes" };
            let has = if all.is_empty() { "no constructor a derived class can call".to_string() } else { all.join(", ") };
            return Err(format!("{what} ({}); it has {has}", shown.join(", ")));
        }
    };
    Ok(fitting.swap_remove(pick))
}

/// A value of Rust type `from` read out of the object `expr` and converted
/// to the constructor parameter `to`, for code outside a nullable
/// context: a reference type is cast as it is, and a value type is
/// unboxed or converted as a generated method's result is.
fn base_arg_from_object(expr: &str, from: &MethodRet, to: &Param, how: Conv) -> String {
    match how {
        Conv::FromObject => as_type(&to.ty, expr),
        _same_or_implicit if is_reference_type(&from.clr, from.value_type) => format!("({}){expr}", global_type(&from.clr)),
        _same_or_implicit => read_boxed(expr, &from.clr, from.optional),
    }
}

fn plan_target(c: &Class, target: &str, types: &BTreeMap<String, TypeInfo>, native: &str) -> Result<TargetPlan, Error> {
    let ty = short_name(&c.name);
    let base_name = match &c.base {
        Some(base) => base.as_str(),
        None => OBJECT,
    };
    let base = found(c, types, base_name, "base", target)?;
    if c.base.is_some() {
        if base.kind != "class" {
            let article = if matches!(base.kind.as_str(), "interface" | "enum") { "an" } else { "a" };
            return Err(refuse(c, format!("base names {}, which is {article} {}, not a class", base.name, base.kind)));
        }
        if base.sealed {
            let what = if base.is_abstract { "a static class" } else { "sealed" };
            return Err(refuse(c, format!("base names {}, which is {what}, so no class can derive from it", base.name)));
        }
        if SPECIAL.contains(&norm(&base.name).as_str()) {
            return Err(refuse(c, format!("base names {}, which C# lets no class derive from", base.name)));
        }
    }
    // Pwrs.ProxyBase implements IDisposable for a class with no base of
    // its own.
    let mut base_interfaces: Vec<String> = base.interfaces.iter().map(|i| norm(i)).collect();
    if c.base.is_none() {
        base_interfaces.push("System.IDisposable".to_string());
    }
    let roles = role_interfaces(c);
    let mut interfaces: Vec<&TypeInfo> = Vec::new();
    for name in &c.implements {
        let info = found(c, types, name, "implements", target)?;
        if info.kind != "interface" {
            return Err(refuse(c, format!("implements names {}, which is a {}, not an interface", info.name, info.kind)));
        }
        if base_interfaces.contains(&norm(&info.name)) {
            let by = if c.base.is_some() { base.name.clone() } else { "Pwrs.ProxyBase".to_string() };
            return Err(refuse(c, format!("implements names {}, which {by} already implements; leave it out", info.name)));
        }
        for made in std::iter::once(&info.name).chain(info.interfaces.iter()) {
            if roles.contains(&generic_definition(made).as_str()) {
                return Err(refuse(c, format!("implements names {}, and its roles already make {made}; implement it through the roles or through #[member] methods, not both", info.name)));
            }
        }
        if let Some(stat) = info.members.iter().find(|m| m.slot == "static-abstract") {
            return Err(refuse(c, format!("implements names {}, whose {} {}.{} is static and abstract, which no method of an object can implement", info.name, stat.kind, stat.from, stat.name)));
        }
        interfaces.push(info);
    }

    let mut cands: Vec<Candidate<'_>> = base.members.iter().map(|member| Candidate { member, interface: None }).collect();
    for info in &interfaces {
        cands.extend(info.members.iter().map(|member| Candidate { member, interface: Some(member.from.as_str()) }));
    }
    let mut matched: Vec<Matched<'_>> = Vec::new();
    let mut misfits: Vec<(u32, Vec<String>)> = Vec::new();
    for m in &c.methods {
        let spec = match &m.member {
            Some(spec) => spec,
            None => continue,
        };
        match match_member(c, m, spec, &cands, target)? {
            Fit::Hit(hit) => {
                if let Some(earlier) = matched.iter().find(|e| e.candidate.key() == hit.candidate.key() && e.part == hit.part) {
                    return Err(refuse(c, format!("{} and {} both back {} of {}", earlier.method.rust, m.rust, hit.part.word(), hit.candidate.describe())));
                }
                matched.push(hit);
            }
            Fit::Misfit(reasons) => misfits.push((m.index, reasons)),
            Fit::Absent => {}
        }
    }

    // Every abstract member of the base and of each interface needs a
    // method, and an abstract event has none it could take.
    let mut missing: Vec<String> = Vec::new();
    let has = |cd: &Candidate<'_>, part: Part| matched.iter().any(|m| m.candidate.key() == cd.key() && m.part == part);
    for cd in &cands {
        let member = cd.member;
        match member.kind.as_str() {
            "method" if member.slot == "abstract" && !has(cd, Part::Method) => missing.push(cd.describe()),
            "property" => {
                if member.get.as_ref().is_some_and(|a| a.slot == "abstract") && !has(cd, Part::Get) {
                    missing.push(format!("the getter of {}", cd.describe()));
                }
                if member.set.as_ref().is_some_and(|a| a.slot == "abstract") && !has(cd, Part::Set) {
                    missing.push(format!("the setter of {}", cd.describe()));
                }
            }
            "event" if member.slot == "abstract" => {
                let owner = if cd.interface.is_some() { "implements" } else { "base" };
                return Err(refuse(c, format!("{owner} names a type with {}, which is abstract, and a Rust method cannot implement an event", cd.describe())));
            }
            _other => {}
        }
    }
    if !missing.is_empty() {
        // A method marked for a member that fits none of that name is
        // most often why one is missing, so it is named with the reasons.
        let mut why: Vec<String> = Vec::new();
        for (index, reasons) in &misfits {
            if let Some(m) = c.methods.iter().find(|m| m.index == *index)
                && let Some(spec) = &m.member
            {
                why.push(format!(" {} is marked #[member = \"{spec}\"], and no member of that name fits it: {}.", m.rust, reasons.join("; ")));
            }
        }
        return Err(refuse(
            c,
            format!(
                "on {target}, these abstract members have no method marked #[member] to back them: {}.{} Give each a #[psmethods] method marked #[member = \"Name\"]",
                missing.join(", "),
                why.join("")
            ),
        ));
    }

    // The names the class declares of its own, which a base member of the
    // same name would be hidden by.
    let mut own: Vec<String> = c.fields.iter().map(|f| f.name.clone()).collect();
    own.extend(c.methods.iter().filter(|m| m.member.is_none() && !m.base && !m.constructor).map(|m| m.name.clone()));
    if c.count.is_some() && c.item.is_some() {
        own.extend(["Count".to_string(), "Item".to_string()]);
    }
    if c.base.is_some() {
        for name in &own {
            if let Some(hidden) = base.members.iter().find(|m| &m.name == name) {
                return Err(refuse(
                    c,
                    format!("it declares {name}, which hides {}'s {} {name}; to back that member mark a method #[member = \"{name}\"], or rename this one", hidden.from, hidden.kind),
                ));
            }
        }
        if c.equals.is_some() && c.hash.is_some() {
            for (name, params) in [("Equals", 1usize), ("GetHashCode", 0)] {
                if let Some(sealed) = base.members.iter().find(|m| m.name == name && m.params.len() == params && m.kind == "method" && !matches!(m.slot.as_str(), "virtual" | "abstract")) {
                    return Err(refuse(c, format!("its equals and hash roles override {name}, which {} does not let a derived class override", sealed.from)));
                }
            }
        }
    }
    let members = render_members(&matched, base, &own);
    let backed: Vec<u32> = matched.iter().map(|m| m.method.index).collect();

    let mut plan = TargetPlan {
        members,
        backed,
        misfits,
        new_base: None,
        factory: String::new(),
        dispose: String::new(),
        added: Vec::new(),
        interfaces: interfaces.iter().map(|i| i.name.clone()).collect(),
    };
    if c.base.is_none() {
        return Ok(plan);
    }

    // The constructor `new` makes calls the base constructor its
    // #[base(...)] arguments fit, or the one taking none.
    if let Some(new) = c.methods.iter().find(|m| m.constructor) {
        let at: Vec<usize> = match &new.base_args {
            Some(at) => at.clone(),
            None => Vec::new(),
        };
        let given: Vec<Ty> = at.iter().map(|&i| ty_of_param(&new.params[i])).collect();
        let (ctor, convs) = choose_constructor(base, &given).map_err(|why| {
            let with = if at.is_empty() { "no arguments, since new names none with #[base(...)]" } else { "the arguments new's #[base(...)] names" };
            refuse(c, format!("new calls {}'s constructor with {with}, and on {target} {why}", base.name))
        })?;
        let args: Vec<String> = at
            .iter()
            .zip(ctor.params.iter().zip(&convs))
            .map(|(&i, (p, how))| {
                let name = cs_name(&new.params[i].name);
                match how {
                    Conv::FromObject => as_type(&p.ty, &name),
                    _same_or_implicit => name,
                }
            })
            .collect();
        plan.new_base = Some(format!("base({})", args.join(", ")));
    }

    // The constructor Rust's factory calls takes the arguments a #[base]
    // method returns, or calls the base constructor taking none.
    let bytes = if c.native_bytes { "true" } else { "false" };
    let core = format!("__pwrsCore = new global::Pwrs.ProxyCore(this, {native}, {}, instance, {bytes});", c.id);
    match c.methods.iter().find(|m| m.base) {
        Some(provider) => {
            let parts: &[MethodRet] = match &provider.base_ret {
                Some(parts) => parts,
                None => return Err(refuse(c, format!("{} is marked #[base], and its descriptor names no types for what it returns", provider.rust))),
            };
            let given: Vec<Ty> = parts.iter().map(ty_of_ret).collect();
            let (ctor, convs) = choose_constructor(base, &given).map_err(|why| refuse(c, format!("{} is marked #[base], and on {target} {}'s {why}", provider.rust, base.name)))?;
            let args: Vec<String> = parts.iter().zip(ctor.params.iter().zip(&convs)).enumerate().map(|(i, (r, (p, how)))| base_arg_from_object(&format!("b[{i}]"), r, p, *how)).collect();
            plan.factory = format!(
                "        internal {ty}(IntPtr instance) : this(default(global::Pwrs.FromFields), instance, __PwrsBase(instance)) {{ }}\n\n\
                 #nullable disable\n        private {ty}(global::Pwrs.FromFields _, IntPtr instance, object[] b) : base({})\n        {{\n            {core}\n        }}\n#nullable restore\n\n\
                 \x20       /// <summary>The arguments of the base class's constructor, from {}.</summary>\n\
                 \x20       private static unsafe object?[] __PwrsBase(IntPtr instance)\n        {{\n            ulong bound = 0;\n            return (object?[])global::Pwrs.ProxyCore.CallUnowned({native}, {}, {}, instance, &bound)!;\n        }}\n\n",
                args.join(", "),
                provider.name,
                c.id,
                provider.index
            );
        }
        None => {
            if let Err(why) = choose_constructor(base, &[]) {
                return Err(refuse(
                    c,
                    format!(
                        "an object Rust makes outside new calls {}'s constructor with no arguments, and on {target} {why}; give the class a #[base] method returning the arguments, as #[base] pub fn base_args(&self) -> PsResult<T>",
                        base.name
                    ),
                ));
            }
            plan.factory = format!("        internal {ty}(IntPtr instance) : base()\n        {{\n            {core}\n        }}\n\n");
        }
    }

    // Dispose frees the value through whatever the base gives a derived
    // class for its own Dispose.
    let dispose_bool = base.members.iter().find(|m| m.kind == "method" && m.name == "Dispose" && m.params.len() == 1 && norm(&m.params[0].ty) == "bool");
    let dispose_plain = base.members.iter().find(|m| m.kind == "method" && m.name == "Dispose" && m.params.is_empty());
    let can_override = |m: &&Member| matches!(m.slot.as_str(), "virtual" | "abstract");
    plan.dispose = if let Some(d) = dispose_bool.filter(can_override) {
        let call_base = if d.slot == "abstract" { "" } else { "\n            base.Dispose(disposing);" };
        format!(
            "        /// <summary>Frees the Rust value, when called from Dispose.</summary>\n        {} override void Dispose(bool disposing)\n        {{\n            if (disposing) __pwrsCore?.Release();{call_base}\n        }}\n",
            d.access
        )
    } else if let Some(d) = dispose_plain.filter(can_override) {
        let call_base = if d.slot == "abstract" { "" } else { "\n            base.Dispose();" };
        format!("        /// <summary>Frees the Rust value.</summary>\n        public override void Dispose()\n        {{\n            __pwrsCore?.Release();{call_base}\n        }}\n")
    } else if base_interfaces.iter().any(|i| i == "System.IDisposable") {
        plan.added.push("global::System.IDisposable".to_string());
        let call_base = if dispose_plain.is_some() { "\n            base.Dispose();" } else { "" };
        format!("        /// <summary>Frees the Rust value.</summary>\n        void global::System.IDisposable.Dispose()\n        {{\n            __pwrsCore?.Release();{call_base}\n        }}\n")
    } else {
        plan.added.push("global::System.IDisposable".to_string());
        if base.members.iter().any(|m| m.name == "Dispose") {
            "        /// <summary>Frees the Rust value.</summary>\n        void global::System.IDisposable.Dispose() => __pwrsCore?.Release();\n".to_string()
        } else {
            "        /// <summary>Frees the Rust value.</summary>\n        public void Dispose() => __pwrsCore?.Release();\n".to_string()
        }
    };
    if !base.members.iter().any(|m| m.name == "IsDisposed") {
        plan.dispose.push_str("\n        public bool IsDisposed => __pwrsCore is null || __pwrsCore.IsDisposed;\n");
    }
    Ok(plan)
}

/// `a` for code the same on both targets, else each under `#if NET`.
fn both(a: &str, b: &str) -> String {
    if a == b {
        a.to_string()
    } else {
        format!("#if NET\n{a}#else\n{b}#endif\n")
    }
}

fn merge(c: &Class, targets: Vec<TargetPlan>) -> Result<ClassPlan, Error> {
    let mut targets = targets.into_iter();
    let (net, standard) = match (targets.next(), targets.next()) {
        (Some(net), Some(standard)) => (net, standard),
        _missing => return Err(refuse(c, "it was not planned for both targets")),
    };
    if net.added != standard.added || net.interfaces != standard.interfaces {
        return Err(refuse(c, format!("its interfaces differ between {} and {}", TARGETS[0], TARGETS[1])));
    }
    // A method backs its member on every target where it fits one. Where
    // it fits on one target only, a member of that name the other has is
    // named in a note; fitting on neither is refused.
    for m in &c.methods {
        let spec = match &m.member {
            Some(spec) => spec,
            None => continue,
        };
        let why = |plan: &TargetPlan| plan.misfits.iter().find(|(index, _)| *index == m.index).map(|(_, reasons)| reasons.join("; "));
        let hits = [net.backed.contains(&m.index), standard.backed.contains(&m.index)];
        let reasons = [why(&net), why(&standard)];
        match hits {
            [false, false] => {
                let mut said: Vec<String> = Vec::new();
                for (target, reason) in TARGETS.iter().zip(&reasons) {
                    if let Some(reason) = reason {
                        said.push(format!("on {target}, {reason}"));
                    }
                }
                if said.is_empty() {
                    let base = match &c.base {
                        Some(base) => base.as_str(),
                        None => OBJECT,
                    };
                    let names: Vec<&str> = std::iter::once(base).chain(c.implements.iter().map(String::as_str)).collect();
                    return Err(refuse(c, format!("{} is marked #[member = \"{spec}\"], and none of {} has a member it names", m.rust, names.join(", "))));
                }
                return Err(refuse(c, format!("{} is marked #[member = \"{spec}\"], and no member of that name fits it: {}", m.rust, said.join("; "))));
            }
            [true, false] | [false, true] => {
                let (only, other) = if hits[0] { (TARGETS[0], 1) } else { (TARGETS[1], 0) };
                if let Some(reason) = &reasons[other] {
                    eprintln!("pwrs: class {}: {} backs its member on {only} only; on {}, {reason}", c.name, m.rust, TARGETS[other]);
                }
            }
            [true, true] => {}
        }
    }
    let mut keys: Vec<&String> = Vec::new();
    for (key, _) in net.members.iter().chain(&standard.members) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let mut members = String::new();
    for key in keys {
        let a = net.members.iter().find(|(k, _)| k == key).map(|(_, text)| text.as_str());
        let b = standard.members.iter().find(|(k, _)| k == key).map(|(_, text)| text.as_str());
        members.push_str(&match (a, b) {
            (Some(a), Some(b)) => both(a, b),
            (Some(a), None) => format!("#if NET\n{a}#endif\n"),
            (None, Some(b)) => format!("#if !NET\n{b}#endif\n"),
            (None, None) => String::new(),
        });
    }
    let base = c.base.as_ref().map(|b| global_type(&norm(b).replace(',', ", ")));
    let mut interfaces: Vec<String> = net.interfaces.iter().map(|i| global_type(i)).collect();
    interfaces.extend(net.added.iter().cloned());
    let new_base = match (net.new_base, standard.new_base) {
        (Some(a), Some(b)) => Some((a, b)),
        _none => None,
    };
    Ok(ClassPlan { base, interfaces, members, new_base, factory: both(&net.factory, &standard.factory), dispose: both(&net.dispose, &standard.dispose) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    /// Every type a test names, read from the toolchain's reference
    /// assemblies for both compiles, once for the process.
    fn read() -> &'static Read {
        static READ: OnceLock<Read> = OnceLock::new();
        READ.get_or_init(|| {
            let tc = pwrs_build::Toolchain::ensure().expect("the toolchain");
            let work = std::env::temp_dir().join(format!("pwrs-bases-{}", std::process::id()));
            let types: Vec<String> = [
                "System.Object",
                "System.IO.Stream",
                "System.Exception",
                "System.String",
                "System.Math",
                "System.IDisposable",
                "System.IO.Strem",
                "System.IFormattable",
                "System.IComparable",
                "System.Collections.IEnumerable",
                "System.Collections.IComparer",
                "System.Collections.Generic.IComparer<long>",
                "System.Collections.ObjectModel.ReadOnlyCollection<string>",
                "System.Numerics.INumberBase<int>",
            ]
            .iter()
            .map(|t| t.to_string())
            .collect();
            let targets = [(TARGETS[0], tc.net10_refs.as_slice()), (TARGETS[1], tc.netstandard_refs.as_slice())];
            let read = crate::members::read(&work, &targets, &types).expect("the types");
            std::fs::remove_dir_all(&work).expect("remove the scratch folder");
            read
        })
    }

    /// A module of one proxy class, `Demo.Thing`, with the descriptor keys
    /// `keys`, a `long` field of each name in `fields`, and `methods`.
    fn module(keys: &str, fields: &[&str], methods: &[String]) -> Module {
        let fields: Vec<String> = fields
            .iter()
            .enumerate()
            .map(|(i, name)| format!(r#"{{"name": "{name}", "rust": "{name}", "index": {i}, "clr": "long", "slot": "i64", "optional": false, "help": ""}}"#))
            .collect();
        let json = format!(
            r#"{{"abi": 1, "name": "Demo", "cmdlets": [], "classes": [{{"id": 0, "name": "Demo.Thing", "rust": "Thing", "mode": "proxy", {keys} "description": "", "fields": [{}], "methods": [{}]}}]}}"#,
            fields.join(", "),
            methods.join(", ")
        );
        serde_json::from_str(&json).expect("a descriptor")
    }

    /// A method's descriptor entry: its index, its name, its arguments as
    /// (name, .NET type, slot), the .NET type of its result, and keys beyond
    /// those every method has.
    fn method(index: u32, name: &str, args: &[(&str, &str, &str)], ret: Option<&str>, more: &str) -> String {
        let params: Vec<String> = args
            .iter()
            .enumerate()
            .map(|(i, (arg, clr, slot))| {
                let value = if *clr == "System.IO.SeekOrigin" { r#", "value_type": true"# } else { "" };
                format!(r#"{{"name": "{arg}", "rust": "{arg}", "index": {i}, "clr": "{clr}", "slot": "{slot}", "optional": false{value}}}"#)
            })
            .collect();
        let ret = match ret {
            Some(clr) => format!(r#"{{"clr": "{clr}", "optional": false}}"#),
            None => "null".to_string(),
        };
        format!(
            r#"{{"name": "{name}", "rust": "{name}", "index": {index}, "help": "", "params": [{}], "ret": {ret}, "static": false, "mutable": true, "constructor": false{more}}}"#,
            params.join(", ")
        )
    }

    fn member(spec: &str) -> String {
        format!(r#", "member": "{spec}""#)
    }

    fn plan_of(m: &Module) -> Result<ClassPlan, Error> {
        match plan(m, Some(read()), "Native")?.remove(&0) {
            Some(p) => Ok(p),
            None => panic!("the class was not planned"),
        }
    }

    fn refused(m: &Module) -> String {
        match plan_of(m) {
            Ok(_) => panic!("the class was accepted"),
            Err(e) => e.to_string(),
        }
    }

    const BUFFER: [(&str, &str, &str); 3] = [("buffer", "byte[]", "handle"), ("offset", "int", "i32"), ("count", "int", "i32")];

    /// Methods backing every abstract member of System.IO.Stream, as
    /// hello's Hello.ByteStream backs them.
    fn stream_methods() -> Vec<String> {
        vec![
            method(0, "CanRead", &[], Some("bool"), &member("CanRead")),
            method(1, "CanSeek", &[], Some("bool"), &member("CanSeek")),
            method(2, "CanWrite", &[], Some("bool"), &member("CanWrite")),
            method(3, "Length", &[], Some("long"), &member("Length")),
            method(4, "Position", &[], Some("long"), &member("Position")),
            method(5, "SetPosition", &[("value", "long", "i64")], None, &member("Position")),
            method(6, "Flush", &[], None, &member("Flush")),
            method(7, "Read", &BUFFER, Some("int"), &member("Read")),
            method(8, "Write", &BUFFER, None, &member("Write")),
            method(9, "Seek", &[("offset", "long", "i64"), ("origin", "System.IO.SeekOrigin", "handle")], Some("long"), &member("Seek")),
            method(10, "SetLength", &[("value", "long", "i64")], None, &member("SetLength")),
        ]
    }

    #[test]
    fn a_stream_overrides_each_member_its_methods_back_and_frees_its_value_on_dispose() {
        let p = match plan_of(&module(r#""base": "System.IO.Stream","#, &[], &stream_methods())) {
            Ok(p) => p,
            Err(e) => panic!("the stream was refused: {e}"),
        };
        assert_eq!(p.base.as_deref(), Some("global::System.IO.Stream"));
        for line in [
            "public override bool CanRead { get => __Pwrs0(); }",
            "public override long Position { get => __Pwrs4(); set => __Pwrs5(value); }",
            "public override void Flush() => __Pwrs6();",
            "public override int Read(byte[] buffer, int offset, int count) => __Pwrs7(buffer, offset, count);",
            "public override long Seek(long offset, global::System.IO.SeekOrigin origin) => __Pwrs9(offset, origin);",
        ] {
            assert!(p.members.contains(line), "{line}\n{}", p.members);
        }
        assert!(!p.members.contains("#if"), "the members are alike on both targets: {}", p.members);
        assert!(p.factory.contains("internal Thing(IntPtr instance) : base()"), "{}", p.factory);
        assert!(p.dispose.contains("protected override void Dispose(bool disposing)") && p.dispose.contains("base.Dispose(disposing);"), "{}", p.dispose);
        assert!(p.dispose.contains("public bool IsDisposed"), "{}", p.dispose);
        assert!(p.interfaces.is_empty(), "Stream implements IDisposable itself: {:?}", p.interfaces);
    }

    #[test]
    fn every_abstract_member_without_a_method_is_named() {
        let e = refused(&module(r#""base": "System.IO.Stream","#, &[], &[method(7, "Read", &BUFFER, Some("int"), &member("Read"))]));
        for part in [
            "these abstract members have no method marked #[member]",
            "System.IO.Stream.Flush()",
            "the getter of the property System.IO.Stream.Length",
            "the setter of the property System.IO.Stream.Position",
        ] {
            assert!(e.contains(part), "{part}: {e}");
        }
    }

    #[test]
    fn a_method_naming_no_member_or_fitting_none_says_why() {
        let mut methods = stream_methods();
        methods.push(method(11, "Rewind", &[], None, &member("Rewind")));
        let e = refused(&module(r#""base": "System.IO.Stream","#, &[], &methods));
        assert!(e.contains("Rewind is marked #[member = \"Rewind\"], and none of System.IO.Stream has a member it names"), "{e}");
        let mut methods = stream_methods();
        methods[7] = method(7, "Read", &[("buffer", "string", "str16"), ("offset", "int", "i32"), ("count", "int", "i32")], Some("int"), &member("Read"));
        let e = refused(&module(r#""base": "System.IO.Stream","#, &[], &methods));
        assert!(e.contains("its argument buffer is byte[], which does not convert to Read's string"), "{e}");
        let mut methods = stream_methods();
        methods.push(method(11, "Close", &[], None, &member("CopyTo")));
        let e = refused(&module(r#""base": "System.IO.Stream","#, &[], &methods));
        assert!(e.contains("no member of that name fits it") && e.contains("it takes 2 arguments, and Close takes 0") && e.contains("it is not virtual"), "{e}");
    }

    #[test]
    fn a_member_fitting_on_one_target_is_declared_there_alone() {
        // CopyTo(Stream, int) is virtual on .NET and not in .NET Standard 2.0.
        let mut methods = stream_methods();
        methods.push(method(11, "CopyInto", &[("destination", "object", "handle"), ("bufferSize", "int", "i32")], None, &member("CopyTo")));
        let p = match plan_of(&module(r#""base": "System.IO.Stream","#, &[], &methods)) {
            Ok(p) => p,
            Err(e) => panic!("the stream was refused: {e}"),
        };
        let copy = "public override void CopyTo(global::System.IO.Stream destination, int bufferSize) => __Pwrs11(destination, bufferSize);";
        let at = match p.members.find(copy) {
            Some(at) => at,
            None => panic!("{copy}\n{}", p.members),
        };
        assert!(p.members[..at].ends_with("#if NET\n#nullable disable\n        "), "{}", p.members);
    }

    #[test]
    fn a_base_no_class_derives_from_is_refused() {
        for (base, why) in [
            ("System.String", "which is sealed, so no class can derive from it"),
            ("System.Math", "which is a static class"),
            ("System.IDisposable", "which is an interface, not a class"),
            ("System.IO.Strem", "System.IO.Strem is in none of the reference assemblies"),
        ] {
            let e = refused(&module(&format!(r#""base": "{base}","#), &[], &[]));
            assert!(e.contains(why), "{base}: {e}");
        }
    }

    #[test]
    fn an_interface_member_is_public_unless_the_class_has_its_name() {
        let format = method(0, "Format", &[("format", "string", "str16"), ("provider", "object", "handle")], Some("string"), &member("ToString"));
        let p = match plan_of(&module(r#""implements": ["System.IFormattable"],"#, &[], &[format])) {
            Ok(p) => p,
            Err(e) => panic!("the class was refused: {e}"),
        };
        assert!(
            p.members.contains("public string ToString(string format, global::System.IFormatProvider formatProvider) => __Pwrs0(format, formatProvider);"),
            "{}",
            p.members
        );
        assert_eq!(p.interfaces, ["global::System.IFormattable"]);
        assert!(p.base.is_none() && p.factory.is_empty() && p.dispose.is_empty(), "a class on Pwrs.ProxyBase keeps its constructor and Dispose");
        assert!(!p.members.contains(AS_HELPER), "a class converting no object declares no helper: {}", p.members);
        let methods = vec![method(0, "Compare", &[], Some("long"), ""), method(1, "Order", &[("x", "object", "handle"), ("y", "object", "handle")], Some("int"), &member("Compare"))];
        let p = match plan_of(&module(r#""implements": ["System.Collections.IComparer"],"#, &[], &methods)) {
            Ok(p) => p,
            Err(e) => panic!("the class was refused: {e}"),
        };
        assert!(p.members.contains("int global::System.Collections.IComparer.Compare(object x, object y) => __Pwrs1(x, y);"), "{}", p.members);
    }

    #[test]
    fn an_object_rust_gives_reaches_a_narrower_type_through_the_class_s_own_helper() {
        let items = method(0, "Items", &[], Some("object"), &member("GetEnumerator"));
        let p = match plan_of(&module(r#""implements": ["System.Collections.IEnumerable"],"#, &[], &[items])) {
            Ok(p) => p,
            Err(e) => panic!("the class was refused: {e}"),
        };
        assert!(
            p.members.contains("public global::System.Collections.IEnumerator GetEnumerator() => __PwrsAs<global::System.Collections.IEnumerator>(__Pwrs0());"),
            "{}",
            p.members
        );
        assert!(p.members.contains(&as_declaration()), "{}", p.members);
        assert!(!p.members.contains("global::Pwrs."), "the class needs nothing of the runtime beyond Pwrs.ProxyBase: {}", p.members);
    }

    #[test]
    fn a_member_two_interfaces_have_is_named_with_its_declaring_type() {
        let pair = |spec: &str| {
            vec![
                method(0, "Order", &[("x", "object", "handle"), ("y", "object", "handle")], Some("int"), &member(spec)),
                method(1, "OrderLong", &[("x", "long", "i64"), ("y", "long", "i64")], Some("int"), &member("IComparer<System.Int64>.Compare")),
            ]
        };
        let keys = r#""implements": ["System.Collections.IComparer", "System.Collections.Generic.IComparer<long>"],"#;
        let e = refused(&module(keys, &[], &pair("Compare")));
        assert!(e.contains("which fits System.Collections.IComparer.Compare(object x, object y) and System.Collections.Generic.IComparer<long>.Compare(long x, long y)"), "{e}");
        let p = match plan_of(&module(keys, &[], &pair("System.Collections.IComparer.Compare"))) {
            Ok(p) => p,
            Err(e) => panic!("the class was refused: {e}"),
        };
        assert!(p.members.contains("public int Compare(object x, object y) => __Pwrs0(x, y);"), "{}", p.members);
        assert!(p.members.contains("public int Compare(long x, long y) => __Pwrs1(x, y);"), "{}", p.members);
    }

    #[test]
    fn an_interface_the_base_or_a_role_already_makes_is_refused() {
        let e = refused(&module(r#""base": "System.IO.Stream", "implements": ["System.IDisposable"],"#, &[], &stream_methods()));
        assert!(e.contains("implements names System.IDisposable, which System.IO.Stream already implements"), "{e}");
        let order = method(0, "Order", &[("other", "Demo.Thing", "peer")], Some("int"), "");
        let e = refused(&module(r#""compare": "Order", "implements": ["System.IComparable"],"#, &[], &[order]));
        assert!(e.contains("its roles already make System.IComparable"), "{e}");
        let e = refused(&module(r#""implements": ["System.Numerics.INumberBase<int>"],"#, &[], &[]));
        assert!(e.contains("is static and abstract, which no method of an object can implement"), "{e}");
    }

    #[test]
    fn a_base_constructor_takes_new_s_arguments_or_a_base_method_s() {
        let new = |base_args: &str| {
            format!(
                r#"{{"name": "New", "rust": "new", "index": 0, "help": "", "params": [{{"name": "message", "rust": "message", "index": 0, "clr": "string", "slot": "str16", "optional": false}}, {{"name": "code", "rust": "code", "index": 1, "clr": "int", "slot": "i32", "optional": false}}], "ret": {{"clr": "Demo.Thing", "optional": false}}, "static": true, "mutable": false, "constructor": true, "base_args": {base_args}}}"#
            )
        };
        let p = match plan_of(&module(r#""base": "System.Exception","#, &[], &[new("[0]")])) {
            Ok(p) => p,
            Err(e) => panic!("the failure was refused: {e}"),
        };
        assert_eq!(p.new_base, Some(("base(message)".to_string(), "base(message)".to_string())));
        assert!(p.factory.contains(": base()"), "with no #[base] method the factory calls the constructor that takes nothing: {}", p.factory);
        assert!(p.interfaces.contains(&"global::System.IDisposable".to_string()) && p.dispose.contains("public void Dispose() => __pwrsCore?.Release();"), "{}", p.dispose);
        let e = refused(&module(r#""base": "System.Exception","#, &[], &[new("[0, 1]")]));
        assert!(e.contains("none of its constructors takes (string, int)"), "{e}");
        let e = refused(&module(r#""base": "System.Collections.ObjectModel.ReadOnlyCollection<string>","#, &[], &[]));
        assert!(e.contains("give the class a #[base] method returning the arguments"), "{e}");
        let base = r#"{"name": "BaseArgs", "rust": "base_args", "index": 0, "help": "", "params": [], "ret": null, "static": false, "mutable": false, "constructor": false, "base": true, "base_ret": [{"clr": "object", "optional": false}]}"#;
        let p = match plan_of(&module(r#""base": "System.Collections.ObjectModel.ReadOnlyCollection<string>","#, &[], &[base.to_string()])) {
            Ok(p) => p,
            Err(e) => panic!("the collection was refused: {e}"),
        };
        assert!(p.factory.contains("base(__PwrsAs<global::System.Collections.Generic.IList<string>>(b[0]))"), "{}", p.factory);
        assert!(p.factory.contains("CallUnowned(Native, 0, 0, instance, &bound)"), "{}", p.factory);
        assert!(p.members.contains("private static T __PwrsAs<T>(object value) where T : class"), "the class declares the helper its factory calls: {}", p.members);
    }

    #[test]
    fn a_name_the_base_has_and_dispose_are_refused() {
        let e = refused(&module(r#""base": "System.Exception","#, &["Message"], &[]));
        assert!(e.contains("it declares Message, which hides System.Exception's property Message"), "{e}");
        let e = refused(&module(r#""base": "System.IO.Stream","#, &[], &[method(0, "Close", &[], None, &member("Dispose"))]));
        assert!(e.contains("Dispose frees the class's Rust value"), "{e}");
    }

    #[test]
    fn a_value_reaches_a_member_as_csharp_converts_it_and_an_object_reaches_any_reference_type() {
        let t = |name: &str, value: bool, nullable: bool| Ty { name: name.to_string(), value, nullable };
        assert_eq!(conv(&t("byte[]", false, false), &t("byte[]", false, false)), Some(Conv::Same));
        assert_eq!(conv(&t("int", true, false), &t("long", true, false)), Some(Conv::Implicit));
        assert_eq!(conv(&t("long", true, false), &t("int", true, false)), None);
        assert_eq!(conv(&t("int", true, false), &t("int", true, true)), Some(Conv::Implicit));
        assert_eq!(conv(&t("int", true, true), &t("int", true, false)), None);
        assert_eq!(conv(&t("string", false, false), &t("object", false, false)), Some(Conv::Implicit));
        assert_eq!(conv(&t("object", false, false), &t("System.IFormatProvider", false, false)), Some(Conv::FromObject));
        assert_eq!(conv(&t("object", false, false), &t("long", true, false)), None);
    }

    #[test]
    fn a_member_name_splits_at_its_last_dot_outside_type_arguments() {
        assert_eq!(split_spec("Read"), (None, "Read".to_string()));
        assert_eq!(
            split_spec("System.Collections.Generic.IComparer<System.Int64>.Compare"),
            (Some("System.Collections.Generic.IComparer<System.Int64>".to_string()), "Compare".to_string())
        );
        assert!(qualifies("System.Collections.Generic.IComparer<long>", "IComparer<System.Int64>"));
        assert!(qualifies("object", "System.Object"));
        assert!(!qualifies("System.Collections.IComparer", "Generic.IComparer"));
        assert_eq!(
            global_type("System.Collections.Generic.IDictionary<string, System.IO.Stream>"),
            "global::System.Collections.Generic.IDictionary<string, global::System.IO.Stream>"
        );
        assert_eq!(global_type("byte[]"), "byte[]");
    }
}
