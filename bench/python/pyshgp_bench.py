"""Workload "count-loop" on PyshGP: add 1 to a counter N times with exec_do_range.
usage: python pyshgp_bench.py <N> <repetitions>   (PyshGP 0.1.8 needs Python <= 3.9)"""
import sys
import time
from pyshgp.push.config import PushConfig
from pyshgp.push.instruction_set import InstructionSet
from pyshgp.push.interpreter import PushInterpreter
from pyshgp.push.program import Program, ProgramSignature
from pyshgp.push.atoms import CodeBlock


def run_once(interp, n):
    code = interp.untyped_to_typed([0, 0, n - 1, "exec_do_range", ["int_pop", 1, "int_add"]])
    cfg = PushConfig(step_limit=10**10, runtime_limit=3600)
    prog = Program(CodeBlock(code), ProgramSignature(arity=0, output_stacks=["int"], push_config=cfg))
    t = time.perf_counter()
    out = interp.run(prog, [])
    return time.perf_counter() - t, out


n, reps = int(sys.argv[1]), int(sys.argv[2])
interp = PushInterpreter(InstructionSet(register_core=True))
run_once(interp, min(n, 2000))
best, res = min((run_once(interp, n) for _ in range(reps)), key=lambda x: x[0])
print(f"PyshGP 0.1.8 (Python)\tcount-loop\t{n}\t{best:.6f}\t{res[0]}")
