# 011 — Differential fuzzing of the JavaScript conversions

Status: **done and deployed (2026-11-25), Worker `f4c09ec0`.** Item 2 of the RIIR 5 review.

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

## Result

5,000 seeded inputs (`gen-js_semantics.ts`). Every result now matches node. The
first run found three real differences, none of which the hand-picked cases
had covered:

1. **U+0085 (NEL).** Rust's `char::is_whitespace` counts it as white space,
   ECMAScript does not. `Number("\u0085")` gave 0 here and NaN in node.
   `is_js_whitespace` now lists ECMAScript's exact set.
2. **Number to text.** JavaScript prints `1e+21`, `1e-7` and the shortest
   digits (`39007199254740990`). We printed every digit. `number_to_string`
   now follows ECMAScript Number::toString, using Rust's shortest round trip
   for the digits.
3. **Query values with broken UTF-8.** node first decodes only a value that
   contains a valid `%XX`. When `decodeURIComponent` throws, it falls back to
   `unescapeBuffer` and reads the bytes with U+FFFD. We returned the raw text.
   Confirmed in node's own `querystring.parse` source before changing it.

Also: `hex_byte` no longer takes a sign (`%+1`), which `u8::from_str_radix`
allowed.
