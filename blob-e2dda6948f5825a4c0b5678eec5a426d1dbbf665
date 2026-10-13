use crate::{ProbeError, units::digits};

/// Parse the one required `MemAvailable: <decimal> kB` record and convert it to bytes.
pub(crate) fn available_bytes(input: &[u8]) -> Result<u64, ProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("meminfo_utf8"))?;
    let mut available = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key != "MemAvailable" {
            continue;
        }
        if available.is_some() {
            return Err(ProbeError::Invalid("meminfo_duplicate"));
        }
        let fields = value.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() != 2 || !digits(fields[0]) || fields[1] != "kB" {
            return Err(ProbeError::Invalid("meminfo_value"));
        }
        let kib = fields[0]
            .parse::<u64>()
            .map_err(|_| ProbeError::Overflow("meminfo"))?;
        available = Some(
            kib.checked_mul(1024)
                .ok_or(ProbeError::Overflow("meminfo"))?,
        );
    }
    available.ok_or(ProbeError::Invalid("meminfo_missing"))
}
