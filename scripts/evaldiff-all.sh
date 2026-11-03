#!/usr/bin/env bash
## Error vectors of push3-fast against clj-push3's run/errors, on every Propeller problem.
## scripts/evaldiff-all.sh [n-programs] [n-cases] [seed]     (needs the clj-push3 oracle checkout)
set -eu
cd "$(dirname "$0")/.."
N="${1:-500}"; K="${2:-40}"; SEED="${3:-1}"
D="cases/ev"; mkdir -p "$D"
cargo build --release 2>&1 | grep -E "^error" || true
(cd oracle && flock /tmp/p3f_jvm.lock timeout 3000 clojure -J-Xmx"${ORACLE_HEAP:-4g}" -M -e \
  "(require 'push3-fast-oracle.eval-dump) (push3-fast-oracle.eval-dump/all-main $N $K $SEED \"../$D\")" 2>&1 | grep -v "^mem-scope\|^Running")
fail=0
for f in "$D"/*.problem.edn; do
  n=$(basename "$f" .problem.edn)
  printf '%-22s ' "$n"
  target/release/push3-fast evaldiff "$f" "$D/$n.programs.edn" 2>&1 | tail -4 || fail=1
done
exit $fail
