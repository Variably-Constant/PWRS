//! The `attribute(...)` key: a .NET attribute the generated shell puts on
//! a cmdlet, a parameter, a class, a property or a method. Each argument
//! is a Rust literal. The build resolves the attribute's type in both
//! shells' references and converts each argument as the constructor
//! parameter or the member it lands in takes it, so text names an enum
//! member or a type where the attribute takes one.
//!
//! ```text
//! attribute(type = "System.ObsoleteAttribute", args = ["Use -New."])
//! attribute(type = "System.Management.Automation.PSDefaultValueAttribute", named(Help = "the current folder"))
//! ```

use crate::json::{self, Obj};
use crate::params::lit_str;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, ExprLit, Ident, Lit, MetaNameValue, Result, Token, UnOp};

/// One attribute: its type's full name, then its positional and named
/// arguments in the order written.
#[derive(Debug)]
pub struct Passed {
    pub ty: String,
    pub args: Vec<Value>,
    pub named: Vec<(String, Value)>,
}

/// An argument as written.
#[derive(Debug, PartialEq)]
pub enum Value {
    Str(String),
    /// An integer's digits, with its sign.
    Int(String),
    /// A floating-point number's digits, with its sign.
    Float(String),
    Bool(bool),
    Char(char),
    Null,
    Array(Vec<Value>),
}

const VALUE_KINDS: &str = "an attribute argument is a string, a number, true or false, a char, null, or an array of them; text names an enum member or a type where the attribute takes one";

/// One part of an `attribute(...)` entry: `key = value`, with `type`
/// read as a key although it is a keyword, or `key(...)` of named
/// arguments.
enum Part {
    Pair(Ident, Expr),
    List(Ident, Punctuated<MetaNameValue, Token![,]>),
}

impl Parse for Part {
    fn parse(input: ParseStream) -> Result<Self> {
        let key = Ident::parse_any(input)?;
        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            Ok(Part::Pair(key, input.parse()?))
        } else if input.peek(syn::token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            Ok(Part::List(key, Punctuated::parse_terminated(&content)?))
        } else {
            Err(syn::Error::new(key.span(), "attribute(...) takes type = \"Namespace.NameAttribute\", args = [...] and named(Member = value)"))
        }
    }
}

/// Parses an `attribute(...)` entry, or a method's `#[attribute(...)]`.
pub fn parse(list: &syn::MetaList) -> Result<Passed> {
    let parts: Punctuated<Part, Token![,]> = list.parse_args_with(Punctuated::parse_terminated)?;
    let mut ty = None;
    let mut args = Vec::new();
    let mut named: Vec<(String, Value)> = Vec::new();
    for part in parts {
        match part {
            Part::Pair(key, value_expr) if key == "type" => {
                let name = lit_str(&value_expr)?;
                if !is_type_name(&name) {
                    return Err(syn::Error::new_spanned(&value_expr, format!("type names an attribute class by its full .NET name, such as \"System.ObsoleteAttribute\"; `{name}` is not one")));
                }
                ty = Some(name);
            }
            Part::Pair(key, value_expr) if key == "args" => {
                args = match &value_expr {
                    Expr::Array(a) => a.elems.iter().map(value).collect::<Result<Vec<Value>>>()?,
                    single => vec![value(single)?],
                };
            }
            Part::List(key, pairs) if key == "named" => {
                for nv in pairs {
                    let Some(name) = nv.path.get_ident().map(|i| i.to_string()) else {
                        return Err(syn::Error::new_spanned(&nv.path, "a named argument is one property or field of the attribute"));
                    };
                    if named.iter().any(|(n, _)| *n == name) {
                        return Err(syn::Error::new_spanned(&nv.path, format!("named(...) sets {name} twice")));
                    }
                    named.push((name, value(&nv.value)?));
                }
            }
            Part::Pair(key, _) | Part::List(key, _) => {
                return Err(syn::Error::new(key.span(), "attribute(...) takes type = \"Namespace.NameAttribute\", args = [...] and named(Member = value)"));
            }
        }
    }
    let ty = ty.ok_or_else(|| syn::Error::new_spanned(list, "attribute(...) needs type = \"Namespace.NameAttribute\""))?;
    Ok(Passed { ty, args, named })
}

/// Whether `name` is a .NET type's full name: identifiers joined by `.`,
/// and by `+` for a nested type.
fn is_type_name(name: &str) -> bool {
    !name.is_empty()
        && name.split(['.', '+']).all(|part| {
            let mut chars = part.chars();
            chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
        })
}

/// One argument, as written.
fn value(expr: &Expr) -> Result<Value> {
    match expr {
        Expr::Lit(ExprLit { lit, .. }) => match lit {
            Lit::Str(s) => Ok(Value::Str(s.value())),
            Lit::Int(i) => Ok(Value::Int(i.base10_digits().to_string())),
            Lit::Float(f) => Ok(Value::Float(f.base10_digits().to_string())),
            Lit::Bool(b) => Ok(Value::Bool(b.value)),
            Lit::Char(c) => Ok(Value::Char(c.value())),
            other => Err(syn::Error::new_spanned(other, VALUE_KINDS)),
        },
        Expr::Unary(u) if matches!(u.op, UnOp::Neg(_)) => match value(&u.expr)? {
            Value::Int(digits) => Ok(Value::Int(format!("-{digits}"))),
            Value::Float(digits) => Ok(Value::Float(format!("-{digits}"))),
            _not_a_number => Err(syn::Error::new_spanned(expr, "only a number takes a minus sign")),
        },
        Expr::Array(a) => Ok(Value::Array(a.elems.iter().map(value).collect::<Result<Vec<Value>>>()?)),
        Expr::Path(p) if p.path.is_ident("null") => Ok(Value::Null),
        other => Err(syn::Error::new_spanned(other, VALUE_KINDS)),
    }
}

/// The descriptor's `attributes` array.
pub fn json<'a>(passed: impl IntoIterator<Item = &'a Passed>) -> String {
    let items: Vec<String> = passed
        .into_iter()
        .map(|p| {
            let named: Vec<String> = p
                .named
                .iter()
                .map(|(name, v)| {
                    let mut o = Obj::new();
                    o.str("name", name).raw("value", &value_json(v));
                    o.finish()
                })
                .collect();
            let mut o = Obj::new();
            o.str("type", &p.ty).raw("args", &json::array(&p.args.iter().map(value_json).collect::<Vec<String>>())).raw("named", &json::array(&named));
            o.finish()
        })
        .collect();
    json::array(&items)
}

fn value_json(v: &Value) -> String {
    let mut o = Obj::new();
    match v {
        Value::Str(s) => {
            o.str("kind", "string").str("value", s);
        }
        Value::Int(digits) => {
            o.str("kind", "int").str("text", digits);
        }
        Value::Float(digits) => {
            o.str("kind", "float").str("text", digits);
        }
        Value::Bool(b) => {
            o.str("kind", "bool").bool("value", *b);
        }
        Value::Char(c) => {
            o.str("kind", "char").str("value", &c.to_string());
        }
        Value::Null => {
            o.str("kind", "null");
        }
        Value::Array(items) => {
            o.str("kind", "array").raw("items", &json::array(&items.iter().map(value_json).collect::<Vec<String>>()));
        }
    }
    o.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::Meta;

    fn entry(tokens: &str) -> Result<Passed> {
        let meta: Meta = syn::parse_str(tokens).expect("an attribute entry");
        match &meta {
            Meta::List(l) => parse(l),
            Meta::Path(_) | Meta::NameValue(_) => panic!("the entry under test is not a list: {tokens}"),
        }
    }

    #[test]
    fn an_entry_reads_its_type_and_every_argument_kind() {
        let p = entry(r#"attribute(type = "Some.ThingAttribute", args = ["text", -3, 1.5, true, 'c', null, ["a", "b"]], named(Help = "words", Count = 2))"#).expect("the entry");
        assert_eq!(p.ty, "Some.ThingAttribute");
        assert_eq!(
            p.args,
            [
                Value::Str("text".into()),
                Value::Int("-3".into()),
                Value::Float("1.5".into()),
                Value::Bool(true),
                Value::Char('c'),
                Value::Null,
                Value::Array(vec![Value::Str("a".into()), Value::Str("b".into())])
            ]
        );
        assert_eq!(p.named, [("Help".to_string(), Value::Str("words".into())), ("Count".to_string(), Value::Int("2".into()))]);
        assert_eq!(
            json(&[p]),
            r#"[{"type":"Some.ThingAttribute","args":[{"kind":"string","value":"text"},{"kind":"int","text":"-3"},{"kind":"float","text":"1.5"},{"kind":"bool","value":true},{"kind":"char","value":"c"},{"kind":"null"},{"kind":"array","items":[{"kind":"string","value":"a"},{"kind":"string","value":"b"}]}],"named":[{"name":"Help","value":{"kind":"string","value":"words"}},{"name":"Count","value":{"kind":"int","text":"2"}}]}]"#
        );
    }

    #[test]
    fn a_single_argument_needs_no_brackets_and_a_nested_type_its_plus() {
        let p = entry(r#"attribute(type = "Outer+InnerAttribute", args = "one")"#).expect("the entry");
        assert_eq!((p.ty.as_str(), p.args), ("Outer+InnerAttribute", vec![Value::Str("one".into())]));
    }

    #[test]
    fn a_malformed_entry_is_refused_saying_what_it_takes() {
        let refusal = |tokens: &str| entry(tokens).expect_err("refused").to_string();
        assert!(refusal(r#"attribute(args = ["x"])"#).contains("needs type"));
        assert!(refusal(r#"attribute(type = "System.Obsolete Attribute")"#).contains("full .NET name"));
        assert!(refusal(r#"attribute(type = "A.BAttribute", args = [some_variable])"#).contains("an attribute argument is a string"));
        assert!(refusal(r#"attribute(type = "A.BAttribute", args = [-"x"])"#).contains("only a number"));
        assert!(refusal(r#"attribute(type = "A.BAttribute", named(Help = "a", Help = "b"))"#).contains("sets Help twice"));
        assert!(refusal(r#"attribute(type = "A.BAttribute", target = "x")"#).contains("takes type"));
    }
}
