//! push3-fast run                         EDN cases on stdin, EDN results on stdout (one per line)
//! push3-fast diff  <cases.edn>           compare final states with the recorded oracle states
//! push3-fast bench <cases.edn> [reps]    EXEC steps per second
//!
//! A case is a map `{:program (...) :inputs [...] :max 1000}`; recorded cases also carry the
//! oracle's final state `{:int [...] :bool [...] :exec [...] :steps n}`.

use push3_fast::edn::{self, Edn};
use push3_fast::{Item, Machine};
use std::io::{BufRead, Write};
use std::time::Instant;

struct Case {
    program: Item,
    inputs: Vec<Item>,
    max: u64,
    want: Option<State>,
}

#[derive(PartialEq, Debug)]
struct State {
    int: Vec<i64>,
    boolean: Vec<bool>,
    exec: Vec<Item>,
    steps: u64,
}

fn items(e: Option<&Edn>) -> Result<Vec<Item>, String> {
    match e {
        None => Ok(Vec::new()),
        Some(e) => e.items().ok_or("expected a list or vector")?.iter().map(Item::from_edn).collect(),
    }
}

fn parse_case(line: &str) -> Result<Case, String> {
    let e = edn::parse(line)?;
    let program = Item::from_edn(e.get("program").ok_or("missing :program")?)?;
    let max = match e.get("max") {
        Some(Edn::Int(v)) => *v as u64,
        None => 1000,
        _ => return Err(":max must be an integer".into()),
    };
    let want = match e.get("steps") {
        Some(Edn::Int(steps)) => {
            let ints = items(e.get("int"))?;
            let bools = items(e.get("bool"))?;
            Some(State {
                int: ints.iter().map(|i| if let Item::Int(v) = i { *v } else { unreachable!() }).collect(),
                boolean: bools.iter().map(|i| if let Item::Bool(v) = i { *v } else { unreachable!() }).collect(),
                exec: items(e.get("exec"))?,
                steps: *steps as u64,
            })
        }
        _ => None,
    };
    Ok(Case { program, inputs: items(e.get("inputs"))?, max, want })
}

fn run_case(c: &Case) -> State {
    let mut m = Machine::new(c.program.clone(), c.inputs.clone());
    m.run(c.max);
    State { int: m.int, boolean: m.boolean, exec: m.exec, steps: m.steps }
}

fn write_state(s: &State) -> String {
    let mut out = String::from("{:int [");
    out.push_str(&s.int.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "));
    out.push_str("] :bool [");
    out.push_str(&s.boolean.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "));
    out.push_str("] :exec [");
    for (i, it) in s.exec.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        it.write_edn(&mut out);
    }
    out.push_str(&format!("] :steps {}}}", s.steps));
    out
}

fn load(path: &str) -> Vec<Case> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| parse_case(l).unwrap_or_else(|e| panic!("{path} line {}: {e}", i + 1)))
        .collect()
}

fn main() {
    // exec_y and exec_s build deeply nested items; recursion in parse, drop and print needs room
    std::thread::Builder::new().stack_size(1 << 30).spawn(real_main).unwrap().join().unwrap();
}

fn real_main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("run") => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            for line in std::io::stdin().lock().lines() {
                let line = line.expect("stdin");
                if line.trim().is_empty() {
                    continue;
                }
                match parse_case(&line) {
                    Ok(c) => writeln!(out, "{}", write_state(&run_case(&c))).unwrap(),
                    Err(e) => writeln!(out, "{{:error {e:?}}}").unwrap(),
                }
                out.flush().unwrap();
            }
        }
        Some("diff") => {
            let cases = load(args.get(2).expect("cases.edn"));
            let mut bad = 0;
            for (i, c) in cases.iter().enumerate() {
                let got = run_case(c);
                if Some(&got) != c.want.as_ref() {
                    bad += 1;
                    if bad <= 5 {
                        eprintln!(
                            "MISMATCH case {i}\n  program {}\n  want {}\n  got  {}",
                            c.program.to_edn_string(),
                            c.want.as_ref().map(write_state).unwrap_or_else(|| "(none recorded)".into()),
                            write_state(&got)
                        );
                    }
                }
            }
            println!("{} cases, {} mismatches", cases.len(), bad);
            if bad > 0 {
                std::process::exit(1);
            }
        }
        Some("bench") => {
            let cases = load(args.get(2).expect("cases.edn"));
            let reps: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
            let (mut steps, t) = (0u64, Instant::now());
            for _ in 0..reps {
                for c in &cases {
                    steps += run_case(c).steps;
                }
            }
            let dt = t.elapsed().as_secs_f64();
            let runs = cases.len() * reps;
            println!("{runs} runs, {steps} steps, {dt:.3} s, {:.0} runs/s, {:.2} M steps/s", runs as f64 / dt, steps as f64 / dt / 1e6);
        }
        _ => eprintln!("usage: push3-fast run | diff <cases.edn> | bench <cases.edn> [reps]"),
    }
}
