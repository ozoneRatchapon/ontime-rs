# 011 — Differential fuzzing of the JavaScript conversions

Status: **planned (2026-11-25).** Item 2 of the RIIR 5 review.

## Why

`integration/js.rs` reimplements `Number()`, `String()`, `decodeURI()` and
Express's query decoding, because upstream reads URL strings with them. The
examples in the contract were picked by hand. A differential fuzzer asks node
itself about inputs nobody thought of.

## Approach

1. `scripts/upstream-oracle/gen-js_semantics.ts` generates a seeded corpus,
   so it is reproducible:
   - numeric shapes: signs, exponents, hex, octal and binary, whitespace
     (including U+FEFF and U+3000), `Infinity`, empty strings, long digit runs;
   - percent sequences: valid, truncated and overlong UTF-8, reserved
     characters, `+`;
   - random Unicode.
2. For each input it records node's `Number(x)`, `String(Number(x))`, the
   result or URIError of `decodeURI(x)`, and Express's query parse.
3. `tests/js_semantics_tests.rs` compares every value. Any mismatch is fixed
   in `js.rs`, or listed as deliberate with the reason.
4. About 20,000 inputs, so the test runs in under a second.

## Done when

The corpus agrees with node. A failing input is added to the fixed list, so
it stays covered after the seed changes.
