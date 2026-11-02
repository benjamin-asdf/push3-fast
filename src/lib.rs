#![allow(unused_comparisons)]
//! Fast Push3 core. Stage 1 (spike): EXEC, INTEGER and BOOLEAN stacks, literals, inputs.
//!
//! The semantics are those of the Clojure interpreter in clj-push3, quirks included
//! (argument order, clamping to +-1e10, `rot`, `yank` index handling). That interpreter is the
//! test oracle: `push3-fast diff <cases.edn>` compares complete final states against it.

pub mod edn;

use std::rc::Rc;

pub const LIM: i64 = 10_000_000_000;

/// Integers are clamped when pushed (`sanitize-value :push/integer`).
#[inline]
pub fn clamp(v: i128) -> i64 {
    v.clamp(-(LIM as i128), LIM as i128) as i64
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    Int,
    Bool,
    Exec,
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
    In(u8),
}

#[derive(Clone, PartialEq, Debug)]
pub enum Item {
    Int(i64),
    Bool(bool),
    Op(Op),
    List(Rc<Vec<Item>>),
}

const FAMS: [(&str, Fam); 13] = [
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
];
const TYS: [(&str, Ty); 3] = [("integer", Ty::Int), ("boolean", Ty::Bool), ("exec", Ty::Exec)];

const PLAIN: [(&str, Op); 38] = [
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
];

impl Op {
    pub fn from_name(name: &str) -> Option<Op> {
        if let Some((_, op)) = PLAIN.iter().find(|(n, _)| *n == name) {
            return Some(*op);
        }
        if let Some(rest) = name.strip_prefix("in") {
            if let Ok(k) = rest.parse::<u8>() {
                if (1..=8).contains(&k) {
                    return Some(Op::In(k));
                }
            }
        }
        for (tn, ty) in TYS {
            if let Some(rest) = name.strip_prefix(tn).and_then(|r| r.strip_prefix('_')) {
                if let Some((_, f)) = FAMS.iter().find(|(n, _)| *n == rest) {
                    return Some(Op::Fam(*f, ty));
                }
            }
        }
        None
    }

    pub fn name(&self) -> String {
        if let Some((n, _)) = PLAIN.iter().find(|(_, o)| o == self) {
            return n.to_string();
        }
        match self {
            Op::In(k) => format!("in{k}"),
            Op::Fam(f, t) => {
                let tn = TYS.iter().find(|(_, x)| x == t).unwrap().0;
                let fname = FAMS.iter().find(|(_, x)| x == f).unwrap().0;
                format!("{tn}_{fname}")
            }
            _ => unreachable!(),
        }
    }
}

pub struct Machine {
    pub exec: Vec<Item>,
    pub int: Vec<i64>,
    pub boolean: Vec<bool>,
    pub inputs: Vec<Item>,
    pub steps: u64,
}

const MAX_STACK_ITEMS: i64 = 1000;

impl Machine {
    pub fn new(program: Item, inputs: Vec<Item>) -> Machine {
        Machine { exec: vec![program], int: Vec::new(), boolean: Vec::new(), inputs, steps: 0 }
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
        }
    }

    fn get(&self, t: Ty, i: usize) -> Item {
        match t {
            Ty::Int => Item::Int(self.int[i]),
            Ty::Bool => Item::Bool(self.boolean[i]),
            Ty::Exec => self.exec[i].clone(),
        }
    }

    fn push_item(&mut self, t: Ty, it: Item) {
        match (t, it) {
            (Ty::Int, Item::Int(v)) => self.int.push(clamp(v as i128)),
            (Ty::Bool, Item::Bool(b)) => self.boolean.push(b),
            (Ty::Exec, it) => self.exec.push(it),
            _ => unreachable!(),
        }
    }

    fn pop_item(&mut self, t: Ty) -> Item {
        match t {
            Ty::Int => Item::Int(self.int.pop().unwrap()),
            Ty::Bool => Item::Bool(self.boolean.pop().unwrap()),
            Ty::Exec => self.exec.pop().unwrap(),
        }
    }

    fn insert_item(&mut self, t: Ty, idx: usize, it: Item) {
        match (t, it) {
            (Ty::Int, Item::Int(v)) => self.int.insert(idx, clamp(v as i128)),
            (Ty::Bool, Item::Bool(b)) => self.boolean.insert(idx, b),
            (Ty::Exec, it) => self.exec.insert(idx, it),
            _ => unreachable!(),
        }
    }

    fn remove_item(&mut self, t: Ty, idx: usize) -> Item {
        match t {
            Ty::Int => Item::Int(self.int.remove(idx)),
            Ty::Bool => Item::Bool(self.boolean.remove(idx)),
            Ty::Exec => self.exec.remove(idx),
        }
    }

    fn flush(&mut self, t: Ty) {
        match t {
            Ty::Int => self.int.clear(),
            Ty::Bool => self.boolean.clear(),
            Ty::Exec => self.exec.clear(),
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
            Fam::Shove => {
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
            Op::In(k) => {
                if let Some(v) = self.inputs.get(k as usize - 1) {
                    self.exec.push(v.clone());
                }
            }
        }
    }
}
