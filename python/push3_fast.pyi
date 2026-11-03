from os import PathLike
from typing import Any, Optional, TypedDict

__version__: str


class Row(TypedDict, total=False):
    """A program's result: `error` alone when the program could not be read, the rest otherwise."""

    errors: list[float]
    total: float
    solved: int
    steps: int
    cut: bool
    overflow: bool
    error: str


class Arrays(TypedDict):
    """`errors` has shape (programs, n_errors); the others have shape (programs,).

    A row holds `lengths[i]` errors and then `fill`. Those of a program that could not be read are all
    `fill`, as is its `total`, `ok[i]` is False, and `messages[i]` says why. `messages` holds only those.
    """

    errors: Any
    lengths: Any
    total: Any
    solved: Any
    steps: Any
    cut: Any
    overflow: Any
    ok: Any
    messages: dict[int, str]


class Example(TypedDict):
    inputs: list[Any]
    output: Any


class Record(TypedDict):
    """`edn` is the record as the CLI writes it. A character in `examples` is a str of length 1."""

    edn: str
    program: str
    plushy: Optional[list[str]]
    examples: list[Example]


class Stats(TypedDict):
    kept: int
    too_few_outputs: int
    too_uniform: int
    limit_hit: int


class Problem:
    def __init__(self, edn_text: str) -> None: ...
    @staticmethod
    def from_file(path: str | PathLike[str]) -> "Problem": ...
    @property
    def n_cases(self) -> int: ...
    @property
    def n_errors(self) -> int:
        """Length of an error vector that was not cut: n_cases, or one per bit on a circuit problem."""
    def evaluate(
        self,
        programs: list[str],
        cap: Optional[float] = None,
        screen: Optional[tuple[int, float]] = None,
        stack_limit: int = 0,
        propeller_limits: bool = False,
        threads: Optional[int] = None,
    ) -> list[Row]: ...
    def evaluate_arrays(
        self,
        programs: list[str],
        cap: Optional[float] = None,
        screen: Optional[tuple[int, float]] = None,
        stack_limit: int = 0,
        propeller_limits: bool = False,
        threads: Optional[int] = None,
        fill: float = float("nan"),
    ) -> Arrays:
        """As `evaluate`, without a Python object per program. Needs NumPy (ImportError otherwise)."""


def hindsight(
    spec_edn_text: str,
    n: int = 1000,
    seed: int = 1,
    examples: int = 10,
    min_examples: int = 5,
    max_size: Optional[int] = None,
    min_distinct: int = 2,
    stack_limit: int = 100000,
    threads: Optional[int] = None,
    genome: str = "plushy",
    length: tuple[int, int] = (5, 60),
) -> tuple[list[Record], Stats]:
    """`max_size` applies to genome="plushy" (default: the spec's, else 100), `length` to genome="tree"."""


def plushy_to_program(genes_edn: str) -> str: ...
def names() -> list[str]: ...
