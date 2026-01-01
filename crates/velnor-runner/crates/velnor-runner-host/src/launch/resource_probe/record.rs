//! Strict schema-1 output parsing for the isolated guest probe.

use serde::{Deserialize, Serialize};

const MAX_OUTPUT_BYTES: usize = 512;
const SCHEMA_VERSION: u8 = 1;
const MAX_PSI_BPS: u64 = 10_000;

/// One validated output record. It is kept in memory for the current attempt.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProbeRecord {
    /// Producer protocol version.
    pub(super) schema_version: u8,
    /// Available bytes on the selected engine's Docker root filesystem.
    pub(super) docker_root_free_bytes: u64,
    /// Total bytes on the selected engine's Docker root filesystem.
    pub(super) docker_root_total_bytes: u64,
    /// Guest memory available to new work.
    pub(super) memory_available_bytes: u64,
    /// One-minute guest load average multiplied by 1000.
    pub(super) load_milli: u32,
    /// Optional diagnostic PSI value in basis points.
    pub(super) memory_psi_some_avg10_bps: Option<u64>,
}

impl ProbeRecord {
    /// Parse canonical one-line JSON and bind its memory value to daemon info.
    pub(super) fn parse(output: &[u8], memory_total_bytes: u64) -> Option<Self> {
        if !valid_framing(output) || memory_total_bytes == 0 {
            return None;
        }
        let body = output.get(..output.len().checked_sub(1)?)?;
        let mut deserializer = serde_json::Deserializer::from_slice(body);
        let record = Self::deserialize(&mut deserializer).ok()?;
        deserializer.end().ok()?;
        (record.valid_values(memory_total_bytes) && canonical_body_matches(&record, body))
            .then_some(record)
    }

    fn valid_values(&self, memory_total_bytes: u64) -> bool {
        self.schema_version == SCHEMA_VERSION
            && self.docker_root_total_bytes > 0
            && self.docker_root_free_bytes <= self.docker_root_total_bytes
            && self.memory_available_bytes <= memory_total_bytes
            && self
                .memory_psi_some_avg10_bps
                .is_none_or(|value| value <= MAX_PSI_BPS)
    }
}

fn canonical_body_matches(record: &ProbeRecord, body: &[u8]) -> bool {
    serde_json::to_vec(record).is_ok_and(|canonical| canonical == body)
}

fn valid_framing(output: &[u8]) -> bool {
    output.len() >= 2
        && output.len() <= MAX_OUTPUT_BYTES
        && output.last() == Some(&b'\n')
        && output[..output.len() - 1].iter().all(|byte| *byte != b'\n')
}
