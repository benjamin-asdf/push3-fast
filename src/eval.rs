//! Batch evaluation of programs on a problem: one error vector per program.
//!
//! A problem file is one EDN map (written by the oracle, see oracle/src/.../eval_dump.clj):
//!
//! ```text
//! {:output :push/string  :error :levenshtein  :max 2000
//!  :num-outputs 6  :input-names [:a2 ...]            ;; only for the circuit problems
//!  :cases [{:inputs [...] :output <expected>} ...]}
//! ```
//!
//! The output readers and error functions mirror `run/read-output` and `problems/error-fn` of
//! clj-push3, so that an error vector here equals the one `run/errors` returns there.

use crate::edn::{self, Edn};
use crate::fmt;
use crate::{Item, Machine};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Error charged when a program left nothing readable on its output (`no-output-penalty`).
pub const NO_OUTPUT_PENALTY: f64 = 1.0e6;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Output {
    Int,
    Float,
    Bool,
    Str,
    Print,
    IntPair,
    Regs(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ErrKind {
    Numeric,
    Levenshtein,
    Exact,
    Bitwise,
    NumericPrinted,
    NumericPair,
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Pair(i64, i64),
    Bits(Vec<Option<bool>>),
    /// Nothing on the output stack.
    Missing,
}

struct ProblemCase {
    /// Kept as EDN: Item is Rc-based and cannot be shared between threads; see `Problem::prepare`.
    inputs: Vec<Edn>,
    expected: Value,
}

pub struct Problem {
    output: Output,
    error: ErrKind,
    max: u64,
    input_names: Vec<String>,
    cases: Vec<ProblemCase>,
}

/// What evaluating one program gave.
pub struct Row {
    pub errors: Vec<f64>,
    pub steps: u64,
    /// The cap on the total error was reached and `errors` stops early.
    pub cut: bool,
}

impl Row {
    pub fn total(&self) -> f64 {
        self.errors.iter().sum()
    }
    pub fn solved(&self) -> usize {
        self.errors.iter().filter(|e| **e == 0.0).count()
    }
    pub fn to_edn(&self) -> String {
        let errs: Vec<String> = self.errors.iter().map(|e| fmt::pr_double(*e)).collect();
        format!(
            "{{:errors [{}] :total {} :solved {} :steps {}{}}}",
            errs.join(" "),
            fmt::pr_double(self.total()),
            self.solved(),
            self.steps,
            if self.cut { " :cut true" } else { "" }
        )
    }
}

fn keyword(e: Option<&Edn>, what: &str) -> Result<String, String> {
    match e {
        Some(Edn::Kw(k)) => Ok(k.clone()),
        _ => Err(format!("{what} must be a keyword")),
    }
}

fn expected_value(e: &Edn) -> Result<Value, String> {
    Ok(match e {
        Edn::Int(i) => Value::Int(*i),
        Edn::Float(f) => Value::Float(*f),
        Edn::Bool(b) => Value::Bool(*b),
        Edn::Str(s) => Value::Str(s.clone()),
        Edn::Vector(xs) | Edn::List(xs) => {
            if xs.iter().all(|x| matches!(x, Edn::Bool(_))) {
                Value::Bits(xs.iter().map(|x| if let Edn::Bool(b) = x { Some(*b) } else { None }).collect())
            } else if let [Edn::Int(a), Edn::Int(b)] = xs.as_slice() {
                Value::Pair(*a, *b)
            } else {
                return Err("unsupported expected vector".into());
            }
        }
        _ => return Err("unsupported expected value".into()),
    })
}

impl Problem {
    pub fn parse(text: &str) -> Result<Problem, String> {
        let e = edn::parse(text)?;
        let out = keyword(e.get("output"), ":output")?;
        let num_outputs = match e.get("num-outputs") {
            Some(Edn::Int(n)) => *n as usize,
            _ => 0,
        };
        let output = match out.as_str() {
            "push/integer" => Output::Int,
            "push/float" => Output::Float,
            "push/boolean" => Output::Bool,
            "push/string" => Output::Str,
            "push/print" => Output::Print,
            "push/integer-pair" => Output::IntPair,
            "push/output" => Output::Regs(num_outputs),
            o => return Err(format!("unknown :output {o}")),
        };
        let error = match keyword(e.get("error"), ":error")?.as_str() {
            "numeric" => ErrKind::Numeric,
            "levenshtein" => ErrKind::Levenshtein,
            "exact" => ErrKind::Exact,
            "bitwise" => ErrKind::Bitwise,
            "numeric-printed" => ErrKind::NumericPrinted,
            "numeric-pair" => ErrKind::NumericPair,
            o => return Err(format!("unknown :error {o}")),
        };
        let max = match e.get("max") {
            Some(Edn::Int(v)) => *v as u64,
            None => 2000,
            _ => return Err(":max must be an integer".into()),
        };
        let input_names = match e.get("input-names") {
            Some(v) => v
                .items()
                .ok_or(":input-names must be a vector")?
                .iter()
                .map(|k| if let Edn::Kw(n) = k { Ok(n.clone()) } else { Err(":input-names must be keywords".to_string()) })
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        let mut cases = Vec::new();
        for c in e.get("cases").and_then(|c| c.items()).ok_or("missing :cases")? {
            let inputs = c.get("inputs").and_then(|i| i.items()).ok_or("case without :inputs")?.to_vec();
            // fail on unreadable inputs at load time, not in a worker
            inputs.iter().map(Item::from_edn).collect::<Result<Vec<_>, _>>()?;
            let expected = expected_value(c.get("output").ok_or("case without :output")?)?;
            cases.push(ProblemCase { inputs, expected });
        }
        Ok(Problem { output, error, max, input_names, cases })
    }

    pub fn n_cases(&self) -> usize {
        self.cases.len()
    }

    fn read_output(&self, m: &Machine) -> Value {
        match self.output {
            Output::Int => m.int.last().map_or(Value::Missing, |v| Value::Int(*v)),
            Output::Float => m.float.last().map_or(Value::Missing, |v| Value::Float(*v)),
            Output::Bool => m.boolean.last().map_or(Value::Missing, |v| Value::Bool(*v)),
            Output::Str => m.string.last().map_or(Value::Missing, |s| Value::Str(s.to_string())),
            Output::Print => Value::Str(m.print.clone()),
            Output::IntPair => {
                let n = m.int.len();
                if n >= 2 {
                    Value::Pair(m.int[n - 2], m.int[n - 1])
                } else {
                    Value::Missing
                }
            }
            // c(n-1) .. c0, most significant first
            Output::Regs(n) => Value::Bits((0..n).rev().map(|i| m.regs[i]).collect()),
        }
    }

    /// The inputs as `Item`s, for one thread.
    pub fn prepare(&self) -> Prepared<'_> {
        let inputs = self.cases.iter().map(|c| c.inputs.iter().map(|e| Item::from_edn(e).unwrap()).collect()).collect();
        Prepared { problem: self, inputs }
    }

    fn error(&self, expected: &Value, actual: &Value, out: &mut Vec<f64>) {
        match self.error {
            ErrKind::Numeric => out.push(numeric_error(expected, actual)),
            ErrKind::Levenshtein => out.push(match (expected, actual) {
                (Value::Str(e), Value::Str(a)) => levenshtein(e, a) as f64,
                _ => NO_OUTPUT_PENALTY,
            }),
            ErrKind::Exact => out.push(match actual {
                Value::Missing => NO_OUTPUT_PENALTY,
                a if a == expected => 0.0,
                _ => 1.0,
            }),
            ErrKind::Bitwise => match (expected, actual) {
                (Value::Bits(e), Value::Bits(a)) => out.extend(e.iter().zip(a).map(|(e, a)| if a == e && a.is_some() { 0.0 } else { 1.0 })),
                (Value::Bits(e), _) => out.extend(e.iter().map(|_| 1.0)),
                _ => out.push(NO_OUTPUT_PENALTY),
            },
            ErrKind::NumericPrinted => out.push(match (expected, actual) {
                (e, Value::Str(s)) => match parse_java_double(s) {
                    Some(v) => numeric_error(e, &Value::Float(v)),
                    None => NO_OUTPUT_PENALTY,
                },
                _ => NO_OUTPUT_PENALTY,
            }),
            ErrKind::NumericPair => out.push(match (expected, actual) {
                (Value::Pair(e1, e2), Value::Pair(a1, a2)) => {
                    numeric_error(&Value::Int(*e1), &Value::Int(*a1)) + numeric_error(&Value::Int(*e2), &Value::Int(*a2))
                }
                _ => NO_OUTPUT_PENALTY,
            }),
        }
    }
}

/// A problem with its inputs parsed for the current thread.
pub struct Prepared<'a> {
    problem: &'a Problem,
    inputs: Vec<Vec<Item>>,
}

impl Prepared<'_> {
    pub fn case_inputs(&self) -> &[Vec<Item>] {
        &self.inputs
    }

    /// Runs `program` on every case. With `cap`, stops as soon as the total error exceeds it.
    pub fn evaluate(&self, program: &Item, cap: Option<f64>) -> Row {
        let p = self.problem;
        let mut row = Row { errors: Vec::with_capacity(p.cases.len()), steps: 0, cut: false };
        let mut total = 0.0;
        let mut m = Machine::new(program.clone(), Vec::new());
        for (c, inputs) in p.cases.iter().zip(&self.inputs) {
            m.reset(program, inputs);
            for (name, v) in p.input_names.iter().zip(inputs) {
                m.named.push((name.clone(), v.clone()));
            }
            m.run(p.max);
            row.steps += m.steps;
            let actual = p.read_output(&m);
            let before = row.errors.len();
            p.error(&c.expected, &actual, &mut row.errors);
            total += row.errors[before..].iter().sum::<f64>();
            if cap.is_some_and(|cap| total > cap) {
                row.cut = true;
                break;
            }
        }
        row
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn numeric_error(expected: &Value, actual: &Value) -> f64 {
    match (expected, actual) {
        // exact in i128, like Clojure's long arithmetic, then to a double
        (Value::Int(e), Value::Int(a)) => (*e as i128 - *a as i128).unsigned_abs() as f64,
        (e, a) => match (as_f64(e), as_f64(a)) {
            (Some(e), Some(a)) => (e - a).abs(),
            _ => NO_OUTPUT_PENALTY,
        },
    }
}

/// `Double/parseDouble (string/trim s)` for the forms a program can print: decimal numbers with an
/// optional exponent, NaN and Infinity. (Java also takes hex floats and a d/f suffix; not modelled.)
fn parse_java_double(s: &str) -> Option<f64> {
    let t = s.trim_matches(|c: char| c <= ' ');
    match t {
        "NaN" | "+NaN" | "-NaN" => return Some(f64::NAN),
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E')) {
        return None;
    }
    t.parse::<f64>().ok()
}

/// Edit distance over chars (insert, delete, substitute), two rows.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = if ca == cb { prev[j] } else { 1 + prev[j].min(prev[j + 1]).min(cur[j]) };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// A program line is either the program itself or a map with `:program`.
pub fn parse_program_line(line: &str) -> Result<(Item, Option<Vec<f64>>), String> {
    let e = edn::parse(line)?;
    match e.get("program") {
        Some(p) => {
            let errs = e.get("errors").and_then(|v| v.items()).map(|xs| {
                xs.iter()
                    .map(|x| match x {
                        Edn::Float(f) => *f,
                        Edn::Int(i) => *i as f64,
                        _ => f64::NAN,
                    })
                    .collect()
            });
            Ok((Item::from_edn(p)?, errs))
        }
        None => Ok((Item::from_edn(&e)?, None)),
    }
}

/// Evaluates every line on `threads` threads; the result keeps the order of `lines`.
pub fn evaluate_lines(problem: &Problem, lines: &[String], threads: usize, cap: Option<f64>) -> Vec<Result<Row, String>> {
    let next = AtomicUsize::new(0);
    let chunk = 16;
    let mut slots: Vec<Option<Result<Row, String>>> = (0..lines.len()).map(|_| None).collect();
    let results = std::sync::Mutex::new(&mut slots);
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            // exec_y and friends nest deeply: parsing and dropping need a big stack
            std::thread::Builder::new()
                .stack_size(1 << 28)
                .spawn_scoped(s, || {
                    let prepared = problem.prepare();
                    loop {
                    let start = next.fetch_add(chunk, Ordering::Relaxed);
                    if start >= lines.len() {
                        break;
                    }
                    let end = (start + chunk).min(lines.len());
                    // programs are parsed here: Item is Rc-based, so it never crosses threads
                    let done: Vec<_> = (start..end)
                        .map(|i| (i, parse_program_line(&lines[i]).map(|(p, _)| prepared.evaluate(&p, cap))))
                        .collect();
                    let mut g = results.lock().unwrap();
                    for (i, r) in done {
                        g[i] = Some(r);
                    }
                    }
                })
                .unwrap();
        }
    });
    slots.into_iter().map(|r| r.unwrap()).collect()
}
