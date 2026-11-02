#!/usr/bin/env bash
## The full parity check: random programs over every implemented type, for each seed, then every
## Propeller problem whose instructions are all implemented.
## scripts/verify-all.sh "41 42 43" [n-programs] [suite-programs]
## Needs the clj-push3 oracle checkout (oracle/deps.edn); the JVM runs are serialised by a lock.
set -u
cd "$(dirname "$0")/.."
SEEDS="${1:-41 42 43}"; N="${2:-30000}"; SUITE_N="${3:-100}"
for s in $SEEDS; do
  echo "##### seed $s, $N programs, all types"
  scripts/difftest.sh "$N" "$s" 2>&1 | grep -a -E "oracle:|oracle execute|cases, |MISMATCH|M steps/s$|not registered" | cut -c1-300
done
echo "##### Propeller problems, $SUITE_N programs x 20 cases"
scripts/propeller-suite.sh "$SUITE_N" 20 1 2>&1 | grep -a -E "^covered|^==|runs written|cases, |note:|M steps/s$" | cut -c1-300
echo "##### done"
