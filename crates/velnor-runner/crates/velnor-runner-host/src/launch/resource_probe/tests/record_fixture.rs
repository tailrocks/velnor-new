use super::super::record::fixture_record;

pub(super) const MEMORY_TOTAL: u64 = 8 * 1024 * 1024 * 1024;
pub(super) const GIB: u64 = 1024 * 1024 * 1024;

pub(super) fn record(psi: &str) -> Vec<u8> {
    record_values(5, 20 * GIB, 40 * GIB, 8 * GIB, 125, psi)
}

pub(super) fn record_values(
    version: u8,
    free: u64,
    total: u64,
    memory: u64,
    load: u64,
    psi: &str,
) -> Vec<u8> {
    let mut record = fixture_record();
    record.schema_version = version;
    record.docker_root_free_bytes = free;
    record.docker_root_total_bytes = total;
    record.docker_root_available_bytes = free.min(total).min(19 * GIB);
    record.memory_available_bytes = memory;
    record.load_milli = load;
    record.memory_psi_some_avg10_bps = if psi == "null" {
        None
    } else {
        psi.parse().ok()
    };
    let mut bytes = serde_json::to_vec(&record).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}
