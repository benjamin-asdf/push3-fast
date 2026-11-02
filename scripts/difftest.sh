#!/usr/bin/env bash
## Differential test against the Clojure oracle: scripts/difftest.sh [n-programs] [seed]
## Needs a clj-push3 checkout where oracle/deps.edn points. Cases go to cases/ (not tracked).
set -eu
cd "$(dirname "$0")/.."
N="${1:-3000}"; SEED="${2:-1}"
mkdir -p cases
cargo build --release 2>&1 | grep -E "^error|Finished" || true
target/release/push3-fast names > cases/names.txt
OUT="cases/oracle_n${N}_s${SEED}.edn"
(cd oracle && flock /tmp/p3f_jvm.lock timeout "${ORACLE_TIMEOUT:-1800}" clojure -J-Xmx"${ORACLE_HEAP:-2g}" -M -m push3-fast-oracle.dump "$N" "$SEED" "../$OUT" ../cases/names.txt) 2>&1 | grep -v "^mem-scope\|^Running"
target/release/push3-fast diff "$OUT"
target/release/push3-fast bench "$OUT" "${BENCH_REPS:-20}"
