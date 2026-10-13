use crate::ProbeError;

/// Parse a finite nonnegative decimal and round upward to thousandths.
pub(crate) fn decimal_milli_ceil(value: &str) -> Result<u64, ProbeError> {
    let (whole, fractional) = value
        .split_once('.')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    if !digits(whole) || fractional.is_some_and(|part| !digits(part) || part.len() > 128) {
        return Err(ProbeError::Invalid("decimal"));
    }
    let whole = whole
        .parse::<u64>()
        .map_err(|_| ProbeError::Overflow("decimal"))?;
    let base = whole
        .checked_mul(1000)
        .ok_or(ProbeError::Overflow("decimal"))?;
    let Some(fractional) = fractional else {
        return Ok(base);
    };
    let mut prefix = fractional
        .bytes()
        .take(3)
        .fold(0_u64, |value, digit| value * 10 + u64::from(digit - b'0'));
    for _ in fractional.len().min(3)..3 {
        prefix *= 10;
    }
    let remainder_nonzero = fractional.bytes().skip(3).any(|digit| digit != b'0');
    base.checked_add(prefix)
        .and_then(|value| value.checked_add(u64::from(remainder_nonzero)))
        .ok_or(ProbeError::Overflow("decimal"))
}

/// Parse a PSI decimal percentage into hundredths of one percent.
pub(crate) fn percent_basis_points(value: &str) -> Result<u64, ProbeError> {
    let (whole, fractional) = value
        .split_once('.')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    if !digits(whole) || fractional.is_some_and(|part| !digits(part) || part.len() > 2) {
        return Err(ProbeError::Invalid("percentage"));
    }
    let whole = whole
        .parse::<u64>()
        .map_err(|_| ProbeError::Overflow("percentage"))?;
    let fraction = match fractional {
        Some(part) => part
            .parse::<u64>()
            .map_err(|_| ProbeError::Overflow("percentage"))?,
        None => 0,
    };
    let fraction = if fractional.is_some_and(|part| part.len() == 1) {
        fraction * 10
    } else {
        fraction
    };
    let value = whole
        .checked_mul(100)
        .and_then(|whole| whole.checked_add(fraction))
        .ok_or(ProbeError::Overflow("percentage"))?;
    if value > 10_000 {
        return Err(ProbeError::Invalid("percentage_range"));
    }
    Ok(value)
}

pub(crate) fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}
