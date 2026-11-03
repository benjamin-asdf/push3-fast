"""hindsight through Python against the CLI: same records for the same seed, on any thread count."""

import re

import pytest
from conftest import HS, THREADS, run_cli, spec_text

import push3_fast

# integer, string, boolean, float, print, circuit, integer pair
PROBLEMS = ["gcd", "small-or-large", "solve-boolean", "bouncing-balls", "grade", "mul3", "find-pair"]
SMALL_SPEC = """{:alphabet [:integer_+ :integer_* :integer_dup :in1 1 2 :exec_if :close] :int-erc [-5 5]
                 :output :push/integer :max 100 :inputs [[1] [2] [3] [4] [5] [6] [7] [8] [9] [10]]}"""
STATS = re.compile(
    r"(\d+) programs drawn, (\d+) kept; dropped (\d+) with fewer than \d+ examples, "
    r"(\d+) with fewer than \d+ distinct outputs, (\d+) that hit a value limit"
)


def cli_hindsight(name, flags):
    done = run_cli(["hindsight", str(HS / f"{name}.spec.edn"), *flags])
    m = STATS.search(done.stderr)
    assert m, done.stderr
    stats = dict(zip(["kept", "too_few_outputs", "too_uniform", "limit_hit"], map(int, m.groups()[1:])))
    return done.stdout.splitlines(), stats


@pytest.mark.parametrize("name", PROBLEMS)
def test_same_records_as_the_cli_on_any_thread_count(name):
    spec = spec_text(name)
    want, want_stats = cli_hindsight(name, ["--n", "3000", "--seed", "3", "--threads", str(THREADS)])
    assert want_stats["kept"] == len(want) > 0
    for threads in (1, 2, THREADS):
        records, stats = push3_fast.hindsight(spec, n=3000, seed=3, threads=threads)
        assert [r["edn"] for r in records] == want
        assert stats == want_stats


@pytest.mark.parametrize(
    "args,flags",
    [
        ({"genome": "tree"}, ["--genome", "tree"]),
        ({"genome": "tree", "length": (3, 12)}, ["--genome", "tree", "--length", "3:12"]),
        ({"max_size": 40, "examples": 6, "min_examples": 3, "min_distinct": 1}, ["--max-size", "40", "--examples", "6", "--min-examples", "3", "--min-distinct", "1"]),
        ({"stack_limit": 500}, ["--stack-limit", "500"]),
    ],
)
def test_options_mean_what_the_cli_flags_mean(args, flags):
    spec = spec_text("gcd")
    want, want_stats = cli_hindsight("gcd", ["--n", "2000", "--seed", "11", "--threads", str(THREADS), *flags])
    records, stats = push3_fast.hindsight(spec, n=2000, seed=11, threads=THREADS, **args)
    assert [r["edn"] for r in records] == want
    assert stats == want_stats


def test_the_spec_gives_the_default_max_size():
    spec = spec_text("gcd")
    assert ":max-initial-plushy-size" in spec
    want, _ = cli_hindsight("gcd", ["--n", "500", "--seed", "5", "--threads", str(THREADS)])
    records, _ = push3_fast.hindsight(spec, n=500, seed=5, threads=THREADS)
    assert [r["edn"] for r in records] == want


@pytest.mark.parametrize("name", PROBLEMS)
def test_decoded_fields_say_what_the_record_says(name):
    records, _ = push3_fast.hindsight(spec_text(name), n=3000, seed=9, threads=THREADS)
    for r in records:
        assert r["edn"].startswith("{:program " + r["program"] + " :plushy [")
        assert push3_fast.plushy_to_program("[" + " ".join(r["plushy"]) + "]") == r["program"]
        assert r["edn"].count(":inputs ") >= len(r["examples"]) >= 2


def test_examples_are_python_values():
    records, stats = push3_fast.hindsight(SMALL_SPEC, n=300, seed=7, max_size=30, threads=2)
    assert stats["kept"] == len(records) > 0
    for r in records:
        assert isinstance(r["plushy"], list)
        for x in r["examples"]:
            assert isinstance(x["inputs"], list) and isinstance(x["inputs"][0], int)
            assert isinstance(x["output"], int)
    tree, _ = push3_fast.hindsight(SMALL_SPEC, n=300, seed=7, length=(3, 12), genome="tree", threads=2)
    assert tree and all(r["plushy"] is None for r in tree)


def test_plushy_translation():
    assert push3_fast.plushy_to_program("[:exec_if 1 :close 2 3]") == "(:exec_if (1) (2 3))"
    assert push3_fast.plushy_to_program("[:close :exec_dup 1 :exec_dup 2]") == "(:exec_dup (1 :exec_dup (2)))"
    assert push3_fast.plushy_to_program("[]") == "()"
    genes = "[" + " ".join(["1", ":integer_+"] * 500) + "]"
    assert run_cli(["plushy"], stdin=genes + "\n").stdout.strip() == push3_fast.plushy_to_program(genes)
