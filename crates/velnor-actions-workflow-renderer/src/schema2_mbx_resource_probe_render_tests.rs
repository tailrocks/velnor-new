use super::super::mbx_qualification_helpers::{base64_encode, verify_embedded_payload};
use super::{
    PATH_VALIDATION, PATH_VALIDATION_SHA256, SAMPLER, SAMPLER_SHA256, START_SCRIPT, STOP_SCRIPT,
};

#[test]
fn sampler_source_keeps_measurements_bounded_and_labeled_as_lower_bounds() {
    assert!(SAMPLER.contains("observed_max_filesystem_used_bytes_lower_bound"));
    assert!(SAMPLER.contains("observed_max_filesystem_used_inodes_lower_bound"));
    assert!(SAMPLER.contains("inode_allocated_bytes_sum_dedup_within_tree"));
    assert!(SAMPLER.contains("st_blocks*512 once per dev/inode"));
    assert!(SAMPLER.contains("reflink/COW extent sharing is unknown"));
    assert!(SAMPLER.contains("GITHUB_ENV file not read"));
    assert!(SAMPLER.contains("max_files=20000"));
    assert!(SAMPLER.contains("max_hash_bytes=$((1024 * 1024 * 1024))"));
}

#[test]
fn shutdown_uses_monotonic_deadlines_for_external_process_work() {
    assert!(PATH_VALIDATION.contains("< /proc/uptime"));
    assert!(PATH_VALIDATION.contains("timeout --signal=KILL"));
    assert!(PATH_VALIDATION.contains("RESOURCE_SESSION_SCAN_LIMIT=4096"));
    assert!(PATH_VALIDATION.contains("resource_deadline_capture_failed \"$?\""));
    assert!(PATH_VALIDATION.contains("resource_deadline_capture_failed \"$?\"; return 1; }"));
    assert!(PATH_VALIDATION.contains("wait_for_owned_session_until_deadline"));
    assert!(!PATH_VALIDATION.contains("$(owned_session_member_count"));
    assert!(STOP_SCRIPT.contains("shutdown_deadline_status"));
    assert!(STOP_SCRIPT.contains("shutdown_elapsed_centiseconds"));
    assert!(STOP_SCRIPT.contains("[ \"${RESOURCE_SHUTDOWN_STATUS:-}\" = within_budget ]"));
}

#[test]
fn embedded_measurement_payloads_decode_and_match_their_sha256_pins() {
    let sampler = base64_encode(SAMPLER.as_bytes());
    let path_validation = base64_encode(PATH_VALIDATION.as_bytes());
    let start = START_SCRIPT
        .replace("__SAMPLER_BASE64__", &sampler)
        .replace("__SAMPLER_SHA256__", SAMPLER_SHA256)
        .replace("__PATH_VALIDATION_BASE64__", &path_validation)
        .replace("__PATH_VALIDATION_SHA256__", PATH_VALIDATION_SHA256);
    assert!(!start.contains("__SAMPLER_"));
    assert!(!start.contains("__PATH_VALIDATION_"));
    assert!(start.contains(&sampler));
    assert!(start.contains(&path_validation));
    verify_embedded_payload(SAMPLER, &sampler, SAMPLER_SHA256);
    verify_embedded_payload(PATH_VALIDATION, &path_validation, PATH_VALIDATION_SHA256);
}
