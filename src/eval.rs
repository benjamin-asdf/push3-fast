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
    /// 0: no limit (the oracle's semantics)
    pub stack_limit: usize,
    input_names: Vec<String>,
    cases: Vec<ProblemCase>,
}

/// Early exits. `cap`: stop as soon as the total error exceeds it. `screen`: `(k, x)` stops after
/// the first `k` cases if the total error so far exceeds `x` (a cheap test for programs that are
/// clearly bad, before paying for all cases; order the cases so the first ones are informative).
#[derive(Clone, Copy, Default)]
pub struct Limits {
    pub cap: Option<f64>,
    pub screen: Option<(usize, f64)>,
}

/// What evaluating one program gave.
pub struct Row {
    pub errors: Vec<f64>,
    pub steps: u64,
    /// Some case was aborted by the stack limit and scored as no output.
    pub overflow: bool,
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
            format!("{}{}", if self.overflow { " :overflow true" } else { "" }, if self.cut { " :cut true" } else { "" })
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
        Ok(Problem { output, error, max, stack_limit: 0, input_names, cases })
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
        Prepared { problem: self, inputs, machine: Machine::new(Item::List(std::rc::Rc::new(Vec::new())), Vec::new()) }
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
    /// One machine per thread, for all its programs: the stacks keep their capacity, so a program
    /// that grows a stack to a million items does not page-fault fresh memory every time.
    machine: Machine,
}

impl Prepared<'_> {
    pub fn case_inputs(&self) -> &[Vec<Item>] {
        &self.inputs
    }

    /// Runs `program` on every case. With `cap`, stops as soon as the total error exceeds it.
    pub fn evaluate(&mut self, program: &Item, limits: Limits) -> Row {
        let p = self.problem;
        let mut row = Row { errors: Vec::with_capacity(p.cases.len()), steps: 0, overflow: false, cut: false };
        let mut total = 0.0;
        let m = &mut self.machine;
        for (k, (c, inputs)) in p.cases.iter().zip(&self.inputs).enumerate() {
            m.reset(program, inputs);
            m.stack_limit = if p.stack_limit == 0 { usize::MAX } else { p.stack_limit };
            for (name, v) in p.input_names.iter().zip(inputs) {
                m.named.push((name.clone(), v.clone()));
            }
            m.run(p.max);
            row.steps += m.steps;
            let actual = if m.overflowed { row.overflow = true; Value::Missing } else { p.read_output(m) };
            let before = row.errors.len();
            p.error(&c.expected, &actual, &mut row.errors);
            total += row.errors[before..].iter().sum::<f64>();
            let screened = limits.screen.is_some_and(|(n, x)| k + 1 == n && total > x);
            if screened || limits.cap.is_some_and(|cap| total > cap) {
                row.cut = true;
                break;
            }
        }
        m.trim();
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
pub fn evaluate_lines(problem: &Problem, lines: &[String], threads: usize, limits: Limits) -> Vec<Result<Row, String>> {
    let next = AtomicUsize::new(0);
    let chunk = 1; // a program costs far more than the atomic counter, and a few programs cost 100x the rest
    let mut slots: Vec<Option<Result<Row, String>>> = (0..lines.len()).map(|_| None).collect();
    let results = std::sync::Mutex::new(&mut slots);
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            // exec_y and friends nest deeply: parsing and dropping need a big stack
            std::thread::Builder::new()
                .stack_size(1 << 28)
                .spawn_scoped(s, || {
                    let mut prepared = problem.prepare();
                    loop {
                    let start = next.fetch_add(chunk, Ordering::Relaxed);
                    if start >= lines.len() {
                        break;
                    }
                    let end = (start + chunk).min(lines.len());
                    // programs are parsed here: Item is Rc-based, so it never crosses threads
                    let done: Vec<_> = (start..end)
                        .map(|i| (i, parse_program_line(&lines[i]).map(|(p, _)| prepared.evaluate(&p, limits))))
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

#[cfg(test)]
mod tests {
    use super::*;

    const PROBLEM: &str = r#"{:output :push/integer :error :numeric :max 100
        :cases [{:inputs [1] :output 2} {:inputs [5] :output 10} {:inputs [7] :output 14} {:inputs [9] :output 18}]}"#;

    fn run(program: &str, limits: Limits) -> Row {
        let p = Problem::parse(PROBLEM).unwrap();
        let (prog, _) = parse_program_line(program).unwrap();
        p.prepare().evaluate(&prog, limits)
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("same", "same"), 0);
    }

    #[test]
    fn solved_program_has_zero_errors() {
        let r = run("(:in1 2 :integer_*)", Limits::default());
        assert_eq!(r.errors, vec![0.0; 4]);
        assert!(!r.cut);
    }

    #[test]
    fn empty_output_gets_the_penalty() {
        let r = run("()", Limits::default());
        assert_eq!(r.errors, vec![NO_OUTPUT_PENALTY; 4]);
    }

    #[test]
    fn cap_and_screen_return_a_prefix() {
        let full = run("(:in1)", Limits::default());
        assert_eq!(full.errors, vec![1.0, 5.0, 7.0, 9.0]);
        let capped = run("(:in1)", Limits { cap: Some(5.0), screen: None });
        assert!(capped.cut);
        assert_eq!(capped.errors, vec![1.0, 5.0]);
        let screened = run("(:in1)", Limits { cap: None, screen: Some((1, 0.5)) });
        assert!(screened.cut);
        assert_eq!(screened.errors, vec![1.0]);
        let passed = run("(:in1)", Limits { cap: None, screen: Some((1, 1.0)) });
        assert!(!passed.cut);
        assert_eq!(passed.errors.len(), 4);
    }

    #[test]
    fn stack_limit_scores_the_case_as_no_output() {
        let mut p = Problem::parse(PROBLEM).unwrap();
        p.stack_limit = 50;
        // 7 pushed 100 times by dup_times, then the limit is exceeded
        let (prog, _) = parse_program_line("(7 100 :integer_dup_times)").unwrap();
        let r = p.prepare().evaluate(&prog, Limits::default());
        assert!(r.overflow);
        assert_eq!(r.errors, vec![NO_OUTPUT_PENALTY; 4]);
    }
}
