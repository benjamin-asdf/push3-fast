# push3-fast

Push3 is a mature language. What program-synthesis research with it needs now is not another
design but a fast implementation, so this is one: a Push3 interpreter in Rust, built for search
loops that evaluate millions of candidate programs.

It does not define new semantics. It reproduces those of the Clojure interpreter in
[clj-push3](https://github.com/benjamin-asdf/benjamin-schwerdtner-clj-push3) (an implementation that covers the
Propeller PSB1/PSB2 instruction sets), and it is tested against that interpreter on complete
final states.

## Status

| | |
|---|---|
| Stacks | EXEC, INTEGER, BOOLEAN, FLOAT, CHAR, STRING, integer vectors, PRINT output, output registers `c0`..`c7` |
| Instructions | 265 names: the stack family (`dup`, `pop`, `swap`, `rot`, `yank`, `shove`, `print`, ...) on seven stacks, 54 string, 37 integer-vector, 36 float, 33 integer, 29 char, 26 boolean and 25 exec instructions, `in1`..`in8` and named inputs, `set_c0`..`set_c7` / `c0`..`c7`, `print_newline` |
| Not modelled | CODE, NAME, vectors of floats/strings/booleans/chars (values of those types are carried but nothing reads them), `string_rand`, `vector_integer_fromlist` |
| Propeller problems | all 31 run completely and match the oracle (table below) |
| Differential test | four seeds of 30,000 random programs over every implemented type: 0 mismatches; the programs that use `sin`, `cos` or `tan` are counted apart (last-digit libm differences, 0.2-0.5% of the calls) |
| Dependencies | none (std only) |

The oracle is the Clojure interpreter in clj-push3 at commit cbb5af5 or later. How it behaves in the
corners, and where push3-fast deliberately follows Rust instead (Unicode edge cases), is in
[docs/oracle-quirks.md](docs/oracle-quirks.md).

## Parity on the Propeller problems

For every problem: random programs drawn from the problem's own instruction set and ERCs, run on
the problem's own training cases with the benchmark's step limit of 2000, on both interpreters; the
complete final state (all stacks, PRINT output, registers, step count) must be identical. Runs whose
final EXEC stack is larger than 20,000 nodes are left out (printing them takes longer than running them).

| Problem | Runs compared | Mismatches | Oracle M steps/s | Rust M steps/s |
|---|---:|---:|---:|---:|
| small-or-large | 2,000 | 0 | 1.31 | 87.8 |
| grade | 1,999 | 0 | 1.28 | 67.9 |
| scrabble-score | 2,000 | 0 | 1.38 | 80.5 |
| basement | 1,020 | 0 | 1.06 | 83.5 |
| bouncing-balls | 2,000 | 0 | 1.36 | 46.2 |
| bowling | 2,000 | 0 | 1.28 | 84.9 |
| camel-case | 2,000 | 0 | 1.29 | 74.4 |
| dice-game | 1,966 | 0 | 1.54 | 78.6 |
| find-pair | 2,000 | 0 | 1.30 | 83.0 |
| fizz-buzz | 2,000 | 0 | 1.35 | 92.3 |
| fuel-cost | 2,000 | 0 | 1.27 | 79.7 |
| gcd | 1,980 | 0 | 1.43 | 80.7 |
| luhn | 2,000 | 0 | 1.31 | 83.0 |
| middle-character | 1,980 | 0 | 1.34 | 95.8 |
| paired-digits | 2,000 | 0 | 1.32 | 78.8 |
| snow-day | 2,000 | 0 | 1.36 | 72.4 |
| solve-boolean | 2,000 | 0 | 1.29 | 78.4 |
| spin-words | 2,000 | 0 | 1.32 | 89.5 |
| square-digits | 1,980 | 0 | 1.31 | 87.8 |
| substitution-cipher | 2,000 | 0 | 1.32 | 88.7 |
| twitter | 1,980 | 0 | 1.35 | 75.1 |
| smallest | 1,952 | 0 | 1.34 | 57.2 |
| number-io | 2,000 | 0 | 1.33 | 44.8 |
| string-classification | 2,000 | 0 | 1.26 | 76.8 |
| integer-regression | 1,980 | 0 | 1.35 | 58.0 |
| nguyen-f1 | 1,980 | 0 | 1.44 | 34.5 |
| nguyen-f5 | 1,980 | 0 | 1.37 | 34.6 |
| mul3 | 6,400 | 0 | 1.18 | 65.0 |
| mul4 | 25,600 | 0 | 1.56 | 56.2 |
| count-odds | 4,000 | 0 | 1.35 | 61.2 |
| shopping-list | 4,000 | 0 | 1.51 | 47.9 |
| **31 problems** | **92,797** | **0** | | |

Speeds are million EXEC steps per second on one thread, on the same programs (the oracle's number
includes JIT warm-up, so read it as an order of magnitude).

## Benchmark: all implementations

One workload, `count-loop`: add 1 to a counter N times with the implementation's own loop
instruction; the final counter must equal N (every row below checked that). Best of 5 runs after
warm-up. Each implementation gets its own N so that its run takes at least a few milliseconds (push3-fast
1,000,000; clj-push3, Propeller, Clojush, pushr 200,000; CPushPush 50,000; PyshGP 20,000), and the
rate is iterations per second. Loop constructs differ (`exec_do*range` or `exec_do_range`, and
`INDEX.DEFINE` + `EXEC.LOOP` in pushr, which does not push the index, so its body has no `integer_pop`),
so steps per iteration differ; compare rates, not step counts. For CPushPush the time of a
0-iteration process run is subtracted.

Machine: AMD Ryzen 9 8945H (8 cores / 16 threads, a laptop: clocks scale with load and
temperature), 14 GB RAM, Linux 7.2.11 (Arch), OpenJDK 27, rustc 1.99.1, GCC 16.2.1, Clojure CLI
1.12.5, Python 3.9 for PyshGP. One thread each. Run it with `bench/run-all.sh`; raw rows are in
`bench/results/`.

| Implementation | Language | Iterations/s | Slower than push3-fast | Comment |
|---|---|---:|---:|---|
| push3-fast (Rust) | Rust | 26,455,026 | 1x | Complete: no, 170 instructions (EXEC, INTEGER, BOOLEAN, FLOAT, CHAR stacks, PRINT, output registers); 11 of the 31 Propeller problems run completely, strings and vectors are missing. Pro: fastest here, no dependencies, EDN interface, semantics pinned to clj-push3 by a differential test (29,995 random programs, 0 mismatches). Con: incomplete; sin/cos/tan differ from Java in the last digit for 0.2-0.5% of calls. |
| pushr 0.4.1 (Rust) | Rust | 4,335,197 | 6x | Complete: no for program synthesis benchmarks. Stacks BOOLEAN, CODE, EXEC, FLOAT, INTEGER, NAME and bool/float/int vectors, INDEX, GRAPH; no CHAR or STRING stack, so the string problems cannot be expressed. Pro: second fastest here, crates.io package, configurable step, time and growth limits. Con: version 0.4.1 is from 2022-12-30; looping uses EXEC.LOOP with an INDEX stack, not DO*RANGE. |
| CPushPush (C++) | C++ | 1,844,678 | 14x | Complete: unknown; README lists Exec, Integer, Code, Boolean, Float, Name stacks and no string or char stack. Pro: fast, interpreter and PushGP in one code base, GPL-2.0. Con: last commit 2017-08-21; needed a one-line patch (operator<< for std::valarray in Literal.h) and make flags that skip autotools regeneration to build with GCC 16.2; runpush stops after 1,000,000 steps. |
| Clojush 3.32 (Clojure) | Clojure | 436,450 | 61x | Complete: yes, the original research system (instruction set not counted here). Pro: PushGP plus a collection of benchmark problems in one code base; used in research since 2010. Con: Clojure 1.10 and Leiningen; run limits are global atoms; last commit in the clone 2023-05-06. |
| clj-push3 perf branch (Clojure) | Clojure | 362,518 | 73x | Complete: as clj-push3 (same instructions; identical error vectors in a seeded comparison on six problems). Pro: faster than base for the same semantics. Con: an unmerged branch (direct get/assoc in the stack operations and in execute-instruction/execute-1), still far behind push3-fast. |
| Propeller (Clojure) | Clojure | 245,417 | 108x | Complete: yes, it defines the PSB1/PSB2 problems and instruction sets. Pro: PushGP with lexicase selection, ClojureScript build, the reference for the benchmark suites. Con: slow on this workload; the state is a persistent map. |
| clj-push3 base (Clojure) | Clojure | 226,078 | 117x | Complete: yes for Propeller: 525 instructions, all 362 Propeller instructions have an equivalent. Pro: the reference semantics of this project and the test oracle for push3-fast; instructions are plain data (:in, :out, :f). Con: slow; integer_div returns a Clojure BigInt, so integer_print shows e.g. 5N. |
| PyshGP 0.1.8 (Python) | Python | 17,069 | 1,550x | Complete: 429 core instructions register; not compared with the PSB sets. Pro: Python, easy to combine with ML pipelines. Con: needs Python 3.9 or older (the current release imports collections.Sequence, removed in 3.10; 3.9 is end-of-life since October 2025), which blocks modern environments; slowest by far. |

The ratios, not the absolute numbers, are the result: a different machine moves all rows together.

## Prior work

Push is Lee Spector's language; the specification is the
[Push3 description](https://faculty.hampshire.edu/lspector/push.html). Implementations we know of:

- [Clojush](https://github.com/lspector/Clojush) (Clojure) and
  [Propeller](https://github.com/lspector/propeller) (Clojure): the research systems behind the
  program-synthesis benchmark suites PSB1 and [PSB2](https://www.cs.hamilton.edu/~thelmuth/PSB2/PSB2.html).
- [PyshGP](https://github.com/erp12/pyshgp) (Python): PushGP for use in Python ML pipelines.
- [CPushPush](https://github.com/Killeroid/CPushPush) (C++, GPL-2.0): interpreter and PushGP;
  its page makes no speed claims.
- [pushr](https://docs.rs/crate/pushr/latest) (Rust, version 0.4.1 released 2022-12-30): a Push
  interpreter with vector types; its page makes no speed claims either.

All of them are in the table above. We do not claim to be the first fast Push. What this
repository adds is checkable: semantics pinned to one reference interpreter by a
differential test on complete final states, an EDN interface that speaks the data Push programs
already are, and no dependencies.

## Interface (EDN)

A Push program is EDN data: keywords are instructions, lists are code, integers and booleans are
literals. No translation layer is needed from Clojure.

```
$ echo '{:program (2 3 :integer_+ :in1 :integer_*) :inputs [4] :max 100}' | push3-fast run
{:int [20] :bool [] :exec [] :steps 6}
```

`run` reads one case per line on stdin and writes one result per line. Any process can drive it,
for example a Clojure `babashka.process` or a Python `subprocess`.

## Testing against the oracle

```
# 1. cases: random programs run on clj-push3, with the full final state recorded
cd oracle && clojure -M -m push3-fast-oracle.dump 5000 7 ../cases.edn   # needs a clj-push3 checkout, see oracle/deps.edn

# 2. compare
cargo build --release
target/release/push3-fast diff cases.edn      # exits 1 and prints the first mismatches
target/release/push3-fast bench cases.edn 50
```

The oracle must stay the reference: when an instruction changes in clj-push3, the differential
test fails, and the change is followed here. Semantics that came out of this, all replicated:
operand order of `-`, `div`, `mod` (second item op top item), `div` through double division and
truncation, floored `mod`, clamping of every pushed integer to ±1e10, `rot` leaving the top item in
place, `yank` taking `(int index)` and treating out-of-range indices as a no-op that still pops.

## Roadmap

1. Stages 1 and 2a: EXEC, INTEGER, BOOLEAN, FLOAT, CHAR, PRINT, registers. Done.
2. Strings (53 instructions), `vector_integer` (37) and NAME, the rest of the 31 Propeller
   problems (324 symbols, 258 of them real instructions).
3. Early exit on the first failed case, case downsampling, parallel evaluation.
4. Python bindings.

## Licence

Same as [Propeller](https://github.com/lspector/propeller): the Eclipse Public License 2.0
(`LICENSE`), with the Secondary Licenses its `project.clj` names: GPL-2.0-or-later with the
Classpath exception. This Source Code may also be made available under that Secondary License
when the conditions for such availability set forth in the Eclipse Public License, v. 2.0 are
satisfied.
