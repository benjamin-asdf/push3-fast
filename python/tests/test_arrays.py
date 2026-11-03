"""Problem.evaluate_arrays: the NumPy form, which must say the same as Problem.evaluate."""

import builtins

import pytest
from conftest import THREADS, problem_files, program_lines, same_number, same_numbers

import push3_fast

SMALL = """{:output :push/integer :error :numeric :max 100
            :cases [{:inputs [1] :output 2} {:inputs [5] :output 10} {:inputs [7] :output 14}]}"""
CIRCUIT = """{:output :push/output :num-outputs 3 :error :bitwise :max 100
              :cases [{:inputs [] :output [true false true]} {:inputs [] :output [false false true]}]}"""


def assert_same_as_rows(a, rows, width):
    assert a["errors"].shape == (len(rows), width)
    for i, r in enumerate(rows):
        n = a["lengths"][i]
        assert a["ok"][i] == ("error" not in r)
        if "error" in r:
            assert a["messages"][i] == r["error"]
            continue
        assert same_numbers(list(a["errors"][i, :n]), r["errors"])
        assert all(x != x for x in a["errors"][i, n:])
        assert same_number(a["total"][i], r["total"])
        assert (a["solved"][i], a["steps"][i], a["cut"][i], a["overflow"][i]) == (r["solved"], r["steps"], r["cut"], r["overflow"])


@pytest.mark.parametrize("name", ["gcd", "mul3"])
def test_same_as_the_rows(name):
    pytest.importorskip("numpy")
    path, programs = problem_files(name)
    lines = program_lines(programs)[:200] + ["(not closed"]
    problem = push3_fast.Problem.from_file(path)
    for options in [{}, {"cap": 5000.0, "screen": (5, 500000.0), "stack_limit": 10000}]:
        arrays = problem.evaluate_arrays(lines, threads=THREADS, **options)
        assert_same_as_rows(arrays, problem.evaluate(lines, threads=THREADS, **options), problem.n_errors)


def test_types_and_a_stable_width():
    np = pytest.importorskip("numpy")
    problem = push3_fast.Problem(SMALL)
    # every row cut: the width is still that of the problem
    a = problem.evaluate_arrays(["(:in1)", "(oops", "()"], cap=0.5, fill=np.inf)
    assert a["errors"].shape == (3, 3) and a["errors"].dtype == np.float64
    assert a["lengths"].tolist() == [1, 0, 1]
    assert a["ok"].tolist() == [True, False, True] and a["ok"].dtype == np.bool_
    assert a["cut"].tolist() == [True, False, True]
    assert a["errors"][0].tolist() == [1.0, np.inf, np.inf]
    assert a["errors"][1].tolist() == [np.inf] * 3
    assert a["total"][1] == np.inf
    assert list(a["messages"]) == [1]
    # owned and writable
    a["errors"][0, 0] = 7.0
    assert a["errors"][0, 0] == 7.0


def test_a_circuit_has_one_error_per_bit():
    np = pytest.importorskip("numpy")
    problem = push3_fast.Problem(CIRCUIT)
    assert problem.n_cases == 2 and problem.n_errors == 6
    a = problem.evaluate_arrays(["(true :set_c0 false :set_c1 true :set_c2)"])
    assert a["errors"].shape == (1, 6) and a["lengths"][0] == 6
    assert np.array_equal(a["errors"][0], [0.0, 0.0, 0.0, 1.0, 0.0, 0.0])


def test_numpy_is_needed_only_for_the_arrays(monkeypatch):
    problem = push3_fast.Problem(SMALL)
    real_import = builtins.__import__

    def no_numpy(name, *args, **kwargs):
        if name == "numpy":
            raise ImportError("numpy hidden by the test")
        return real_import(name, *args, **kwargs)

    monkeypatch.setattr(builtins, "__import__", no_numpy)
    monkeypatch.delitem(__import__("sys").modules, "numpy", raising=False)
    with pytest.raises(ImportError):
        problem.evaluate_arrays(["(:in1)"])
    assert problem.evaluate(["(:in1)"])[0]["errors"] == [1.0, 5.0, 7.0]
