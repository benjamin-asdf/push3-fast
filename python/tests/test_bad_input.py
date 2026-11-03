"""Bad input is a Python exception or that program's error, and the process lives."""

import subprocess
import sys

import pytest

import push3_fast

SMALL = """{:output :push/integer :error :numeric :max 100
            :cases [{:inputs [1] :output 2} {:inputs [5] :output 10}]}"""
SPEC = """{:alphabet [:integer_+ :in1 1 2] :output :push/integer :max 100 :inputs [[1] [2] [3] [4] [5] [6]]}"""
TRUNCATED_ESCAPE = '"\\u12'


@pytest.mark.parametrize(
    "bad",
    [
        "(oops",
        ")",
        "",
        "   ",
        "[1 2",
        '"never closed',
        "{:program}",
        "{:a 1}",
        "#bad",
        "(1 $$ 2)",
        "(1) (2)",
        TRUNCATED_ESCAPE,
        "(" + TRUNCATED_ESCAPE,
    ],
)
def test_a_malformed_program_is_that_programs_error(bad):
    problem = push3_fast.Problem(SMALL)
    before, row, after = problem.evaluate(["(:in1)", bad, "(:in1 2 :integer_*)"], threads=2)
    assert before["errors"] == [1.0, 5.0]
    assert set(row) == {"error"} and isinstance(row["error"], str) and row["error"]
    assert after["errors"] == [0.0, 0.0]


def test_a_batch_with_many_bad_programs_keeps_its_order():
    problem = push3_fast.Problem(SMALL)
    programs = [TRUNCATED_ESCAPE if i % 3 == 0 else f"({i})" for i in range(90)]
    rows = problem.evaluate(programs, threads=4)
    assert ["error" in r for r in rows] == [i % 3 == 0 for i in range(90)]
    good = [r for r in rows if "error" not in r]
    assert good[0]["errors"] == [abs(1 - 2), abs(1 - 10)]


def test_programs_must_be_strings():
    problem = push3_fast.Problem(SMALL)
    with pytest.raises(TypeError):
        problem.evaluate("(:in1)")
    with pytest.raises(TypeError):
        problem.evaluate([1, 2])


@pytest.mark.parametrize(
    "bad",
    [
        "",
        "(oops",
        "5",
        "{:output :push/integer}",
        "{:output :push/nothing :error :numeric :cases [{:inputs [] :output 1}]}",
        "{:output :push/integer :error :nothing :cases [{:inputs [] :output 1}]}",
        "{:output :push/integer :error :numeric :cases []}",
        "{:output :push/integer :error :numeric :cases [{:output 1}]}",
        "{:output :push/integer :error :numeric :max -1 :cases [{:inputs [] :output 1}]}",
        "{:output :push/output :num-outputs 9 :error :bitwise :cases [{:inputs [] :output [true]}]}",
        SPEC,
        TRUNCATED_ESCAPE,
    ],
)
def test_a_bad_problem_is_a_value_error(bad):
    with pytest.raises(ValueError):
        push3_fast.Problem(bad)


def test_a_missing_file_is_file_not_found(tmp_path):
    missing = tmp_path / "missing.edn"
    with pytest.raises(FileNotFoundError) as e:
        push3_fast.Problem.from_file(missing)
    assert e.value.filename == missing
    with pytest.raises(FileNotFoundError):
        push3_fast.Problem.from_file(str(missing))


def test_other_unreadable_files(tmp_path):
    with pytest.raises(IsADirectoryError):
        push3_fast.Problem.from_file(tmp_path)
    binary = tmp_path / "binary.edn"
    binary.write_bytes(b"\xff\xfe\x00")
    with pytest.raises(ValueError):
        push3_fast.Problem.from_file(binary)
    bad = tmp_path / "bad.edn"
    bad.write_text("{:output")
    with pytest.raises(ValueError, match="bad.edn"):
        push3_fast.Problem.from_file(bad)


@pytest.mark.parametrize(
    "bad",
    [
        "",
        "(oops",
        "5",
        "{:output :push/integer :inputs [[1]]}",
        "{:alphabet [] :output :push/integer :inputs [[1]]}",
        "{:alphabet [:in1] :inputs [[1]]}",
        "{:alphabet [:in1] :output :push/integer}",
        "{:alphabet [:in1] :output :push/integer :inputs []}",
        "{:alphabet [:in1] :output :push/integer :max -1 :inputs [[1]]}",
        "{:alphabet [:in1] :output :push/output :num-outputs 99 :inputs [[1]]}",
        "{:alphabet [:in1] :output :push/integer :max-initial-plushy-size -1 :inputs [[1]]}",
        "{:alphabet [:in1 {:a 1}] :output :push/integer :inputs [[1]]}",
        TRUNCATED_ESCAPE,
    ],
)
def test_a_bad_spec_is_a_value_error(bad):
    with pytest.raises(ValueError):
        push3_fast.hindsight(bad, n=10)


@pytest.mark.parametrize(
    "kwargs",
    [{"genome": "bush"}, {"length": (9, 3)}, {"genome": "tree", "length": (1, 1 << 40)}, {"max_size": 100_001}, {"threads": 0}],
)
def test_bad_hindsight_arguments_are_value_errors(kwargs):
    with pytest.raises(ValueError):
        push3_fast.hindsight(SPEC, n=10, **kwargs)


@pytest.mark.parametrize("bad", ["", "(1", "5", "[:in1 {:a 1}]", TRUNCATED_ESCAPE])
def test_a_bad_genome_is_a_value_error(bad):
    with pytest.raises(ValueError):
        push3_fast.plushy_to_program(bad)


def test_threads_zero_is_a_value_error():
    with pytest.raises(ValueError):
        push3_fast.Problem(SMALL).evaluate([], threads=0)


def nested(depth):
    return "(" * depth + ")" * depth


def test_deep_nesting():
    """Past the limit an error; up to it fine, on the worker's stack and on the helper's."""
    problem = push3_fast.Problem(SMALL)
    rows = problem.evaluate([nested(100_000), nested(100_001), nested(5_000_000)], threads=2)
    assert "errors" in rows[0]
    assert "limit" in rows[1]["error"] and "limit" in rows[2]["error"]
    assert push3_fast.plushy_to_program("[" + nested(100_000 - 1) + "]")
    with pytest.raises(ValueError, match="limit"):
        push3_fast.plushy_to_program("[" + nested(100_000) + "]")
    # a chain of openers makes a program as deep as the genome is long
    chain = "[" + ":exec_dup " * 100_000 + "]"
    assert push3_fast.plushy_to_program(chain).count("(") == 100_001
    with pytest.raises(ValueError, match="limit"):
        push3_fast.plushy_to_program("[" + ":exec_dup " * 100_001 + "]")
    with pytest.raises(ValueError, match="limit"):
        push3_fast.Problem("{:output :push/integer :error :numeric :cases [{:inputs [" + nested(5_000_000) + "] :output 1}]}")
    with pytest.raises(ValueError, match="limit"):
        push3_fast.hindsight("{:alphabet [" + nested(5_000_000) + "] :output :push/integer :inputs [[1]]}", n=1)


def test_parens_in_text_do_not_count_as_nesting():
    problem = push3_fast.Problem(SMALL)
    text = '("' + "(" * 110_000 + '" \\( ; ' + "(" * 110_000 + "\n 1)"
    assert "errors" in problem.evaluate([text])[0]


def test_a_panic_does_not_end_the_process_in_a_fresh_interpreter():
    """The whole sequence in its own process, whose exit status says whether it survived."""
    code = f"""
import push3_fast
p = push3_fast.Problem({SMALL!r})
rows, read = push3_fast._panic_for_tests({SMALL!r}, 40, 4)
assert all(("panic" in r) if i % 2 == 0 else r is None for i, r in enumerate(rows)), rows
assert "panic" in read
assert p.evaluate(["(:in1)"], threads=4)[0]["errors"] == [1.0, 5.0]
print("alive")
"""
    done = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True, timeout=60)
    assert done.returncode == 0, done.stderr
    assert done.stdout.strip() == "alive"
