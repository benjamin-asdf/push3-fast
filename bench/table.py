#!/usr/bin/env python3
"""Markdown table from bench/results/*.tsv plus the per-implementation comments below.

usage: python3 bench/table.py bench/results/count-loop_YYYYMMDD.tsv > table.md

The TSV is written by bench/run-all.sh. The comments (completeness, pros, cons) are facts we
checked while setting the benchmark up; edit them here when they change.
"""
import sys

# key = start of the implementation name in the TSV; (language, comment)
INFO = {
    "push3-fast": (
        "Rust",
        "Complete: for the Propeller benchmarks yes, 265 instruction names; all 31 problems run and match the "
        "oracle (92,797 runs, 0 mismatches). Not modelled: CODE, NAME, vectors other than integer. "
        "Pro: fastest here, no dependencies, EDN interface, semantics pinned to clj-push3 by a differential test "
        "(four seeds of 30,000 random programs over every type, 0 mismatches). "
        "Con: young; sin/cos/tan differ from Java in the last digit for 0.2-0.5% of calls; Unicode edge cases "
        "follow Rust, not Java (docs/oracle-quirks.md).",
    ),
    "pushr": (
        "Rust",
        "Complete: no for program synthesis benchmarks. Stacks BOOLEAN, CODE, EXEC, FLOAT, INTEGER, NAME and "
        "bool/float/int vectors, INDEX, GRAPH; no CHAR or STRING stack, so the string problems cannot be expressed. "
        "Pro: second fastest here, crates.io package, configurable step, time and growth limits. "
        "Con: version 0.4.1 is from 2022-12-30; looping uses EXEC.LOOP with an INDEX stack, not DO*RANGE.",
    ),
    "CPushPush": (
        "C++",
        "Complete: unknown; README lists Exec, Integer, Code, Boolean, Float, Name stacks and no string or char stack. "
        "Pro: fast, interpreter and PushGP in one code base, GPL-2.0. "
        "Con: last commit 2017-08-21; needed a one-line patch (operator<< for std::valarray in Literal.h) and "
        "make flags that skip autotools regeneration to build with GCC 16.2; runpush stops after 1,000,000 steps.",
    ),
    "Clojush": (
        "Clojure",
        "Complete: yes, the original research system (instruction set not counted here). "
        "Pro: PushGP plus a collection of benchmark problems in one code base; used in research since 2010. "
        "Con: Clojure 1.10 and Leiningen; run limits are global atoms; last commit in the clone 2023-05-06.",
    ),
    "clj-push3 perf": (
        "Clojure",
        "Complete: as clj-push3 (same instructions; identical error vectors in a seeded comparison on six problems). "
        "Pro: faster than base for the same semantics. Con: an unmerged branch (direct get/assoc in the stack "
        "operations and in execute-instruction/execute-1), still far behind push3-fast.",
    ),
    "clj-push3 base": (
        "Clojure",
        "Complete: yes for Propeller: 525 instructions, all 362 Propeller instructions have an equivalent. "
        "Pro: the reference semantics of this project and the test oracle for push3-fast; instructions are "
        "plain data (:in, :out, :f). "
        "Con: slow; integer_div returns a Clojure BigInt, so integer_print shows e.g. 5N.",
    ),
    "Propeller": (
        "Clojure",
        "Complete: yes, it defines the PSB1/PSB2 problems and instruction sets. "
        "Pro: PushGP with lexicase selection, ClojureScript build, the reference for the benchmark suites. "
        "Con: slow on this workload; the state is a persistent map.",
    ),
    "PyshGP": (
        "Python",
        "Complete: 429 core instructions register; not compared with the PSB sets. "
        "Pro: Python, easy to combine with ML pipelines. "
        "Con: needs Python 3.9 or older (the current release imports collections.Sequence, removed in 3.10; "
        "3.9 is end-of-life since October 2025), which blocks modern environments; slowest by far.",
    ),
}


def info(name):
    for key, v in INFO.items():
        if name.startswith(key):
            return v
    return ("", "")


def main(path):
    rows = []
    for line in open(path):
        impl, _wl, n, secs, res = line.rstrip("\n").split("\t")
        n, secs = int(n), float(secs)
        rows.append((impl, n, secs, n / secs, res))
    rows.sort(key=lambda r: -r[3])
    top = rows[0][3]
    print("| Implementation | Language | Iterations/s | Slower than push3-fast | Comment |")
    print("|---|---|---:|---:|---|")
    for impl, n, secs, rate, res in rows:
        lang, comment = info(impl)
        slower = "1x" if rate == top else f"{top / rate:,.0f}x"
        print(f"| {impl} | {lang} | {rate:,.0f} | {slower} | {comment} |")


if __name__ == "__main__":
    main(sys.argv[1])
