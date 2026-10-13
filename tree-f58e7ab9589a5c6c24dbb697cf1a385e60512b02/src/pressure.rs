use crate::{ProbeError, units::percent_basis_points};

/// Parse PSI output. Any missing, malformed, unsupported, or duplicate record is nonfatal.
pub(crate) fn memory_some_avg10(input: &[u8]) -> Option<u64> {
    parse_pressure(input).ok()
}

fn parse_pressure(input: &[u8]) -> Result<u64, ProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("psi_utf8"))?;
    let mut some = None;
    let mut full_seen = false;
    for line in text.lines() {
        let mut fields = line.split_ascii_whitespace();
        let kind = fields.next().ok_or(ProbeError::Invalid("psi_line"))?;
        if kind != "some" && kind != "full" {
            return Err(ProbeError::Invalid("psi_kind"));
        }
        let mut avg10 = None;
        let mut avg60 = false;
        let mut avg300 = false;
        let mut total = false;
        for field in fields {
            let Some((key, value)) = field.split_once('=') else {
                return Err(ProbeError::Invalid("psi_field"));
            };
            match key {
                "avg10" if avg10.is_none() => avg10 = Some(percent_basis_points(value)?),
                "avg60" if !avg60 => {
                    percent_basis_points(value)?;
                    avg60 = true;
                }
                "avg300" if !avg300 => {
                    percent_basis_points(value)?;
                    avg300 = true;
                }
                "total" if !total => {
                    parse_integer(value)?;
                    total = true;
                }
                _ => return Err(ProbeError::Invalid("psi_duplicate_or_unknown_field")),
            }
        }
        if avg10.is_none() || !avg60 || !avg300 || !total {
            return Err(ProbeError::Invalid("psi_missing_field"));
        }
        if kind == "some" {
            if some.is_some() {
                return Err(ProbeError::Invalid("psi_duplicate_some"));
            }
            some = avg10;
        } else if full_seen {
            return Err(ProbeError::Invalid("psi_duplicate_full"));
        } else {
            full_seen = true;
        }
    }
    some.ok_or(ProbeError::Invalid("psi_missing_some"))
}

fn parse_integer(value: &str) -> Result<u64, ProbeError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProbeError::Invalid("psi_total"));
    }
    value
        .parse::<u64>()
        .map_err(|_| ProbeError::Overflow("psi_total"))
}
