"""Problem.evaluate against the CLI and against the oracle's recorded error vectors."""

import pytest
from conftest import THREADS, cli_rows, oracle_errors, problem_files, program_lines, same_number, same_numbers

import push3_fast

# every output reader and error function: integer, string, print with Levenshtein, circuit bits, float,
# printed number, integer pair, boolean, print with exact
PROBLEMS = ["gcd", "fizz-buzz", "grade", "mul3", "bouncing-balls", "number-io", "find-pair", "solve-boolean", "smallest"]
TRIG = (":float_sin", ":float_cos", ":float_tan")

SMALL = """{:output :push/integer :error :numeric :max 100
            :cases [{:inputs [1] :output 2} {:inputs [5] :output 10} {:inputs [7] :output 14}]}"""


def assert_same_rows(got, want):
    assert len(got) == len(want)
    for i, (g, w) in enumerate(zip(got, want)):
        assert same_numbers(g["errors"], w["errors"]), f"row {i}"
        assert same_number(g["total"], w["total"]), f"row {i}"
        assert (g["solved"], g["steps"], g["cut"], g["overflow"]) == (w["solved"], w["steps"], w["cut"], w["overflow"]), f"row {i}"


@pytest.mark.parametrize("name", PROBLEMS)
def test_same_rows_as_the_cli(name):
    path, programs = problem_files(name)
    lines = program_lines(programs)
    got = push3_fast.Problem.from_file(path).evaluate(lines, threads=THREADS)
    assert_same_rows(got, cli_rows(path, lines))


@pytest.mark.parametrize("name", PROBLEMS)
def test_same_rows_as_the_cli_with_every_option(name):
    path, programs = problem_files(name)
    lines = program_lines(programs)
    problem = push3_fast.Problem.from_file(path)
    # a circuit's errors are bits, so its total stays far below the others'
    cap = 10 if name == "mul3" else 5000
    got = problem.evaluate(lines, cap=float(cap), screen=(5, 500000.0), stack_limit=10000, propeller_limits=True, threads=THREADS)
    flags = ["--cap", str(cap), "--screen", "5:500000", "--stack-limit", "10000", "--propeller-limits"]
    assert_same_rows(got, cli_rows(path, lines, flags))
    assert any(r["cut"] for r in got)


@pytest.mark.parametrize("name", PROBLEMS)
def test_same_errors_as_the_oracle(name):
    path, programs = problem_files(name)
    lines = program_lines(programs)
    rows = push3_fast.Problem.from_file(path).evaluate(lines, threads=THREADS)
    trig = 0
    for i, (line, row) in enumerate(zip(lines, rows)):
        if not same_numbers(row["errors"], oracle_errors(line)):
            # java.lang.Math differs from libm in the last digit; the README counts these apart
            assert any(t in line for t in TRIG), f"program {i}"
            trig += 1
    assert trig <= len(lines) // 20


def test_a_row_of_every_kind():
    problem = push3_fast.Problem(SMALL)
    assert problem.n_cases == 3 and problem.n_errors == 3
    solved, cut, bad = problem.evaluate(["(:in1 2 :integer_*)", "(:in1)", "(oops"], cap=4.0)
    assert solved["errors"] == [0.0, 0.0, 0.0] and solved["total"] == 0.0 and solved["solved"] == 3
    assert solved["steps"] > 0 and not solved["cut"] and not solved["overflow"]
    assert set(bad) == {"error"}
    assert cut["cut"] and cut["errors"] == [1.0, 5.0]
    overflow = problem.evaluate(["(7 100 :integer_dup_times)"], stack_limit=50)[0]
    assert overflow["overflow"] and overflow["errors"] == [1e6] * 3


def test_a_program_may_be_a_map_with_program():
    problem = push3_fast.Problem(SMALL)
    a, b = problem.evaluate(["(:in1 2 :integer_*)", "{:program (:in1 2 :integer_*) :errors [9.0]}"])
    assert a == b and a["solved"] == 3


def test_thread_counts_agree():
    path, programs = problem_files("gcd")
    lines = program_lines(programs)[:100]
    problem = push3_fast.Problem.from_file(path)
    assert_same_rows(problem.evaluate(lines, threads=1), problem.evaluate(lines, threads=THREADS))


def test_empty_batch():
    problem = push3_fast.Problem(SMALL)
    assert problem.evaluate([]) == []
    arrays = problem.evaluate_arrays([]) if _numpy() else None
    if arrays:
        assert arrays["errors"].shape == (0, 3)


def test_names_are_the_implemented_instructions():
    names = push3_fast.names()
    assert len(names) == len(set(names)) and "integer_+" in names and "exec_if" in names


def _numpy():
    try:
        import numpy  # noqa: F401

        return True
    except ImportError:
        return False
