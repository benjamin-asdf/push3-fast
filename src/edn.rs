//! A small EDN reader and printer, enough for Push3 programs and run results.
//!
//! Supported: nil, true/false, integers (an `N` suffix is accepted), keywords, lists, vectors,
//! maps, `,` as whitespace and `;` comments. Push programs are plain EDN data, so Clojure can
//! produce and consume them with `pr-str` and `read-string` without a translation layer.

use crate::{Item, Op};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Edn {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Char(char),
    Str(String),
    Kw(String),
    List(Vec<Edn>),
    Vector(Vec<Edn>),
    Map(Vec<(Edn, Edn)>),
}

impl Edn {
    /// The value under keyword key `key` (without the colon) of a map.
    pub fn get(&self, key: &str) -> Option<&Edn> {
        match self {
            Edn::Map(kv) => kv.iter().find(|(k, _)| matches!(k, Edn::Kw(n) if n == key)).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn items(&self) -> Option<&[Edn]> {
        match self {
            Edn::List(v) | Edn::Vector(v) => Some(v),
            _ => None,
        }
    }
}

pub fn parse(s: &str) -> Result<Edn, String> {
    let mut p = Parser { b: s.as_bytes(), pos: 0 };
    let v = p.value()?;
    p.skip();
    if p.pos < p.b.len() {
        return Err(format!("trailing input at byte {}", p.pos));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    pos: usize,
}

fn delimiter(c: u8) -> bool {
    c.is_ascii_whitespace() || matches!(c, b',' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b';')
}

impl<'a> Parser<'a> {
    fn string(&mut self) -> Result<Edn, String> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let rest = std::str::from_utf8(&self.b[self.pos..]).map_err(|e| e.to_string())?;
            let c = rest.chars().next().ok_or("unterminated string")?;
            self.pos += c.len_utf8();
            match c {
                '"' => return Ok(Edn::Str(out)),
                '\\' => {
                    let e = *self.b.get(self.pos).ok_or("unterminated escape")?;
                    self.pos += 1;
                    match e {
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'u' => {
                            let hex = std::str::from_utf8(&self.b[self.pos..self.pos + 4]).map_err(|e| e.to_string())?;
                            self.pos += 4;
                            out.push(char::from_u32(u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?).ok_or("bad \\u escape")?);
                        }
                        other => return Err(format!("unsupported escape \\{}", other as char)),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn character(&mut self) -> Result<Edn, String> {
        self.pos += 1;
        let rest = std::str::from_utf8(&self.b[self.pos..]).map_err(|e| e.to_string())?;
        let first = rest.chars().next().ok_or("unterminated character")?;
        let start = self.pos;
        self.pos += first.len_utf8();
        while self.pos < self.b.len() && !delimiter(self.b[self.pos]) {
            self.pos += 1;
        }
        let tok = std::str::from_utf8(&self.b[start..self.pos]).map_err(|e| e.to_string())?;
        match tok {
            "space" => Ok(Edn::Char(' ')),
            "newline" => Ok(Edn::Char('\n')),
            "tab" => Ok(Edn::Char('\t')),
            "return" => Ok(Edn::Char('\r')),
            "backspace" => Ok(Edn::Char('\u{8}')),
            "formfeed" => Ok(Edn::Char('\u{c}')),
            t if t.chars().count() == 1 => Ok(Edn::Char(t.chars().next().unwrap())),
            t if t.starts_with('u') && t.len() == 5 => u32::from_str_radix(&t[1..], 16)
                .ok()
                .and_then(char::from_u32)
                .map(Edn::Char)
                .ok_or_else(|| format!("bad character \\{t}")),
            t => Err(format!("unsupported character \\{t}")),
        }
    }

    fn skip(&mut self) {
        while let Some(&c) = self.b.get(self.pos) {
            if c.is_ascii_whitespace() || c == b',' {
                self.pos += 1;
            } else if c == b';' {
                while self.pos < self.b.len() && self.b[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn seq(&mut self, close: u8) -> Result<Vec<Edn>, String> {
        let mut out = Vec::new();
        loop {
            self.skip();
            match self.b.get(self.pos) {
                None => return Err("unterminated collection".into()),
                Some(&c) if c == close => {
                    self.pos += 1;
                    return Ok(out);
                }
                _ => out.push(self.value()?),
            }
        }
    }

    fn value(&mut self) -> Result<Edn, String> {
        self.skip();
        let c = *self.b.get(self.pos).ok_or("unexpected end of input")?;
        match c {
            b'(' => {
                self.pos += 1;
                Ok(Edn::List(self.seq(b')')?))
            }
            b'[' => {
                self.pos += 1;
                Ok(Edn::Vector(self.seq(b']')?))
            }
            b'{' => {
                self.pos += 1;
                let flat = self.seq(b'}')?;
                if flat.len() % 2 != 0 {
                    return Err("map with an odd number of forms".into());
                }
                let mut it = flat.into_iter();
                let mut kv = Vec::new();
                while let (Some(k), Some(v)) = (it.next(), it.next()) {
                    kv.push((k, v));
                }
                Ok(Edn::Map(kv))
            }
            b')' | b']' | b'}' => Err(format!("unexpected '{}' at byte {}", c as char, self.pos)),
            b'"' => self.string(),
            b'\\' => self.character(),
            _ => {
                let start = self.pos;
                while self.pos < self.b.len() && !delimiter(self.b[self.pos]) {
                    self.pos += 1;
                }
                let tok = std::str::from_utf8(&self.b[start..self.pos]).map_err(|e| e.to_string())?;
                match tok {
                    "##NaN" => Ok(Edn::Float(f64::NAN)),
                    "##Inf" => Ok(Edn::Float(f64::INFINITY)),
                    "##-Inf" => Ok(Edn::Float(f64::NEG_INFINITY)),
                    "nil" => Ok(Edn::Nil),
                    "true" => Ok(Edn::Bool(true)),
                    "false" => Ok(Edn::Bool(false)),
                    _ if tok.starts_with(':') => Ok(Edn::Kw(tok[1..].to_string())),
                    _ => {
                        let t = tok.strip_suffix('N').unwrap_or(tok);
                        if let Ok(v) = t.parse::<i64>() {
                            Ok(Edn::Int(v))
                        } else if t.contains(['.', 'e', 'E']) {
                            t.parse::<f64>().map(Edn::Float).map_err(|_| format!("unsupported token '{tok}'"))
                        } else {
                            Err(format!("unsupported token '{tok}'"))
                        }
                    }
                }
            }
        }
    }
}

impl Item {
    /// Keywords are instruction names, lists and vectors are code lists.
    pub fn from_edn(e: &Edn) -> Result<Item, String> {
        match e {
            Edn::Bool(b) => Ok(Item::Bool(*b)),
            Edn::Int(v) => Ok(Item::Int(*v)),
            Edn::Float(v) => Ok(Item::Float(*v)),
            Edn::Char(c) => Ok(Item::Char(*c)),
            Edn::Kw(n) => Ok(Op::from_name(n).map(Item::Op).unwrap_or_else(|| Item::Name(Rc::from(n.as_str())))),
            Edn::List(v) => Ok(Item::List(Rc::new(v.iter().map(Item::from_edn).collect::<Result<Vec<_>, _>>()?))),
            // a vector is a literal (a value for a vector stack), not code: not modelled yet
            Edn::Vector(_) => Err("vector literals are not supported yet".into()),
            other => Err(format!("not a supported Push item yet: {other:?}")),
        }
    }

    pub fn write_edn(&self, out: &mut String) {
        crate::fmt::write_pr(self, out)
    }

    pub fn to_edn_string(&self) -> String {
        crate::fmt::pr_str(self)
    }
}
