//! Clojure `pr-str` / Java `Double.toString` formatting, needed because PRINT output and EDN
//! results must be byte-identical to what the Clojure oracle produces.

use crate::{Item, TINY};

/// Shortest round-trip digits of a finite non-zero `|x|` and the decimal exponent of the first digit.
/// Rust's shortest-digit search rounds an exact tie up (`...0625` to `...063`); Java picks the
/// candidate whose last digit is even (`...062`). Ties only exist at 16-17 digits, where the exact
/// decimal expansion ends right after the 5.
fn java_digits(a: f64) -> (String, i32) {
    let s = format!("{a:e}");
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let mut digits: String = mant.chars().filter(|c| *c != '.').collect();
    let exp: i32 = exp.parse().unwrap_or(0);
    let n = digits.len();
    if n >= 16 {
        let exact = format!("{:.*e}", n + 40, a);
        let ex: String = exact.split('e').next().unwrap_or("").chars().filter(|c| *c != '.').collect();
        if ex.len() > n && ex.as_bytes()[n] == b'5' && ex[n + 1..].bytes().all(|b| b == b'0') {
            let mut t: Vec<u8> = ex.as_bytes()[..n].to_vec();
            if (t[n - 1] - b'0') % 2 == 1 {
                // round the truncated digits up (no carry out of the first digit is handled)
                let mut i = n;
                while i > 0 {
                    i -= 1;
                    if t[i] == b'9' {
                        t[i] = b'0';
                    } else {
                        t[i] += 1;
                        break;
                    }
                }
            }
            if let Ok(d) = String::from_utf8(t) {
                if d.as_bytes()[0] != b'0' {
                    digits = d;
                }
            }
        }
    }
    (digits, exp)
}

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
    let sign = if x < 0.0 { "-" } else { "" };
    let (digits, exp) = java_digits(x.abs());
    let a = x.abs();
    if (1e-3..1e7).contains(&a) {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            let padded = format!("{digits:0<int_len$}");
            let (int, frac) = padded.split_at(int_len);
            format!("{sign}{int}.{}", if frac.is_empty() { "0" } else { frac })
        } else {
            format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize))
        }
    } else {
        let (first, rest) = digits.split_at(1);
        format!("{sign}{first}.{}E{exp}", if rest.is_empty() { "0" } else { rest })
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
        Item::Nil => out.push_str("nil"),
        Item::Str(t) => out.push_str(&pr_string(t)),
        Item::IntVec(v) => {
            out.push('[');
            for (i, x) in v.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&x.to_string());
            }
            out.push(']');
        }
        Item::Op(o) => {
            out.push(':');
            out.push_str(&o.name());
        }
        Item::Name(n) => {
            out.push(':');
            out.push_str(n);
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

#[cfg(test)]
mod tests {
    use super::java_double;

    #[test]
    fn java_double_forms() {
        assert_eq!(java_double(-2.2897963681364062e12), "-2.2897963681364062E12");
        assert_eq!(java_double(1.0), "1.0");
        assert_eq!(java_double(100.0), "100.0");
        assert_eq!(java_double(1234567.5), "1234567.5");
        assert_eq!(java_double(0.001), "0.001");
        assert_eq!(java_double(0.00123), "0.00123");
        assert_eq!(java_double(1e7), "1.0E7");
        assert_eq!(java_double(1.5e-5), "1.5E-5");
        assert_eq!(java_double(-98.3764220847985), "-98.3764220847985");
        assert_eq!(java_double(1e12), "1.0E12");
    }
}
