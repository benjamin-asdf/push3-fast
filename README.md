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
| push3-fast (Rust) | Rust | 21,186,441 | 1x | Complete: for the Propeller benchmarks yes, 265 instruction names; all 31 problems run and match the oracle (92,797 runs, 0 mismatches). Not modelled: CODE, NAME, vectors other than integer. Pro: fastest here, no dependencies, EDN interface, semantics pinned to clj-push3 by a differential test (four seeds of 30,000 random programs over every type, 0 mismatches). Con: young; sin/cos/tan differ from Java in the last digit for 0.2-0.5% of calls; Unicode edge cases follow Rust, not Java (docs/oracle-quirks.md). |
| pushr 0.4.1 (Rust) | Rust | 4,343,105 | 5x | Complete: no for program synthesis benchmarks. Stacks BOOLEAN, CODE, EXEC, FLOAT, INTEGER, NAME and bool/float/int vectors, INDEX, GRAPH; no CHAR or STRING stack, so the string problems cannot be expressed. Pro: second fastest here, crates.io package, configurable step, time and growth limits. Con: version 0.4.1 is from 2022-12-30; looping uses EXEC.LOOP with an INDEX stack, not DO*RANGE. |
| CPushPush (C++) | C++ | 1,976,363 | 11x | Complete: unknown; README lists Exec, Integer, Code, Boolean, Float, Name stacks and no string or char stack. Pro: fast, interpreter and PushGP in one code base, GPL-2.0. Con: last commit 2017-08-21; needed a one-line patch (operator<< for std::valarray in Literal.h) and make flags that skip autotools regeneration to build with GCC 16.2; runpush stops after 1,000,000 steps. |
| Clojush 3.32 (Clojure) | Clojure | 458,026 | 46x | Complete: yes, the original research system (instruction set not counted here). Pro: PushGP plus a collection of benchmark problems in one code base; used in research since 2010. Con: Clojure 1.10 and Leiningen; run limits are global atoms; last commit in the clone 2023-05-06. |
| clj-push3 perf branch (Clojure) | Clojure | 376,210 | 56x | Complete: as clj-push3 (same instructions; identical error vectors in a seeded comparison on six problems). Pro: faster than base for the same semantics. Con: an unmerged branch (direct get/assoc in the stack operations and in execute-instruction/execute-1), still far behind push3-fast. |
| Propeller (Clojure) | Clojure | 247,507 | 86x | Complete: yes, it defines the PSB1/PSB2 problems and instruction sets. Pro: PushGP with lexicase selection, ClojureScript build, the reference for the benchmark suites. Con: slow on this workload; the state is a persistent map. |
| clj-push3 base (Clojure) | Clojure | 204,237 | 104x | Complete: yes for Propeller: 525 instructions, all 362 Propeller instructions have an equivalent. Pro: the reference semantics of this project and the test oracle for push3-fast; instructions are plain data (:in, :out, :f). Con: slow; integer_div returns a Clojure BigInt, so integer_print shows e.g. 5N. |
| PyshGP 0.1.8 (Python) | Python | 16,999 | 1,246x | Complete: 429 core instructions register; not compared with the PSB sets. Pro: Python, easy to combine with ML pipelines. Con: needs Python 3.9 or older (the current release imports collections.Sequence, removed in 3.10; 3.9 is end-of-life since October 2025), which blocks modern environments; slowest by far. |

The ratios, not the absolute numbers, are the result: a different machine moves all rows together.

## Batch evaluation (error vectors)

For search loops: `eval` takes a problem file and many programs and returns one error vector per
program, on all cases, on all cores. The output readers and error functions (numeric, Levenshtein,
exact, bitwise, printed number, integer pair) are those of clj-push3's `run/errors`.

```
$ scripts/evaldiff-all.sh 500 40 1                 # writes cases/ev/<problem>.problem.edn and .programs.edn
$ push3-fast eval cases/ev/fizz-buzz.problem.edn --threads 8 --cap 5000 < programs.txt
{:errors [3.0 1.0 ...] :total 91.0 :solved 4 :steps 61234}
```

A problem file is `{:output :push/string :error :levenshtein :max 2000 :cases [{:inputs [...] :output ...}]}`
(plus `:num-outputs` and `:input-names` for the circuits). A program line is the program or a map
with `:program`. Parity: 500 random programs on each of the 31 problems, every error vector
identical to the oracle's (programs using sin/cos/tan are counted apart). `push3-fast evaldiff
<problem> <programs>` is the comparison.

Options, all off by default so that the results are exactly the oracle's:

| Option | Effect |
|---|---|
| `--threads N` | worker threads (default: all hardware threads) |
| `--cap X` | stop a program once its total error exceeds X; the line ends in `:cut true` and the vector is a prefix |
| `--screen K:X` | after the first K cases, stop if the total error is above X: a cheap test that drops clearly bad programs before they pay for every case (order the cases so the first ones are informative) |
| `--stack-limit N` | a case in which the stacks grow past N items is aborted and scored as no output; the line carries `:overflow true`. Not in the oracle: a program that duplicates a stack to a million items costs more than ten thousand ordinary ones, and its memory traffic does not scale across threads |
| `--propeller-limits` | Propeller's value limits: strings and vectors truncated to 1000 elements when pushed. Not in the oracle |

One limit is always on, in every mode: a run that creates more than 256 MB of strings, vectors and
print output, or holds more than 2^27 stack items, is aborted the same way, and an EXEC item of more
than 4 million nodes is not printed. Without this, one program could end the process, because
`string_replace` can multiply a string's length in one step. The oracle has no such bound, so parity
does not cover those programs.

Cut programs and programs that stay under the limits get exactly the errors the unrestricted run
gives (checked on six problems with several option combinations).

Measured on gcd, 500 random programs, one thread: 0.25 s without options, 0.04 s with
`--screen 5:500000` (482 of the 500 programs are cut after five cases).

Scaling (fizz-buzz, 40 cases, 20,000 programs, best of 7; AMD Ryzen 9 8945H: 8 cores, 16 threads, a
laptop, so clocks scale with load and temperature; 14 GB RAM, Linux 7.2.11, rustc 1.99.1): 2,850
programs/s on one thread, 21,600 on 8 threads (7.6x), 25,900 on 16 threads (9.1x). Past the 8 cores,
the extra threads add little. `evalprof`, `opprof` and `slowstep` show where the time goes.

### Server mode

`push3-fast serve [--threads N]` keeps the process and the loaded problems alive. One request per
line on stdin (an EDN map), one response per line on stdout; a request may carry an `:id` that the
response repeats, and a malformed request gets `{:error "..."}` without stopping the server.

```
{:op :problem :name "gcd" :path "gcd.problem.edn"}             -> {:ok true :cases 40}
{:op :eval :problem "gcd" :programs [(:in1 :in2 :integer_+) ...]
 :cap 5000 :screen [5 500000] :stack-limit 100000 :propeller-limits true :threads 8}  -> {:results [{:errors [...] :total ...} ...]}
{:op :quit}
```

The rows are those of `eval` (checked identical on 500 programs). The saving over one process per
batch is the process start and reading the problem file: small for a 40-case problem (2.2 ms against
2.8 ms for a batch of 32 programs), larger for problems with many cases.

## Hindsight data

A random program solves some problem: the one whose examples are its own inputs and outputs.
`push3-fast hindsight` draws programs the way Propeller draws its initial population, runs each on
inputs sampled from a pool, and writes the program with the examples it produces: training data for
a model that maps examples to programs.

```
$ (cd oracle && clojure -M -m push3-fast-oracle.hindsight-spec gcd 1000 1 ../cases/gcd.spec.edn)
$ push3-fast hindsight cases/gcd.spec.edn --n 200000 --seed 3 > gcd.hindsight.edn
{:program (...) :plushy [...] :examples [{:inputs [48 18] :output 6} ...]}
```

What follows Propeller:

- The genome is a flat "plushy" of `rand-int max-initial-plushy-size` genes, each drawn uniformly
  from the instruction list. It is written out as `:plushy`, a flat token sequence.
- `plushy_to_program` is `plushy->push`, with Propeller's `opens` table under this interpreter's names
  (identical to Propeller's own function on 5,000 random genomes; those genomes do not use the
  `exec_do*`, `exec_print` and `vector_integer_iterate` openers, whose names differ or were left out).
- Every ERC generator is one gene slot and gives a fresh value (from a pool of 1,000 drawn values).
- The step limit, the genome size and the `close` gene come from the problem's Propeller file; the
  spec script reads them there.
- Strings and vectors are truncated to 1000 elements when pushed (`max-string-length`,
  `max-vector-length`).

What does not:

- The instruction list is clj-push3's equivalent of Propeller's, and the instructions are clj-push3's.
  Propeller's other limits (numbers to 1e6, `max-stack-items` 100 instead of 1000, code size) are not
  applied.
- A program that hits a value limit is dropped, so that every label holds on the oracle too (which has
  no such limit). This drops at most 15 in 20,000 programs per problem.
- A program must give at least `--min-examples` outputs (default 5; pool inputs are tried, up to 3
  times `--examples`, until K gave one) and `--min-distinct` different ones (default 2). A circuit's
  answer counts only when every output register was written.

`--genome tree` draws random trees instead (the earlier behaviour). Other options: `--max-size S`,
`--stack-limit L` (default 100,000), `--seed`, `--threads`. The same seed gives the same file on any
thread count.

Check (`scripts/hindsight-check-all.sh`): 20,000 programs per problem on all 31 problems; up to 300
kept programs of each re-run on the oracle, every recorded output compared. 74,272 examples, 0
mismatches, and 43 differences counted apart because the program uses sin, cos or tan (the script
goes by the program's text; it does not establish that the function caused the difference).

The share of programs kept depends on the problem, from 0.2% (square-digits) to 20% (snow-day). Two
problems keep none with Propeller's settings. mul4: its genome size of 10 cannot write 8 registers;
`--max-size 100` keeps 11% of the programs. shopping-list: its instruction set cannot read
the input vectors (clj-push3 notes this; the set that can needs `vector_float`, which is not modelled
here), so the outputs are constant.

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
scripts/difftest.sh 30000 7        # needs a clj-push3 checkout at cbb5af5 or later, see oracle/deps.edn
scripts/verify-all.sh "41 42 43"  # several seeds, then every Propeller problem

# 2. compare
cargo build --release
target/release/push3-fast diff cases.edn      # exits 1 and prints the first mismatches
target/release/push3-fast bench cases.edn 50
```

The oracle must stay the reference: when an instruction changes in clj-push3, the differential
test fails, and the change is followed here. What this has turned up in the corners is in
[docs/oracle-quirks.md](docs/oracle-quirks.md).

## Roadmap

1. Done: all 31 Propeller problems, 265 instruction names, parity with the oracle.
2. Done: batch evaluation with early exit by error cap and parallel threads. Open: case downsampling.
3. Python bindings.
4. The CODE and NAME stacks, and vectors of floats, strings, booleans and chars, for instruction
   sets beyond the Propeller benchmarks.

## Licence

Same as [Propeller](https://github.com/lspector/propeller): the Eclipse Public License 2.0
(`LICENSE`), with the Secondary Licenses its `project.clj` names: GPL-2.0-or-later with the
Classpath exception. This Source Code may also be made available under that Secondary License
when the conditions for such availability set forth in the Eclipse Public License, v. 2.0 are
satisfied.
