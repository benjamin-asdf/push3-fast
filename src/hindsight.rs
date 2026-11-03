//! Hindsight data: random programs, relabelled with the outputs they produce.
//!
//! A program drawn at random solves *some* problem: the one whose examples are its own
//! (input, output) pairs. This module draws programs from an instruction alphabet, runs each on
//! inputs sampled from a pool, and writes the program together with the examples it produces, as
//! training data for a model that maps examples to programs.
//!
//! The spec file is one EDN map; it carries the same keys as a problem file except `:cases`:
//!
//! ```text
//! {:alphabet [:integer_+ :exec_if "small" 5 ...]   ;; instructions and literal ERC values
//!  :int-erc [-100 100] :float-erc [-10.0 10.0]     ;; optional: one extra atom slot each, drawn per use
//!  :output :push/string :num-outputs 6 :input-names [...] :max 2000
//!  :inputs [[5] [17] ...]}                         ;; pool of input tuples, from the problem's generator
//! ```
//!
//! Output, one line per kept program, in program-index order (the same seed gives the same file
//! whatever the thread count): `{:program (...) :examples [{:inputs [...] :output ...} ...]}`.

use crate::edn::{self, Edn};
use crate::eval::{parallel, Problem};
use crate::Item;
use std::cell::RefCell;
use std::rc::Rc;

pub struct Params {
    pub n: usize,
    pub seed: u64,
    /// examples per program: pool entries are tried (up to 3x this many) until this many gave an output
    pub examples: usize,
    /// keep a program only if at least this many of its tried inputs gave an output
    pub min_examples: usize,
    /// program length in atoms, drawn uniformly from lo..=hi
    pub length: (usize, usize),
    /// keep a program only if it gives at least this many distinct outputs on its examples
    pub min_distinct: usize,
    pub stack_limit: usize,
    pub threads: usize,
}

pub struct Spec {
    problem: Problem,
    alphabet: Vec<Edn>,
    int_erc: Option<(i64, i64)>,
    float_erc: Option<(f64, f64)>,
}

fn range(e: Option<&Edn>) -> Option<(f64, f64)> {
    let num = |e: &Edn| match e {
        Edn::Int(i) => Some(*i as f64),
        Edn::Float(f) => Some(*f),
        _ => None,
    };
    match e?.items()? {
        [a, b] => Some((num(a)?, num(b)?)),
        _ => None,
    }
}

impl Spec {
    pub fn parse(text: &str) -> Result<Spec, String> {
        let e = edn::parse(text)?;
        let alphabet = e.get("alphabet").and_then(|a| a.items()).ok_or("missing :alphabet")?.to_vec();
        for a in &alphabet {
            Item::from_edn(a)?;
        }
        let int_erc = range(e.get("int-erc")).map(|(a, b)| (a as i64, b as i64));
        let float_erc = range(e.get("float-erc"));
        if alphabet.is_empty() && int_erc.is_none() && float_erc.is_none() {
            return Err("empty alphabet".into());
        }
        Ok(Spec { problem: Problem::from_edn(&e)?, alphabet, int_erc, float_erc })
    }

    fn slots(&self) -> usize {
        self.alphabet.len() + self.int_erc.is_some() as usize + self.float_erc.is_some() as usize
    }
}

/// SplitMix64: small, fast, and good enough to draw programs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// uniform in 0..n
    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 11) as u128 * n as u128 >> 53) as usize
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

thread_local! {
    static ATOMS: RefCell<Vec<Item>> = const { RefCell::new(Vec::new()) };
}

/// A random program of `size` atoms: a list, in which each next item is a sublist with probability
/// 1/6 (its size drawn from what is left), and an atom otherwise.
fn random_list(rng: &mut Rng, spec: &Spec, atoms: &[Item], size: usize) -> Item {
    let mut items = Vec::with_capacity(size);
    let mut left = size;
    while left > 0 {
        if left > 1 && rng.below(6) == 0 {
            let sub = 1 + rng.below(left - 1);
            items.push(random_list(rng, spec, atoms, sub));
            left -= sub;
        } else {
            let k = rng.below(spec.slots());
            let atom = if k < atoms.len() {
                atoms[k].clone()
            } else if spec.int_erc.is_some() && k == atoms.len() {
                let (lo, hi) = spec.int_erc.unwrap();
                Item::Int(lo + rng.below((hi - lo + 1) as usize) as i64)
            } else {
                let (lo, hi) = spec.float_erc.unwrap();
                // two decimals: the ERCs of the benchmarks are short numbers
                Item::Float(((lo + (hi - lo) * rng.unit()) * 100.0).round() / 100.0)
            };
            items.push(atom);
            left -= 1;
        }
    }
    Item::List(Rc::new(items))
}

#[derive(Default, Debug)]
pub struct Stats {
    pub kept: usize,
    pub too_few_outputs: usize,
    pub too_uniform: usize,
}

/// Draws and labels `p.n` programs; returns the output lines (kept programs only) and counts.
pub fn generate(spec: &Spec, p: &Params) -> (Vec<String>, Stats) {
    let pool = spec.problem.n_cases();
    let k = p.examples.min(pool);
    let lines = parallel(&spec.problem, p.n, p.threads, |i, prepared| {
        ATOMS.with(|a| {
            let mut a = a.borrow_mut();
            if a.is_empty() {
                *a = spec.alphabet.iter().map(|e| Item::from_edn(e).unwrap()).collect();
            }
        });
        let mut rng = Rng(p.seed ^ (i as u64).wrapping_mul(0xD1B5_4A32_D192_ED03));
        rng.next();
        let size = p.length.0 + rng.below(p.length.1 - p.length.0 + 1);
        let program = ATOMS.with(|a| random_list(&mut rng, spec, &a.borrow(), size));
        // distinct pool entries, tried until k of them gave an output; most random programs
        // write nothing on most inputs, so a program that does is cheap to spot on its first few
        let tries = (3 * k).min(pool);
        let mut tried: Vec<usize> = Vec::with_capacity(tries);
        let mut examples = Vec::with_capacity(k);
        let mut outputs: Vec<String> = Vec::new();
        while tried.len() < tries && examples.len() < k {
            let c = rng.below(pool);
            if tried.contains(&c) {
                continue;
            }
            tried.push(c);
            if let Some(out) = prepared.output_of(&program, c, p.stack_limit) {
                examples.push(format!("{{:inputs {} :output {}}}", prepared.case_text(c), out));
                if !outputs.contains(&out) {
                    outputs.push(out);
                }
            } else if examples.is_empty() && tried.len() >= 3 {
                break; // nothing on the first three inputs: not a program that outputs
            }
        }
        if examples.len() < p.min_examples.min(k) {
            return Err(0);
        }
        if outputs.len() < p.min_distinct {
            return Err(1);
        }
        Ok(format!("{{:program {} :examples [{}]}}", program.to_edn_string(), examples.join(" ")))
    });
    let mut stats = Stats::default();
    let mut out = Vec::new();
    for l in lines {
        match l {
            Ok(s) => {
                stats.kept += 1;
                out.push(s);
            }
            Err(0) => stats.too_few_outputs += 1,
            Err(_) => stats.too_uniform += 1,
        }
    }
    (out, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = r#"{:alphabet [:integer_+ :integer_* :integer_dup :in1 1 2] :int-erc [-5 5]
        :output :push/integer :max 100 :inputs [[1] [2] [3] [4] [5] [6] [7] [8] [9] [10]]}"#;

    fn params(threads: usize) -> Params {
        Params { n: 300, seed: 7, examples: 5, min_examples: 5, length: (3, 12), min_distinct: 2, stack_limit: 1000, threads }
    }

    #[test]
    fn same_seed_gives_the_same_file_on_any_thread_count() {
        let spec = Spec::parse(SPEC).unwrap();
        let (a, _) = generate(&spec, &params(1));
        let (b, _) = generate(&spec, &params(8));
        assert!(!a.is_empty());
        assert_eq!(a, b);
    }

    #[test]
    fn every_example_is_what_the_program_outputs() {
        let spec = Spec::parse(SPEC).unwrap();
        let (lines, stats) = generate(&spec, &params(2));
        assert_eq!(stats.kept, lines.len());
        let mut prepared = spec.problem.prepare();
        for l in lines.iter().take(50) {
            let e = edn::parse(l).unwrap();
            let program = Item::from_edn(e.get("program").unwrap()).unwrap();
            for ex in e.get("examples").unwrap().items().unwrap() {
                let input = ex.get("inputs").unwrap().items().unwrap();
                let Edn::Int(x) = &input[0] else { panic!() };
                let case = (*x - 1) as usize; // the pool is [[1] .. [10]]
                let want = prepared.output_of(&program, case, 1000).unwrap();
                assert_eq!(edn::edn_text(ex.get("output").unwrap()), want);
            }
        }
    }
}
