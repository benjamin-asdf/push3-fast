# Behaviour inherited from the oracle

push3-fast reproduces the Clojure interpreter in clj-push3 (the oracle), quirks included, because a
differential test only means something if the quirks match. This file lists the non-obvious ones, so
a researcher is not surprised by them. Each one is pinned by a unit test or by the differential test.

## How an instruction runs

Arguments are popped from the input stacks first, top item first. If a needed stack is empty the
instruction is a no-op that pops nothing. If the Clojure function throws, the instruction is a no-op
that HAS popped its arguments. Otherwise the result is pushed, and integers and floats are
sanitised on every push.

## Numbers

- INTEGER pushes are clamped to +-1e10. `integer_div` goes through double division and truncation.
  `mod` is floored (the sign of the divisor). Operand order for `-`, `div`, `mod`, `quot` is
  "second item op top item".
- FLOAT pushes are clamped to +-1e12, and every value with |x| < 1e-12, zero included, becomes the
  32-bit float `1.0E-12` (the sign is lost). `rem` and `quot` on doubles are `n - trunc(n/d)*d`, not `fmod`.
- `sin`, `cos`, `tan` call Java's `Math` intrinsics, which differ from every libm in the last digit for
  0.2-0.5% of the calls. Programs using them are compared apart and tolerated.
- `rot` leaves the top item in place and swaps the two below it.
- `yank` converts its index with `(int index)`: outside the 32-bit range the instruction is a
  no-op that already popped the index.

## CHAR and nil

- `string_first`, `string_last` and `string_nth` of an empty string (and `string_nth` with a negative
  index) return nil, and nil is pushed onto the CHAR stack.
- `char_print` of nil prints `nil`; `string_from_char` of nil is the empty string.
- Instructions that cannot take nil throw, so they are no-ops that popped it: the `char_is_*`
  predicates, `char_to_integer`, case conversion, `char_<`, `char_>`, `integer_fromchar`, `float_fromchar`.
- `char_<` and `char_>` do not swap their operands (unlike the numeric comparisons): top < second.

## Strings

- `string_concat` puts the top string after the second. `string_split` trims, splits on whitespace,
  gives `[""]` for the empty string, and leaves the first part on top.
- `string_set_char` with nil deletes that character; `string_remove_char` with nil removes nothing;
  `string_contains_char` with nil is true and `string_indexof_char` with nil is 0;
  `string_occurences_char` with nil is 0; `string_conj_char` with nil adds nothing.
- `string_replace_char` with nil on either side is a no-op that popped its arguments;
  `string_replace_first_char` throws on a nil match but treats a nil replacement as "" (it deletes).
- `string_take` and `string_drop` treat negative counts as 0.
- `string_iterate` pops the string even when EXEC is empty; for a one-character string the EXEC top
  stays and runs next.
- `integer_fromstring` and `float_fromstring` read the first form of the text like `read-string`
  and keep it only if it is a number.

## Integer vectors

- `vector_integer_shove` pops its arguments and does nothing else: the oracle looks the stack up with
  a vector as the function, which throws. The other stack-family instructions on vectors work.
- `concat` returns the second vector followed by the top one; `replace` and `replacefirst` pop the new
  value first, then the old one; `pushall` leaves the first element on top.
- `first`, `last` and `nth` of an empty vector give 0; `set` on an empty vector pushes it back unchanged.
- `iterate` does not push its body back for the recursion: each round takes whatever follows it on the
  EXEC stack, and with no EXEC item it is a no-op that popped the vector.
- Vector elements are not clamped, but every value that goes to the INTEGER stack is.

## Where the oracle was wrong and has been fixed

- `integer_div` pushed a Clojure `BigInt`, so `integer_print` printed e.g. `3N`. Fixed in clj-push3
  (cbb5af5): the result is a `Long`.

## Deliberate deviations (Rust semantics at the edges)

- Strings are Rust strings: counts and indices are in Unicode scalar values, not UTF-16 units. Case
  conversion and character classes outside ASCII follow Rust. The tests use ASCII.
- A string longer than 16 MiB, or a vector built by `concat` beyond 20 million elements, is not pushed and
  marks the run unsupported (the oracle would run out of memory first).
- `integer_fromstring` / `float_fromstring` cover decimals, signs, exponents, hex, octal, `NrDIGITS`,
  `N`/`M` suffixes, ratios, `##Inf`/`##NaN`, leading whitespace, commas and comments, and take the first
  form. Numbers beyond 128 bits outside radix 10 and `#_` forms mark the run unsupported; non-ASCII text
  is never a number.
- Not modelled, because their stacks are not: CODE, NAME, `string_rand`, `vector_integer_fromlist`,
  and instructions on `vector_string`, `vector_float`, `vector_boolean`, `vector_char`, CLJ-OBJECT.
  A vector of anything other than non-empty integers lands on a stack no implemented instruction
  reads and is carried as an opaque item (it still prints and compares correctly via `exec_print` and `exec_=`).
- NaN: `(= x x)` is true in Clojure for one NaN object and false for two separately computed ones;
  push3-fast does not track object identity. No implemented instruction should produce NaN (every FLOAT is clamped on push and division by zero is a no-op); it can only enter as a literal or input, and the test generator emits none.
