//! Workload "count-loop" on pushr: add 1 to a counter N times, using pushr's own loop
//! (`INDEX.DEFINE` + `EXEC.LOOP`). Prints one TSV line: implementation, workload, iterations,
//! best seconds, result, steps.   usage: bench-pushr <N> <repetitions>
use pushr::push::instructions::InstructionSet;
use pushr::push::interpreter::PushInterpreter;
use pushr::push::parser::PushParser;
use pushr::push::state::PushState;
use std::time::Instant;

fn run_once(n: i64) -> (f64, String) {
    let input = format!("( 0 {n} INDEX.DEFINE EXEC.LOOP ( 1 INTEGER.+ ) )");
    let mut st = PushState::new();
    st.configuration.eval_push_limit = i32::MAX;
    st.configuration.eval_time_limit = 3_600_000;
    st.configuration.growth_cap = usize::MAX / 4;
    let mut set = InstructionSet::new();
    set.load();
    PushParser::parse_program(&mut st, &set, &input);
    let t = Instant::now();
    PushInterpreter::run(&mut st, &mut set);
    (t.elapsed().as_secs_f64(), st.int_stack.to_string())
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: i64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let reps: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    run_once(n.min(10_000)); // warm-up
    let (mut best, mut result) = (f64::MAX, String::new());
    for _ in 0..reps {
        let (s, r) = run_once(n);
        if s < best {
            best = s;
            result = r;
        }
    }
    println!("pushr 0.4.1 (Rust)\tcount-loop\t{n}\t{best:.6}\t{result}");
}
