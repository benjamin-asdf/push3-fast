"""Workload "count-loop" on PyshGP: add 1 to a counter N times with exec_do_range.
usage: python pyshgp_bench.py <N> <repetitions>   (PyshGP 0.1.8 needs Python <= 3.9)"""
import sys
import time
from pyshgp.push.config import PushConfig
from pyshgp.push.instruction_set import InstructionSet
from pyshgp.push.interpreter import PushInterpreter
from pyshgp.push.program import Program, ProgramSignature
from pyshgp.push.atoms import CodeBlock, InstructionMeta, Literal


def run_once(interp, n):
    int_type = interp.type_library["int"]
    lit = lambda v: Literal(value=v, push_type=int_type)
    ins = lambda name, blocks=0: InstructionMeta(name=name, code_blocks=blocks)
    code = CodeBlock([lit(0), lit(0), lit(n - 1), ins("exec_do_range", 1),
                      CodeBlock([ins("int_pop"), lit(1), ins("int_add")])])
    cfg = PushConfig(step_limit=10**10, runtime_limit=3600)
    prog = Program(code=code, signature=ProgramSignature(arity=0, output_stacks=["int"], push_config=cfg))
    t = time.perf_counter()
    out = interp.run(prog, [])
    return time.perf_counter() - t, out


n, reps = int(sys.argv[1]), int(sys.argv[2])
interp = PushInterpreter(InstructionSet(register_core=True))
run_once(interp, min(n, 2000))
best, res = min((run_once(interp, n) for _ in range(reps)), key=lambda x: x[0])
print(f"PyshGP 0.1.8 (Python)\tcount-loop\t{n}\t{best:.6f}\t{res[0]}")
