//! Detects blank or zero-instant assignment times.

pub(crate) fn is_empty(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    let Some(rest) = trimmed.strip_prefix("0001-01-01T00:00:00") else {
        return false;
    };
    let rest = rest
        .strip_prefix('.')
        .map_or(rest, |frac| frac.trim_start_matches('0'));
    matches!(rest, "" | "Z" | "z" | "+00:00" | "-00:00")
}
