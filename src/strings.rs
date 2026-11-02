//! `string_*` instructions (the part that is not the shared stack family) and the other
//! instructions that take or return strings: `char_all_from_string`, `char_from_string`,
//! `integer_fromstring`, `float_fromstring`. The semantics are those of `instructions/string.clj`
//! in clj-push3 (oracle), including the Java/Clojure library behaviour it relies on
//! (`clojure.string/replace`, `split`, `trim`, `capitalize`, `Character/isWhitespace`, ...).
//!
//! How the oracle runs an instruction: arguments are popped in `:in` order (top item first); when
//! any needed stack is empty the instruction is a no-op that pops nothing; when the Clojure
//! function throws it is a no-op that HAS popped its arguments. Instructions that return nil
//! (`string_first` of "") push nil on the CHAR stack (`None` in `Machine::chr`).
//!
//! Deviations: strings are Rust `str`, Clojure's are UTF-16. Indices and counts are in `char`s, so
//! they agree for the BMP; astral characters count 1 here and 2 in Clojure. Case conversion uses
//! Rust's Unicode tables, which agree with Java for ASCII (the generator only produces chars < 128).
//! A string longer than `MAX_STR_BYTES` is not pushed and sets `unsupported` (the oracle would
//! be out of memory long before; this keeps the process from following it).
//! `string_rand` is not implemented (random).

use crate::fmt::java_double;
use crate::{clamp, clj_long, is_java_whitespace, sanitize_float, Item, Machine, Op, TINY};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrOp {
    Length,
    Concat,
    Reverse,
    Eq,
    FromInteger,
    FromFloat,
    FromBoolean,
    FromChar,
    First,
    Last,
    Nth,
    Rest,
    Butlast,
    Take,
    Drop,
    Substr,
    IsEmpty,
    Contains,
    ContainsChar,
    Replace,
    ReplaceFirst,
    ReplaceChar,
    ReplaceFirstChar,
    RemoveChar,
    EmptyString,
    Uppercase,
    Lowercase,
    Capitalize,
    ConjChar,
    Split,
    ParseToChars,
    IndexofChar,
    OccurencesChar,
    SetChar,
    Iterate,
    Lt,
    Gt,
    Trim,
    TrimLeft,
    TrimRight,
    CharAllFromString,
    CharFromString,
    IntegerFromString,
    FloatFromString,
}

pub(crate) const NAMES: &[(&str, StrOp)] = &[
    ("string_length", StrOp::Length),
    ("string_concat", StrOp::Concat),
    ("string_reverse", StrOp::Reverse),
    ("string_=", StrOp::Eq),
    ("string_from_integer", StrOp::FromInteger),
    ("string_from_float", StrOp::FromFloat),
    ("string_from_boolean", StrOp::FromBoolean),
    ("string_from_char", StrOp::FromChar),
    ("string_first", StrOp::First),
    ("string_last", StrOp::Last),
    ("string_nth", StrOp::Nth),
    ("string_rest", StrOp::Rest),
    ("string_butlast", StrOp::Butlast),
    ("string_take", StrOp::Take),
    ("string_drop", StrOp::Drop),
    ("string_substr", StrOp::Substr),
    ("string_empty?", StrOp::IsEmpty),
    ("string_contains", StrOp::Contains),
    ("string_contains_char", StrOp::ContainsChar),
    ("string_replace", StrOp::Replace),
    ("string_replace_first", StrOp::ReplaceFirst),
    ("string_replace_char", StrOp::ReplaceChar),
    ("string_replace_first_char", StrOp::ReplaceFirstChar),
    ("string_remove_char", StrOp::RemoveChar),
    ("string_empty_string", StrOp::EmptyString),
    ("string_uppercase", StrOp::Uppercase),
    ("string_lowercase", StrOp::Lowercase),
    ("string_capitalize", StrOp::Capitalize),
    ("string_conj_char", StrOp::ConjChar),
    ("string_split", StrOp::Split),
    ("string_parse_to_chars", StrOp::ParseToChars),
    ("string_indexof_char", StrOp::IndexofChar),
    ("string_occurences_char", StrOp::OccurencesChar),
    ("string_set_char", StrOp::SetChar),
    ("string_iterate", StrOp::Iterate),
    ("string_<", StrOp::Lt),
    ("string_>", StrOp::Gt),
    ("string_trim", StrOp::Trim),
    ("string_trim_left", StrOp::TrimLeft),
    ("string_trim_right", StrOp::TrimRight),
    ("char_all_from_string", StrOp::CharAllFromString),
    ("char_from_string", StrOp::CharFromString),
    ("integer_fromstring", StrOp::IntegerFromString),
    ("float_fromstring", StrOp::FloatFromString),
];

pub(crate) fn from_name(name: &str) -> Option<StrOp> {
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, o)| *o)
}

pub(crate) fn name(op: StrOp) -> String {
    NAMES.iter().find(|(_, o)| *o == op).map(|(n, _)| n.to_string()).unwrap_or_default()
}

/// Longest string (in bytes) this build will build; see the module comment.
const MAX_STR_BYTES: usize = 1 << 24;

fn char_count(s: &str) -> usize {
    s.chars().count()
}

/// Byte offset of the `n`-th char (or the end of the string).
fn byte_offset(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(i, _)| i)
}

/// `s[a..b]` by byte offsets that come from `byte_offset`; "" instead of a panic.
fn sub(s: &str, a: usize, b: usize) -> &str {
    s.get(a..b).unwrap_or("")
}

/// `(str c)`: nil is the empty string.
fn char_string(c: Option<char>) -> String {
    c.map_or_else(String::new, |c| c.to_string())
}

/// Java regex `\s` without UNICODE_CHARACTER_CLASS (narrower than `Character/isWhitespace`).
fn is_regex_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

fn to_usize(n: i64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

impl Machine {
    fn have(&self, strings: usize, ints: usize, chars: usize) -> bool {
        self.string.len() >= strings && self.int.len() >= ints && self.chr.len() >= chars
    }

    fn push_string(&mut self, s: String) {
        if s.len() > MAX_STR_BYTES {
            self.unsupported = true;
        } else {
            self.string.push(Rc::from(s));
        }
    }

    pub(crate) fn run_str_op(&mut self, op: StrOp) {
        macro_rules! ps {
            () => {
                match self.string.pop() {
                    Some(s) => s,
                    None => return,
                }
            };
        }
        macro_rules! pi {
            () => {
                match self.int.pop() {
                    Some(v) => v,
                    None => return,
                }
            };
        }
        macro_rules! pc {
            () => {
                match self.chr.pop() {
                    Some(c) => c,
                    None => return,
                }
            };
        }
        macro_rules! need {
            ($s:expr, $i:expr, $c:expr) => {
                if !self.have($s, $i, $c) {
                    return;
                }
            };
        }
        match op {
            StrOp::Length => {
                need!(1, 0, 0);
                let s = ps!();
                self.int.push(char_count(&s) as i64);
            }
            // (str s2 s1): the second string, then the top one
            StrOp::Concat => {
                need!(2, 0, 0);
                let s1 = ps!();
                let s2 = ps!();
                let mut r = String::with_capacity(s1.len() + s2.len());
                r.push_str(&s2);
                r.push_str(&s1);
                self.push_string(r);
            }
            StrOp::Reverse => {
                need!(1, 0, 0);
                let s = ps!();
                self.push_string(s.chars().rev().collect());
            }
            StrOp::Eq => {
                need!(2, 0, 0);
                let a = ps!();
                let b = ps!();
                self.boolean.push(a == b);
            }
            StrOp::FromInteger => {
                need!(0, 1, 0);
                let v = pi!();
                self.push_string(v.to_string());
            }
            // (str f): Java's Double.toString ("NaN", "Infinity"); the java.lang.Float 1e-12 is "1.0E-12"
            StrOp::FromFloat => {
                let Some(x) = self.float.pop() else { return };
                let t = if x == TINY { "1.0E-12".to_string() } else { java_double(x) };
                self.push_string(t);
            }
            StrOp::FromBoolean => {
                let Some(b) = self.boolean.pop() else { return };
                self.push_string(b.to_string());
            }
            // (str c): nil is ""
            StrOp::FromChar => {
                need!(0, 0, 1);
                let c = pc!();
                self.push_string(char_string(c));
            }
            StrOp::First => {
                need!(1, 0, 0);
                let s = ps!();
                self.chr.push(s.chars().next());
            }
            StrOp::Last => {
                need!(1, 0, 0);
                let s = ps!();
                self.chr.push(s.chars().next_back());
            }
            // nil unless (and (seq s) (>= i 0)); then (nth s (mod i (count s)))
            StrOp::Nth => {
                need!(1, 1, 0);
                let s = ps!();
                let i = pi!();
                let n = char_count(&s);
                let c = if n > 0 && i >= 0 { s.chars().nth((i as u64 % n as u64) as usize) } else { None };
                self.chr.push(c);
            }
            StrOp::Rest => {
                need!(1, 0, 0);
                let s = ps!();
                let off = byte_offset(&s, 1);
                self.push_string(sub(&s, off, s.len()).to_string());
            }
            StrOp::Butlast => {
                need!(1, 0, 0);
                let s = ps!();
                let end = s.char_indices().next_back().map_or(0, |(i, _)| i);
                self.push_string(sub(&s, 0, end).to_string());
            }
            StrOp::Take => {
                need!(1, 1, 0);
                let s = ps!();
                let n = pi!().max(0);
                let off = byte_offset(&s, to_usize(n));
                self.push_string(sub(&s, 0, off).to_string());
            }
            StrOp::Drop => {
                need!(1, 1, 0);
                let s = ps!();
                let n = pi!().max(0);
                let off = byte_offset(&s, to_usize(n));
                self.push_string(sub(&s, off, s.len()).to_string());
            }
            // in [string integer integer]: start is the top integer, end the one below
            StrOp::Substr => {
                need!(1, 2, 0);
                let s = ps!();
                let start = pi!();
                let end = pi!();
                let len = char_count(&s) as i64;
                let start = start.max(0).min(len);
                let end = end.max(start).min(len);
                let (a, b) = (byte_offset(&s, to_usize(start)), byte_offset(&s, to_usize(end)));
                self.push_string(sub(&s, a, b).to_string());
            }
            StrOp::IsEmpty => {
                need!(1, 0, 0);
                let s = ps!();
                self.boolean.push(s.is_empty());
            }
            // (str/includes? s1 s2): the top string contains the second
            StrOp::Contains => {
                need!(2, 0, 0);
                let s1 = ps!();
                let s2 = ps!();
                self.boolean.push(s1.contains(&*s2));
            }
            // (str/includes? s (str c)): nil is "", which every string contains
            StrOp::ContainsChar => {
                need!(1, 0, 1);
                let s = ps!();
                let c = pc!();
                self.boolean.push(match c {
                    Some(c) => s.contains(c),
                    None => true,
                });
            }
            // (str/replace s match replacement) with strings: String.replace, literal, all occurrences
            StrOp::Replace => {
                need!(3, 0, 0);
                let s = ps!();
                let m = ps!();
                let r = ps!();
                // Rust and Java agree on the empty match: the replacement goes between all chars
                if m.is_empty() && s.len().saturating_add(r.len().saturating_mul(char_count(&s) + 1)) > MAX_STR_BYTES {
                    self.unsupported = true;
                    return;
                }
                self.push_string(s.replace(&*m, &r));
            }
            // replace-first-str: first indexOf, "" matches at 0
            StrOp::ReplaceFirst => {
                need!(3, 0, 0);
                let s = ps!();
                let m = ps!();
                let r = ps!();
                match s.find(&*m) {
                    None => self.push_string(s.to_string()),
                    Some(i) => {
                        let mut t = String::with_capacity(s.len() + r.len());
                        t.push_str(sub(&s, 0, i));
                        t.push_str(&r);
                        t.push_str(sub(&s, i + m.len(), s.len()));
                        self.push_string(t);
                    }
                }
            }
            // (.replace s ^Character old ^Character new): a nil on either side throws (no-op)
            StrOp::ReplaceChar => {
                need!(1, 0, 2);
                let s = ps!();
                let old = pc!();
                let new = pc!();
                if let (Some(old), Some(new)) = (old, new) {
                    self.push_string(s.chars().map(|c| if c == old { new } else { c }).collect());
                }
            }
            // replace-first-char: a nil match throws ("Invalid match arg"); a nil replacement is (str nil) = ""
            StrOp::ReplaceFirstChar => {
                need!(1, 0, 2);
                let s = ps!();
                let old = pc!();
                let new = pc!();
                let Some(old) = old else { return };
                match s.find(old) {
                    None => self.push_string(s.to_string()),
                    Some(i) => {
                        let mut t = String::with_capacity(s.len() + 4);
                        t.push_str(sub(&s, 0, i));
                        t.push_str(&char_string(new));
                        t.push_str(sub(&s, i + old.len_utf8(), s.len()));
                        self.push_string(t);
                    }
                }
            }
            // (filter #(not= c %) s): with nil nothing is removed
            StrOp::RemoveChar => {
                need!(1, 0, 1);
                let s = ps!();
                let c = pc!();
                self.push_string(s.chars().filter(|x| Some(*x) != c).collect());
            }
            StrOp::EmptyString => self.string.push(Rc::from("")),
            StrOp::Uppercase => {
                need!(1, 0, 0);
                let s = ps!();
                self.push_string(s.to_uppercase());
            }
            StrOp::Lowercase => {
                need!(1, 0, 0);
                let s = ps!();
                self.push_string(s.to_lowercase());
            }
            // fewer than 2 chars: toUpperCase of all; else upper(first char) + lower(rest)
            StrOp::Capitalize => {
                need!(1, 0, 0);
                let s = ps!();
                if char_count(&s) < 2 {
                    self.push_string(s.to_uppercase());
                } else {
                    let off = byte_offset(&s, 1);
                    let mut t = sub(&s, 0, off).to_uppercase();
                    t.push_str(&sub(&s, off, s.len()).to_lowercase());
                    self.push_string(t);
                }
            }
            // (str s c): nil adds nothing
            StrOp::ConjChar => {
                need!(1, 0, 1);
                let s = ps!();
                let c = pc!();
                let mut t = s.to_string();
                if let Some(c) = c {
                    t.push(c);
                }
                self.push_string(t);
            }
            // (str/split (str/trim s) #"\s+"), parts pushed last-first so the first part is on top.
            // After the trim (isWhitespace, a superset of \s) the string has no leading \s, so the parts
            // are the non-empty runs; the empty string splits to [""].
            StrOp::Split => {
                need!(1, 0, 0);
                let s = ps!();
                let t = s.trim_matches(is_java_whitespace);
                let mut parts: Vec<&str> = t.split(is_regex_space).filter(|p| !p.is_empty()).collect();
                if parts.is_empty() {
                    parts.push("");
                }
                for p in parts.iter().rev() {
                    self.push_string((*p).to_string());
                }
            }
            // reduce over (reverse s): the first char ends up on top
            StrOp::ParseToChars | StrOp::CharAllFromString => {
                need!(1, 0, 0);
                let s = ps!();
                for c in s.chars().rev() {
                    self.chr.push(Some(c));
                }
            }
            // (str/index-of s (str c)) or -1; nil is "" which is found at 0
            StrOp::IndexofChar => {
                need!(1, 0, 1);
                let s = ps!();
                let c = pc!();
                let idx = match c {
                    None => 0,
                    Some(c) => s.find(c).map_or(-1, |b| char_count(sub(&s, 0, b)) as i64),
                };
                self.int.push(idx);
            }
            StrOp::OccurencesChar => {
                need!(1, 0, 1);
                let s = ps!();
                let c = pc!();
                let n = match c {
                    None => 0,
                    Some(c) => s.chars().filter(|x| *x == c).count(),
                };
                self.int.push(n as i64);
            }
            // in [string integer char]; (str (subs s 0 idx) c (subs s (inc idx))): nil deletes the char
            StrOp::SetChar => {
                need!(1, 1, 1);
                let s = ps!();
                let i = pi!();
                let c = pc!();
                let n = char_count(&s) as i64;
                if n == 0 {
                    self.string.push(s);
                    return;
                }
                let idx = to_usize(i.rem_euclid(n));
                let (a, b) = (byte_offset(&s, idx), byte_offset(&s, idx + 1));
                let mut t = String::with_capacity(s.len() + 4);
                t.push_str(sub(&s, 0, a));
                if let Some(c) = c {
                    t.push(c);
                }
                t.push_str(sub(&s, b, s.len()));
                self.push_string(t);
            }
            // The string is popped first. With nothing on EXEC that is all. "" pops EXEC. One char: push it
            // (the EXEC top stays and runs next). Longer: push [iterate, rest, copy of the EXEC top] and the
            // first char.
            StrOp::Iterate => {
                need!(1, 0, 0);
                let s = ps!();
                let Some(top) = self.exec.last().cloned() else { return };
                let mut it = s.chars();
                let Some(first) = it.next() else {
                    self.exec.pop();
                    return;
                };
                let rest = it.as_str();
                if rest.is_empty() {
                    self.chr.push(Some(first));
                } else {
                    self.exec.push(Item::Op(Op::Str(StrOp::Iterate)));
                    self.exec.push(Item::Str(Rc::from(rest)));
                    self.exec.push(top);
                    self.chr.push(Some(first));
                }
            }
            // (neg? (compare s1 s2)) with s1 the top string; Java compares UTF-16 units, Rust bytes
            // (same order below U+D800)
            StrOp::Lt => {
                need!(2, 0, 0);
                let a = ps!();
                let b = ps!();
                self.boolean.push(a < b);
            }
            StrOp::Gt => {
                need!(2, 0, 0);
                let a = ps!();
                let b = ps!();
                self.boolean.push(a > b);
            }
            StrOp::Trim => {
                need!(1, 0, 0);
                let s = ps!();
                self.push_string(s.trim_matches(is_java_whitespace).to_string());
            }
            StrOp::TrimLeft => {
                need!(1, 0, 0);
                let s = ps!();
                self.push_string(s.trim_start_matches(is_java_whitespace).to_string());
            }
            StrOp::TrimRight => {
                need!(1, 0, 0);
                let s = ps!();
                self.push_string(s.trim_end_matches(is_java_whitespace).to_string());
            }
            // in [string integer]: (nth s i) when 0 <= i < count, else \space
            StrOp::CharFromString => {
                need!(1, 1, 0);
                let s = ps!();
                let i = pi!();
                let c = if i >= 0 { s.chars().nth(to_usize(i)) } else { None };
                self.chr.push(Some(c.unwrap_or(' ')));
            }
            // the string is consumed; a failed parse, or a (long v) that throws, leaves nothing
            StrOp::IntegerFromString => {
                need!(1, 0, 0);
                let s = ps!();
                match parse_number(&s) {
                    Parsed::Num { long: Some(v), .. } => self.int.push(clamp(v as i128)),
                    Parsed::Unsupported => self.unsupported = true,
                    _ => {}
                }
            }
            StrOp::FloatFromString => {
                need!(1, 0, 0);
                let s = ps!();
                match parse_number(&s) {
                    Parsed::Num { double, .. } => self.float.push(sanitize_float(double)),
                    Parsed::Unsupported => self.unsupported = true,
                    Parsed::Not => {}
                }
            }
        }
    }
}

/// What `(parse-number s)` of the oracle (`read-string`, then `number?`) gives, with the two
/// conversions the instructions apply to it: `(long v)` (`long`, None where it throws) and
/// `(double v)`.
#[derive(Debug, PartialEq)]
pub(crate) enum Parsed {
    /// nil: the reader threw, or the first form is not a number.
    Not,
    /// A form outside what is modelled (a number that does not fit 128 bits in a radix other than 10,
    /// `#_` discard forms): the run is flagged `unsupported`.
    Unsupported,
    Num { long: Option<i64>, double: f64 },
}

fn is_reader_ws(c: char) -> bool {
    is_java_whitespace(c) || c == ','
}

/// LispReader.isMacro: a number token ends before any of these.
fn is_macro(c: char) -> bool {
    matches!(c, '"' | ';' | '\'' | '@' | '^' | '`' | '~' | '(' | ')' | '[' | ']' | '{' | '}' | '\\' | '%' | '#')
}

/// A symbol token ends before these (macros except `#`, `'` and `%`).
fn is_terminating_macro(c: char) -> bool {
    is_macro(c) && !matches!(c, '#' | '\'' | '%')
}

fn skip_ws_and_comments(c: &[char], mut i: usize) -> usize {
    loop {
        match c.get(i) {
            Some(&ch) if is_reader_ws(ch) => i += 1,
            Some(';') => {
                while i < c.len() && c[i] != '\n' && c[i] != '\r' {
                    i += 1;
                }
            }
            _ => return i,
        }
    }
}

fn long_of_magnitude(neg: bool, v: u128) -> Option<i64> {
    if neg {
        if v <= 1u128 << 63 {
            Some((-(v as i128)) as i64)
        } else {
            None
        }
    } else {
        i64::try_from(v).ok()
    }
}

/// Covers: leading whitespace, commas and `;` comments; a number token (decimal, hex `0x`, octal
/// `0`-prefixed, radix `NrDIGITS`, `N` suffix, ratios, doubles with exponent, `M` suffix) with
/// everything after the token ignored, as `read-string` reads one form; `##Inf`, `##-Inf`, `##NaN`.
/// Anything else that starts a form (symbols, strings, lists, quotes, `#_` is flagged unsupported)
/// is not a number. Not modelled: numbers beyond 128 bits outside radix 10 and ratios beyond
/// 128 bits (flagged `Unsupported`), non-ASCII text (never a number).
pub(crate) fn parse_number(s: &str) -> Parsed {
    let c: Vec<char> = s.chars().collect();
    let i = skip_ws_and_comments(&c, 0);
    let Some(&ch) = c.get(i) else { return Parsed::Not };
    let signed_digit = (ch == '+' || ch == '-') && c.get(i + 1).is_some_and(|d| d.is_ascii_digit());
    if ch.is_ascii_digit() || signed_digit {
        let tok: String = c[i..].iter().take_while(|x| !is_reader_ws(**x) && !is_macro(**x)).collect();
        return match_number(&tok);
    }
    if ch == '#' {
        match c.get(i + 1) {
            Some('#') => {
                let j = skip_ws_and_comments(&c, i + 2);
                let tok: String = c[j.min(c.len())..].iter().take_while(|x| !is_reader_ws(**x) && !is_terminating_macro(**x)).collect();
                let d = match tok.as_str() {
                    "Inf" => f64::INFINITY,
                    "-Inf" => f64::NEG_INFINITY,
                    "NaN" => f64::NAN,
                    _ => return Parsed::Not,
                };
                return Parsed::Num { long: clj_long(d), double: d };
            }
            Some('_') => return Parsed::Unsupported,
            _ => {}
        }
    }
    Parsed::Not
}

fn digits_ok(s: &str, f: fn(&u8) -> bool) -> bool {
    !s.is_empty() && s.as_bytes().iter().all(f)
}

/// LispReader.matchNumber: intPat, then ratioPat, then floatPat, each matched against the whole token.
fn match_number(tok: &str) -> Parsed {
    if !tok.is_ascii() {
        return Parsed::Not;
    }
    let (neg, rest) = match tok.as_bytes().first() {
        Some(b'-') => (true, &tok[1..]),
        Some(b'+') => (false, &tok[1..]),
        _ => (false, tok),
    };
    if let Some(r) = match_int(neg, rest) {
        return r;
    }
    if let Some(r) = match_ratio(neg, rest) {
        return r;
    }
    match_float(neg, rest).unwrap_or(Parsed::Not)
}

fn int_value(neg: bool, digits: &str, radix: u32) -> Parsed {
    if !(2..=36).contains(&radix) {
        return Parsed::Not; // BigInteger throws NumberFormatException
    }
    let mut v: u128 = 0;
    let mut overflow = false;
    for b in digits.bytes() {
        let Some(d) = (b as char).to_digit(36).filter(|d| *d < radix) else { return Parsed::Not };
        match v.checked_mul(radix as u128).and_then(|x| x.checked_add(d as u128)) {
            Some(x) => v = x,
            None => overflow = true,
        }
    }
    if overflow {
        if radix == 10 {
            // a BigInt: (long) throws, (double) is the correctly rounded value
            let d = digits.parse::<f64>().unwrap_or(f64::INFINITY);
            return Parsed::Num { long: None, double: if neg { -d } else { d } };
        }
        return Parsed::Unsupported;
    }
    let d = v as f64;
    Parsed::Num { long: long_of_magnitude(neg, v), double: if neg { -d } else { d } }
}

/// intPat: `(0)|([1-9][0-9]*)|0[xX]([0-9A-Fa-f]+)|0([0-7]+)|([1-9][0-9]?)[rR]([0-9A-Za-z]+)|0[0-9]+`
/// followed by an optional `N`. `Some(Parsed::Not)` is a match that the reader rejects (`08`).
fn match_int(neg: bool, rest: &str) -> Option<Parsed> {
    let body = rest.strip_suffix('N').unwrap_or(rest);
    let bb = body.as_bytes();
    let dec = |b: &u8| b.is_ascii_digit();
    let hex = |b: &u8| b.is_ascii_hexdigit();
    let oct = |b: &u8| (b'0'..=b'7').contains(b);
    if body == "0" {
        return Some(int_value(neg, "0", 10));
    }
    if bb.first().is_some_and(|b| (b'1'..=b'9').contains(b)) && digits_ok(body, dec) {
        return Some(int_value(neg, body, 10));
    }
    if bb.len() > 2 && bb[0] == b'0' && (bb[1] == b'x' || bb[1] == b'X') && digits_ok(&body[2..], hex) {
        return Some(int_value(neg, &body[2..], 16));
    }
    if bb.len() > 1 && bb[0] == b'0' && digits_ok(&body[1..], oct) {
        return Some(int_value(neg, &body[1..], 8));
    }
    // radix form, matched on the whole text: the digit class contains N, so no N suffix is cut off
    let rb = rest.as_bytes();
    if rb.first().is_some_and(|b| (b'1'..=b'9').contains(b)) {
        let rpos = if rb.get(1).is_some_and(|b| *b == b'r' || *b == b'R') {
            Some(1)
        } else if rb.get(1).is_some_and(|b| b.is_ascii_digit()) && rb.get(2).is_some_and(|b| *b == b'r' || *b == b'R') {
            Some(2)
        } else {
            None
        };
        if let Some(p) = rpos {
            let digits = &rest[p + 1..];
            if digits_ok(digits, |b| b.is_ascii_alphanumeric()) {
                let radix = rest[..p].parse::<u32>().unwrap_or(0);
                return Some(int_value(neg, digits, radix));
            }
        }
    }
    if bb.len() > 1 && bb[0] == b'0' && digits_ok(&body[1..], dec) {
        return Some(Parsed::Not); // "0[0-9]+" matches but is no valid octal: the reader throws
    }
    None
}

fn pow10(k: u32) -> Option<u128> {
    10u128.checked_pow(k)
}

/// ratioPat `([-+]?[0-9]+)/([0-9]+)`. A whole result is an integer; otherwise `(long r)` truncates and
/// `(double r)` is `Ratio.doubleValue`: the quotient rounded to 16 digits (MathContext.DECIMAL64,
/// half-even), then converted.
fn match_ratio(neg: bool, rest: &str) -> Option<Parsed> {
    let (n, d) = rest.split_once('/')?;
    let dec = |b: &u8| b.is_ascii_digit();
    if !digits_ok(n, dec) || !digits_ok(d, dec) {
        return None;
    }
    let (Ok(n), Ok(d)) = (n.parse::<u128>(), d.parse::<u128>()) else { return Some(Parsed::Unsupported) };
    if d == 0 {
        return Some(Parsed::Not); // Divide by zero
    }
    let sign = |x: f64| if neg { -x } else { x };
    let (q, r) = (n / d, n % d);
    let long = long_of_magnitude(neg, q);
    if r == 0 {
        return Some(Parsed::Num { long, double: sign(q as f64) });
    }
    // find k with 10^15 <= floor(n * 10^k / d) < 10^16
    let digits = |x: u128| x.to_string().len() as i32;
    let mut k: i32 = 16 - (digits(n) - digits(d));
    let scaled = |k: i32| -> Option<(u128, u128)> {
        let (a, b) = if k >= 0 { (n.checked_mul(pow10(k as u32)?)?, d) } else { (n, d.checked_mul(pow10((-k) as u32)?)?) };
        Some((a / b, a % b, b)).map(|(q, r, b)| (q, if 2 * r > b || (2 * r == b && q % 2 == 1) { 1 } else { 0 }))
    };
    for _ in 0..8 {
        let Some((q0, _)) = scaled(k) else { return Some(Parsed::Unsupported) };
        if q0 < 10u128.pow(15) {
            k += 1;
        } else if q0 >= 10u128.pow(16) {
            k -= 1;
        } else {
            let Some((q1, up)) = scaled(k) else { return Some(Parsed::Unsupported) };
            let text = format!("{}e{}", q1 + up, -k);
            let double = text.parse::<f64>().unwrap_or(f64::NAN);
            return Some(Parsed::Num { long, double: sign(double) });
        }
    }
    Some(Parsed::Unsupported)
}

/// floatPat `([-+]?[0-9]+(\.[0-9]*)?([eE][-+]?[0-9]+)?)(M)?`: a Double (`Double.parseDouble`) or, with `M`,
/// a BigDecimal whose `(long v)` and `(double v)` both go through its double value.
fn match_float(neg: bool, rest: &str) -> Option<Parsed> {
    let (body, big) = match rest.strip_suffix('M') {
        Some(b) => (b, true),
        None => (rest, false),
    };
    let b = body.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let int_part = &body[..i];
    let mut frac_part = "";
    if b.get(i) == Some(&b'.') {
        let st = i + 1;
        i = st;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_part = &body[st..i];
    }
    let mut exp = "0";
    let mut exp_digits = 0;
    if matches!(b.get(i), Some(b'e') | Some(b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+') | Some(b'-')) {
            j += 1;
        }
        let st = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == st {
            return None;
        }
        exp = &body[i + 1..j];
        exp_digits = j - st;
        i = j;
    }
    if i != b.len() {
        return None;
    }
    if big && exp_digits > 9 {
        return Some(Parsed::Not); // BigDecimal's exponent must fit an int
    }
    let text = format!("{}.{}e{}", int_part, if frac_part.is_empty() { "0" } else { frac_part }, exp);
    let d = text.parse::<f64>().unwrap_or(f64::NAN);
    let d = if neg { -d } else { d };
    Some(Parsed::Num { long: clj_long(d), double: d })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edn;

    fn run(prog: &str) -> Machine {
        let e = edn::parse(prog).expect("edn");
        let item = Item::from_edn(&e).expect("item");
        let mut m = Machine::new(item, vec![]);
        m.run(10_000);
        m
    }

    fn strings(m: &Machine) -> Vec<String> {
        m.string.iter().map(|s| s.to_string()).collect()
    }

    fn num(s: &str) -> (Option<i64>, f64) {
        match parse_number(s) {
            Parsed::Num { long, double } => (long, double),
            other => panic!("{s:?} gave {other:?}"),
        }
    }

    #[test]
    fn first_of_empty_is_nil() {
        let m = run(r#"("" :string_first "ab" :string_last)"#);
        assert_eq!(m.chr, vec![None, Some('b')]);
    }

    #[test]
    fn set_char_with_nil_deletes() {
        let m = run(r#"("" :string_first "abc" 1 :string_set_char)"#);
        assert_eq!(strings(&m), vec!["ac"]);
    }

    #[test]
    fn split_pushes_first_part_on_top() {
        let m = run(r#"(" a  b\tc " :string_split)"#);
        assert_eq!(strings(&m), vec!["c", "b", "a"]);
        assert_eq!(strings(&run(r#"("  " :string_split)"#)), vec![""]);
    }

    #[test]
    fn replace_char_with_nil_is_a_noop_that_pops() {
        let m = run(r#"("abc" "" :string_first \a :string_replace_char)"#);
        assert!(m.string.is_empty());
        assert!(m.chr.is_empty());
    }

    #[test]
    fn replace_first_char_with_nil_replacement_deletes() {
        let m = run(r#"("abca" "" :string_first \a :string_replace_first_char)"#);
        assert_eq!(strings(&m), vec!["bca"]);
    }

    #[test]
    fn iterate_runs_the_body_once_per_char() {
        let m = run(r#"("abc" :string_iterate :char_dup)"#);
        assert_eq!(m.chr.len(), 6);
    }

    #[test]
    fn number_forms() {
        assert_eq!(num("  42 rest"), (Some(42), 42.0));
        assert_eq!(num("-0x1F"), (Some(-31), -31.0));
        assert_eq!(num("010"), (Some(8), 8.0));
        assert_eq!(num("36rZ"), (Some(35), 35.0));
        assert_eq!(num("1e3"), (Some(1000), 1000.0));
        assert_eq!(num("7/2"), (Some(3), 3.5));
        assert_eq!(num("1/3").1, 0.3333333333333333);
        assert_eq!(num("2.50M"), (Some(2), 2.5));
        assert_eq!(num("##Inf").0, None);
        assert_eq!(num("99999999999999999999"), (None, 1e20));
        assert_eq!(parse_number("08"), Parsed::Not);
        assert_eq!(parse_number("1/0"), Parsed::Not);
        assert_eq!(parse_number("-x"), Parsed::Not);
        assert_eq!(parse_number("12abc"), Parsed::Not);
        assert_eq!(num(",;c\n 5)"), (Some(5), 5.0));
    }
}
