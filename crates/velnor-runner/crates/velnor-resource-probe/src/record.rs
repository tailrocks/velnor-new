use serde::Serialize;

/// Version-1 bounded output record. `None` serializes as the required JSON `null` value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeRecord {
    /// Protocol schema version.
    pub schema_version: u8,
    /// Guest Docker-root available bytes from checked `f_bavail * f_frsize`.
    pub docker_root_free_bytes: u64,
    /// Guest `MemAvailable`, converted from KiB to bytes.
    pub memory_available_bytes: u64,
    /// Guest one-minute load rounded upward to thousandths.
    pub load_milli: u64,
    /// Optional PSI `some avg10` in hundredths of a percent.
    pub memory_psi_some_avg10_bps: Option<u64>,
}

/// Maximum output record size, including its terminating newline.
pub const MAX_OUTPUT_BYTES: usize = 512;
