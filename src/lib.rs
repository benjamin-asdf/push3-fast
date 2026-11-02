//! Fast Push3 core. Stage 2a: EXEC, INTEGER, BOOLEAN, FLOAT, CHAR, PRINT, output registers.
//!
//! The semantics are those of the Clojure interpreter in clj-push3, quirks included
//! (argument order, clamping, `rot`, `yank` index handling, how FLOAT values are sanitised).
//! That interpreter is the test oracle: `push3-fast diff <cases.edn>` compares complete final
//! states against it.

pub mod edn;
pub mod fmt;
pub mod strings;
pub mod vectors;

use std::rc::Rc;

pub const LIM: i64 = 10_000_000_000;

/// Integers are clamped when pushed (`sanitize-value :push/integer`).
#[inline]
pub fn clamp(v: i128) -> i64 {
    v.clamp(-(LIM as i128), LIM as i128) as i64
}

/// `(float 1e-12)`: a java.lang.Float, widened. This is what every FLOAT value with |x| < 1e-12
/// (zero included, sign lost) becomes when pushed.
pub const TINY: f64 = 1e-12_f32 as f64;

/// `sanitize-value :push/float`: clamp to +-1e12 (NaN propagates), tiny values become TINY.
#[inline]
pub fn sanitize_float(v: f64) -> f64 {
    let v = if v.is_nan() { v } else { v.clamp(-1e12, 1e12) };
    if v.abs() < 1e-12 {
        TINY
    } else {
        v
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    Int,
    Bool,
    Exec,
    Float,
    Char,
    /// STRING stack (items are `Rc<str>`).
    Str,
    /// `[:push/vector :push/integer]` stack.
    IntVec,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fam {
    StackDepth,
    Pop,
    Dup,
    Flush,
    Shove,
    Yank,
    YankDup,
    Rot,
    Swap,
    Empty,
    DupTimes,
    DupItems,
    DeepDup,
    Print,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Fam(Fam, Ty),
    IntAdd,
    IntSub,
    IntMul,
    IntDiv,
    IntMod,
    IntQuot,
    IntInc,
    IntDec,
    IntLt,
    IntGt,
    IntEq,
    IntGte,
    IntLte,
    IntMin,
    IntMax,
    IntFromBool,
    IntFromFloat,
    IntFromChar,
    BoolAnd,
    BoolOr,
    BoolNot,
    BoolEq,
    BoolXor,
    BoolInvFirstAnd,
    BoolInvSecondAnd,
    BoolNand,
    BoolNor,
    BoolXnor,
    BoolFromInt,
    BoolFromFloat,
    ExecIf,
    ExecWhen,
    ExecWhile,
    ExecDoWhile,
    ExecK,
    ExecS,
    ExecY,
    ExecDoRange,
    ExecDoCount,
    ExecDoTimes,
    ExecEq,
    FloatAdd,
    FloatSub,
    FloatMul,
    FloatDiv,
    FloatMod,
    FloatQuot,
    FloatInc,
    FloatDec,
    FloatLt,
    FloatGt,
    FloatEq,
    FloatGte,
    FloatLte,
    FloatMin,
    FloatMax,
    FloatFromBool,
    FloatFromInt,
    FloatFromChar,
    FloatSin,
    FloatCos,
    FloatTan,
    CharIsLetter,
    CharIsDigit,
    CharIsWhitespace,
    CharIsUpper,
    CharIsLower,
    CharFromInt,
    CharFromFloat,
    CharToInt,
    CharToUpper,
    CharToLower,
    CharEq,
    CharLt,
    CharGt,
    PrintNewline,
    SetC(u8),
    C(u8),
    In(u8),
    /// `string_*` instructions that are not the shared stack family; see `strings.rs`.
    Str(strings::StrOp),
    /// `vector_integer_*` instructions that are not the shared stack family; see `vectors.rs`.
    Vec(vectors::VecOp),
}

#[derive(Clone, PartialEq, Debug)]
pub enum Item {
    Int(i64),
    Bool(bool),
    Float(f64),
    Char(char),
    /// `nil`, as it can sit on the CHAR stack (e.g. `string_first` of "").
    Nil,
    Str(Rc<str>),
    IntVec(Rc<Vec<i64>>),
    Op(Op),
    /// A keyword that is not an instruction. It resolves to a named input (`a2`, `b0`, ...) or,
    /// in the oracle, lands on the CLJ-OBJECT stack, which this build does not model.
    Name(Rc<str>),
    List(Rc<Vec<Item>>),
}

const FAMS: [(&str, Fam); 14] = [
    ("stackdepth", Fam::StackDepth),
    ("pop", Fam::Pop),
    ("dup", Fam::Dup),
    ("flush", Fam::Flush),
    ("shove", Fam::Shove),
    ("yank", Fam::Yank),
    ("yankdup", Fam::YankDup),
    ("rot", Fam::Rot),
    ("swap", Fam::Swap),
    ("empty", Fam::Empty),
    ("dup_times", Fam::DupTimes),
    ("dup_items", Fam::DupItems),
    ("deep_dup", Fam::DeepDup),
    ("print", Fam::Print),
];
const TYS: [(&str, Ty); 7] = [
    ("integer", Ty::Int),
    ("boolean", Ty::Bool),
    ("exec", Ty::Exec),
    ("float", Ty::Float),
    ("char", Ty::Char),
    ("string", Ty::Str),
    ("vector_integer", Ty::IntVec),
];

const PLAIN: &[(&str, Op)] = &[
    ("integer_+", Op::IntAdd),
    ("integer_-", Op::IntSub),
    ("integer_*", Op::IntMul),
    ("integer_div", Op::IntDiv),
    ("integer_mod", Op::IntMod),
    ("integer_quot", Op::IntQuot),
    ("integer_inc", Op::IntInc),
    ("integer_dec", Op::IntDec),
    ("integer_<", Op::IntLt),
    ("integer_>", Op::IntGt),
    ("integer_=", Op::IntEq),
    ("integer_>=", Op::IntGte),
    ("integer_<=", Op::IntLte),
    ("integer_min", Op::IntMin),
    ("integer_max", Op::IntMax),
    ("integer_fromboolean", Op::IntFromBool),
    ("integer_fromfloat", Op::IntFromFloat),
    ("integer_fromchar", Op::IntFromChar),
    ("boolean_and", Op::BoolAnd),
    ("boolean_or", Op::BoolOr),
    ("boolean_not", Op::BoolNot),
    ("boolean_=", Op::BoolEq),
    ("boolean_xor", Op::BoolXor),
    ("boolean_invert_first_then_and", Op::BoolInvFirstAnd),
    ("boolean_invert_second_then_and", Op::BoolInvSecondAnd),
    ("boolean_nand", Op::BoolNand),
    ("boolean_nor", Op::BoolNor),
    ("boolean_xnor", Op::BoolXnor),
    ("boolean_frominteger", Op::BoolFromInt),
    ("boolean_fromfloat", Op::BoolFromFloat),
    ("exec_if", Op::ExecIf),
    ("exec_when", Op::ExecWhen),
    ("exec_while", Op::ExecWhile),
    ("exec_do_while", Op::ExecDoWhile),
    ("exec_k", Op::ExecK),
    ("exec_s", Op::ExecS),
    ("exec_y", Op::ExecY),
    ("exec_do*range", Op::ExecDoRange),
    ("exec_do*count", Op::ExecDoCount),
    ("exec_do*times", Op::ExecDoTimes),
    ("exec_=", Op::ExecEq),
    ("float_+", Op::FloatAdd),
    ("float_-", Op::FloatSub),
    ("float_*", Op::FloatMul),
    ("float_div", Op::FloatDiv),
    ("float_mod", Op::FloatMod),
    ("float_quot", Op::FloatQuot),
    ("float_inc", Op::FloatInc),
    ("float_dec", Op::FloatDec),
    ("float_<", Op::FloatLt),
    ("float_>", Op::FloatGt),
    ("float_=", Op::FloatEq),
    ("float_>=", Op::FloatGte),
    ("float_<=", Op::FloatLte),
    ("float_min", Op::FloatMin),
    ("float_max", Op::FloatMax),
    ("float_fromboolean", Op::FloatFromBool),
    ("float_frominteger", Op::FloatFromInt),
    ("float_fromchar", Op::FloatFromChar),
    ("float_sin", Op::FloatSin),
    ("float_cos", Op::FloatCos),
    ("float_tan", Op::FloatTan),
    ("char_is_letter", Op::CharIsLetter),
    ("char_is_digit", Op::CharIsDigit),
    ("char_is_whitespace", Op::CharIsWhitespace),
    ("char_is_uppercase", Op::CharIsUpper),
    ("char_is_lowercase", Op::CharIsLower),
    ("char_from_integer", Op::CharFromInt),
    ("char_from_float", Op::CharFromFloat),
    ("char_to_integer", Op::CharToInt),
    ("char_to_uppercase", Op::CharToUpper),
    ("char_to_lowercase", Op::CharToLower),
    ("char_=", Op::CharEq),
    ("char_<", Op::CharLt),
    ("char_>", Op::CharGt),
    ("print_newline", Op::PrintNewline),
];

impl Op {
    pub fn from_name(name: &str) -> Option<Op> {
        if let Some((_, op)) = PLAIN.iter().find(|(n, _)| *n == name) {
            return Some(*op);
        }
        let indexed = |prefix: &str, max: u8, lo: u8| -> Option<u8> {
            let k = name.strip_prefix(prefix)?.parse::<u8>().ok()?;
            (lo..=max).contains(&k).then_some(k)
        };
        if let Some(k) = indexed("in", 8, 1) {
            return Some(Op::In(k));
        }
        if let Some(k) = indexed("set_c", 7, 0) {
            return Some(Op::SetC(k));
        }
        if let Some(k) = indexed("c", 7, 0) {
            return Some(Op::C(k));
        }
        for (tn, ty) in TYS {
            if let Some(rest) = name.strip_prefix(tn).and_then(|r| r.strip_prefix('_')) {
                if let Some((_, f)) = FAMS.iter().find(|(n, _)| *n == rest) {
                    return Some(Op::Fam(*f, ty));
                }
            }
        }
        if let Some(o) = strings::from_name(name) {
            return Some(Op::Str(o));
        }
        vectors::from_name(name).map(Op::Vec)
    }

    pub fn name(&self) -> String {
        if let Some((n, _)) = PLAIN.iter().find(|(_, o)| o == self) {
            return n.to_string();
        }
        match self {
            Op::In(k) => format!("in{k}"),
            Op::SetC(k) => format!("set_c{k}"),
            Op::C(k) => format!("c{k}"),
            Op::Str(o) => strings::name(*o),
            Op::Vec(o) => vectors::name(*o),
            Op::Fam(f, t) => {
                let tn = TYS.iter().find(|(_, x)| x == t).unwrap().0;
                let fname = FAMS.iter().find(|(_, x)| x == f).unwrap().0;
                format!("{tn}_{fname}")
            }
            _ => unreachable!(),
        }
    }
}

/// Every instruction name this build implements (one per line in `push3-fast names`).
pub fn all_names() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for (tn, _) in TYS {
        for (fname, _) in FAMS {
            v.push(format!("{tn}_{fname}"));
        }
    }
    v.extend(PLAIN.iter().map(|(n, _)| n.to_string()));
    v.extend((1..=8).map(|k| format!("in{k}")));
    v.extend((0..8).map(|k| format!("set_c{k}")));
    v.extend((0..8).map(|k| format!("c{k}")));
    v.extend(strings::NAMES.iter().map(|(n, _)| n.to_string()));
    v.extend(vectors::NAMES.iter().map(|(n, _)| n.to_string()));
    v
}

pub struct Machine {
    pub exec: Vec<Item>,
    pub int: Vec<i64>,
    pub boolean: Vec<bool>,
    pub float: Vec<f64>,
    /// `None` is a `nil` item; instructions that cannot take it are no-ops that still pop it.
    pub chr: Vec<Option<char>>,
    pub string: Vec<Rc<str>>,
    pub ivec: Vec<Rc<Vec<i64>>>,
    /// Everything printed so far (the oracle keeps it as the single item of the PRINT stack).
    pub print: String,
    /// Output registers c0..c7; `None` while unset.
    pub regs: [Option<bool>; 8],
    pub inputs: Vec<Item>,
    /// Inputs bound by name instead of `inN` (the multiplier problems).
    pub named: Vec<(String, Item)>,
    /// Set when the program touched behaviour this build does not model (so the run is not comparable).
    pub unsupported: bool,
    pub steps: u64,
}

const MAX_STACK_ITEMS: i64 = 1000;

/// Java's Character predicates, exact for ASCII and approximated by Unicode properties above.
fn is_java_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{1c}'..='\u{1f}' | ' ')
        || (!c.is_ascii() && c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}'))
}

fn java_upper(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_uppercase()
    } else {
        let mut u = c.to_uppercase();
        match (u.next(), u.next()) {
            (Some(x), None) => x,
            _ => c,
        }
    }
}

fn java_lower(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        let mut u = c.to_lowercase();
        match (u.next(), u.next()) {
            (Some(x), None) => x,
            _ => c,
        }
    }
}

/// Clojure `rem` on doubles: `n - (long)(n/d) * d` (not fmod). `None` where Clojure throws.
fn clj_rem(n: f64, d: f64) -> Option<f64> {
    let q = n / d;
    if q.is_nan() || q.is_infinite() {
        return None;
    }
    Some(n - q.trunc() * d)
}

/// Clojure `quot` on doubles; `None` where Clojure throws.
fn clj_quot(n: f64, d: f64) -> Option<f64> {
    let q = n / d;
    if q.is_nan() || q.is_infinite() {
        return None;
    }
    Some(q.trunc())
}

/// Clojure `mod` on doubles.
fn clj_mod(n: f64, d: f64) -> Option<f64> {
    let m = clj_rem(n, d)?;
    if m == 0.0 || ((n > 0.0) == (d > 0.0)) {
        Some(m)
    } else {
        Some(m + d)
    }
}

/// `(long x)` on a double: throws outside the long range, NaN gives 0.
fn clj_long(x: f64) -> Option<i64> {
    if x < i64::MIN as f64 || x > i64::MAX as f64 {
        None
    } else {
        Some(x as i64)
    }
}

impl Machine {
    pub fn new(program: Item, inputs: Vec<Item>) -> Machine {
        Machine {
            exec: vec![program],
            int: Vec::new(),
            boolean: Vec::new(),
            float: Vec::new(),
            chr: Vec::new(),
            string: Vec::new(),
            ivec: Vec::new(),
            print: String::new(),
            regs: [None; 8],
            inputs,
            named: Vec::new(),
            unsupported: false,
            steps: 0,
        }
    }

    /// Runs until the EXEC stack is empty or `max_steps` EXEC pops have happened.
    pub fn run(&mut self, max_steps: u64) {
        while self.steps < max_steps && !self.exec.is_empty() {
            self.step();
        }
    }

    #[inline]
    pub fn step(&mut self) {
        let item = match self.exec.pop() {
            Some(i) => i,
            None => return,
        };
        self.steps += 1;
        match item {
            Item::Op(op) => self.run_op(op),
            Item::Int(v) => self.int.push(clamp(v as i128)),
            Item::Bool(b) => self.boolean.push(b),
            Item::Float(v) => self.float.push(sanitize_float(v)),
            Item::Char(c) => self.chr.push(Some(c)),
            Item::Str(t) => self.string.push(t),
            Item::IntVec(v) => self.ivec.push(v),
            // a nil literal goes to the CLJ-OBJECT stack in the oracle, which is not modelled
            Item::Nil => self.unsupported = true,
            Item::Name(n) => match self.named.iter().find(|(k, _)| k.as_str() == &*n) {
                Some((_, v)) => self.exec.push(v.clone()),
                None => self.unsupported = true,
            },
            Item::List(l) => {
                for it in l.iter().rev() {
                    self.exec.push(it.clone());
                }
            }
        }
    }

    fn len(&self, t: Ty) -> usize {
        match t {
            Ty::Int => self.int.len(),
            Ty::Bool => self.boolean.len(),
            Ty::Exec => self.exec.len(),
            Ty::Float => self.float.len(),
            Ty::Char => self.chr.len(),
            Ty::Str => self.string.len(),
            Ty::IntVec => self.ivec.len(),
        }
    }

    fn get(&self, t: Ty, i: usize) -> Item {
        match t {
            Ty::Int => Item::Int(self.int[i]),
            Ty::Bool => Item::Bool(self.boolean[i]),
            Ty::Exec => self.exec[i].clone(),
            Ty::Float => Item::Float(self.float[i]),
            Ty::Char => self.chr[i].map_or(Item::Nil, Item::Char),
            Ty::Str => Item::Str(self.string[i].clone()),
            Ty::IntVec => Item::IntVec(self.ivec[i].clone()),
        }
    }

    fn push_item(&mut self, t: Ty, it: Item) {
        match (t, it) {
            (Ty::Int, Item::Int(v)) => self.int.push(clamp(v as i128)),
            (Ty::Bool, Item::Bool(b)) => self.boolean.push(b),
            (Ty::Float, Item::Float(v)) => self.float.push(sanitize_float(v)),
            (Ty::Char, Item::Char(c)) => self.chr.push(Some(c)),
            (Ty::Char, Item::Nil) => self.chr.push(None),
            (Ty::Str, Item::Str(t)) => self.string.push(t),
            (Ty::IntVec, Item::IntVec(v)) => self.ivec.push(v),
            (Ty::Exec, it) => self.exec.push(it),
            _ => unreachable!(),
        }
    }

    fn pop_item(&mut self, t: Ty) -> Item {
        match t {
            Ty::Int => Item::Int(self.int.pop().unwrap()),
            Ty::Bool => Item::Bool(self.boolean.pop().unwrap()),
            Ty::Exec => self.exec.pop().unwrap(),
            Ty::Float => Item::Float(self.float.pop().unwrap()),
            Ty::Char => self.chr.pop().unwrap().map_or(Item::Nil, Item::Char),
            Ty::Str => Item::Str(self.string.pop().unwrap()),
            Ty::IntVec => Item::IntVec(self.ivec.pop().unwrap()),
        }
    }

    fn insert_item(&mut self, t: Ty, idx: usize, it: Item) {
        match (t, it) {
            (Ty::Int, Item::Int(v)) => self.int.insert(idx, clamp(v as i128)),
            (Ty::Bool, Item::Bool(b)) => self.boolean.insert(idx, b),
            (Ty::Float, Item::Float(v)) => self.float.insert(idx, sanitize_float(v)),
            (Ty::Char, Item::Char(c)) => self.chr.insert(idx, Some(c)),
            (Ty::Char, Item::Nil) => self.chr.insert(idx, None),
            (Ty::Str, Item::Str(t)) => self.string.insert(idx, t),
            (Ty::IntVec, Item::IntVec(v)) => self.ivec.insert(idx, v),
            (Ty::Exec, it) => self.exec.insert(idx, it),
            _ => unreachable!(),
        }
    }

    fn remove_item(&mut self, t: Ty, idx: usize) -> Item {
        match t {
            Ty::Int => Item::Int(self.int.remove(idx)),
            Ty::Bool => Item::Bool(self.boolean.remove(idx)),
            Ty::Exec => self.exec.remove(idx),
            Ty::Float => Item::Float(self.float.remove(idx)),
            Ty::Char => self.chr.remove(idx).map_or(Item::Nil, Item::Char),
            Ty::Str => Item::Str(self.string.remove(idx)),
            Ty::IntVec => Item::IntVec(self.ivec.remove(idx)),
        }
    }

    fn flush(&mut self, t: Ty) {
        match t {
            Ty::Int => self.int.clear(),
            Ty::Bool => self.boolean.clear(),
            Ty::Exec => self.exec.clear(),
            Ty::Float => self.float.clear(),
            Ty::Char => self.chr.clear(),
            Ty::Str => self.string.clear(),
            Ty::IntVec => self.ivec.clear(),
        }
    }

    /// `takes-index?` of the polymorphic operators in impl.clj.
    fn takes_index(&self, t: Ty) -> bool {
        if t == Ty::Int {
            self.int.len() >= 2
        } else {
            !self.int.is_empty() && self.len(t) > 0
        }
    }

    /// `print-form`: strings and chars print as themselves, everything else through `pr-str`.
    fn print_form(t: Ty, it: &Item) -> String {
        match (t, it) {
            // `print-form`: a char (or string) item prints as itself, whatever stack it came from
            (_, Item::Char(c)) => c.to_string(),
            (_, Item::Str(t)) => t.to_string(),
            (Ty::Float, Item::Float(v)) => fmt::pr_float_stack(*v),
            _ => fmt::pr_str(it),
        }
    }

    fn family(&mut self, f: Fam, t: Ty) {
        match f {
            Fam::StackDepth => {
                let n = self.len(t) as i64;
                self.int.push(n);
            }
            Fam::Pop => {
                if self.len(t) >= 1 {
                    self.pop_item(t);
                }
            }
            Fam::Dup => {
                let n = self.len(t);
                if n >= 1 {
                    let top = self.get(t, n - 1);
                    self.push_item(t, top);
                }
            }
            Fam::Flush => self.flush(t),
            Fam::Swap => {
                if self.len(t) >= 2 {
                    let a = self.pop_item(t);
                    let b = self.pop_item(t);
                    self.push_item(t, a);
                    self.push_item(t, b);
                }
            }
            // pop-n gives [a b c] with a the top; it pushes b, c, a.
            Fam::Rot => {
                if self.len(t) >= 3 {
                    let a = self.pop_item(t);
                    let b = self.pop_item(t);
                    let c = self.pop_item(t);
                    self.push_item(t, b);
                    self.push_item(t, c);
                    self.push_item(t, a);
                }
            }
            Fam::Empty => {
                let e = self.len(t) == 0;
                self.boolean.push(e);
            }
            Fam::Print => {
                if self.len(t) >= 1 {
                    let it = self.pop_item(t);
                    let s = Self::print_form(t, &it);
                    self.print.push_str(&s);
                }
            }
            Fam::Shove => {
                // Oracle quirk: shove-item reads the stack with (-> state :stacks push-type); for the
                // vector stack push-type is a vector, not a function of a map, so it throws: a no-op
                // that has already popped the vector and the index.
                if t == Ty::IntVec {
                    if self.len(t) >= 1 && !self.int.is_empty() {
                        self.ivec.pop();
                        self.int.pop();
                    }
                    return;
                }
                let ok = if t == Ty::Int { self.int.len() >= 2 } else { self.len(t) >= 1 && !self.int.is_empty() };
                if !ok {
                    return;
                }
                let item = self.pop_item(t);
                let index = self.int.pop().unwrap() as i128;
                let n = self.len(t);
                let idx = if n == 0 { 0 } else { (index.abs() % n as i128) as usize };
                self.insert_item(t, idx, item);
            }
            Fam::Yank => {
                if self.int.is_empty() {
                    return;
                }
                let index = self.int.pop().unwrap();
                let n = self.len(t);
                if n == 0 {
                    return;
                }
                // (Math/abs (int index)): `int` throws outside the int range, the exception is a
                // no-op that has already popped the index; abs of Integer/MIN_VALUE stays negative.
                if index < i32::MIN as i64 || index > i32::MAX as i64 {
                    return;
                }
                let a = (index as i32).wrapping_abs() as i64;
                let idx = a.rem_euclid(n as i64) as usize;
                let item = self.remove_item(t, idx);
                self.push_item(t, item);
            }
            Fam::YankDup => {
                if self.int.is_empty() {
                    return;
                }
                let index = self.int.pop().unwrap() as i128;
                let n = self.len(t);
                if n == 0 {
                    return;
                }
                let idx = (index.abs() % n as i128) as usize;
                let item = self.get(t, idx);
                self.push_item(t, item);
            }
            Fam::DupTimes => {
                if !self.takes_index(t) {
                    return;
                }
                let n = self.int.pop().unwrap();
                let top = self.get(t, self.len(t) - 1);
                if n > 0 {
                    for _ in 0..(n.min(MAX_STACK_ITEMS) - 1) {
                        self.push_item(t, top.clone());
                    }
                } else {
                    self.pop_item(t);
                }
            }
            Fam::DupItems => {
                if self.int.is_empty() {
                    return;
                }
                let n = self.int.pop().unwrap();
                let len = self.len(t);
                let n = n.min(len as i64).min(MAX_STACK_ITEMS);
                if n <= 0 {
                    return;
                }
                let start = len - n as usize;
                let items: Vec<Item> = (start..len).map(|i| self.get(t, i)).collect();
                for it in items {
                    self.push_item(t, it);
                }
            }
            Fam::DeepDup => {
                if !self.takes_index(t) {
                    return;
                }
                let index = self.int.pop().unwrap();
                let n = self.len(t) as i64;
                let idx = index.min(n - 1).max(0) as usize;
                let item = self.get(t, idx);
                self.push_item(t, item);
            }
        }
    }

    fn run_op(&mut self, op: Op) {
        macro_rules! need {
            ($i:expr, $b:expr, $e:expr) => {
                if self.int.len() < $i || self.boolean.len() < $b || self.exec.len() < $e {
                    return;
                }
            };
        }
        // `a` is the top of the stack, `b` the item below it, as in the Clojure arglists.
        macro_rules! int2 {
            ($a:ident, $b:ident) => {
                need!(2, 0, 0);
                let $a = self.int.pop().unwrap() as i128;
                let $b = self.int.pop().unwrap() as i128;
            };
        }
        macro_rules! bool2 {
            ($a:ident, $b:ident) => {
                need!(0, 2, 0);
                let $a = self.boolean.pop().unwrap();
                let $b = self.boolean.pop().unwrap();
            };
        }
        macro_rules! float2 {
            ($a:ident, $b:ident) => {
                if self.float.len() < 2 {
                    return;
                }
                let $a = self.float.pop().unwrap();
                let $b = self.float.pop().unwrap();
            };
        }
        macro_rules! float1 {
            ($a:ident) => {
                if self.float.is_empty() {
                    return;
                }
                let $a = self.float.pop().unwrap();
            };
        }
        // A nil on the CHAR stack makes the Clojure instruction throw (NullPointerException or
        // "no matching method"): a no-op that has already popped its arguments.
        macro_rules! char1 {
            ($a:ident) => {
                if self.chr.is_empty() {
                    return;
                }
                let $a = match self.chr.pop().unwrap() {
                    Some(c) => c,
                    None => return,
                };
            };
        }
        macro_rules! char2 {
            ($a:ident, $b:ident) => {
                if self.chr.len() < 2 {
                    return;
                }
                let a = self.chr.pop().unwrap();
                let b = self.chr.pop().unwrap();
                let ($a, $b) = match (a, b) {
                    (Some(a), Some(b)) => (a, b),
                    _ => return,
                };
            };
        }
        macro_rules! pushf {
            ($v:expr) => {
                self.float.push(sanitize_float($v))
            };
        }
        match op {
            Op::Fam(f, t) => self.family(f, t),
            Op::IntAdd => {
                int2!(a, b);
                self.int.push(clamp(a + b));
            }
            Op::IntSub => {
                int2!(a, b);
                self.int.push(clamp(b - a));
            }
            Op::IntMul => {
                int2!(a, b);
                self.int.push(clamp(a * b));
            }
            Op::IntDiv => {
                int2!(a, b);
                if a != 0 {
                    // (bigint (/ (Math/floor dividend) (Math/floor divisor))): double division, then truncation
                    let q = (b as f64 / a as f64).trunc();
                    self.int.push(clamp(q as i128));
                }
            }
            Op::IntMod => {
                int2!(a, b);
                if a != 0 {
                    // Clojure `mod` is floored: the result has the sign of the divisor
                    self.int.push(clamp(((b % a) + a) % a));
                }
            }
            Op::IntQuot => {
                int2!(a, b);
                if a != 0 {
                    self.int.push(clamp(b / a));
                }
            }
            Op::IntInc => {
                need!(1, 0, 0);
                let a = self.int.pop().unwrap() as i128;
                self.int.push(clamp(a + 1));
            }
            Op::IntDec => {
                need!(1, 0, 0);
                let a = self.int.pop().unwrap() as i128;
                self.int.push(clamp(a - 1));
            }
            Op::IntLt => {
                int2!(a, b);
                self.boolean.push(b < a);
            }
            Op::IntGt => {
                int2!(a, b);
                self.boolean.push(b > a);
            }
            Op::IntEq => {
                int2!(a, b);
                self.boolean.push(a == b);
            }
            Op::IntGte => {
                int2!(a, b);
                self.boolean.push(b >= a);
            }
            Op::IntLte => {
                int2!(a, b);
                self.boolean.push(b <= a);
            }
            Op::IntMin => {
                int2!(a, b);
                self.int.push(clamp(a.min(b)));
            }
            Op::IntMax => {
                int2!(a, b);
                self.int.push(clamp(a.max(b)));
            }
            Op::IntFromBool => {
                need!(0, 1, 0);
                let b = self.boolean.pop().unwrap();
                self.int.push(b as i64);
            }
            Op::IntFromFloat => {
                float1!(x);
                if let Some(v) = clj_long(x) {
                    self.int.push(clamp(v as i128));
                }
            }
            Op::IntFromChar => {
                char1!(c);
                self.int.push(c as i64);
            }
            Op::BoolAnd => {
                bool2!(a, b);
                self.boolean.push(a && b);
            }
            Op::BoolOr => {
                bool2!(a, b);
                self.boolean.push(a || b);
            }
            Op::BoolNot => {
                need!(0, 1, 0);
                let a = self.boolean.pop().unwrap();
                self.boolean.push(!a);
            }
            Op::BoolEq | Op::BoolXnor => {
                bool2!(a, b);
                self.boolean.push(a == b);
            }
            Op::BoolXor => {
                bool2!(a, b);
                self.boolean.push(a != b);
            }
            Op::BoolInvFirstAnd => {
                bool2!(a, b);
                self.boolean.push(!a && b);
            }
            Op::BoolInvSecondAnd => {
                bool2!(a, b);
                self.boolean.push(a && !b);
            }
            Op::BoolNand => {
                bool2!(a, b);
                self.boolean.push(!(a && b));
            }
            Op::BoolNor => {
                bool2!(a, b);
                self.boolean.push(!(a || b));
            }
            Op::BoolFromInt => {
                need!(1, 0, 0);
                let a = self.int.pop().unwrap();
                self.boolean.push(a != 0);
            }
            Op::BoolFromFloat => {
                float1!(x);
                self.boolean.push(x != 0.0);
            }
            // in [boolean exec exec]: the top EXEC item is `alternative`, the one below `consequent`.
            Op::ExecIf => {
                need!(0, 1, 2);
                let c = self.boolean.pop().unwrap();
                let alt = self.exec.pop().unwrap();
                let cons = self.exec.pop().unwrap();
                self.exec.push(if c { cons } else { alt });
            }
            Op::ExecWhen => {
                need!(0, 1, 1);
                let c = self.boolean.pop().unwrap();
                let x = self.exec.pop().unwrap();
                if c {
                    self.exec.push(x);
                }
            }
            Op::ExecWhile => {
                need!(0, 1, 1);
                let c = self.boolean.pop().unwrap();
                let x = self.exec.pop().unwrap();
                if c {
                    self.exec.push(Item::Op(Op::ExecWhile));
                    self.exec.push(x);
                }
            }
            Op::ExecDoWhile => {
                need!(0, 1, 1);
                self.boolean.pop();
                let x = self.exec.pop().unwrap();
                self.exec.push(Item::Op(Op::ExecWhile));
                self.exec.push(x);
            }
            Op::ExecK => {
                need!(0, 0, 2);
                let a = self.exec.pop().unwrap();
                self.exec.pop();
                self.exec.push(a);
            }
            Op::ExecS => {
                need!(0, 0, 3);
                let a = self.exec.pop().unwrap();
                let b = self.exec.pop().unwrap();
                let c = self.exec.pop().unwrap();
                self.exec.push(Item::List(Rc::new(vec![b, c.clone()])));
                self.exec.push(c);
                self.exec.push(a);
            }
            Op::ExecY => {
                need!(0, 0, 1);
                let x = self.exec.pop().unwrap();
                self.exec.push(Item::List(Rc::new(vec![Item::Op(Op::ExecY), x.clone()])));
                self.exec.push(x);
            }
            // in [exec integer integer]: code, then destination (top integer), then current.
            Op::ExecDoRange => {
                need!(2, 0, 1);
                let code = self.exec.pop().unwrap();
                let dest = self.int.pop().unwrap();
                let cur = self.int.pop().unwrap();
                let inc = if cur < dest { 1 } else if cur > dest { -1 } else { 0 };
                if inc != 0 {
                    self.exec.push(Item::List(Rc::new(vec![
                        Item::Int(cur + inc),
                        Item::Int(dest),
                        Item::Op(Op::ExecDoRange),
                        code.clone(),
                    ])));
                }
                self.int.push(clamp(cur as i128));
                self.exec.push(code);
            }
            // in [integer exec]
            Op::ExecDoCount => {
                need!(1, 0, 1);
                let n = self.int.pop().unwrap();
                let code = self.exec.pop().unwrap();
                if n > 0 {
                    self.exec.push(Item::List(Rc::new(vec![
                        Item::Int(0),
                        Item::Int(n - 1),
                        Item::Op(Op::ExecDoRange),
                        code,
                    ])));
                }
            }
            Op::ExecDoTimes => {
                need!(1, 0, 1);
                let n = self.int.pop().unwrap();
                let code = self.exec.pop().unwrap();
                if n > 0 {
                    // (conj (ensure-list code) :integer_pop): prepend to a list, wrap anything else
                    let mut body = vec![Item::Op(Op::Fam(Fam::Pop, Ty::Int))];
                    match code {
                        Item::List(l) => body.extend(l.iter().cloned()),
                        other => body.push(other),
                    }
                    self.exec.push(Item::List(Rc::new(vec![
                        Item::Int(0),
                        Item::Int(n - 1),
                        Item::Op(Op::ExecDoRange),
                        Item::List(Rc::new(body)),
                    ])));
                }
            }
            Op::ExecEq => {
                need!(0, 0, 2);
                let a = self.exec.pop().unwrap();
                let b = self.exec.pop().unwrap();
                self.boolean.push(a == b);
            }
            // FLOAT. Same argument convention as INTEGER: `a` top, `b` second.
            Op::FloatAdd => {
                float2!(a, b);
                pushf!(a + b);
            }
            Op::FloatSub => {
                float2!(a, b);
                pushf!(b - a);
            }
            Op::FloatMul => {
                float2!(a, b);
                pushf!(a * b);
            }
            Op::FloatDiv => {
                float2!(a, b);
                if a != 0.0 {
                    pushf!(b / a);
                }
            }
            Op::FloatMod => {
                float2!(a, b);
                if a != 0.0 {
                    if let Some(m) = clj_mod(b, a) {
                        pushf!(m);
                    }
                }
            }
            Op::FloatQuot => {
                float2!(a, b);
                if a != 0.0 {
                    if let Some(q) = clj_quot(b, a) {
                        pushf!(q);
                    }
                }
            }
            Op::FloatInc => {
                float1!(a);
                pushf!(a + 1.0);
            }
            Op::FloatDec => {
                float1!(a);
                pushf!(a - 1.0);
            }
            Op::FloatLt => {
                float2!(a, b);
                self.boolean.push(b < a);
            }
            Op::FloatGt => {
                float2!(a, b);
                self.boolean.push(b > a);
            }
            Op::FloatEq => {
                float2!(a, b);
                self.boolean.push(a == b);
            }
            Op::FloatGte => {
                float2!(a, b);
                self.boolean.push(b >= a);
            }
            Op::FloatLte => {
                float2!(a, b);
                self.boolean.push(b <= a);
            }
            // Numbers.min/max(double,double): NaN wins, otherwise `x<y ? x : y` / `x>y ? x : y`
            Op::FloatMin => {
                float2!(a, b);
                let r = if a.is_nan() { a } else if b.is_nan() { b } else if a < b { a } else { b };
                pushf!(r);
            }
            Op::FloatMax => {
                float2!(a, b);
                let r = if a.is_nan() { a } else if b.is_nan() { b } else if a > b { a } else { b };
                pushf!(r);
            }
            Op::FloatFromBool => {
                need!(0, 1, 0);
                let b = self.boolean.pop().unwrap();
                pushf!(if b { 1.0 } else { 0.0 });
            }
            Op::FloatFromInt => {
                need!(1, 0, 0);
                let v = self.int.pop().unwrap();
                pushf!(v as f64);
            }
            Op::FloatFromChar => {
                char1!(c);
                pushf!(c as u32 as f64);
            }
            Op::FloatSin => {
                float1!(x);
                pushf!(x.sin());
            }
            Op::FloatCos => {
                float1!(x);
                pushf!(x.cos());
            }
            Op::FloatTan => {
                float1!(x);
                pushf!(x.tan());
            }
            // CHAR
            Op::CharIsLetter => {
                char1!(c);
                self.boolean.push(if c.is_ascii() { c.is_ascii_alphabetic() } else { c.is_alphabetic() });
            }
            Op::CharIsDigit => {
                char1!(c);
                self.boolean.push(if c.is_ascii() { c.is_ascii_digit() } else { c.is_numeric() });
            }
            Op::CharIsWhitespace => {
                char1!(c);
                self.boolean.push(is_java_whitespace(c));
            }
            Op::CharIsUpper => {
                char1!(c);
                self.boolean.push(c.is_uppercase());
            }
            Op::CharIsLower => {
                char1!(c);
                self.boolean.push(c.is_lowercase());
            }
            Op::CharFromInt => {
                need!(1, 0, 0);
                let i = self.int.pop().unwrap();
                self.chr.push(Some(char::from_u32(i.rem_euclid(128) as u32).unwrap()));
            }
            Op::CharFromFloat => {
                float1!(f);
                if let Some(l) = clj_long(f) {
                    self.chr.push(Some(char::from_u32(l.rem_euclid(128) as u32).unwrap()));
                }
            }
            Op::CharToInt => {
                char1!(c);
                self.int.push(c as i64);
            }
            Op::CharToUpper => {
                char1!(c);
                self.chr.push(Some(java_upper(c)));
            }
            Op::CharToLower => {
                char1!(c);
                self.chr.push(Some(java_lower(c)));
            }
            // `(= a b)`: nil equals nil, nothing else
            Op::CharEq => {
                if self.chr.len() < 2 {
                    return;
                }
                let a = self.chr.pop().unwrap();
                let b = self.chr.pop().unwrap();
                self.boolean.push(a == b);
            }
            // [_ a b] (< (int a) (int b)) with a the top: unlike the numeric comparators, no swap
            Op::CharLt => {
                char2!(a, b);
                self.boolean.push((a as u32) < (b as u32));
            }
            Op::CharGt => {
                char2!(a, b);
                self.boolean.push((a as u32) > (b as u32));
            }
            Op::PrintNewline => self.print.push('\n'),
            // in [boolean]
            Op::SetC(k) => {
                need!(0, 1, 0);
                let v = self.boolean.pop().unwrap();
                self.regs[k as usize] = Some(v);
            }
            Op::C(k) => {
                if let Some(v) = self.regs[k as usize] {
                    self.boolean.push(v);
                }
            }
            Op::Str(o) => self.run_str_op(o),
            Op::Vec(o) => self.run_vec_op(o),
            Op::In(k) => {
                if let Some(v) = self.inputs.get(k as usize - 1) {
                    self.exec.push(v.clone());
                }
            }
        }
    }
}
