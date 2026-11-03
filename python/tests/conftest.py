"""Paths and helpers. The data under cases/ is git-ignored, so everything using it skips without it."""

import os
import re
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
CLI = ROOT / "target" / "release" / "push3-fast"
EV = ROOT / "cases" / "ev"
HS = ROOT / "cases" / "hs"
# low enough to keep memory bounded: each worker reserves 256 MB of stack and can use more
THREADS = 4
# not on the way to a hang: a run over any of these files takes seconds
CLI_TIMEOUT = 300

ROW = re.compile(
    r"^\{:errors \[(?P<errors>[^\]]*)\] :total (?P<total>\S+) :solved (?P<solved>\d+) :steps (?P<steps>\d+)"
    r"(?P<overflow> :overflow true)?(?P<cut> :cut true)?\}$"
)


def number(text):
    return {"##NaN": float("nan"), "##Inf": float("inf"), "##-Inf": float("-inf")}.get(text) or float(text)


def same_number(a, b):
    return a == b or (a != a and b != b)


def same_numbers(xs, ys):
    return len(xs) == len(ys) and all(same_number(x, y) for x, y in zip(xs, ys))


def need_cli():
    if not CLI.exists():
        pytest.skip(f"{CLI} is not built")
    return str(CLI)


def problem_files(name):
    problem, programs = EV / f"{name}.problem.edn", EV / f"{name}.programs.edn"
    if not (problem.exists() and programs.exists()):
        pytest.skip(f"no test data for {name} under {EV}")
    return problem, programs


def spec_text(name):
    path = HS / f"{name}.spec.edn"
    if not path.exists():
        pytest.skip(f"no spec for {name} under {HS}")
    return path.read_text()


def program_lines(path):
    return [line for line in path.read_text().splitlines() if line.strip()]


def run_cli(args, stdin=None):
    env = {k: v for k, v in os.environ.items() if k.startswith("PATH")}
    return subprocess.run(
        [need_cli(), *args], input=stdin, capture_output=True, text=True, timeout=CLI_TIMEOUT, check=True, env=env
    )


def cli_rows(problem, lines, flags=()):
    """The rows of `push3-fast eval`, as the dicts Problem.evaluate gives."""
    out = run_cli(["eval", str(problem), "--threads", str(THREADS), *flags], stdin="\n".join(lines) + "\n").stdout
    rows = []
    for text in out.splitlines():
        m = ROW.match(text)
        assert m, f"unexpected row: {text[:200]}"
        rows.append(
            {
                "errors": [number(x) for x in m["errors"].split()],
                "total": number(m["total"]),
                "solved": int(m["solved"]),
                "steps": int(m["steps"]),
                "cut": m["cut"] is not None,
                "overflow": m["overflow"] is not None,
            }
        )
    return rows


def oracle_errors(line):
    """The `:errors` of a program line, which is the last key of the map."""
    head = line.rindex(":errors [") + len(":errors [")
    return [number(x) for x in line[head : line.rindex("]")].split()]
