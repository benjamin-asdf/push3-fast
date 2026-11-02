//! `string_*` instructions (the part that is not the shared stack family) and the other
//! instructions that take or return strings. Owned by the strings work; see
//! `~/repos/benjamin-schwerdtner-clj-push3/.../instructions/string.clj` for the semantics.

use crate::{Item, Machine, Ty};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrOp {
    Length,
    EmptyString,
}

pub(crate) const NAMES: &[(&str, StrOp)] = &[("string_length", StrOp::Length), ("string_empty_string", StrOp::EmptyString)];

pub(crate) fn from_name(name: &str) -> Option<StrOp> {
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, o)| *o)
}

pub(crate) fn name(op: StrOp) -> String {
    NAMES.iter().find(|(_, o)| *o == op).unwrap().0.to_string()
}

impl Machine {
    pub(crate) fn run_str_op(&mut self, op: StrOp) {
        match op {
            // in [string], out integer: (count s)
            StrOp::Length => {
                if let Some(s) = self.string.pop() {
                    self.int.push(s.chars().count() as i64);
                }
            }
            // in [], out string
            StrOp::EmptyString => self.string.push(Rc::from("")),
        }
    }
}

#[allow(dead_code)]
fn _uses(_: Item, _: Ty) {}
