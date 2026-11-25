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

/// The characters StringToNumber trims: white space and line terminators.
fn is_js_whitespace(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
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

/// Number.prototype.toString for the values that come up here: whole numbers
/// print without a fraction, everything else as Rust's shortest round trip,
/// which is the same digits.
pub fn number_to_string(number: f64) -> String {
    match number {
        n if n.is_nan() => "NaN".to_string(),
        n if n.is_infinite() && n > 0.0 => "Infinity".to_string(),
        n if n.is_infinite() => "-Infinity".to_string(),
        0.0 => "0".to_string(),
        n if n.fract() == 0.0 && n.abs() < 1e21 => format!("{n:.0}"),
        n => format!("{n}"),
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
            match RESERVED.contains(&first) {
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
        let character = std::str::from_utf8(&sequence).map_err(|_| MalformedUri)?;
        out.extend_from_slice(character.as_bytes());
        i += 3 * length;
    }
    String::from_utf8(out).map_err(|_| MalformedUri)
}

fn hex_byte(bytes: &[u8], at: usize) -> Option<u8> {
    let pair = bytes.get(at + 1..at + 3)?;
    let text = std::str::from_utf8(pair).ok()?;
    u8::from_str_radix(text, 16).ok()
}

/// `decodeURIComponent`, as Express's query parser applies it (with `+` as a
/// space). Unlike `decode_uri` nothing stays encoded; a broken sequence is
/// kept as it was, which is what the `qs` package does.
pub fn decode_query_component(text: &str) -> String {
    let spaced = text.replace('+', " ");
    let bytes = spaced.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match (bytes[i], hex_byte(bytes, i)) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or(spaced)
}
