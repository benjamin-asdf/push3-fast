#!/usr/bin/env bash
## Differential test and speed on the Propeller problems that the current build covers fully.
## scripts/propeller-suite.sh [n-programs] [n-cases] [seed]     (needs the clj-push3 oracle checkout)
set -eu
cd "$(dirname "$0")/.."
N="${1:-300}"; K="${2:-20}"; SEED="${3:-1}"
mkdir -p cases
cargo build --release 2>&1 | grep -E "^error" || true
target/release/push3-fast names > cases/names.txt
run_clj() { (cd oracle && flock /tmp/p3f_jvm.lock timeout 1800 clojure -J-Xmx"${ORACLE_HEAP:-2g}" -M -m "$@") 2>&1 | grep -v "^mem-scope\|^Running"; }
COVERED=$(run_clj push3-fast-oracle.coverage ../cases/names.txt | awk '/COVERED/ {print $1}')
echo "covered problems: $(echo $COVERED | wc -w)"
for p in $COVERED; do
  echo "== $p"
  run_clj push3-fast-oracle.problem-dump "$p" "$N" "$K" "$SEED" "../cases/prop_$p.edn"
  target/release/push3-fast diff "cases/prop_$p.edn" 2>&1 | tail -3
  target/release/push3-fast bench "cases/prop_$p.edn" 5
done
