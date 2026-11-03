//! `vector_integer_*` instructions (the part that is not the shared stack family), as registered
//! in the oracle's `instructions/vector.clj`.
//!
//! Elements are `i64` and are NOT clamped (only the INTEGER stack is); every element that goes to
//! the INTEGER stack is clamped on the way. Arguments are popped in the order of the oracle's `:in`
//! (top item first); if any needed stack is short the instruction is a no-op that pops nothing.
//! `vector_integer_fromlist` needs the CODE stack, which is not modelled, and is left out.

use crate::{clamp, Item, Machine, Op};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecOp {
    Length,
    Butlast,
    Concat,
    Conj,
    Contains,
    IsEmpty,
    Eq,
    First,
    IndexOf,
    Last,
    Nth,
    OccurrencesOf,
    PushAll,
    Remove,
    Replace,
    ReplaceFirst,
    Rest,
    Reverse,
    Set,
    Subvec,
    Take,
    EmptyVector,
    Iterate,
}

pub(crate) const NAMES: &[(&str, VecOp)] = &[
    ("vector_integer_length", VecOp::Length),
    ("vector_integer_butlast", VecOp::Butlast),
    ("vector_integer_concat", VecOp::Concat),
    ("vector_integer_conj", VecOp::Conj),
    ("vector_integer_contains", VecOp::Contains),
    ("vector_integer_empty?", VecOp::IsEmpty),
    ("vector_integer_=", VecOp::Eq),
    ("vector_integer_first", VecOp::First),
    ("vector_integer_indexof", VecOp::IndexOf),
    ("vector_integer_last", VecOp::Last),
    ("vector_integer_nth", VecOp::Nth),
    ("vector_integer_occurrencesof", VecOp::OccurrencesOf),
    ("vector_integer_pushall", VecOp::PushAll),
    ("vector_integer_remove", VecOp::Remove),
    ("vector_integer_replace", VecOp::Replace),
    ("vector_integer_replacefirst", VecOp::ReplaceFirst),
    ("vector_integer_rest", VecOp::Rest),
    ("vector_integer_reverse", VecOp::Reverse),
    ("vector_integer_set", VecOp::Set),
    ("vector_integer_subvec", VecOp::Subvec),
    ("vector_integer_take", VecOp::Take),
    ("vector_integer_emptyvector", VecOp::EmptyVector),
    ("vector_integer_iterate", VecOp::Iterate),
];

pub(crate) fn from_name(name: &str) -> Option<VecOp> {
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, o)| *o)
}

pub(crate) fn name(op: VecOp) -> String {
    NAMES.iter().find(|(_, o)| *o == op).map(|(n, _)| n.to_string()).unwrap_or_default()
}

/// A concat result longer than this marks the run unsupported (the oracle would run out of heap
/// long before; this only protects the process from a runaway doubling loop).
const MAX_VEC_LEN: usize = 20_000_000;

fn owned(v: Rc<Vec<i64>>) -> Vec<i64> {
    Rc::try_unwrap(v).unwrap_or_else(|rc| (*rc).clone())
}

impl Machine {
    fn push_vec(&mut self, mut v: Vec<i64>) {
        if v.len() > self.max_vector {
            v.truncate(self.max_vector);
            self.limit_hit = true;
        }
        if self.charge(v.len() * 8) {
            self.ivec.push(Rc::new(v));
        }
    }

    #[inline(never)]
    pub(crate) fn run_vec_op(&mut self, op: VecOp) {
        // (number of INTEGER items, number of vectors) needed
        let (ni, nv) = match op {
            VecOp::Length | VecOp::Butlast | VecOp::IsEmpty | VecOp::First | VecOp::Last | VecOp::PushAll
            | VecOp::Rest | VecOp::Reverse => (0, 1),
            VecOp::Concat | VecOp::Eq => (0, 2),
            VecOp::Conj | VecOp::Contains | VecOp::IndexOf | VecOp::Nth | VecOp::OccurrencesOf | VecOp::Remove
            | VecOp::Take => (1, 1),
            VecOp::Replace | VecOp::ReplaceFirst | VecOp::Set | VecOp::Subvec => (2, 1),
            VecOp::EmptyVector => (0, 0),
            VecOp::Iterate => (0, 1),
        };
        if self.int.len() < ni || self.ivec.len() < nv || (op == VecOp::Iterate && self.exec.is_empty()) {
            return;
        }
        match op {
            VecOp::EmptyVector => self.push_vec(Vec::new()),
            VecOp::Length => {
                if let Some(v) = self.ivec.pop() {
                    self.int.push(clamp(v.len() as i128));
                }
            }
            VecOp::IsEmpty => {
                if let Some(v) = self.ivec.pop() {
                    self.boolean.push(v.is_empty());
                }
            }
            VecOp::First | VecOp::Last => {
                if let Some(v) = self.ivec.pop() {
                    let x = if op == VecOp::First { v.first() } else { v.last() };
                    self.int.push(clamp(x.copied().unwrap_or(0) as i128));
                }
            }
            VecOp::Butlast | VecOp::Rest | VecOp::Reverse => {
                if let Some(v) = self.ivec.pop() {
                    let mut v = owned(v);
                    match op {
                        VecOp::Butlast => {
                            v.pop();
                        }
                        VecOp::Rest => {
                            if !v.is_empty() {
                                v.remove(0);
                            }
                        }
                        _ => v.reverse(),
                    }
                    self.push_vec(v);
                }
            }
            VecOp::PushAll => {
                if let Some(v) = self.ivec.pop() {
                    // (reduce push ... (reverse v)): the first element ends on top
                    for x in v.iter().rev() {
                        self.int.push(clamp(*x as i128));
                    }
                }
            }
            VecOp::Concat => {
                if let (Some(top), Some(second)) = (self.ivec.pop(), self.ivec.pop()) {
                    if top.len() + second.len() > MAX_VEC_LEN {
                        self.unsupported = true;
                        return;
                    }
                    let mut r = owned(second);
                    r.extend(top.iter().copied());
                    self.push_vec(r);
                }
            }
            VecOp::Eq => {
                if let (Some(a), Some(b)) = (self.ivec.pop(), self.ivec.pop()) {
                    self.boolean.push(a == b);
                }
            }
            VecOp::Conj => {
                if let (Some(e), Some(v)) = (self.int.pop(), self.ivec.pop()) {
                    let mut v = owned(v);
                    v.push(e);
                    self.push_vec(v);
                }
            }
            VecOp::Contains => {
                if let (Some(e), Some(v)) = (self.int.pop(), self.ivec.pop()) {
                    self.boolean.push(v.contains(&e));
                }
            }
            VecOp::IndexOf => {
                if let (Some(e), Some(v)) = (self.int.pop(), self.ivec.pop()) {
                    let i = v.iter().position(|x| *x == e).map_or(-1, |i| i as i64);
                    self.int.push(i);
                }
            }
            VecOp::OccurrencesOf => {
                if let (Some(e), Some(v)) = (self.int.pop(), self.ivec.pop()) {
                    self.int.push(v.iter().filter(|x| **x == e).count() as i64);
                }
            }
            VecOp::Nth => {
                if let (Some(n), Some(v)) = (self.int.pop(), self.ivec.pop()) {
                    let x = if v.is_empty() {
                        0
                    } else {
                        v.get((n as i128).rem_euclid(v.len() as i128) as usize).copied().unwrap_or(0)
                    };
                    self.int.push(clamp(x as i128));
                }
            }
            VecOp::Remove => {
                if let (Some(e), Some(v)) = (self.int.pop(), self.ivec.pop()) {
                    let mut v = owned(v);
                    v.retain(|x| *x != e);
                    self.push_vec(v);
                }
            }
            // :in [new old vector]: new is popped first
            VecOp::Replace | VecOp::ReplaceFirst => {
                if let (Some(new), Some(old), Some(v)) = (self.int.pop(), self.int.pop(), self.ivec.pop()) {
                    let mut v = owned(v);
                    if op == VecOp::Replace {
                        for x in v.iter_mut() {
                            if *x == old {
                                *x = new;
                            }
                        }
                    } else if let Some(x) = v.iter_mut().find(|x| **x == old) {
                        *x = new;
                    }
                    self.push_vec(v);
                }
            }
            // :in [elem n vector]; an empty vector is pushed back unchanged
            VecOp::Set => {
                if let (Some(e), Some(n), Some(v)) = (self.int.pop(), self.int.pop(), self.ivec.pop()) {
                    if v.is_empty() {
                        self.ivec.push(v);
                    } else {
                        let i = (n as i128).rem_euclid(v.len() as i128) as usize;
                        let mut v = owned(v);
                        if let Some(x) = v.get_mut(i) {
                            *x = e;
                        }
                        self.push_vec(v);
                    }
                }
            }
            // :in [start-raw stop-raw vector]
            VecOp::Subvec => {
                if let (Some(a), Some(b), Some(v)) = (self.int.pop(), self.int.pop(), self.ivec.pop()) {
                    let len = v.len() as i64;
                    let start = len.min(0.max(a));
                    let stop = len.min(start.max(b));
                    let r: Vec<i64> = v.iter().skip(start as usize).take((stop - start) as usize).copied().collect();
                    self.push_vec(r);
                }
            }
            VecOp::Take => {
                if let (Some(n), Some(v)) = (self.int.pop(), self.ivec.pop()) {
                    let k = if n <= 0 { 0 } else { (n as u64).min(v.len() as u64) as usize };
                    let r: Vec<i64> = v.iter().take(k).copied().collect();
                    self.push_vec(r);
                }
            }
            // :in [vector exec]
            VecOp::Iterate => {
                if let (Some(v), Some(e)) = (self.ivec.pop(), self.exec.pop()) {
                    match v.len() {
                        0 => {}
                        1 => self.int.push(clamp(v.first().copied().unwrap_or(0) as i128)),
                        _ => {
                            let rest: Vec<i64> = v.iter().skip(1).copied().collect();
                            self.exec.push(Item::Op(Op::Vec(VecOp::Iterate)));
                            self.exec.push(Item::IntVec(Rc::new(rest)));
                            self.exec.push(e);
                            self.int.push(clamp(v.first().copied().unwrap_or(0) as i128));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{edn, Item, Machine};

    fn run(src: &str) -> Machine {
        let e = edn::parse(src).expect("edn");
        let p = Item::from_edn(&e).expect("item");
        let mut m = Machine::new(p, Vec::new());
        m.run(1000);
        m
    }

    fn vecs(m: &Machine) -> Vec<Vec<i64>> {
        m.ivec.iter().map(|v| (**v).clone()).collect()
    }

    #[test]
    fn concat_puts_second_before_top() {
        let m = run("([1 2] [3] :vector_integer_concat)");
        assert_eq!(vecs(&m), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn shove_on_vector_stack_pops_and_does_nothing() {
        let m = run("([1] [2] 0 :vector_integer_shove)");
        assert_eq!(vecs(&m), vec![vec![1]]);
        assert!(m.int.is_empty());
    }

    #[test]
    fn set_on_empty_vector_keeps_it() {
        let m = run("(:vector_integer_emptyvector 7 3 :vector_integer_set)");
        assert_eq!(vecs(&m), vec![Vec::<i64>::new()]);
        assert!(m.int.is_empty());
    }

    #[test]
    fn subvec_and_take_clamp_their_arguments() {
        let m = run("([1 2 3 4] 9 -5 :vector_integer_subvec)");
        assert_eq!(vecs(&m), vec![vec![1, 2, 3, 4]]);
        let m = run("([1 2 3 4] -3 :vector_integer_take)");
        assert_eq!(vecs(&m), vec![Vec::<i64>::new()]);
    }

    #[test]
    fn pushall_leaves_first_element_on_top() {
        let m = run("([1 2 3] :vector_integer_pushall)");
        assert_eq!(m.int, vec![3, 2, 1]);
    }

    #[test]
    fn iterate_consumes_the_next_exec_item_each_round() {
        // oracle quirk: the body is not re-pushed for the recursion, the next iterate takes
        // whatever follows it on the EXEC stack
        let m = run("([1 2 3] :vector_integer_iterate :integer_inc :integer_inc)");
        assert_eq!(m.int, vec![2, 3]);
        assert_eq!(vecs(&m), vec![vec![3]]);
    }
}
