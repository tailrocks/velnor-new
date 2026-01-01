use crate::{ProbeError, units::decimal_milli_ceil};

/// Parse standard five-field `/proc/loadavg` and return the one-minute load.
pub(crate) fn one_minute_milli(input: &[u8]) -> Result<u64, ProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("loadavg_utf8"))?;
    let fields = text.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() != 5 {
        return Err(ProbeError::Invalid("loadavg_fields"));
    }
    let _five_minute = decimal_milli_ceil(fields[1])?;
    let _fifteen_minute = decimal_milli_ceil(fields[2])?;
    parse_running_counts(fields[3])?;
    parse_integer(fields[4])?;
    decimal_milli_ceil(fields[0])
}

fn parse_running_counts(value: &str) -> Result<(), ProbeError> {
    let Some((running, total)) = value.split_once('/') else {
        return Err(ProbeError::Invalid("loadavg_counts"));
    };
    parse_integer(running)?;
    parse_integer(total)?;
    Ok(())
}

fn parse_integer(value: &str) -> Result<u64, ProbeError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProbeError::Invalid("loadavg_integer"));
    }
    value
        .parse::<u64>()
        .map_err(|_| ProbeError::Overflow("loadavg_integer"))
}
