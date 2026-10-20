use super::fixture::fixture_record;

/// Encode a probe record that the parser accepts in Docker's stdout log format.
pub(in crate::launch) fn canonical_logs_response(
    probe_pid: u32,
    available_bytes: u64,
) -> Result<Vec<u8>, crate::error::HostError> {
    let mut record = fixture_record();
    record.probe_host_pid = probe_pid;
    record.probe_namespace_pid = probe_pid;
    record.docker_root_available_bytes = available_bytes;
    record.docker_root_free_bytes = record.docker_root_free_bytes.max(available_bytes);
    let mut body = serde_json::to_vec(&record).map_err(|_| crate::error::HostError::Frame)?;
    body.push(b'\n');
    let length = u32::try_from(body.len()).map_err(|_| crate::error::HostError::Frame)?;
    let mut response = Vec::with_capacity(8 + body.len());
    response.extend_from_slice(&[1, 0, 0, 0]);
    response.extend_from_slice(&length.to_be_bytes());
    response.extend_from_slice(&body);
    Ok(response)
}
