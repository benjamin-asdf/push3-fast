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
            _ => {
                let start = self.pos;
                while self.pos < self.b.len() && !delimiter(self.b[self.pos]) {
                    self.pos += 1;
                }
                let tok = std::str::from_utf8(&self.b[start..self.pos]).map_err(|e| e.to_string())?;
                match tok {
                    "nil" => Ok(Edn::Nil),
                    "true" => Ok(Edn::Bool(true)),
                    "false" => Ok(Edn::Bool(false)),
                    _ if tok.starts_with(':') => Ok(Edn::Kw(tok[1..].to_string())),
                    _ => tok
                        .strip_suffix('N')
                        .unwrap_or(tok)
                        .parse::<i64>()
                        .map(Edn::Int)
                        .map_err(|_| format!("unsupported token '{tok}'")),
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
            Edn::Kw(n) => Op::from_name(n).map(Item::Op).ok_or_else(|| format!("unknown instruction :{n}")),
            Edn::List(v) | Edn::Vector(v) => {
                Ok(Item::List(Rc::new(v.iter().map(Item::from_edn).collect::<Result<Vec<_>, _>>()?)))
            }
            other => Err(format!("not a stage 1 Push item: {other:?}")),
        }
    }

    pub fn write_edn(&self, out: &mut String) {
        match self {
            Item::Int(v) => out.push_str(&v.to_string()),
            Item::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Item::Op(o) => {
                out.push(':');
                out.push_str(&o.name());
            }
            Item::List(l) => {
                out.push('(');
                for (i, it) in l.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    it.write_edn(out);
                }
                out.push(')');
            }
        }
    }

    pub fn to_edn_string(&self) -> String {
        let mut s = String::new();
        self.write_edn(&mut s);
        s
    }
}
