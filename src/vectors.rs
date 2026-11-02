//! `vector_integer_*` instructions (the part that is not the shared stack family). Owned by the
//! vectors work; see `~/repos/benjamin-schwerdtner-clj-push3/.../instructions/vector.clj`.

use crate::{Item, Machine, Ty};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecOp {
    Length,
}

pub(crate) const NAMES: &[(&str, VecOp)] = &[("vector_integer_length", VecOp::Length)];

pub(crate) fn from_name(name: &str) -> Option<VecOp> {
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, o)| *o)
}

pub(crate) fn name(op: VecOp) -> String {
    NAMES.iter().find(|(_, o)| *o == op).unwrap().0.to_string()
}

impl Machine {
    pub(crate) fn run_vec_op(&mut self, op: VecOp) {
        match op {
            // in [vector_integer], out integer: (count v)
            VecOp::Length => {
                if let Some(v) = self.ivec.pop() {
                    self.int.push(v.len() as i64);
                }
            }
        }
    }
}

#[allow(dead_code)]
fn _uses(_: Item, _: Ty) {}
