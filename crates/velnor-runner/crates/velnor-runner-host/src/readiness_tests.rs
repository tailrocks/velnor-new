//! An empty controller is waiting for credentials, never ready.

use crate::{
    Readiness, ResourceProbeInodePolicy, ResourceProbeReport, doctor_json, doctor_json_with_probe,
    readiness_for_empty, status_json,
};

#[test]
fn empty_controller_is_not_ready() {
    let state = readiness_for_empty();
    assert_eq!(state, Readiness::WaitingForCredentials);
    assert_eq!(state.as_str(), "waiting_for_credentials");
    assert_ne!(state, Readiness::Ready);
    let status = status_json(state);
    let probed = doctor_json(state, true);
    let quiet = doctor_json(state, false);
    assert_eq!(status, r#"{"state":"waiting_for_credentials"}"#);
    assert_eq!(
        probed,
        r#"{"state":"waiting_for_credentials","probe":true}"#
    );
    assert_eq!(
        quiet,
        r#"{"state":"waiting_for_credentials","probe":false}"#
    );
    assert!(!status.contains("\"ready\""));
    assert!(!probed.contains("\"ready\""));
    assert!(!quiet.contains("\"ready\""));
}

#[test]
fn doctor_report_contains_probe_facts_without_upgrading_readiness() -> Result<(), String> {
    let report = ResourceProbeReport {
        sample_generation_id: "a".repeat(32),
        guest_cpus: 8,
        memory_total_bytes: 16 * 1024 * 1024 * 1024,
        memory_available_bytes: 8 * 1024 * 1024 * 1024,
        load_milli: 125,
        docker_root_total_bytes: 40 * 1024 * 1024 * 1024,
        docker_root_free_bytes: 20 * 1024 * 1024 * 1024,
        docker_root_available_bytes: 19 * 1024 * 1024 * 1024,
        docker_root_inode_total: 0,
        docker_root_inode_free: 0,
        docker_root_inode_available: 0,
        docker_root_inode_policy: ResourceProbeInodePolicy::DynamicBtrfs,
        docker_root_fs_type_name: "btrfs".to_owned(),
        docker_root_filesystem_id: 17,
        memory_psi_some_avg10_bps: None,
    };
    let json = doctor_json_with_probe(Readiness::WaitingForCredentials, Some(&report))
        .map_err(|error| error.to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&json).map_err(|error| error.to_string())?;
    if value["state"] != "waiting_for_credentials"
        || value["probe"].is_boolean()
        || value["probe"]["sample_generation_id"] != "a".repeat(32)
        || value["probe"]["guest_cpus"] != 8
    {
        return Err("doctor JSON did not preserve readiness and actual probe facts".to_owned());
    }
    Ok(())
}
