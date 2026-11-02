//! Clojure `pr-str` / Java `Double.toString` formatting, needed because PRINT output and EDN
//! results must be byte-identical to what the Clojure oracle produces.

use crate::{Item, TINY};

/// Java `Double.toString`: shortest round-trip digits (JDK 19+), plain decimal for
/// 1e-3 <= |x| < 1e7, otherwise computerized scientific notation with at least one fraction digit.
pub fn java_double(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let a = x.abs();
    if (1e-3..1e7).contains(&a) {
        let s = format!("{x}");
        if s.contains('.') {
            s
        } else {
            format!("{s}.0")
        }
    } else {
        let s = format!("{x:e}");
        let (mant, exp) = s.split_once('e').unwrap();
        let mant = if mant.contains('.') { mant.to_string() } else { format!("{mant}.0") };
        format!("{mant}E{exp}")
    }
}

/// `pr-str` of a double: ##NaN, ##Inf, ##-Inf, otherwise Java's text.
pub fn pr_double(x: f64) -> String {
    if x.is_nan() {
        "##NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "##Inf".into() } else { "##-Inf".into() }
    } else {
        java_double(x)
    }
}

/// `pr-str` of an object that sits on the FLOAT stack. The only java.lang.Float that can get
/// there is `(float 1e-12)` (see `sanitize_float`); it prints with Float's shorter digits.
pub fn pr_float_stack(x: f64) -> String {
    if x == TINY {
        "1.0E-12".into()
    } else {
        pr_double(x)
    }
}

pub fn pr_char(c: char) -> String {
    match c {
        ' ' => "\\space".into(),
        '\n' => "\\newline".into(),
        '\t' => "\\tab".into(),
        '\r' => "\\return".into(),
        '\u{8}' => "\\backspace".into(),
        '\u{c}' => "\\formfeed".into(),
        c => format!("\\{c}"),
    }
}

pub fn pr_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `pr-str` of a Push item, as the oracle's `exec_print` and the EDN results show it.
pub fn pr_str(item: &Item) -> String {
    let mut s = String::new();
    write_pr(item, &mut s);
    s
}

pub fn write_pr(item: &Item, out: &mut String) {
    match item {
        Item::Int(v) => out.push_str(&v.to_string()),
        Item::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Item::Float(v) => out.push_str(&pr_double(*v)),
        Item::Char(c) => out.push_str(&pr_char(*c)),
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
                write_pr(it, out);
            }
            out.push(')');
        }
    }
}
