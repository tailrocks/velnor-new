use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeRecord {
    pub schema_version: u8,

    pub docker_root_free_bytes: u64,

    pub docker_root_total_bytes: u64,

    pub memory_available_bytes: u64,

    pub load_milli: u64,

    pub memory_psi_some_avg10_bps: Option<u64>,
}

pub const MAX_OUTPUT_BYTES: usize = 512;
