# push3-fast

Push3 is a mature language. What program-synthesis research with it needs now is not another
design but a fast implementation, so this is one: a Push3 interpreter in Rust, built for search
loops that evaluate millions of candidate programs.

It does not define new semantics. It reproduces those of the Clojure interpreter in
[clj-push3](https://github.com/benjamin-asdf/benjamin-schwerdtner-clj-push3) (an implementation that covers the
Propeller PSB1/PSB2 instruction sets), and it is tested against that interpreter on complete
final states.

## Status: stage 1

| | |
|---|---|
| Stacks | EXEC, INTEGER, BOOLEAN |
| Instructions | 85 (the stack family on each of the three stacks, integer arithmetic and comparison, boolean logic, `exec_if/when/while/do_while/k/s/y/=`, `exec_do*range/count/times`), plus `in1`..`in8` |
| Not yet | float, char, string, vectors, CODE, NAME, PRINT, output registers (stages 2 and 3) |
| Differential test | 29,975 random programs, 0 mismatches against the oracle |
| Dependencies | none (std only) |

Speed, same machine (gamelap, an Arch Linux laptop, one thread), same 4,996 programs, 568,850 EXEC steps:

| | EXEC steps / s |
|---|---|
| clj-push3 (Clojure, oracle), `execute` only, JIT warm-up included | 1.05 M |
| push3-fast | 83.6 M |

That is about 80 times. The oracle number is a single JVM run including warm-up and is
conservative for the Clojure side; take it as an order of magnitude, not a benchmark of Clojure.
Reproduce: `oracle/` writes the cases, `push3-fast bench` measures (below).

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

We have not benchmarked these against each other, and we do not claim to be the first fast
Push. What this repository adds is checkable: semantics pinned to one reference interpreter by a
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

1. Stage 1 (this): EXEC, INTEGER, BOOLEAN. Done.
2. Float, char, string, vectors, PRINT, output registers, NAME, and the remaining instructions
   used by the 31 Propeller problems (324 symbols, 258 of them real instructions).
3. Early exit on the first failed case, case downsampling, parallel evaluation.
4. Python bindings.

## Licence

Not yet chosen.
