"""The GIL is released: other Python threads run while the library works."""

import random
import threading
import time

import pytest

import push3_fast

# About 40 million steps a program. No test data needed.
SLOW = "{:output :push/integer :error :numeric :max 100000000 :cases [{:inputs [] :output 0} {:inputs [] :output 0}]}"
SLOW_PROGRAM = "(20000000 :exec_do*times (1 :integer_pop))"
SPEC = """{:alphabet [:exec_y :exec_dup :integer_dup :integer_+ :in1 :in1 1 2 3] :int-erc [-5 5]
           :output :push/integer :max 200000 :inputs [[1] [2] [3] [4] [5] [6] [7] [8] [9] [10] [11] [12]]}"""


def ticks_during(work):
    """Runs `work` on a thread while this thread counts; returns (seconds, ticks, longest gap)."""
    worker = threading.Thread(target=work)
    start = last = time.perf_counter()
    longest, ticks = 0.0, 0
    worker.start()
    while worker.is_alive():
        now = time.perf_counter()
        longest = max(longest, now - last)
        last = now
        ticks += 1
    worker.join()
    return time.perf_counter() - start, ticks, longest


def assert_ran_alongside(work):
    seconds, ticks, longest = ticks_during(work)
    assert seconds > 0.2, f"the work took only {seconds:.3f} s, too short to tell"
    assert ticks > 1000
    assert longest < seconds / 4, f"this thread stood still for {longest:.3f} s of {seconds:.3f} s"


def test_the_test_can_tell():
    """A C call that holds the GIL, as a control: a long sort of ints runs no bytecode."""
    random.seed(1)
    xs = [random.random() for _ in range(3_000_000)]
    seconds, _, longest = ticks_during(lambda: sorted(xs))
    assert longest > seconds / 4, f"longest gap {longest:.3f} s of {seconds:.3f} s"


@pytest.mark.parametrize("threads", [1, 2])
def test_evaluate(threads):
    problem = push3_fast.Problem(SLOW)
    rows = []
    assert_ran_alongside(lambda: rows.extend(problem.evaluate([SLOW_PROGRAM] * 2, threads=threads)))
    assert rows[0]["steps"] > 10_000_000


def test_evaluate_arrays():
    pytest.importorskip("numpy")
    problem = push3_fast.Problem(SLOW)
    assert_ran_alongside(lambda: problem.evaluate_arrays([SLOW_PROGRAM] * 2, threads=1))


def test_hindsight():
    assert_ran_alongside(lambda: push3_fast.hindsight(SPEC, n=3000, seed=1, max_size=200, threads=2, stack_limit=2000))


def test_two_python_threads_use_two_cores():
    problem = push3_fast.Problem(SLOW)

    def run():
        problem.evaluate([SLOW_PROGRAM], threads=1)

    start = time.perf_counter()
    run()
    one = time.perf_counter() - start

    workers = [threading.Thread(target=run) for _ in range(2)]
    start = time.perf_counter()
    for w in workers:
        w.start()
    for w in workers:
        w.join()
    both = time.perf_counter() - start
    # serial would be 2x; leave room for a busy machine
    assert both < 1.7 * one, f"one {one:.3f} s, two at once {both:.3f} s"
