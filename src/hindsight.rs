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
//!  :erc-pools [[\a \b ...] ["word" ...]]           ;; optional: one slot per ERC generator, values drawn from it
//!  :max-initial-plushy-size 250                    ;; optional: default for --max-size
//!  :output :push/string :num-outputs 6 :input-names [...] :max 2000
//!  :inputs [[5] [17] ...]}                         ;; pool of input tuples, from the problem's generator
//! ```
//!
//! Output, one line per kept program, in program-index order (the same seed gives the same file
//! whatever the thread count): `{:program (...) :examples [{:inputs [...] :output ...} ...]}`.

use crate::edn::{self, Edn};
use crate::eval::{parallel, Limits, Problem};
use crate::Item;
use std::cell::RefCell;
use std::rc::Rc;

/// How a program is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Genome {
    /// Propeller's: a flat genome ("plushy") of `rand-int max_size` genes, each drawn uniformly from
    /// the alphabet (which holds `:close` where the problem has it), translated to a program with
    /// the `opens` table of Propeller (`exec_if` opens two blocks, `exec_s` three, ...); blocks still
    /// open at the end are closed there.
    Plushy { max_size: usize },
    /// A random tree: each next item is a sublist with probability 1/6; length in atoms from `length`.
    Tree,
}

pub struct Params {
    pub genome: Genome,
    pub n: usize,
    pub seed: u64,
    /// examples per program: pool entries are tried (up to 3x this many) until this many gave an output
    pub examples: usize,
    /// keep a program only if at least this many of its tried inputs gave an output
    pub min_examples: usize,
    /// `Genome::Tree` only: program length in atoms, drawn uniformly from lo..=hi
    pub length: (usize, usize),
    /// keep a program only if it gives at least this many distinct outputs on its examples
    pub min_distinct: usize,
    pub stack_limit: usize,
    pub threads: usize,
}

/// Why a drawn program is not in the output.
enum Dropped {
    TooFewOutputs,
    TooUniform,
    LimitHit,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            genome: Genome::Plushy { max_size: 100 },
            n: 1000,
            seed: 1,
            examples: 10,
            min_examples: 5,
            length: (5, 60),
            min_distinct: 2,
            stack_limit: 100_000,
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
        }
    }
}

pub struct Spec {
    problem: Problem,
    alphabet: Vec<Edn>,
    /// Propeller's `:max-initial-plushy-size` for the problem, when the spec gives it.
    pub max_size: Option<usize>,
    int_erc: Option<(i64, i64)>,
    float_erc: Option<(f64, f64)>,
    /// One gene slot per ERC generator, drawn from a pool of its values (Propeller calls the function).
    erc_pools: Vec<Vec<Edn>>,
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
        let erc_pools = match e.get("erc-pools").and_then(|p| p.items()) {
            Some(pools) => pools.iter().map(|p| p.items().ok_or(":erc-pools holds vectors").map(|x| x.to_vec())).collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        for v in erc_pools.iter().flatten() {
            Item::from_edn(v)?;
        }
        if erc_pools.iter().any(|p| p.is_empty()) {
            return Err("an empty ERC pool".into());
        }
        let max_size = match e.get("max-initial-plushy-size") {
            Some(Edn::Int(n)) => Some(*n as usize),
            _ => None,
        };
        Ok(Spec { problem: Problem::from_edn(&e)?, alphabet, max_size, int_erc, float_erc, erc_pools })
    }

    fn slots(&self) -> usize {
        self.alphabet.len() + self.int_erc.is_some() as usize + self.float_erc.is_some() as usize + self.erc_pools.len()
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

/// Blocks opened by an instruction (Propeller's `parentheses/opens`, under this interpreter's names).
fn opens(name: &str) -> usize {
    match name {
        "exec_dup" | "exec_dup_times" | "exec_pop" | "exec_shove" | "exec_print" | "exec_when" | "exec_while"
        | "exec_do_while" | "exec_do*range" | "exec_do*count" | "exec_do*times" | "exec_y" | "string_iterate"
        | "vector_integer_iterate" => 1,
        "exec_swap" | "exec_if" | "exec_k" => 2,
        "exec_rot" | "exec_s" => 3,
        _ => 0,
    }
}

fn is_close(item: &Item) -> bool {
    matches!(item, Item::Name(n) if n.as_str() == "close")
}

/// Propeller's `plushy->push`: an instruction that opens `n` blocks is followed by a block; a
/// `:close` ends the innermost open block (and starts the next one of the same instruction, if it
/// opens more than one); a `:close` with nothing open is ignored; what is still open at the end is
/// closed.
pub fn plushy_to_program(genes: &[Item]) -> Item {
    // (items before the open block, blocks still to open for the same instruction)
    let mut stack: Vec<(Vec<Item>, usize)> = Vec::new();
    let mut current: Vec<Item> = Vec::new();
    fn close(stack: &mut Vec<(Vec<Item>, usize)>, current: &mut Vec<Item>) {
        let (mut pre, n) = stack.pop().unwrap();
        pre.push(Item::List(Rc::new(std::mem::take(current))));
        if n > 1 {
            stack.push((pre, n - 1));
        } else {
            *current = pre;
        }
    }
    for g in genes {
        if is_close(g) {
            if !stack.is_empty() {
                close(&mut stack, &mut current);
            }
            continue;
        }
        current.push(g.clone());
        if let Item::Op(op) = g {
            let n = opens(&op.name());
            if n > 0 {
                stack.push((std::mem::take(&mut current), n));
            }
        }
    }
    while !stack.is_empty() {
        close(&mut stack, &mut current);
    }
    Item::List(Rc::new(current))
}

thread_local! {
    static ATOMS: RefCell<Vec<Item>> = const { RefCell::new(Vec::new()) };
}

/// One uniformly drawn atom; the ERC slots draw a fresh value each time (Propeller's
/// `random-instruction` calls the ERC function).
fn random_atom(rng: &mut Rng, spec: &Spec, atoms: &[Item]) -> Item {
    let k = rng.below(spec.slots());
    if k < atoms.len() {
        atoms[k].clone()
    } else if k < atoms.len() + spec.erc_pools.len() {
        let pool = &spec.erc_pools[k - atoms.len()];
        // parsed per use: an ERC gene is rare, and Item is not shareable between threads
        Item::from_edn(&pool[rng.below(pool.len())]).unwrap()
    } else if spec.int_erc.is_some() && k == atoms.len() + spec.erc_pools.len() {
        let (lo, hi) = spec.int_erc.unwrap();
        Item::Int(lo + rng.below((hi - lo + 1) as usize) as i64)
    } else {
        let (lo, hi) = spec.float_erc.unwrap();
        Item::Float(lo + (hi - lo) * rng.unit())
    }
}

/// `Genome::Tree`: a random program of `size` atoms: a list, in which each next item is a sublist
/// with probability 1/6 (its size drawn from what is left), and an atom otherwise.
fn random_list(rng: &mut Rng, spec: &Spec, atoms: &[Item], size: usize) -> Item {
    let mut items = Vec::with_capacity(size);
    let mut left = size;
    while left > 0 {
        if left > 1 && rng.below(6) == 0 {
            let sub = 1 + rng.below(left - 1);
            items.push(random_list(rng, spec, atoms, sub));
            left -= sub;
        } else {
            let mut atom = random_atom(rng, spec, atoms);
            while is_close(&atom) {
                atom = random_atom(rng, spec, atoms);
            }
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
    pub limit_hit: usize,
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
        let (program, plushy) = ATOMS.with(|a| {
            let atoms = a.borrow();
            match p.genome {
                Genome::Tree => {
                    let size = p.length.0 + rng.below(p.length.1 - p.length.0 + 1);
                    (random_list(&mut rng, spec, &atoms, size), None)
                }
                Genome::Plushy { max_size } => {
                    let genes: Vec<Item> = (0..rng.below(max_size.max(1))).map(|_| random_atom(&mut rng, spec, &atoms)).collect();
                    (plushy_to_program(&genes), Some(genes))
                }
            }
        });
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
            let limits = Limits { stack_limit: p.stack_limit, propeller_limits: true, ..Limits::default() };
            let (out, hit) = prepared.output_of(&program, c, limits);
            // a label from a truncated value would hold in Propeller but not in the oracle, so the
            // files keep only programs on which both agree; they are rare
            if hit {
                return Err(Dropped::LimitHit);
            }
            if let Some(out) = out {
                examples.push(format!("{{:inputs {} :output {}}}", prepared.case_text(c), out));
                if !outputs.contains(&out) {
                    outputs.push(out);
                }
            } else if examples.is_empty() && tried.len() >= 3 {
                break; // nothing on the first three inputs: not a program that outputs
            }
        }
        if examples.len() < p.min_examples.min(k) {
            return Err(Dropped::TooFewOutputs);
        }
        if outputs.len() < p.min_distinct {
            return Err(Dropped::TooUniform);
        }
        let plushy = plushy.map_or(String::new(), |g| {
            format!(" :plushy [{}]", g.iter().map(|x| if is_close(x) { ":close".to_string() } else { x.to_edn_string() }).collect::<Vec<_>>().join(" "))
        });
        Ok(format!("{{:program {}{plushy} :examples [{}]}}", program.to_edn_string(), examples.join(" ")))
    });
    let mut stats = Stats::default();
    let mut out = Vec::new();
    for l in lines {
        match l {
            Ok(s) => {
                stats.kept += 1;
                out.push(s);
            }
            Err(Dropped::TooFewOutputs) => stats.too_few_outputs += 1,
            Err(Dropped::TooUniform) => stats.too_uniform += 1,
            Err(Dropped::LimitHit) => stats.limit_hit += 1,
        }
    }
    (out, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = r#"{:alphabet [:integer_+ :integer_* :integer_dup :in1 1 2] :int-erc [-5 5]
        :output :push/integer :max 100 :inputs [[1] [2] [3] [4] [5] [6] [7] [8] [9] [10]]}"#;

    fn params_for(genome: Genome, threads: usize) -> Params {
        Params { genome, n: 300, seed: 7, examples: 5, min_examples: 5, length: (3, 12), min_distinct: 2, stack_limit: 1000, threads }
    }

    fn params(threads: usize) -> Params {
        params_for(Genome::Plushy { max_size: 30 }, threads)
    }

    fn program_text(genes: &str) -> String {
        let genes: Vec<Item> = edn::parse(genes).unwrap().items().unwrap().iter().map(|e| Item::from_edn(e).unwrap()).collect();
        plushy_to_program(&genes).to_edn_string()
    }

    #[test]
    fn plushy_translation_follows_propeller() {
        // no opener: flat
        assert_eq!(program_text("[:integer_+ 1 :close 2]"), "(:integer_+ 1 2)");
        // exec_if opens two blocks; an explicit close ends the first and starts the second
        assert_eq!(program_text("[:exec_if 1 :close 2 3]"), "(:exec_if (1) (2 3))");
        // unclosed blocks are closed at the end, a close with nothing open is ignored
        assert_eq!(program_text("[:close :exec_dup 1 :exec_dup 2]"), "(:exec_dup (1 :exec_dup (2)))");
        assert_eq!(program_text("[:exec_s 1]"), "(:exec_s (1) () ())");
        assert_eq!(program_text("[]"), "()");
    }

    #[test]
    fn tree_genome_still_works() {
        let spec = Spec::parse(SPEC).unwrap();
        let (a, _) = generate(&spec, &params_for(Genome::Tree, 2));
        assert!(!a.is_empty());
        assert!(!a[0].contains(":plushy"));
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
                let limits = Limits { stack_limit: 1000, propeller_limits: true, ..Limits::default() };
                let want = prepared.output_of(&program, case, limits).0.unwrap();
                assert_eq!(edn::edn_text(ex.get("output").unwrap()), want);
            }
        }
    }
}
