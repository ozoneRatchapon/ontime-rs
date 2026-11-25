// Differential fuzzing of the JavaScript conversions the integration API leans
// on (plan 011). Upstream reads strings from URLs with Number(), String() and
// decodeURI(), and Express 5 decodes query values with node's querystring.
// integration/js.rs reimplements them; tests/js_semantics_tests.rs asks node's
// answers here about inputs nobody picked by hand.
//
// Seeded, so the corpus is the same on every run.
import { parse as parseQuery } from 'node:querystring';

let seed = 0x5eed_2026;
const random = () => {
  // mulberry32
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};
const pick = <T>(list: readonly T[]): T => list[Math.floor(random() * list.length)];

// what numbers in URLs are made of, and the characters around them that a
// parser can get wrong: every kind of space, signs, prefixes and words
const numberParts = [
  ...'0123456789',
  ...'0123456789',
  '.',
  'e',
  'E',
  '+',
  '-',
  'x',
  'X',
  'o',
  'O',
  'b',
  'B',
  'a',
  'f',
  'F',
  '_',
  ',',
  ' ',
  '\t',
  '\n',
  '\r',
  '\v',
  '\f',
  ' ',
  '﻿',
  '　',
  ' ',
  ' ',
  '\u0085',
  ' ',
  ' ',
  '​',
  '᠎',
  'Infinity',
  'infinity',
  'inf',
  'NaN',
  'nan',
  '0x',
  '0b',
  '0o',
  '1e21',
  '1e-7',
  '9007199254740993',
];
// percent sequences: plain, reserved, multi-byte, truncated, overlong, surrogates
const uriParts = [
  ...'abcXYZ019 -_.~!*()/?#&=+;:@$,',
  '%',
  '%2',
  '%20',
  '%2F',
  '%2f',
  '%23',
  '%25',
  '%3A',
  '%3B',
  '%40',
  '%2B',
  '%2C',
  '%24',
  '%26',
  '%3D',
  '%3F',
  '%41',
  '%7E',
  '%7F',
  '%80',
  '%C2%A9',
  '%C3',
  '%C3%28',
  '%E0%A4%A8',
  '%E2%82%AC',
  '%E2%82',
  '%F0%9F%98%80',
  '%F0%9F%98',
  '%C0%80',
  '%C1%BF',
  '%ED%A0%80',
  '%F4%90%80%80',
  '%FF',
  '%G0',
  '%%',
  '%zz',
  '+',
  'é',
  '😀',
  '中',
];

const build = (parts: readonly string[], max: number) => {
  const length = Math.floor(random() * (max + 1));
  let text = '';
  for (let i = 0; i < length; i++) text += pick(parts);
  return text;
};

const inputs = new Set<string>([
  '',
  ' ',
  '0',
  '-0',
  '+0',
  '00',
  '0.0',
  '.5',
  '5.',
  '.',
  '+',
  '-',
  'e5',
  '1e',
  '1e+',
  '1e+5',
  '1E-5',
  '0x10',
  '0X1f',
  '0x',
  '-0x10',
  '0b101',
  '0o17',
  '0b2',
  '1_000',
  '1,5',
  'Infinity',
  '-Infinity',
  '+Infinity',
  'infinity',
  'NaN',
  '1e400',
  '-1e400',
  '1e-400',
  '123456789012345678901234567890',
  '0.1',
  '1e21',
  '1e-7',
  '%',
  '%2',
  '100%',
  '%E0%A4%A8',
  '%C0%80',
  '%ED%A0%80',
  '%2F%23',
  'a+b',
  '%2B',
]);
while (inputs.size < 3000) inputs.add(build(numberParts, 6));
while (inputs.size < 5000) inputs.add(build(uriParts, 6));

const numberOf = (text: string) => {
  const n = Number(text);
  return { string: String(n), negativeZero: Object.is(n, -0) };
};
const decoded = (text: string) => {
  try {
    return decodeURI(text);
  } catch {
    return null;
  }
};
// a query value as Express 5 reads it; `&` and `=` would split the pair, so
// those inputs have no query answer
const queryValue = (text: string) => (/[&=]/.test(text) ? null : (parseQuery(`k=${text}`).k as string));

const cases = [...inputs].map((text) => ({
  text,
  number: numberOf(text),
  decodeURI: decoded(text),
  query: queryValue(text),
}));
process.stdout.write(JSON.stringify(cases));
