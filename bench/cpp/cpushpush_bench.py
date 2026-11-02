"""Workload "count-loop" on CPushPush (C++): `runpush` reads the program on stdin and prints the
stacks. Wall time of a process includes start-up, so the time of a 0-iteration run is subtracted.
usage: python3 cpushpush_bench.py <runpush-binary> <N> <repetitions>   (LD_LIBRARY_PATH must find libpush)
runpush stops after 1,000,000 steps, about 10 per iteration here, so keep N <= 50,000."""
import re
import subprocess
import sys
import time

binary, n, reps = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])


def run(k):
    prog = f"( 0 0 {k - 1} EXEC.DO*RANGE ( INTEGER.POP 1 INTEGER.+ ) )" if k > 0 else "( 0 )"
    t = time.perf_counter()
    out = subprocess.run([binary], input=prog, capture_output=True, text=True, check=True).stdout
    dt = time.perf_counter() - t
    final = out[out.rindex("\n(\n"):]          # the last printed environment; first list = INTEGER stack
    m = re.search(r"\(\s*\((-?\d+)", final.replace("\t", " ").replace("\n", " "))
    return dt, (m.group(1) if m else "?"), "Bailed out" in out


run(1000)
base = min(run(0)[0] for _ in range(reps))
best = min((run(n) for _ in range(reps)), key=lambda x: x[0])
note = " (bailed out at the step limit)" if best[2] else ""
print(f"CPushPush (C++)\tcount-loop\t{n}\t{max(best[0] - base, 1e-9):.6f}\t{best[1]}{note}")
