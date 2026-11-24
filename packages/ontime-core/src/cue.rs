//! Cue numbers as the renumber dialog sends them: decimal strings such as
//! `"10"` or `"1.05"`, kept as integer and fraction digits so `1.05` stays
//! `1.05` rather than becoming a float. Ported from upstream's
//! `getIntegerAndFraction` and the cue formatting in `renumber`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CueNumber {
    pub integer: i64,
    /// The digits after the point, read as a number: `"05"` is 5.
    pub fraction: i64,
    /// How many digits there were after the point.
    pub precision: u32,
}

/// Bounds that keep the arithmetic in `nth_cue` far from overflowing; no real
/// cue comes near them.
const MAX_INTEGER_DIGITS: usize = 12;
const MAX_FRACTION_DIGITS: usize = 6;

impl CueNumber {
    /// An optional `-`, digits, and optionally a point followed by digits.
    pub fn parse(value: &str) -> Option<Self> {
        let (integer, fraction) = match value.split_once('.') {
            Some((integer, fraction)) => (integer, Some(fraction)),
            None => (value, None),
        };
        let digits = integer.strip_prefix('-').unwrap_or(integer);
        if digits.is_empty()
            || digits.len() > MAX_INTEGER_DIGITS
            || !digits.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let fraction = match fraction {
            None => "",
            Some(f)
                if !f.is_empty()
                    && f.len() <= MAX_FRACTION_DIGITS
                    && f.bytes().all(|b| b.is_ascii_digit()) =>
            {
                f
            }
            Some(_) => return None,
        };
        Some(Self {
            integer: integer.parse().ok()?,
            fraction: if fraction.is_empty() {
                0
            } else {
                fraction.parse().ok()?
            },
            precision: u32::try_from(fraction.len()).ok()?,
        })
    }

    /// This number with its fraction scaled up to `precision` digits.
    fn scaled_fraction(self, precision: u32) -> i64 {
        self.fraction * 10_i64.pow(precision - self.precision)
    }
}

/// The cue for the `index`th renumbered entry. Like upstream, the fraction does
/// not roll over into the integer: `1.75 + 0.25` gives `1.100`.
pub fn nth_cue(prefix: &str, start: CueNumber, increment: CueNumber, index: i64) -> String {
    let precision = start.precision.max(increment.precision);
    let integer = start.integer + increment.integer * index;
    if precision == 0 {
        return format!("{prefix}{integer}");
    }
    let fraction = start.scaled_fraction(precision) + increment.scaled_fraction(precision) * index;
    let width = precision as usize;
    format!("{prefix}{integer}.{fraction:0width$}")
}

/// Upstream adds a space after a prefix that does not already end in one or a `-`.
pub fn separated_prefix(prefix: &str) -> String {
    match prefix {
        "" => String::new(),
        p if p.ends_with('-') || p.ends_with(' ') => p.to_string(),
        p => format!("{p} "),
    }
}
