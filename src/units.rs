//! Strict number parsing: plain ASCII digits and exact decimals.

use std::str::FromStr;
use std::time::Duration;

/// A whole number written in plain ASCII digits, e.g. `1488`.
pub(crate) fn natural<T: FromStr>(text: &str) -> Option<T> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// What to do with fraction digits finer than a nanosecond.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Finer {
    /// Treat them as an error, for what people write.
    Reject,
    /// Drop them, for what tools print.
    Truncate,
}

/// Parses `<digits>[.<digits>]` or `.<digits>` seconds exactly.
pub(crate) fn seconds(text: &str, finer: Finer) -> Option<Duration> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !digits(whole) || !digits(fraction) {
        return None;
    }
    let fraction = match (fraction.get(..9), finer) {
        (Some(nanos), Finer::Truncate) => nanos,
        (Some(nanos), Finer::Reject) if nanos.len() == fraction.len() => fraction,
        (Some(_), Finer::Reject) => return None,
        (None, _) => fraction,
    };
    let whole = if whole.is_empty() {
        Some(0)
    } else {
        natural(whole)
    }?;
    let nanos = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<9}").parse().ok()?
    };
    Some(Duration::new(whole, nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naturals_are_plain_digits() {
        assert_eq!(natural::<u32>("1488"), Some(1488));
        for bad in ["", "+5", "-5", "1.5", " 5", "x"] {
            assert_eq!(natural::<u32>(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn seconds_are_exact() {
        assert_eq!(
            seconds("8.233333", Finer::Reject),
            Some(Duration::new(8, 233_333_000))
        );
        assert_eq!(seconds("12", Finer::Reject), Some(Duration::from_secs(12)));
        assert_eq!(
            seconds(".5", Finer::Reject),
            Some(Duration::from_millis(500))
        );
        assert_eq!(seconds("5.", Finer::Reject), Some(Duration::from_secs(5)));
        for bad in ["", ".", "-1.0", "+1", "1.2.3", "1e3", "N/A"] {
            assert_eq!(seconds(bad, Finer::Truncate), None, "{bad:?}");
        }
    }

    #[test]
    fn finer_than_nanoseconds_is_rejected_or_truncated() {
        assert_eq!(seconds("1.0123456789", Finer::Reject), None);
        assert_eq!(
            seconds("1.0123456789", Finer::Truncate),
            Some(Duration::new(1, 12_345_678))
        );
    }
}
