//! push3-fast names                       the implemented instruction names, one per line
//! push3-fast run                         EDN cases on stdin, EDN results on stdout (one per line)
//! push3-fast eval <problem.edn> [--threads N] [--cap X] [--stack-limit N] [--screen K:X]
//!                                        programs on stdin (one per line), one error vector per program
//! push3-fast evaldiff <problem.edn> <programs.edn>   compare error vectors with the recorded oracle ones
//! push3-fast evalprof <problem.edn> <programs.edn>   one thread: time per program, how much the slowest 10% cost
//! push3-fast opprof  <problem.edn> <programs.edn>   time per instruction name over all programs and cases
//! push3-fast slowstep <problem.edn> <programs.edn> <line>   per-op time of one program (0-based line)
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

static UNSUPPORTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct Case {
    program: Item,
    inputs: Vec<Item>,
    input_names: Vec<String>,
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
    string: Vec<String>,
    ivec: Vec<String>,
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

/// The expected `:vector_integer` stack as text. Unlike a literal in a program, an EMPTY vector is
/// a normal value here (`rest` of a one-element vector), so it must not make the case unreadable.
fn int_vector_stack(e: Option<&Edn>) -> Result<Vec<String>, String> {
    let Some(e) = e else { return Ok(Vec::new()) };
    e.items()
        .ok_or("expected a vector")?
        .iter()
        .map(|v| match v {
            Edn::Vector(xs) => xs
                .iter()
                .map(|x| if let Edn::Int(i) = x { Ok(*i) } else { Err("non-integer vector element".to_string()) })
                .collect::<Result<Vec<i64>, String>>()
                .map(|xs| fmt::pr_str(&Item::IntVec(std::rc::Rc::new(xs)))),
            _ => Err("expected a vector".to_string()),
        })
        .collect()
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
                string: text("string")?,
                ivec: int_vector_stack(e.get("vector_integer"))?,
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
    let input_names = match e.get("input-names") {
        Some(v) => v.items().ok_or(":input-names must be a vector")?.iter().map(|k| match k {
            Edn::Kw(n) => Ok(n.clone()),
            _ => Err(":input-names must be keywords".to_string()),
        }).collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    Ok(Case { program, inputs: items(e.get("inputs"))?, input_names, max, want })
}

fn run_case(c: &Case) -> State {
    let mut m = Machine::new(c.program.clone(), c.inputs.clone());
    m.named = c.input_names.iter().cloned().zip(c.inputs.iter().cloned()).collect();
    m.run(c.max);
    if m.unsupported {
        UNSUPPORTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    State {
        int: m.int,
        boolean: m.boolean,
        float: m.float.iter().map(|v| fmt::pr_double(*v)).collect(),
        chr: m.chr.iter().map(|c| c.map_or("nil".to_string(), fmt::pr_char)).collect(),
        string: m.string.iter().map(|t| fmt::pr_string(t)).collect(),
        ivec: m.ivec.iter().map(|v| fmt::pr_str(&push3_fast::Item::IntVec(v.clone()))).collect(),
        exec: m.exec.iter().map(fmt::pr_str).collect(),
        print: m.print,
        regs: m.regs.iter().map(|r| r.map_or("nil".to_string(), |b| b.to_string())).collect(),
        steps: m.steps,
    }
}

fn write_state(s: &State) -> String {
    let join = |v: &[String]| v.join(" ");
    format!(
        "{{:int [{}] :bool [{}] :float [{}] :char [{}] :string [{}] :vector_integer [{}] :exec [{}] :print {} :regs [{}] :steps {}}}",
        s.int.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "),
        s.boolean.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "),
        join(&s.float),
        join(&s.chr),
        join(&s.string),
        join(&s.ivec),
        join(&s.exec),
        fmt::pr_string(&s.print),
        join(&s.regs),
        s.steps
    )
}

/// Cases using item types this build does not model (vectors, strings) are skipped and counted.
fn load(path: &str) -> Vec<Case> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut skipped = 0;
    let cases: Vec<Case> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| match parse_case(l) {
            Ok(c) => Some(c),
            Err(_) => {
                skipped += 1;
                None
            }
        })
        .collect();
    if skipped > 0 {
        println!("note: {skipped} cases skipped (they use item types that are not supported yet)");
    }
    cases
}

fn eval_args(args: &[String]) -> (push3_fast::eval::Problem, usize, push3_fast::eval::Limits) {
    let text = std::fs::read_to_string(args.get(2).expect("problem.edn")).expect("problem.edn");
    let problem = push3_fast::eval::Problem::parse(&text).unwrap_or_else(|e| panic!("problem.edn: {e}"));
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let threads = flag("--threads")
        .map(|v| v.parse().expect("--threads N"))
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let cap = flag("--cap").map(|v| v.parse().expect("--cap X"));
    let screen = flag("--screen").map(|v| {
        let (k, x) = v.split_once(':').expect("--screen K:X");
        (k.parse().expect("--screen K:X"), x.parse().expect("--screen K:X"))
    });
    let mut problem = problem;
    problem.stack_limit = flag("--stack-limit").map_or(0, |v| v.parse().expect("--stack-limit N"));
    (problem, threads, push3_fast::eval::Limits { cap, screen })
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
                let ch: Vec<String> = m.chr.iter().map(|c| c.map_or("nil".to_string(), fmt::pr_char)).collect();
                let st: Vec<String> = m.string.iter().map(|t| fmt::pr_string(t)).collect();
                let vc: Vec<String> = m.ivec.iter().map(|v| fmt::pr_str(&push3_fast::Item::IntVec(v.clone()))).collect();
                println!(
                    "{k} [[{}] [{}] [{}] [{}] [{}] [{}] [{}] {}]",
                    m.int.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "),
                    m.boolean.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "),
                    fl.join(" "),
                    ch.join(" "),
                    st.join(" "),
                    vc.join(" "),
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
        // evalprof <problem.edn> <programs.edn>: single thread, time per program, where it goes
        Some("evalprof") => {
            let (problem, _, _) = eval_args(&args);
            let lines: Vec<String> = std::fs::read_to_string(args.get(3).expect("programs.edn")).unwrap().lines().map(String::from).collect();
            let mut prepared = problem.prepare();
            let mut rows: Vec<(f64, u64, usize)> = Vec::new();
            let t0 = Instant::now();
            let parsed: Vec<_> = lines.iter().map(|l| push3_fast::eval::parse_program_line(l).unwrap().0).collect();
            let parse_s = t0.elapsed().as_secs_f64();
            for (i, p) in parsed.iter().enumerate() {
                let t = Instant::now();
                let r = prepared.evaluate(p, Default::default());
                rows.push((t.elapsed().as_secs_f64(), r.steps, i));
            }
            let total: f64 = rows.iter().map(|r| r.0).sum();
            let steps: u64 = rows.iter().map(|r| r.1).sum();
            rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            let top10: f64 = rows.iter().take(rows.len() / 10).map(|r| r.0).sum();
            let maxed = rows.iter().filter(|r| r.1 as usize >= problem.n_cases() * 2000 * 9 / 10).count();
            println!("parse {parse_s:.3}s run {total:.3}s {:.1}M steps/s; slowest 10% of programs = {:.0}% of time; {maxed} programs near the step limit on every case; mean steps/program {}", steps as f64 / total / 1e6, top10 / total * 100.0, steps / rows.len() as u64);
            for r in rows.iter().take(3) {
                println!("  {:.1} ms, {} steps: {}", r.0 * 1e3, r.1, &lines[r.2][..lines[r.2].len().min(160)]);
            }
        }
        // slowstep <problem.edn> <programs.edn> <line>: per-op time on the first case of one program
        Some("slowstep") => {
            let (problem, _, _) = eval_args(&args);
            let lines: Vec<String> = std::fs::read_to_string(args.get(3).unwrap()).unwrap().lines().map(String::from).collect();
            let idx: usize = args.get(4).unwrap().parse().unwrap();
            let (p, _) = push3_fast::eval::parse_program_line(&lines[idx]).unwrap();
            let mut acc: std::collections::HashMap<String, (f64, u64)> = Default::default();
            for inputs in problem.prepare().case_inputs() {
                let mut m = Machine::new(p.clone(), inputs.clone());
                let mut maxexec = 0;
                while m.steps < 2000 && !m.exec.is_empty() {
                    let top = m.exec.last().unwrap().to_edn_string();
                    let name: String = top.chars().take(30).collect();
                    let t = Instant::now();
                    m.step();
                    let e = acc.entry(if top.starts_with('(') { "(list)".into() } else { name }).or_default();
                    e.0 += t.elapsed().as_secs_f64();
                    e.1 += 1;
                    maxexec = maxexec.max(m.exec.len());
                }
                if maxexec > 1000 { println!("case: steps {} exec {} max exec {} int {}", m.steps, m.exec.len(), maxexec, m.int.len()); }
            }
            let mut v: Vec<_> = acc.into_iter().collect();
            v.sort_by(|a, b| b.1 .0.partial_cmp(&a.1 .0).unwrap());
            for (k, (t, n)) in v.iter().take(8) {
                println!("{k:32} {:8.3} ms {n:6} calls {:8.0} ns/call", t * 1e3, t * 1e9 / *n as f64);
            }
        }
        // opprof <problem.edn> <programs.edn>: time per instruction over every program and case
        Some("opprof") => {
            let (problem, _, _) = eval_args(&args);
            let lines: Vec<String> = std::fs::read_to_string(args.get(3).unwrap()).unwrap().lines().map(String::from).collect();
            let mut prepared = problem.prepare();
            let overhead = {
                let t = Instant::now();
                for _ in 0..1_000_000 { std::hint::black_box(Instant::now().elapsed()); }
                t.elapsed().as_secs_f64() * 1e9 / 1e6
            };
            let mut acc: std::collections::HashMap<String, (f64, u64)> = Default::default();
            for l in &lines {
                let (p, _) = push3_fast::eval::parse_program_line(l).unwrap();
                for inputs in prepared.case_inputs() {
                    let mut m = Machine::new(p.clone(), inputs.clone());
                    while m.steps < 2000 && !m.exec.is_empty() {
                        let key = match m.exec.last().unwrap() {
                            Item::Op(op) => op.name(),
                            Item::List(_) => "(list)".to_string(),
                            _ => "(literal)".to_string(),
                        };
                        let t = Instant::now();
                        m.step();
                        let e = acc.entry(key).or_default();
                        e.0 += t.elapsed().as_secs_f64() * 1e9;
                        e.1 += 1;
                    }
                }
            }
            let total: f64 = acc.values().map(|v| v.0).sum();
            let calls: u64 = acc.values().map(|v| v.1).sum();
            println!("timer overhead ~{overhead:.0} ns/step; {calls} steps, {:.1} ns/step measured", total / calls as f64);
            let mut v: Vec<_> = acc.into_iter().collect();
            v.sort_by(|a, b| b.1 .0.partial_cmp(&a.1 .0).unwrap());
            for (k, (t, n)) in v.iter().take(18) {
                println!("{k:34} {:5.1}% of time {n:9} calls {:8.0} ns/call", t / total * 100.0, t / *n as f64);
            }
        }
        Some("eval") => {
            let (problem, threads, cap) = eval_args(&args);
            let lines: Vec<String> = std::io::stdin().lock().lines().map(|l| l.expect("stdin")).filter(|l| !l.trim().is_empty()).collect();
            let stdout = std::io::stdout();
            let mut out = std::io::BufWriter::new(stdout.lock());
            for r in push3_fast::eval::evaluate_lines(&problem, &lines, threads, cap) {
                match r {
                    Ok(row) => writeln!(out, "{}", row.to_edn()).unwrap(),
                    Err(e) => writeln!(out, "{{:error {e:?}}}").unwrap(),
                }
            }
        }
        Some("evaldiff") => {
            let (problem, threads, _) = eval_args(&args);
            let lines: Vec<String> = std::fs::read_to_string(args.get(3).expect("programs.edn"))
                .expect("programs.edn")
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(String::from)
                .collect();
            let t = Instant::now();
            let rows = push3_fast::eval::evaluate_lines(&problem, &lines, threads, Default::default());
            let dt = t.elapsed().as_secs_f64();
            let (mut bad, mut trig, mut steps) = (0, 0, 0u64);
            for (i, (line, r)) in lines.iter().zip(&rows).enumerate() {
                let want = push3_fast::eval::parse_program_line(line).ok().and_then(|(_, w)| w);
                let (Ok(row), Some(want)) = (r, want) else { bad += 1; continue };
                steps += row.steps;
                let same = row.errors.len() == want.len() && row.errors.iter().zip(&want).all(|(a, b)| a == b || (a.is_nan() && b.is_nan()));
                if !same {
                    if [":float_sin", ":float_cos", ":float_tan"].iter().any(|t| line.contains(t)) {
                        trig += 1;
                        continue;
                    }
                    bad += 1;
                    if bad <= 5 {
                        eprintln!("MISMATCH program {i}\n  {line}\n  got {:?}", row.errors);
                    }
                }
            }
            println!(
                "{} programs x {} cases, {} mismatches, {} in sin/cos/tan programs (tolerated); {:.3} s on {} threads, {:.0} programs/s, {:.1} M steps/s",
                lines.len(), problem.n_cases(), bad, trig, dt, threads, lines.len() as f64 / dt, steps as f64 / dt / 1e6
            );
            if bad > 0 {
                std::process::exit(1);
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
            let show: usize = std::env::var("P3F_SHOW").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
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
                    if bad <= show {
                        eprintln!(
                            "MISMATCH case {i}\n  program {}\n  want {}\n  got  {}",
                            c.program.to_edn_string(),
                            c.want.as_ref().map(write_state).unwrap_or_else(|| "(none recorded)".into()),
                            write_state(&got)
                        );
                    }
                }
            }
            let unsup = UNSUPPORTED.load(std::sync::atomic::Ordering::Relaxed);
            if unsup > 0 {
                println!("note: {unsup} runs touched unmodelled behaviour (unknown keywords)");
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
