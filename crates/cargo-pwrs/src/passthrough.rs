//! The `attribute(...)` keys: each .NET attribute a module names on a
//! cmdlet, a parameter, a class, a property or a method, found in the
//! reference assemblies of each shell's compile and written as the C# that
//! shell declares it with. An attribute only one shell's references have
//! is written for that shell alone, under `#if NET` or `#if !NET`, with a
//! warning; one that neither has is refused, as is an argument that fits
//! no constructor or names no settable member, each naming where it was
//! written.

use crate::descriptor::{Module, PassedAttribute, PassedValue};
use crate::members::{Read, TypeInfo};
use crate::Error;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The kind of member an attribute goes on, as `AttributeTargets` has it.
/// A parameter and a class's field are both properties in the shell, and
/// `new` is a constructor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Class,
    Constructor,
    Property,
    Method,
}

impl Target {
    fn bit(self) -> i64 {
        match self {
            Target::Class => 4,
            Target::Constructor => 32,
            Target::Property => 128,
            Target::Method => 64,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Target::Class => "a class",
            Target::Constructor => "a constructor",
            Target::Property => "a property",
            Target::Method => "a method",
        }
    }
}

/// Each site's attribute lines, by the keys this type's functions give.
#[derive(Default, Debug)]
pub struct Attributes {
    lines: BTreeMap<String, Vec<String>>,
}

impl Attributes {
    pub fn cmdlet(id: u32) -> String {
        format!("cmdlet {id}")
    }

    pub fn param(cmdlet: u32, name: &str) -> String {
        format!("param {cmdlet} {name}")
    }

    pub fn class(name: &str) -> String {
        format!("class {name}")
    }

    pub fn field(class: &str, name: &str) -> String {
        format!("field {class} {name}")
    }

    pub fn method(class: &str, index: u32) -> String {
        format!("method {class} {index}")
    }

    /// The lines to write before the member `key` names, each attribute
    /// after `indent` and each `#if` line at the start of its line, as the
    /// shell's other directives are; empty for a member with no attributes.
    pub fn before(&self, key: &str, indent: &str) -> String {
        match self.lines.get(key) {
            Some(lines) => lines.iter().map(|line| if line.starts_with('#') { format!("{line}\n") } else { format!("{indent}{line}\n") }).collect(),
            None => String::new(),
        }
    }
}

/// One member with attributes: its key, its kind, how messages name it.
struct Site<'a> {
    key: String,
    target: Target,
    name: String,
    attributes: &'a [PassedAttribute],
}

fn sites(module: &Module) -> Vec<Site<'_>> {
    let mut out = Vec::new();
    for c in &module.cmdlets {
        out.push(Site { key: Attributes::cmdlet(c.id), target: Target::Class, name: format!("cmdlet {}", c.name), attributes: &c.attributes });
        for p in &c.params {
            out.push(Site { key: Attributes::param(c.id, &p.name), target: Target::Property, name: format!("parameter -{} of {}", p.name, c.name), attributes: &p.attributes });
        }
    }
    for k in &module.classes {
        out.push(Site { key: Attributes::class(&k.name), target: Target::Class, name: format!("class {}", k.name), attributes: &k.attributes });
        for f in &k.fields {
            out.push(Site { key: Attributes::field(&k.name, &f.name), target: Target::Property, name: format!("property {} of {}", f.name, k.name), attributes: &f.attributes });
        }
        for m in &k.methods {
            let (target, name) = if m.constructor { (Target::Constructor, format!("constructor {} of {}", m.rust, k.name)) } else { (Target::Method, format!("method {} of {}", m.name, k.name)) };
            out.push(Site { key: Attributes::method(&k.name, m.index), target, name, attributes: &m.attributes });
        }
    }
    out.retain(|s| !s.attributes.is_empty());
    out
}

/// The two shells: the target name their reads use, how a warning names
/// each, and the condition that is true in its compile alone.
const SHELLS: [(&str, &str, &str); 2] = [("net10.0", "PowerShell 7", "NET"), ("netstandard2.0", "Windows PowerShell", "!NET")];

/// Resolves every attribute of `module` against `targets`, the shells'
/// reference sets by target name, and returns each site's lines and the
/// warnings for attributes one shell leaves off.
pub fn resolve(module: &Module, work: &Path, targets: &[(&str, &[PathBuf])]) -> Result<(Attributes, Vec<String>), Error> {
    let sites = sites(module);
    if sites.is_empty() {
        return Ok((Attributes::default(), Vec::new()));
    }
    let mut names: Vec<String> = sites.iter().flat_map(|s| s.attributes.iter().map(|a| a.ty.clone())).collect();
    names.sort();
    names.dedup();
    let first = crate::members::read_as(work, "attributes", targets, &names)?;
    // The types the attributes' constructors and settable members take,
    // whose members text names, and the strings that may name a type for
    // a System.Type argument.
    let mut more: Vec<String> = Vec::new();
    for by_name in first.targets.values() {
        for name in &names {
            let Some(info) = by_name.get(name).filter(|i| i.found) else { continue };
            let mut takes_type = false;
            let taken = info.constructors.iter().flat_map(|c| c.params.iter().map(|p| p.ty.as_str())).chain(info.members.iter().filter(|m| settable(m)).map(|m| m.ty.as_str()));
            for ty in taken {
                let element = normal(ty.trim_end_matches("[]"));
                takes_type |= element == "System.Type";
                if !is_primitive(&element) && element != "System.Type" && !more.contains(&element) {
                    more.push(element);
                }
            }
            if takes_type {
                for a in sites.iter().flat_map(|s| s.attributes.iter()).filter(|a| &a.ty == name) {
                    for v in a.args.iter().chain(a.named.iter().map(|n| &n.value)) {
                        strings(v, &mut more);
                    }
                }
            }
        }
    }
    let second = if more.is_empty() { None } else { Some(crate::members::read_as(work, "arguments", targets, &more)?) };
    let types = Types { first: &first, second: second.as_ref() };

    let mut out = Attributes::default();
    let mut warnings = Vec::new();
    for site in &sites {
        let mut lines = Vec::new();
        for (i, a) in site.attributes.iter().enumerate() {
            let fail = |problem: String| Error::msg(format!("{}: attribute(type = \"{}\") {problem}", site.name, a.ty));
            let mut rendered: Vec<Option<String>> = Vec::new();
            for (shell, display, _) in SHELLS {
                rendered.push(match types.get(shell, &a.ty) {
                    Some(info) => Some(render(a, info, site.target, shell, &types).map_err(|problem| fail(format!("{problem} (read from {display}'s references)")))?),
                    None => None,
                });
            }
            if let Some(info) = SHELLS.iter().find_map(|(shell, _, _)| types.get(shell, &a.ty))
                && !info.usage.allow_multiple
                && site.attributes[..i].iter().any(|b| b.ty == a.ty)
            {
                return Err(fail("is given twice, and its [AttributeUsage] allows one per member".to_string()));
            }
            match (&rendered[0], &rendered[1]) {
                (Some(core), Some(desktop)) if core == desktop => lines.push(core.clone()),
                (Some(core), Some(desktop)) => lines.extend(["#if NET".to_string(), core.clone(), "#else".to_string(), desktop.clone(), "#endif".to_string()]),
                (Some(only), None) | (None, Some(only)) => {
                    let (have, lack, guard) = if rendered[0].is_some() { (SHELLS[0].1, SHELLS[1].1, SHELLS[0].2) } else { (SHELLS[1].1, SHELLS[0].1, SHELLS[1].2) };
                    warnings.push(format!("pwrs: {}: {} is in {have}'s references and not {lack}'s, so the {lack} shell leaves the attribute off", site.name, a.ty));
                    lines.extend([format!("#if {guard}"), only.clone(), "#endif".to_string()]);
                }
                (None, None) => return Err(fail("names a type that is in neither shell's references as a public attribute class".to_string())),
            }
        }
        out.lines.insert(site.key.clone(), lines);
    }
    Ok((out, warnings))
}

/// Every string in `v`, for the second read: one may name a type.
fn strings(v: &PassedValue, into: &mut Vec<String>) {
    match v {
        PassedValue::String { value } if !into.contains(value) => into.push(value.clone()),
        PassedValue::Array { items } => items.iter().for_each(|i| strings(i, into)),
        _not_a_string => {}
    }
}

/// The two reads, looked in together.
struct Types<'a> {
    first: &'a Read,
    second: Option<&'a Read>,
}

impl Types<'_> {
    /// The type `name` as `shell` reads it, when that shell has it.
    fn get(&self, shell: &str, name: &str) -> Option<&TypeInfo> {
        look(self.first, shell, name).or_else(|| self.second.and_then(|read| look(read, shell, name)))
    }
}

/// The type `name` as `shell` reads it in `read`, when found there.
fn look<'r>(read: &'r Read, shell: &str, name: &str) -> Option<&'r TypeInfo> {
    read.targets.get(shell).and_then(|by_name| by_name.get(name)).filter(|info| info.found)
}

/// Whether member `m` is one a named argument can set: a public property
/// with a public setter, or a public field neither static nor readonly.
fn settable(m: &crate::members::Member) -> bool {
    match m.kind.as_str() {
        "property" => m.set.as_ref().is_some_and(|s| s.access == "public"),
        "field" => !m.is_static && !m.readonly,
        _other_kind => false,
    }
}

/// A type's C# name as this module compares it: the keyword for a
/// primitive, the full name for any other.
fn normal(ty: &str) -> String {
    let t = ty.trim_start_matches("global::");
    let keyword = match t {
        "System.String" => "string",
        "System.Object" => "object",
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
        other => other,
    };
    keyword.to_string()
}

fn is_primitive(t: &str) -> bool {
    matches!(t, "string" | "object" | "bool" | "char" | "sbyte" | "byte" | "short" | "ushort" | "int" | "uint" | "long" | "ulong" | "float" | "double")
}

/// The C# for one attribute in one shell, or why it cannot be written.
fn render(a: &PassedAttribute, info: &TypeInfo, target: Target, shell: &str, types: &Types<'_>) -> Result<String, String> {
    if !info.bases.iter().any(|b| normal(b) == "System.Attribute") {
        return Err("names a type that is not an attribute class".to_string());
    }
    if info.is_abstract {
        return Err("names an abstract attribute class".to_string());
    }
    if info.usage.targets & target.bit() == 0 {
        return Err(format!("names an attribute its [AttributeUsage] does not allow on {}", target.noun()));
    }
    if normal(&info.name) == "System.ObsoleteAttribute" && matches!(a.args.get(1), Some(PassedValue::Bool { value: true })) {
        return Err(
            "passes true for error, which PowerShell ignores, writing its warning either way, and which makes C# refuse the shell's own uses of a parameter, a property, a class or a method; pass the message alone".to_string(),
        );
    }
    // Every public constructor the arguments fit: how many of its
    // parameters take object or spread a params array, its arguments as
    // C#, and its signature for messages.
    let mut fits: Vec<(usize, Vec<String>, String)> = Vec::new();
    for c in info.constructors.iter().filter(|c| c.access == "public") {
        let params = &c.params;
        let spreads = params.last().is_some_and(|p| p.params) && a.args.len() != params.len();
        let shape: Vec<String> = if spreads {
            if a.args.len() + 1 < params.len() {
                continue;
            }
            let fixed = params.len() - 1;
            let element = params[fixed].ty.trim_end_matches("[]").to_string();
            (0..a.args.len()).map(|i| if i < fixed { params[i].ty.clone() } else { element.clone() }).collect()
        } else if params.len() == a.args.len() {
            params.iter().map(|p| p.ty.clone()).collect()
        } else {
            continue;
        };
        let mut texts = Vec::new();
        for (v, ty) in a.args.iter().zip(&shape) {
            match convert(v, ty, shell, types)? {
                Some(text) => texts.push(text),
                None => break,
            }
        }
        if texts.len() == a.args.len() {
            let objects = params.iter().filter(|p| normal(&p.ty) == "object").count() + usize::from(spreads);
            let signature = params.iter().map(|p| p.ty.clone()).collect::<Vec<String>>().join(", ");
            fits.push((objects, texts, signature));
        }
    }
    let best = fits.iter().map(|(objects, _, _)| *objects).min();
    let chosen: Vec<&(usize, Vec<String>, String)> = fits.iter().filter(|(objects, _, _)| Some(*objects) == best).collect();
    let args = match chosen.as_slice() {
        [] => return Err(signatures("fits no public constructor", info)),
        [one] => one.1.clone(),
        several => {
            let names: Vec<String> = several.iter().map(|(_, _, s)| format!("({s})")).collect();
            return Err(format!("fits more than one constructor equally: {}", names.join(", ")));
        }
    };
    let mut named = Vec::new();
    for n in &a.named {
        let exact = info.members.iter().filter(|m| settable(m)).find(|m| m.name == n.name);
        let Some(member) = exact.or_else(|| info.members.iter().filter(|m| settable(m)).find(|m| m.name.eq_ignore_ascii_case(&n.name))) else {
            let settable: Vec<&str> = info.members.iter().filter(|m| settable(m)).map(|m| m.name.as_str()).collect();
            return Err(format!("names {} in named(...), which is no settable property or field of it; those are {}", n.name, if settable.is_empty() { "none".to_string() } else { settable.join(", ") }));
        };
        match convert(&n.value, &member.ty, shell, types)? {
            Some(text) => named.push(format!("{} = {text}", member.name)),
            None => return Err(format!("sets {} to a value its type {} does not take", member.name, member.ty)),
        }
    }
    let all: Vec<String> = args.into_iter().chain(named).collect();
    Ok(format!("[global::{}({})]", info.name.trim_start_matches("global::"), all.join(", ")))
}

/// A refusal followed by the public constructors there are.
fn signatures(problem: &str, info: &TypeInfo) -> String {
    let list: Vec<String> = info.constructors.iter().filter(|c| c.access == "public").map(|c| format!("({})", c.params.iter().map(|p| p.ty.clone()).collect::<Vec<String>>().join(", "))).collect();
    format!("{problem}; its public constructors are {}", if list.is_empty() { "none".to_string() } else { list.join(", ") })
}

/// `v` written as C# of type `ty`; `None` when that type does not take it.
/// Text names an enum member, or several joined by commas, where `ty` is
/// an enum, and a type where it is `System.Type`.
fn convert(v: &PassedValue, ty: &str, shell: &str, types: &Types<'_>) -> Result<Option<String>, String> {
    let t = normal(ty);
    if let Some(element) = t.strip_suffix("[]") {
        return match v {
            PassedValue::Null => Ok(Some("null".to_string())),
            PassedValue::Array { items } => {
                let mut texts = Vec::new();
                for i in items {
                    match convert(i, element, shell, types)? {
                        Some(text) => texts.push(text),
                        None => return Ok(None),
                    }
                }
                Ok(Some(format!("new {}[] {{ {} }}", cs_type(element), texts.join(", "))))
            }
            _single_value => Ok(None),
        };
    }
    let enumeration = types.get(shell, &t).filter(|info| info.kind == "enum");
    Ok(match (v, t.as_str()) {
        (PassedValue::String { value }, "string" | "object") => Some(verbatim(value)),
        (PassedValue::String { value }, "System.Type") => types.get(shell, value).map(|info| format!("typeof(global::{})", info.name.trim_start_matches("global::"))),
        (PassedValue::String { value }, _) => match enumeration {
            Some(info) => {
                let mut parts = Vec::new();
                for part in value.split(',').map(str::trim) {
                    match info.members.iter().find(|m| m.kind == "field" && m.value.is_some() && m.name.eq_ignore_ascii_case(part)) {
                        Some(m) => parts.push(format!("global::{}.{}", info.name.trim_start_matches("global::"), m.name)),
                        None => return Ok(None),
                    }
                }
                Some(parts.join(" | "))
            }
            None => None,
        },
        (PassedValue::Int { text }, "sbyte" | "byte" | "short" | "ushort" | "int" | "uint" | "long" | "ulong") => fits_integer(text, &t)?.then(|| text.clone()),
        (PassedValue::Int { text }, "float") => Some(format!("{text}f")),
        (PassedValue::Int { text }, "double" | "object") => Some(text.clone()),
        (PassedValue::Int { text }, _) => enumeration.map(|info| format!("(global::{}){text}", info.name.trim_start_matches("global::"))),
        (PassedValue::Float { text }, "float") => Some(format!("{text}f")),
        (PassedValue::Float { text }, "double" | "object") => Some(text.clone()),
        (PassedValue::Bool { value }, "bool" | "object") => Some(value.to_string()),
        (PassedValue::Char { value }, "char" | "object") => value.chars().next().filter(|c| (*c as u32) <= 0xFFFF).map(|c| format!("'\\u{:04X}'", c as u32)),
        (PassedValue::Null, "string" | "object" | "System.Type") => Some("null".to_string()),
        (PassedValue::Null, _) => types.get(shell, &t).filter(|info| info.kind == "class" || info.kind == "interface").map(|_| "null".to_string()),
        (PassedValue::Array { items }, "object") => {
            let mut texts = Vec::new();
            for i in items {
                match convert(i, "object", shell, types)? {
                    Some(text) => texts.push(text),
                    None => return Ok(None),
                }
            }
            Some(format!("new object[] {{ {} }}", texts.join(", ")))
        }
        _other => None,
    })
}

/// Whether the integer `text`, the digits and sign the macro wrote, lies
/// in the range of the primitive `t`.
fn fits_integer(text: &str, t: &str) -> Result<bool, String> {
    let n: i128 = text.parse().map_err(|e| format!("is the integer {text}, which is beyond every integer type: {e}"))?;
    let (min, max): (i128, i128) = match t {
        "sbyte" => (i8::MIN.into(), i8::MAX.into()),
        "byte" => (0, u8::MAX.into()),
        "short" => (i16::MIN.into(), i16::MAX.into()),
        "ushort" => (0, u16::MAX.into()),
        "int" => (i32::MIN.into(), i32::MAX.into()),
        "uint" => (0, u32::MAX.into()),
        "long" => (i64::MIN.into(), i64::MAX.into()),
        "ulong" => (0, u64::MAX.into()),
        other => return Err(format!("checks whether an integer fits {other}, which is no integer type")),
    };
    Ok((min..=max).contains(&n))
}

/// A type's name as C# writes it in `new T[]`: a keyword, or rooted at
/// `global::`.
fn cs_type(t: &str) -> String {
    if is_primitive(t) { t.to_string() } else { format!("global::{}", t.trim_start_matches("global::")) }
}

/// `s` as a C# verbatim string.
fn verbatim(s: &str) -> String {
    format!("@\"{}\"", s.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// The net10.0 types, as members.ps1 writes them.
    fn read(types: Value) -> Read {
        serde_json::from_value(json!({ "targets": { "net10.0": types } })).expect("a read")
    }

    fn passed(attribute: Value) -> PassedAttribute {
        serde_json::from_value(attribute).expect("an attribute")
    }

    fn param(ty: &str) -> Value {
        json!({ "name": "p", "type": ty, "mode": "", "optional": false })
    }

    /// A public attribute class allowed on `targets`, with a constructor
    /// for each parameter list, a settable Message and a read-only IsError.
    fn attribute_class(name: &str, targets: i64, constructors: Vec<Vec<Value>>) -> Value {
        let constructors: Vec<Value> = constructors.into_iter().map(|params| json!({ "access": "public", "params": params })).collect();
        json!({
            "found": true, "name": name, "kind": "class",
            "bases": ["System.Attribute", "System.Object"],
            "usage": { "targets": targets, "allow_multiple": false },
            "constructors": constructors,
            "members": [
                { "kind": "property", "name": "Message", "from": name, "type": "string", "set": { "access": "public", "slot": "none" } },
                { "kind": "property", "name": "IsError", "from": name, "type": "bool" }
            ]
        })
    }

    /// `attribute` rendered on `target` against `types`.
    fn rendered(types: &Read, attribute: Value, target: Target) -> Result<String, String> {
        let a = passed(attribute);
        let found = Types { first: types, second: None };
        let info = found.get("net10.0", &a.ty).expect("the attribute's class");
        render(&a, info, target, "net10.0", &found)
    }

    #[test]
    fn an_attribute_is_written_through_the_constructor_its_arguments_fit() {
        let types = read(json!({ "System.ObsoleteAttribute": attribute_class("System.ObsoleteAttribute", 32767, vec![vec![], vec![param("string")], vec![param("string"), param("bool")]]) }));
        let obsolete = |args: Value, named: Value| rendered(&types, json!({ "type": "System.ObsoleteAttribute", "args": args, "named": named }), Target::Method);
        assert_eq!(obsolete(json!([]), json!([])).expect("no arguments"), "[global::System.ObsoleteAttribute()]");
        assert_eq!(obsolete(json!([{ "kind": "string", "value": "Use Next." }]), json!([])).expect("a message"), "[global::System.ObsoleteAttribute(@\"Use Next.\")]");
        assert_eq!(
            obsolete(json!([]), json!([{ "name": "message", "value": { "kind": "string", "value": "m" } }])).expect("a named message"),
            "[global::System.ObsoleteAttribute(Message = @\"m\")]"
        );
        let refused = obsolete(json!([{ "kind": "string", "value": "m" }, { "kind": "bool", "value": true }]), json!([])).expect_err("error true");
        assert!(refused.contains("PowerShell ignores"), "{refused}");
        let refused = obsolete(json!([]), json!([{ "name": "IsError", "value": { "kind": "bool", "value": true } }])).expect_err("a read-only member");
        assert!(refused.contains("no settable property or field of it; those are Message"), "{refused}");
        let refused = obsolete(json!([{ "kind": "int", "text": "1" }]), json!([])).expect_err("an integer for a string");
        assert!(refused.contains("fits no public constructor; its public constructors are (), (string), (string, bool)"), "{refused}");
    }

    #[test]
    fn a_params_array_takes_its_elements_as_separate_arguments() {
        let mut spread = param("string[]");
        spread["params"] = json!(true);
        let types = read(json!({ "Demo.DrivesAttribute": attribute_class("Demo.DrivesAttribute", 128, vec![vec![spread]]) }));
        let drives = |args: Value| rendered(&types, json!({ "type": "Demo.DrivesAttribute", "args": args }), Target::Property);
        assert_eq!(drives(json!([])).expect("no drives"), "[global::Demo.DrivesAttribute()]");
        assert_eq!(
            drives(json!([{ "kind": "string", "value": "Env" }, { "kind": "string", "value": "Variable" }])).expect("two drives"),
            "[global::Demo.DrivesAttribute(@\"Env\", @\"Variable\")]"
        );
        assert_eq!(
            drives(json!([{ "kind": "array", "items": [{ "kind": "string", "value": "Env" }] }])).expect("the array itself"),
            "[global::Demo.DrivesAttribute(new string[] { @\"Env\" })]"
        );
    }

    #[test]
    fn text_names_an_enum_member_whatever_its_case() {
        let types = read(json!({
            "Demo.ShownAttribute": attribute_class("Demo.ShownAttribute", 32767, vec![vec![param("string"), param("Demo.Action")]]),
            "Demo.Action": { "found": true, "name": "Demo.Action", "kind": "enum", "members": [
                { "kind": "field", "name": "Hide", "from": "Demo.Action", "static": true, "readonly": true, "value": 1 },
                { "kind": "field", "name": "Show", "from": "Demo.Action", "static": true, "readonly": true, "value": 2 }
            ] }
        }));
        let shown = |action: &str| rendered(&types, json!({ "type": "Demo.ShownAttribute", "args": [{ "kind": "string", "value": "Demo.Feature" }, { "kind": "string", "value": action }] }), Target::Class);
        assert_eq!(shown("show").expect("one member"), "[global::Demo.ShownAttribute(@\"Demo.Feature\", global::Demo.Action.Show)]");
        assert_eq!(shown("Hide, Show").expect("two members"), "[global::Demo.ShownAttribute(@\"Demo.Feature\", global::Demo.Action.Hide | global::Demo.Action.Show)]");
        let refused = shown("Visible").expect_err("no such member");
        assert!(refused.contains("fits no public constructor"), "{refused}");
    }

    #[test]
    fn a_class_that_is_no_attribute_a_target_its_usage_refuses_and_an_even_choice_are_refused() {
        let mut plain = attribute_class("Demo.Plain", 32767, vec![vec![]]);
        plain["bases"] = json!(["System.Object"]);
        let types = read(json!({
            "Demo.Plain": plain,
            "Demo.OnPropertyAttribute": attribute_class("Demo.OnPropertyAttribute", 128, vec![vec![]]),
            "Demo.EitherAttribute": attribute_class("Demo.EitherAttribute", 32767, vec![vec![param("int")], vec![param("long")]]),
        }));
        let refused = rendered(&types, json!({ "type": "Demo.Plain" }), Target::Class).expect_err("not an attribute");
        assert!(refused.contains("is not an attribute class"), "{refused}");
        let refused = rendered(&types, json!({ "type": "Demo.OnPropertyAttribute" }), Target::Constructor).expect_err("on a constructor");
        assert!(refused.contains("does not allow on a constructor"), "{refused}");
        assert_eq!(rendered(&types, json!({ "type": "Demo.OnPropertyAttribute" }), Target::Property).expect("on a property"), "[global::Demo.OnPropertyAttribute()]");
        let refused = rendered(&types, json!({ "type": "Demo.EitherAttribute", "args": [{ "kind": "int", "text": "7" }] }), Target::Class).expect_err("two constructors fit");
        assert!(refused.contains("fits more than one constructor equally: (int), (long)"), "{refused}");
    }

    #[test]
    fn a_directive_starts_its_line_and_an_attribute_takes_the_indent() {
        let mut attributes = Attributes::default();
        attributes.lines.insert(Attributes::cmdlet(3), vec!["#if NET".to_string(), "[global::A()]".to_string(), "#endif".to_string()]);
        assert_eq!(attributes.before(&Attributes::cmdlet(3), "    "), "#if NET\n    [global::A()]\n#endif\n");
        assert_eq!(attributes.before(&Attributes::cmdlet(4), "    "), "");
    }

    #[test]
    fn text_is_written_verbatim_and_an_integer_only_where_it_fits() {
        assert_eq!(verbatim("say \"hi\"\\"), "@\"say \"\"hi\"\"\\\"");
        assert!(fits_integer("255", "byte").expect("digits"));
        assert!(!fits_integer("256", "byte").expect("digits"));
        assert!(!fits_integer("-1", "uint").expect("digits"));
        assert!(fits_integer("-9223372036854775808", "long").expect("digits"));
        assert!(fits_integer("18446744073709551615", "ulong").expect("digits"));
        assert!(fits_integer("1", "float").is_err());
        assert_eq!(normal("global::System.Int32"), "int");
        assert_eq!(cs_type("System.Type"), "global::System.Type");
    }
}
