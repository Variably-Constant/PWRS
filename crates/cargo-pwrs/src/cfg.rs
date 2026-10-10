//! cargo's `cfg(...)` expressions, which scope a `license-files` table
//! to the targets it covers, and the target's cfg they are matched
//! against.
//!
//! The grammar is the one cargo-platform 0.3 reads a
//! `[target.'cfg(...)']` key with: `all(...)`, `any(...)` and `not(...)`
//! over names such as `unix` and pairs such as `target_os = "linux"`,
//! with `true` and `false`, raw identifiers such as `r#async`, and tokens
//! separated by spaces. A target's cfg is what `rustc --print cfg` prints
//! for it, each line read as a name or a pair the same way.

use crate::Error;

/// A cfg value: a name such as `unix`, or a key and its value such as
/// `target_os = "linux"`. A raw identifier is held without its `r#`,
/// which cargo does not compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cfg {
    Name(String),
    Pair(String, String),
}

/// A cfg expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Not(Box<Expr>),
    All(Vec<Expr>),
    Any(Vec<Expr>),
    Value(Cfg),
    True,
    False,
}

impl Expr {
    /// Whether a target whose cfg is `target` satisfies the expression.
    /// An empty `all()` is satisfied and an empty `any()` is not.
    pub fn matches(&self, target: &[Cfg]) -> bool {
        match self {
            Expr::Not(e) => !e.matches(target),
            Expr::All(es) => es.iter().all(|e| e.matches(target)),
            Expr::Any(es) => es.iter().any(|e| e.matches(target)),
            Expr::Value(c) => target.contains(c),
            Expr::True => true,
            Expr::False => false,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Token<'a> {
    LeftParen,
    RightParen,
    Comma,
    Equals,
    /// An identifier, and whether it was written raw, as `r#name`.
    Ident(&'a str, bool),
    Str(&'a str),
}

impl Token<'_> {
    fn describe(&self) -> String {
        match self {
            Token::LeftParen => "`(`".to_string(),
            Token::RightParen => "`)`".to_string(),
            Token::Comma => "`,`".to_string(),
            Token::Equals => "`=`".to_string(),
            Token::Ident(name, _raw) => format!("`{name}`"),
            Token::Str(text) => format!("\"{text}\""),
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_ident_rest(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

/// Splits an expression into tokens. Only a space separates them, as in
/// cargo, so a tab or a line break is refused; a string runs to the next
/// `"` with no escapes.
fn tokens(text: &str) -> Result<Vec<Token<'_>>, String> {
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            ' ' => {}
            '(' => out.push(Token::LeftParen),
            ')' => out.push(Token::RightParen),
            ',' => out.push(Token::Comma),
            '=' => out.push(Token::Equals),
            '"' => match chars.find(|&(_, d)| d == '"') {
                Some((end, _quote)) => out.push(Token::Str(&text[at + 1..end])),
                None => return Err("a string is not closed".to_string()),
            },
            c if is_ident_start(c) => {
                let raw = c == 'r' && chars.next_if(|&(_, d)| d == '#').is_some();
                let start = if raw {
                    match chars.next() {
                        Some((first, d)) if is_ident_start(d) => first,
                        Some((_, d)) => return Err(format!("{d:?} cannot begin the identifier after `r#`")),
                        None => return Err("the expression ends after `r#`".to_string()),
                    }
                } else {
                    at
                };
                // Identifier characters are ASCII, one byte each.
                let mut end = start + 1;
                while let Some((i, _)) = chars.next_if(|&(_, d)| is_ident_rest(d)) {
                    end = i + 1;
                }
                out.push(Token::Ident(&text[start..end], raw));
            }
            other => return Err(format!("{other:?} cannot appear in a cfg expression")),
        }
    }
    Ok(out)
}

/// Recursive descent over `tokens`, as cargo's parser walks them.
struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    at: usize,
}

impl Parser<'_> {
    /// Steps over `token` when it is next, and says whether it was.
    fn skip(&mut self, token: &Token<'_>) -> bool {
        let next = self.tokens.get(self.at) == Some(token);
        if next {
            self.at += 1;
        }
        next
    }

    fn expect(&mut self, token: &Token<'_>) -> Result<(), String> {
        match self.tokens.get(self.at) {
            Some(t) if t == token => {
                self.at += 1;
                Ok(())
            }
            Some(t) => Err(format!("{} was expected where it says {}", token.describe(), t.describe())),
            None => Err(format!("the expression ends where {} belongs", token.describe())),
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        match self.tokens.get(self.at) {
            Some(Token::Ident(op @ ("all" | "any"), false)) => {
                let all = *op == "all";
                self.at += 1;
                self.expect(&Token::LeftParen)?;
                let mut items = Vec::new();
                while !self.skip(&Token::RightParen) {
                    items.push(self.expr()?);
                    if !self.skip(&Token::Comma) {
                        self.expect(&Token::RightParen)?;
                        break;
                    }
                }
                Ok(if all { Expr::All(items) } else { Expr::Any(items) })
            }
            Some(Token::Ident("not", false)) => {
                self.at += 1;
                self.expect(&Token::LeftParen)?;
                let inner = self.expr()?;
                self.expect(&Token::RightParen)?;
                Ok(Expr::Not(Box::new(inner)))
            }
            Some(_value) => Ok(match self.value()? {
                Cfg::Name(name) if name == "true" => Expr::True,
                Cfg::Name(name) if name == "false" => Expr::False,
                value => Expr::Value(value),
            }),
            None => Err("the expression ends where a predicate belongs".to_string()),
        }
    }

    fn value(&mut self) -> Result<Cfg, String> {
        let name = match self.tokens.get(self.at) {
            Some(Token::Ident(name, _raw)) => name.to_string(),
            Some(t) => return Err(format!("an identifier was expected where it says {}", t.describe())),
            None => return Err("the expression ends where an identifier belongs".to_string()),
        };
        self.at += 1;
        if !self.skip(&Token::Equals) {
            return Ok(Cfg::Name(name));
        }
        match self.tokens.get(self.at) {
            Some(Token::Str(text)) => {
                let text = text.to_string();
                self.at += 1;
                Ok(Cfg::Pair(name, text))
            }
            Some(t) => Err(format!("a string was expected where it says {}", t.describe())),
            None => Err("the expression ends where a string belongs".to_string()),
        }
    }

    /// Refuses whatever follows a complete expression or value.
    fn end(&self) -> Result<(), String> {
        match self.tokens.get(self.at) {
            Some(extra) => Err(format!("{} follows a complete expression", extra.describe())),
            None => Ok(()),
        }
    }
}

/// The expression in a key written `cfg(...)`, as cargo writes a
/// `[target.'cfg(...)']` key, or `None` for a key written any other way.
pub fn key_expression(key: &str) -> Option<&str> {
    key.strip_prefix("cfg(").and_then(|rest| rest.strip_suffix(')'))
}

/// Parses an expression, the text inside `cfg(...)`.
pub fn parse(text: &str) -> Result<Expr, String> {
    let mut p = Parser { tokens: tokens(text)?, at: 0 };
    let expr = p.expr()?;
    p.end()?;
    Ok(expr)
}

/// The first name or key in `expr` that describes the compilation
/// rather than the target: `test`, `debug_assertions`, `proc_macro` or
/// `feature`, which cargo warns cannot select a target's dependencies.
pub fn non_target_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Not(e) => non_target_name(e),
        Expr::All(es) | Expr::Any(es) => es.iter().find_map(non_target_name),
        Expr::Value(Cfg::Name(name)) if ["test", "debug_assertions", "proc_macro"].contains(&name.as_str()) => Some(name),
        Expr::Value(Cfg::Pair(key, _value)) if key == "feature" => Some(key),
        Expr::Value(_) | Expr::True | Expr::False => None,
    }
}

/// A target's cfg values from what `rustc --print cfg` prints for it,
/// one per line.
pub fn target(printed: &str) -> Result<Vec<Cfg>, Error> {
    let mut values = Vec::new();
    for line in printed.lines().filter(|l| !l.trim().is_empty()) {
        let value = tokens(line).and_then(|tokens| {
            let mut p = Parser { tokens, at: 0 };
            let value = p.value()?;
            p.end()?;
            Ok(value)
        });
        values.push(value.map_err(|why| Error::msg(format!("rustc --print cfg printed `{line}`, which is not a cfg value: {why}")))?);
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(n: &str) -> Expr {
        Expr::Value(Cfg::Name(n.to_string()))
    }

    fn pair(key: &str, value: &str) -> Expr {
        Expr::Value(Cfg::Pair(key.to_string(), value.to_string()))
    }

    fn parsed(text: &str) -> Expr {
        parse(text).unwrap_or_else(|e| panic!("{text:?} did not parse: {e}"))
    }

    #[test]
    fn expressions_parse_as_cargo_reads_them() {
        assert_eq!(parsed("unix"), name("unix"));
        assert_eq!(parsed("target_os = \"linux\""), pair("target_os", "linux"));
        assert_eq!(parsed("target_os=\"linux\""), pair("target_os", "linux"));
        assert_eq!(parsed("target_abi = \"\""), pair("target_abi", ""));
        assert_eq!(parsed("  not( windows )  "), Expr::Not(Box::new(name("windows"))));
        assert_eq!(
            parsed("any(target_os = \"linux\", target_os = \"freebsd\",)"),
            Expr::Any(vec![pair("target_os", "linux"), pair("target_os", "freebsd")]),
            "a list may end with a comma"
        );
        assert_eq!(parsed("all()"), Expr::All(Vec::new()));
        assert_eq!(parsed("any(  )"), Expr::Any(Vec::new()));
        assert_eq!(
            parsed("all(unix, not(any(target_os = \"macos\", target_os = \"ios\")))"),
            Expr::All(vec![name("unix"), Expr::Not(Box::new(Expr::Any(vec![pair("target_os", "macos"), pair("target_os", "ios")])))])
        );
        assert_eq!(parsed("true"), Expr::True);
        assert_eq!(parsed("false"), Expr::False);
        assert_eq!(parsed("r#async"), name("async"));
        assert_eq!(parsed("r#all"), name("all"), "a raw all is a name, not the operator");
        assert_eq!(parsed("rust"), name("rust"));
    }

    #[test]
    fn malformed_expressions_are_refused() {
        for bad in [
            "",
            "   ",
            "all(",
            "all(unix",
            "all(unix,",
            "any(,)",
            "all(unix))",
            "not()",
            "not(unix, windows)",
            "not unix",
            "unix windows",
            "unix,",
            "foo(bar)",
            "(unix)",
            "target_os =",
            "target_os = linux",
            "target_os = \"linux",
            "\"linux\"",
            "unix\t",
            "any(unix,\nwindows)",
            "1unix",
            "r#",
            "r#1",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} parsed as {:?}", parse(bad));
        }
    }

    #[test]
    fn an_expression_matches_the_targets_cfg() {
        let linux = target("debug_assertions\ntarget_abi=\"\"\ntarget_family=\"unix\"\ntarget_os=\"linux\"\nunix\n").expect("read the cfg");
        let yes = |text: &str| parsed(text).matches(&linux);
        assert!(yes("unix"));
        assert!(!yes("windows"));
        assert!(yes("target_os = \"linux\""));
        assert!(!yes("target_os = \"Linux\""), "values compare exactly");
        assert!(yes("target_abi = \"\""));
        assert!(yes("any(windows, target_os = \"linux\")"));
        assert!(!yes("all(unix, target_os = \"macos\")"));
        assert!(yes("not(windows)"));
        assert!(yes("all()"));
        assert!(!yes("any()"));
        assert!(yes("true"));
        assert!(!yes("false"));
        assert!(yes("r#unix"), "a raw identifier matches its plain name");
        assert!(target("target_os=linux\n").is_err(), "an unquoted value is not a cfg value");
    }

    #[test]
    fn names_that_describe_the_compilation_are_found() {
        assert_eq!(non_target_name(&parsed("unix")), None);
        assert_eq!(non_target_name(&parsed("target_feature = \"avx2\"")), None);
        assert_eq!(non_target_name(&parsed("all(unix, not(debug_assertions))")), Some("debug_assertions"));
        assert_eq!(non_target_name(&parsed("any(test, windows)")), Some("test"));
        assert_eq!(non_target_name(&parsed("proc_macro")), Some("proc_macro"));
        assert_eq!(non_target_name(&parsed("feature = \"gpu\"")), Some("feature"));
    }

    /// rustc's cfg for each target a module ships for reads without
    /// error, and the expressions a table is scoped by pick the targets
    /// they name.
    #[test]
    fn each_shipped_targets_cfg_is_read_from_rustc() {
        let targets = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "x86_64-unknown-freebsd", "aarch64-apple-darwin"];
        let cfgs: Vec<(&str, Vec<Cfg>)> = targets
            .iter()
            .map(|t| (*t, target(&crate::cpu::print_cfg(Some(t)).expect("run rustc --print cfg")).expect("read rustc's cfg")))
            .collect();
        let picks = |text: &str| {
            let expr = parsed(text);
            cfgs.iter().filter(|(_, cfg)| expr.matches(cfg)).map(|(t, _)| *t).collect::<Vec<_>>()
        };
        assert_eq!(picks("windows"), ["x86_64-pc-windows-msvc"]);
        assert_eq!(picks("unix"), ["x86_64-unknown-linux-gnu", "x86_64-unknown-freebsd", "aarch64-apple-darwin"]);
        assert_eq!(picks("any(target_os = \"linux\", target_os = \"freebsd\")"), ["x86_64-unknown-linux-gnu", "x86_64-unknown-freebsd"]);
        assert_eq!(picks("all(unix, not(target_os = \"macos\"))"), ["x86_64-unknown-linux-gnu", "x86_64-unknown-freebsd"]);
        assert_eq!(picks("target_os = \"macos\""), ["aarch64-apple-darwin"]);
    }
}
