use crate::{ProbeError, filesystem::parse_u64, units::digits};

const CGROUP_PATH_LIMIT: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbeCgroup {
    pub(crate) path: String,
    pub(crate) memory_max_bytes: Option<u64>,
    pub(crate) memory_current_bytes: u64,
    pub(crate) swap_max_bytes: Option<u64>,
    pub(crate) swap_current_bytes: u64,
    pub(crate) cpu_quota_micros: Option<u64>,
    pub(crate) cpu_period_micros: u64,
    pub(crate) pids_max: Option<u64>,
}

impl ProbeCgroup {
    pub(crate) fn parse(
        path: &str,
        memory: &[u8],
        memory_current: &[u8],
        swap: &[u8],
        swap_current: &[u8],
        cpu: &[u8],
        pids: &[u8],
    ) -> Result<Self, ProbeError> {
        if path.is_empty()
            || path.len() > CGROUP_PATH_LIMIT
            || !path.starts_with('/')
            || path.contains("..")
        {
            return Err(ProbeError::Invalid("probe_cgroup_path"));
        }
        let (cpu_quota_micros, cpu_period_micros) = parse_cpu(cpu)?;
        Ok(Self {
            path: path.to_owned(),
            memory_max_bytes: parse_limit(memory, "probe_cgroup_memory_max")?,
            memory_current_bytes: parse_value(memory_current, "probe_cgroup_memory_current")?,
            swap_max_bytes: parse_limit(swap, "probe_cgroup_swap_max")?,
            swap_current_bytes: parse_value(swap_current, "probe_cgroup_swap_current")?,
            cpu_quota_micros,
            cpu_period_micros,
            pids_max: parse_limit(pids, "probe_cgroup_pids_max")?,
        })
    }
}

fn token<'a>(input: &'a [u8], error: &'static str) -> Result<&'a str, ProbeError> {
    let value = std::str::from_utf8(input)
        .map_err(|_| ProbeError::Invalid(error))?
        .trim();
    if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(ProbeError::Invalid(error));
    }
    Ok(value)
}

fn parse_limit(input: &[u8], error: &'static str) -> Result<Option<u64>, ProbeError> {
    let value = token(input, error)?;
    if value == "max" {
        Ok(None)
    } else {
        Ok(Some(parse_u64(value, error)?))
    }
}

fn parse_value(input: &[u8], error: &'static str) -> Result<u64, ProbeError> {
    parse_u64(token(input, error)?, error)
}

fn parse_cpu(input: &[u8]) -> Result<(Option<u64>, u64), ProbeError> {
    let value = std::str::from_utf8(input)
        .map_err(|_| ProbeError::Invalid("probe_cgroup_cpu_max"))?
        .trim();
    let mut fields = value.split_ascii_whitespace();
    let quota = fields
        .next()
        .ok_or(ProbeError::Invalid("probe_cgroup_cpu_max"))?;
    let period = fields
        .next()
        .ok_or(ProbeError::Invalid("probe_cgroup_cpu_max"))?;
    if fields.next().is_some() || !digits(period) {
        return Err(ProbeError::Invalid("probe_cgroup_cpu_max"));
    }
    let period = parse_u64(period, "probe_cgroup_cpu_period")?;
    if period == 0 {
        return Err(ProbeError::Invalid("probe_cgroup_cpu_period"));
    }
    let quota = if quota == "max" {
        None
    } else {
        Some(parse_u64(quota, "probe_cgroup_cpu_quota")?)
    };
    Ok((quota, period))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_finite_and_unbounded_limits() -> Result<(), ProbeError> {
        let facts = ProbeCgroup::parse(
            "/runner/probe",
            b"4096\n",
            b"1024\n",
            b"max\n",
            b"0\n",
            b"200000 100000\n",
            b"max\n",
        )?;
        assert_eq!(facts.memory_max_bytes, Some(4096));
        assert_eq!(facts.memory_current_bytes, 1024);
        assert_eq!(facts.swap_max_bytes, None);
        assert_eq!(facts.cpu_quota_micros, Some(200_000));
        assert_eq!(facts.cpu_period_micros, 100_000);
        assert_eq!(facts.pids_max, None);
        Ok(())
    }

    #[test]
    fn rejects_missing_malformed_and_unsafe_path_facts() {
        assert!(
            ProbeCgroup::parse("/runner", b"", b"1", b"max", b"0", b"max 100", b"max").is_err()
        );
        assert!(
            ProbeCgroup::parse("/runner/../host", b"1", b"1", b"1", b"1", b"max 1", b"1").is_err()
        );
        assert!(ProbeCgroup::parse("/runner", b"1", b"1", b"1", b"1", b"max 0", b"1").is_err());
        assert!(
            ProbeCgroup::parse("/runner", b"1", b"1", b"1", b"1", b"max 1 extra", b"1").is_err()
        );
    }
}
