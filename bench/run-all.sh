#!/usr/bin/env bash
## One workload, "count-loop": add 1 to a counter N times with each implementation's own loop
## instruction; the final counter must equal N. One TSV line per implementation:
##   implementation  workload  iterations  best-seconds  result
## usage: bench/run-all.sh [out.tsv]       env: P3F=<push3-fast checkout> CLJ_PUSH3=<clj-push3 base> CLJ_PUSH3_PERF=<perf branch>
##        PYSHGP_PY=<python 3.9 with pyshgp> CPUSHPUSH=<dir of a built CPushPush> REPS=5
set -u
cd "$(dirname "$0")/.."
ROOT=$(pwd)
OUT="${1:-bench/results/count-loop_$(date +%Y%m%d_%H%M%S).tsv}"
REPS="${REPS:-5}"
CLJ_PUSH3="${CLJ_PUSH3:-$ROOT/../benjamin-schwerdtner-clj-push3}"
CLJ_PUSH3_PERF="${CLJ_PUSH3_PERF:-}"
mkdir -p "$(dirname "$OUT")"; : > "$OUT"
add() { grep -P "\tcount-loop\t" | tee -a "$OUT"; }

# push3-fast: the recorded case holds N=100000; the bench subcommand repeats it
N=1000000
echo "{:program (0 0 $((N-1)) :exec_do*range (:integer_pop 1 :integer_+)) :inputs [] :max 1000000000}" > bench/cases/count-loop-big.edn
cargo build --release 2>&1 | grep -E "^error" || true
S=$(target/release/push3-fast bench bench/cases/count-loop-big.edn $REPS | awk '{print $5}')   # seconds for REPS runs
RES=$(echo "{:program (0 0 999 :exec_do*range (:integer_pop 1 :integer_+)) :inputs [] :max 100000000}" | target/release/push3-fast run | sed -E 's/.*:int \[([-0-9]+)\].*/\1/')
python3 - "$N" "$REPS" "$S" "$RES" <<'PY' | add
import sys
n, reps, secs, res = int(sys.argv[1]), int(sys.argv[2]), float(sys.argv[3]), sys.argv[4]
print(f"push3-fast (Rust)\tcount-loop\t{n}\t{secs/reps:.6f}\t{res} (checked at N=1000)")
PY

N=200000
(cd bench/clj && clojure -J-Xmx2g -M:clj-push3 clj_push3_bench.clj "clj-push3 base (Clojure)" $N $REPS 2>/dev/null) | add
if [ -n "$CLJ_PUSH3_PERF" ]; then
  (cd bench/clj && clojure -J-Xmx2g -Sdeps "{:aliases {:perf {:extra-deps {benjamin-schwerdtner/clj-push3 {:local/root \"$CLJ_PUSH3_PERF\"}}}}}" -M:perf clj_push3_bench.clj "clj-push3 perf branch (Clojure)" $N $REPS 2>/dev/null) | add
fi
(cd bench/clj && clojure -J-Xmx2g -M:propeller propeller_bench.clj $N $REPS 2>/dev/null) | add
(cd bench/pushr && cargo build --release 2>&1 | grep -E "^error" || true; ./target/release/bench-pushr $N $REPS) | add
[ -n "${PYSHGP_PY:-}" ] && "$PYSHGP_PY" bench/python/pyshgp_bench.py 20000 $REPS | add
[ -n "${CPUSHPUSH:-}" ] && LD_LIBRARY_PATH="$CPUSHPUSH/src/.libs" python3 bench/cpp/cpushpush_bench.py "$CPUSHPUSH/src/.libs/runpush" 50000 $REPS | add
echo "$OUT"
