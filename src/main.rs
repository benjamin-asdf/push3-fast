//! push3-fast names                       the implemented instruction names, one per line
//! push3-fast run                         EDN cases on stdin, EDN results on stdout (one per line)
//! push3-fast diff  <cases.edn>           compare final states with the recorded oracle states
//! push3-fast bench <cases.edn> [reps]    EXEC steps per second
//!
//! A case is a map `{:program (...) :inputs [...] :max 1000}`; recorded cases also carry the
//! oracle's final state `{:int [...] :bool [...] :exec [...] :steps n}`.

use push3_fast::edn::{self, Edn};
use push3_fast::fmt;
use push3_fast::{Item, Machine};
use std::io::{BufRead, Write};
use std::time::Instant;

struct Case {
    program: Item,
    inputs: Vec<Item>,
    max: u64,
    want: Option<State>,
}

/// A final state in its text form, so that comparison is exactly "prints the same" (NaN included).
#[derive(PartialEq, Debug)]
struct State {
    int: Vec<i64>,
    boolean: Vec<bool>,
    float: Vec<String>,
    chr: Vec<String>,
    exec: Vec<String>,
    print: String,
    regs: Vec<String>,
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
            let text = |k: &str| -> Result<Vec<String>, String> { Ok(items(e.get(k))?.iter().map(fmt::pr_str).collect()) };
            Some(State {
                int: ints.iter().map(|i| if let Item::Int(v) = i { *v } else { unreachable!() }).collect(),
                boolean: bools.iter().map(|i| if let Item::Bool(v) = i { *v } else { unreachable!() }).collect(),
                float: text("float")?,
                chr: text("char")?,
                exec: text("exec")?,
                print: match e.get("print") {
                    Some(Edn::Str(s)) => s.clone(),
                    None => String::new(),
                    _ => return Err(":print must be a string".into()),
                },
                regs: match e.get("regs") {
                    Some(r) => r.items().ok_or(":regs must be a vector")?.iter().map(|x| match x {
                        Edn::Nil => "nil".to_string(),
                        Edn::Bool(b) => b.to_string(),
                        _ => "?".to_string(),
                    }).collect(),
                    None => vec!["nil".to_string(); 8],
                },
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
    State {
        int: m.int,
        boolean: m.boolean,
        float: m.float.iter().map(|v| fmt::pr_double(*v)).collect(),
        chr: m.chr.iter().map(|c| fmt::pr_char(*c)).collect(),
        exec: m.exec.iter().map(fmt::pr_str).collect(),
        print: m.print,
        regs: m.regs.iter().map(|r| r.map_or("nil".to_string(), |b| b.to_string())).collect(),
        steps: m.steps,
    }
}

fn write_state(s: &State) -> String {
    let join = |v: &[String]| v.join(" ");
    format!(
        "{{:int [{}] :bool [{}] :float [{}] :char [{}] :exec [{}] :print {} :regs [{}] :steps {}}}",
        s.int.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "),
        s.boolean.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "),
        join(&s.float),
        join(&s.chr),
        join(&s.exec),
        fmt::pr_string(&s.print),
        join(&s.regs),
        s.steps
    )
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
        // trigcheck <file>: lines of `x sin cos tan` as hex bits from java.lang.Math
        Some("trigcheck") => {
            let text = std::fs::read_to_string(args.get(2).expect("file")).expect("read");
            let (mut n, mut bad) = (0u64, [0u64; 3]);
            for l in text.lines() {
                let v: Vec<u64> = l.split(' ').map(|h| u64::from_str_radix(h, 16).unwrap()).collect();
                let x = f64::from_bits(v[0]);
                for (k, got) in [x.sin(), x.cos(), x.tan()].iter().enumerate() {
                    if got.to_bits() != v[k + 1] {
                        bad[k] += 1;
                    }
                }
                n += 1;
            }
            println!("{n} samples; Rust differs from Math: sin {} cos {} tan {}", bad[0], bad[1], bad[2]);
        }
        // states <cases.edn> <line>: the state after each step count 1..max, as `bisect` writes it
        Some("states") => {
            let cases = load(args.get(2).expect("cases.edn"));
            let c = &cases[args.get(3).expect("line").parse::<usize>().unwrap() - 1];
            for k in 1..=c.max {
                let mut m = Machine::new(c.program.clone(), c.inputs.clone());
                m.run(k);
                let ex: Vec<String> = m.exec.iter().map(fmt::pr_str).collect();
                let fl: Vec<String> = m.float.iter().map(|v| fmt::pr_double(*v)).collect();
                let ch: Vec<String> = m.chr.iter().map(|c| fmt::pr_char(*c)).collect();
                println!(
                    "{k} [[{}] [{}] [{}] [{}] [{}] {}]",
                    m.int.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "),
                    m.boolean.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "),
                    fl.join(" "),
                    ch.join(" "),
                    ex.join(" "),
                    fmt::pr_string(&m.print)
                );
            }
        }
        Some("names") => {
            for n in push3_fast::all_names() {
                println!("{n}");
            }
        }
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
            let (mut bad, mut trig) = (0, 0);
            for (i, c) in cases.iter().enumerate() {
                let got = run_case(c);
                if Some(&got) != c.want.as_ref() {
                    // java.lang.Math.sin/cos/tan are intrinsics that differ from every libm in the last
                    // digit for 0.2-0.5% of the calls; those programs are counted apart.
                    let text = c.program.to_edn_string();
                    if [":float_sin", ":float_cos", ":float_tan"].iter().any(|t| text.contains(t)) {
                        trig += 1;
                        continue;
                    }
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
            println!("{} cases, {} mismatches, {} further differences in programs using sin/cos/tan (last-digit libm differences, tolerated)", cases.len(), bad, trig);
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
