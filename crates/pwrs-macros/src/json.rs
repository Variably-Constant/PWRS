//! Minimal JSON writer for descriptors; no dependency, no parsing.

pub struct Obj {
    buf: String,
    first: bool,
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Obj {
    pub fn new() -> Self {
        Obj { buf: String::from("{"), first: true }
    }

    fn key(&mut self, k: &str) {
        if !self.first {
            self.buf.push(',');
        }
        self.first = false;
        self.buf.push_str(&escape(k));
        self.buf.push(':');
    }

    pub fn str(&mut self, k: &str, v: &str) -> &mut Self {
        self.key(k);
        self.buf.push_str(&escape(v));
        self
    }

    pub fn opt_str(&mut self, k: &str, v: Option<&str>) -> &mut Self {
        self.key(k);
        match v {
            Some(s) => self.buf.push_str(&escape(s)),
            None => self.buf.push_str("null"),
        }
        self
    }

    pub fn bool(&mut self, k: &str, v: bool) -> &mut Self {
        self.key(k);
        self.buf.push_str(if v { "true" } else { "false" });
        self
    }

    pub fn opt_num(&mut self, k: &str, v: Option<i64>) -> &mut Self {
        self.key(k);
        match v {
            Some(n) => self.buf.push_str(&n.to_string()),
            None => self.buf.push_str("null"),
        }
        self
    }

    pub fn num(&mut self, k: &str, v: i64) -> &mut Self {
        self.key(k);
        self.buf.push_str(&v.to_string());
        self
    }

    pub fn strs(&mut self, k: &str, v: &[String]) -> &mut Self {
        self.key(k);
        self.buf.push('[');
        for (i, s) in v.iter().enumerate() {
            if i > 0 {
                self.buf.push(',');
            }
            self.buf.push_str(&escape(s));
        }
        self.buf.push(']');
        self
    }

    pub fn raw(&mut self, k: &str, v: &str) -> &mut Self {
        self.key(k);
        self.buf.push_str(v);
        self
    }

    /// A string value whose body is the CLR name of custom type `i`,
    /// filled in when the descriptor is built at run time, followed by
    /// `suffix` (`[]` for an array of it).
    pub fn custom_clr(&mut self, k: &str, i: usize, suffix: &str) -> &mut Self {
        self.key(k);
        self.buf.push('"');
        self.buf.push_str(&clr_sentinel(i));
        self.buf.push_str(suffix);
        self.buf.push('"');
        self
    }

    /// A boolean value read from custom type `i` at run time: whether
    /// it is a CLR value type.
    pub fn custom_value_type(&mut self, k: &str, i: usize) -> &mut Self {
        self.key(k);
        self.buf.push_str(&value_type_sentinel(i));
        self
    }

    pub fn finish(mut self) -> String {
        self.buf.push('}');
        self.buf
    }
}

pub fn clr_sentinel(i: usize) -> String {
    format!("@@PWRS_CLR_{i}@@")
}

pub fn value_type_sentinel(i: usize) -> String {
    format!("@@PWRS_VT_{i}@@")
}

/// A raw value that becomes the class's `#[psmethods]` descriptor
/// array at run time.
pub fn methods_sentinel() -> String {
    "@@PWRS_METHODS@@".to_string()
}

/// One part of a descriptor that is assembled at run time.
pub enum Piece {
    Text(String),
    ClrName(usize),
    ValueType(usize),
    Methods,
}

/// Splits a descriptor holding sentinels into literal text and the
/// custom-type references the sentinels stand for.
pub fn pieces(json: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut rest = json;
    while let Some(start) = rest.find("@@PWRS_") {
        if start > 0 {
            out.push(Piece::Text(rest[..start].to_string()));
        }
        let after = &rest[start + 2..];
        let end = match after.find("@@") {
            Some(e) => e,
            None => {
                out.push(Piece::Text(rest[start..].to_string()));
                rest = "";
                break;
            }
        };
        let token = &after[..end];
        // The index in a sentinel is written by Customs::index, so a
        // number that does not parse is a bug in this crate rather
        // than anything a module author wrote. It fails the compile
        // here instead of selecting the type at index 0 and declaring
        // a parameter with the wrong CLR name.
        let piece = if let Some(n) = token.strip_prefix("PWRS_CLR_") {
            Piece::ClrName(n.parse().unwrap_or_else(|e| panic!("pwrs-macros emitted the malformed type sentinel @@{token}@@: {e}")))
        } else if let Some(n) = token.strip_prefix("PWRS_VT_") {
            Piece::ValueType(n.parse().unwrap_or_else(|e| panic!("pwrs-macros emitted the malformed value-type sentinel @@{token}@@: {e}")))
        } else if token == "PWRS_METHODS" {
            Piece::Methods
        } else {
            Piece::Text(format!("@@{token}@@"))
        };
        out.push(piece);
        rest = &after[end + 2..];
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest.to_string()));
    }
    out
}

pub fn array(items: &[String]) -> String {
    let mut s = String::from("[");
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(it);
    }
    s.push(']');
    s
}
