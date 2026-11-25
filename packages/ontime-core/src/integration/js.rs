//! JavaScript's conversions, as upstream's integration code relies on them.
//!
//! Values from an HTTP path or query arrive as strings, so upstream leans on
//! `Number(value)`, `String(value)` and `decodeURI(value)` to read them. The
//! same requests have to mean the same thing here, so these follow the
//! ECMAScript rules rather than Rust's parsers (`"inf".parse::<f64>()` is
//! infinity in Rust and `NaN` in JavaScript).

use serde_json::Value;

/// `Number(value)`; `None` where JavaScript gives `NaN`. `undefined` is `None`
/// at the call site, which also gives `NaN`.
pub fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Null => Some(0.0),
        Value::Bool(flag) => Some(f64::from(u8::from(*flag))),
        Value::Number(number) => number.as_f64(),
        Value::String(text) => string_to_number(text),
        // an array converts through its string form: [] is 0, [7] is 7
        Value::Array(items) => match items.as_slice() {
            [] => Some(0.0),
            [only] if !only.is_array() && !only.is_object() => number(only),
            _ => None,
        },
        Value::Object(_) => None,
    }
}

/// ECMAScript StringToNumber: surrounding whitespace is ignored, an empty
/// string is 0, and `0x`, `0o` and `0b` prefixes are integer literals.
fn string_to_number(text: &str) -> Option<f64> {
    let trimmed = text.trim_matches(is_js_whitespace);
    if trimmed.is_empty() {
        return Some(0.0);
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = trimmed.strip_prefix(prefix) {
            return integer_in_radix(digits, radix);
        }
    }
    let (sign, unsigned) = match trimmed.as_bytes()[0] {
        b'-' => (-1.0, &trimmed[1..]),
        b'+' => (1.0, &trimmed[1..]),
        _ => (1.0, trimmed),
    };
    if unsigned == "Infinity" {
        return Some(sign * f64::INFINITY);
    }
    // only digits, one point and one exponent: Rust also takes "inf", "nan"
    // and "infinity", which JavaScript does not
    let is_decimal_literal = !unsigned.is_empty()
        && !unsigned.starts_with(['+', '-'])
        && unsigned
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        && unsigned.bytes().any(|b| b.is_ascii_digit());
    if !is_decimal_literal {
        return None;
    }
    unsigned.parse::<f64>().ok().map(|n| sign * n)
}

fn integer_in_radix(digits: &str, radix: u32) -> Option<f64> {
    if digits.is_empty() {
        return None;
    }
    digits.chars().try_fold(0.0_f64, |total, c| {
        c.to_digit(radix)
            .map(|d| total * f64::from(radix) + f64::from(d))
    })
}

/// The characters StringToNumber trims: ECMAScript's WhiteSpace and
/// LineTerminator. Narrower than Rust's `char::is_whitespace`, which also
/// takes U+0085 (found by the differential fuzz, plan 011).
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// `String(value)`.
pub fn string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number_to_string(number.as_f64().unwrap_or(f64::NAN)),
        Value::String(text) => text.clone(),
        // Array.prototype.join: null and undefined items read as empty
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `String(undefined)`, for messages about a value that was not sent at all.
pub fn string_or_undefined(value: Option<&Value>) -> String {
    value.map_or_else(|| "undefined".to_string(), string)
}

/// Number.prototype.toString (ECMAScript Number::toString, radix 10): the
/// shortest digits that read back as the same number, laid out as JavaScript
/// does, with an exponent from 1e21 up and below 1e-6.
pub fn number_to_string(number: f64) -> String {
    match number {
        n if n.is_nan() => "NaN".to_string(),
        n if n.is_infinite() && n > 0.0 => "Infinity".to_string(),
        n if n.is_infinite() => "-Infinity".to_string(),
        0.0 => "0".to_string(),
        n if n < 0.0 => format!("-{}", number_to_string(-n)),
        n => {
            // Rust's `{:e}` is the shortest round trip, as "1.2345e-7"
            let scientific = format!("{n:e}");
            let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
            let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
            let k = digits.len() as i64;
            let n = exponent.parse::<i64>().unwrap_or(0) + 1;
            match n {
                n if k <= n && n <= 21 => format!("{digits}{}", "0".repeat((n - k) as usize)),
                n if 0 < n && n <= 21 => {
                    format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
                }
                n if -6 < n && n <= 0 => format!("0.{}{digits}", "0".repeat((-n) as usize)),
                n => {
                    let sign = if n - 1 < 0 { '-' } else { '+' };
                    let (first, rest) = digits.split_at(1);
                    let fraction = if rest.is_empty() {
                        String::new()
                    } else {
                        format!(".{rest}")
                    };
                    format!("{first}{fraction}e{sign}{}", (n - 1).abs())
                }
            }
        }
    }
}

/// Characters `decodeURI` leaves encoded, because they mean something in a
/// URI: `; / ? : @ & = + $ , #`.
const RESERVED: &[u8] = b";/?:@&=+$,#";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MalformedUri;

/// `decodeURI(text)`: `%XX` sequences become the UTF-8 they spell, except
/// those for reserved characters. A broken sequence is an error, as the
/// `URIError` JavaScript throws.
pub fn decode_uri(text: &str) -> Result<String, MalformedUri> {
    decode(text, RESERVED)
}

/// `decodeURIComponent(text)`: as `decode_uri`, with nothing left encoded.
pub fn decode_uri_component(text: &str) -> Result<String, MalformedUri> {
    decode(text, b"")
}

/// ECMAScript's Decode: `%XX` to UTF-8, leaving `reserved` ASCII encoded.
fn decode(text: &str, reserved: &[u8]) -> Result<String, MalformedUri> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let first = hex_byte(bytes, i).ok_or(MalformedUri)?;
        if first < 0x80 {
            match reserved.contains(&first) {
                true => out.extend_from_slice(&bytes[i..i + 3]),
                false => out.push(first),
            }
            i += 3;
            continue;
        }
        // a multi-byte character: every byte of it has to be escaped
        let length = match first {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return Err(MalformedUri),
        };
        let mut sequence = vec![first];
        for k in 1..length {
            let at = i + 3 * k;
            if bytes.get(at) != Some(&b'%') {
                return Err(MalformedUri);
            }
            sequence.push(hex_byte(bytes, at).ok_or(MalformedUri)?);
        }
        // rejects overlong forms and surrogates, as Decode does
        let character = std::str::from_utf8(&sequence).map_err(|_| MalformedUri)?;
        out.extend_from_slice(character.as_bytes());
        i += 3 * length;
    }
    String::from_utf8(out).map_err(|_| MalformedUri)
}

/// The two hex digits after a `%` at `at`. Only digits: `u8::from_str_radix`
/// would also take a sign, as in `%+1`.
fn hex_byte(bytes: &[u8], at: usize) -> Option<u8> {
    let pair = bytes.get(at + 1..at + 3)?;
    if !pair.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let text = std::str::from_utf8(pair).ok()?;
    u8::from_str_radix(text, 16).ok()
}

/// A query value as Express 5 reads it (node's `querystring`): `+` is a
/// space; a value with no valid `%XX` is taken as it is, and any other goes
/// through `decodeURIComponent`. When that throws, node falls back to
/// `unescapeBuffer`: each valid `%XX` is a byte, every other UTF-16 unit is
/// kept as its low byte, and the bytes are read as UTF-8 with U+FFFD for
/// anything malformed.
pub fn decode_query_component(text: &str) -> String {
    let spaced = text.replace('+', " ");
    // querystring.parse decodes a value only once it has seen an escape
    let has_escape = (0..spaced.len())
        .any(|i| spaced.as_bytes()[i] == b'%' && hex_byte(spaced.as_bytes(), i).is_some());
    if !has_escape {
        return spaced;
    }
    if let Ok(decoded) = decode_uri_component(&spaced) {
        return decoded;
    }
    let units: Vec<u16> = spaced.encode_utf16().collect();
    let mut bytes = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let escaped = (units[i] == u16::from(b'%'))
            .then(|| units.get(i + 1..i + 3))
            .flatten()
            .and_then(|pair| {
                let text = String::from_utf16(pair).ok()?;
                hex_byte(format!("%{text}").as_bytes(), 0)
            });
        match escaped {
            Some(byte) => {
                bytes.push(byte);
                i += 3;
            }
            None => {
                bytes.push((units[i] & 0xff) as u8);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
