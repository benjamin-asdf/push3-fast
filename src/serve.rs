//! `push3-fast serve`: a long-running evaluator for a search loop. One request per line on stdin
//! (an EDN map), one response per line on stdout. Problems are loaded once and kept.
//!
//! ```text
//! {:op :problem :name "gcd" :path "gcd.problem.edn"}      -> {:ok true :cases 40}
//! {:op :problem :name "gcd" :problem {:output ... :cases [...]}}   (inline instead of :path)
//! {:op :eval :problem "gcd" :programs [(...) (...)]
//!  :cap 5000 :screen [5 500000] :stack-limit 100000 :threads 8}
//!                                                         -> {:results [{:errors [...] :total ...} ...]}
//! {:op :quit}
//! ```
//!
//! Every request may carry an `:id`, which the response repeats. A malformed request gets
//! `{:error "..."}` and the server keeps running. `:cap`, `:screen`, `:stack-limit` and `:threads`
//! are the options of `push3-fast eval` and are optional.

use crate::edn::{self, Edn};
use crate::eval::{evaluate_batch, Limits, Problem};
use crate::{fmt, Item};
use std::collections::HashMap;
use std::io::{BufRead, Write};

fn number(e: &Edn) -> Option<f64> {
    match e {
        Edn::Int(i) => Some(*i as f64),
        Edn::Float(f) => Some(*f),
        _ => None,
    }
}

fn limits(req: &Edn) -> Result<Limits, String> {
    let cap = match req.get("cap") {
        Some(c) => Some(number(c).ok_or(":cap must be a number")?),
        None => None,
    };
    let screen = match req.get("screen").and_then(|s| s.items()) {
        Some([k, x]) => Some((number(k).ok_or(":screen is [k x]")? as usize, number(x).ok_or(":screen is [k x]")?)),
        Some(_) => return Err(":screen is [k x]".into()),
        None => None,
    };
    let stack_limit = match req.get("stack-limit") {
        Some(Edn::Int(n)) => *n as usize,
        _ => 0,
    };
    Ok(Limits { cap, screen, stack_limit })
}

fn handle(req: &Edn, problems: &mut HashMap<String, Problem>, default_threads: usize) -> Result<Option<String>, String> {
    let op = match req.get("op") {
        Some(Edn::Kw(k)) => k.as_str(),
        _ => return Err("missing :op".into()),
    };
    let name = |req: &Edn| match req.get("name").or(req.get("problem")) {
        Some(Edn::Str(s)) => Ok(s.clone()),
        Some(Edn::Kw(s)) => Ok(s.clone()),
        _ => Err("missing problem name".to_string()),
    };
    match op {
        "problem" => {
            let p = match (req.get("path"), req.get("problem")) {
                (Some(Edn::Str(path)), _) => Problem::parse(&std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?)?,
                (_, Some(inline)) => Problem::from_edn(inline)?,
                _ => return Err(":problem needs :path or an inline :problem".into()),
            };
            let n = p.n_cases();
            problems.insert(name(req)?, p);
            Ok(Some(format!("{{:ok true :cases {n}}}")))
        }
        "eval" => {
            let key = name(req)?;
            let problem = problems.get(&key).ok_or_else(|| format!("unknown problem {key:?}; send :op :problem first"))?;
            let programs = req.get("programs").and_then(|p| p.items()).ok_or(":programs must be a list")?;
            let threads = match req.get("threads") {
                Some(Edn::Int(n)) if *n > 0 => *n as usize,
                _ => default_threads,
            };
            let rows = evaluate_batch(problem, programs.len(), threads, limits(req)?, |i| Item::from_edn(&programs[i]));
            let rows: Vec<String> = rows
                .into_iter()
                .map(|r| match r {
                    Ok(row) => row.to_edn(),
                    Err(e) => format!("{{:error {}}}", fmt::pr_string(&e)),
                })
                .collect();
            Ok(Some(format!("{{:results [{}]}}", rows.join(" "))))
        }
        "quit" => Ok(None),
        other => Err(format!("unknown :op {other}")),
    }
}

pub fn serve(default_threads: usize) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut problems = HashMap::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let req = edn::parse(&line);
        let id = req.as_ref().ok().and_then(|r| r.get("id")).map(edn::edn_text);
        let reply = match req.and_then(|r| handle(&r, &mut problems, default_threads)) {
            Ok(Some(body)) => body,
            Ok(None) => break,
            Err(e) => format!("{{:error {}}}", fmt::pr_string(&e)),
        };
        // the id goes in front: "{:id 7 :results ..."
        let reply = match id {
            Some(id) => format!("{{:id {id} {}", &reply[1..]),
            None => reply,
        };
        writeln!(out, "{reply}").unwrap();
        out.flush().unwrap();
    }
}
