#!/usr/bin/env bash
## Hindsight data for every Propeller problem, every label re-run on the oracle.
## scripts/hindsight-check-all.sh [n-programs] [n-checked] [seed]     (needs the clj-push3 oracle checkout)
## Memory: the generator runs under a 12 GB address-space cap, the oracle under -Xmx.
set -u
cd "$(dirname "$0")/.."
N="${1:-20000}"; C="${2:-300}"; SEED="${3:-1}"
D="cases/hs"; mkdir -p "$D"
cargo build --release 2>&1 | grep -E "^error" || true
for f in cases/ev/*.problem.edn; do
  p=$(basename "$f" .problem.edn)
  (cd oracle && timeout 120 clojure -J-Xmx"${ORACLE_HEAP:-3g}" -M -m push3-fast-oracle.hindsight-spec "$p" 1000 "$SEED" "../$D/$p.spec.edn" 2>&1 | grep -v "^mem-scope\|^Running\|->")
  printf '%-22s ' "$p"
  ( ulimit -v 12000000; timeout 300 target/release/push3-fast hindsight "$D/$p.spec.edn" --n "$N" --seed "$SEED" 2>&1 >"$D/$p.edn" | cut -c1-170 )
  [ "${PIPESTATUS[0]}" = 0 ] || echo "GENERATOR FAILED $p"
  (cd oracle && timeout 600 clojure -J-Xmx"${ORACLE_HEAP:-3g}" -M -m push3-fast-oracle.hindsight-check "$p" "../$D/$p.spec.edn" "../$D/$p.edn" "$C" 2>&1 | grep -v "^mem-scope\|^Running" | cut -c1-260) || echo "CHECK FAILED $p"
done
echo ALL-DONE
