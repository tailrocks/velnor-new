//! Validate the sealed host cleanup proof before journal acceptance.

use std::path::Path;

use crate::error::HostError;

use super::{CleanupDisposition, CleanupRecord, PhysicalCleanupProof, PostActionDisposition};

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub(super) fn from_proof<P: PhysicalCleanupProof>(proof: &P) -> Result<CleanupRecord, HostError> {
    validate_identity(proof)?;
    validate_evidence(proof)?;
    let post_actions = proof.post_actions();
    let cleanup = proof.cleanup_disposition();
    let (post_action_state, post_action_reason) = post_action_fields(&post_actions);
    let (cleanup_state, cleanup_reason) = cleanup_fields(&cleanup);
    Ok(CleanupRecord {
        launch_id: proof.launch_id(),
        expected_runner_name: proof.expected_runner_name().to_owned(),
        worker_volume: proof.worker_volume().to_owned(),
        runner_id: proof.runner_container_id().to_owned(),
        dind_id: proof.dind_container_id().to_owned(),
        outer_network_name: proof.outer_network_name().map(str::to_owned),
        outer_network_id: proof.outer_network_id().map(str::to_owned),
        observed_workflow_run_id: proof.observed_workflow_run_id(),
        observed_attempt: proof.observed_attempt(),
        observed_job_id: proof.observed_job_id().map(str::to_owned),
        observed_actions_job_id: proof.observed_actions_job_id(),
        observed_runner_id: proof.observed_runner_id(),
        observed_runner_name: proof.observed_runner_name().map(str::to_owned),
        diagnostics_path: proof.diagnostics_relative_path().to_owned(),
        diagnostics_sha256: proof.diagnostics_sha256().to_owned(),
        diagnostics_bytes: i64::try_from(proof.diagnostics_bytes())
            .map_err(|_| HostError::Journal)?,
        diagnostics_source_absent: proof.diagnostics_source_absent(),
        runner_start_observation: proof.runner_start_observation(),
        post_action_state,
        post_action_reason,
        cleanup_state,
        cleanup_reason,
        cleanup_resources: encode_resources(proof)?,
    })
}

fn validate_identity<P: PhysicalCleanupProof>(proof: &P) -> Result<(), HostError> {
    if proof.launch_id() <= 0
        || !safe_token(proof.expected_runner_name(), 64)
        || !safe_token(proof.worker_volume(), 128)
        || !container_id(proof.runner_container_id())
        || !container_id(proof.dind_container_id())
        || !proof.launch_fenced()
        || !proof.all_owned_children_networks_and_volumes_absent()
        || !proof.outer_network_absent()
    {
        return Err(HostError::Journal);
    }
    match (proof.outer_network_name(), proof.outer_network_id()) {
        (Some(name), Some(id)) if safe_token(name, 128) && container_id(id) => Ok(()),
        (None, None) => Ok(()),
        _ => Err(HostError::Journal),
    }
}

fn validate_evidence<P: PhysicalCleanupProof>(proof: &P) -> Result<(), HostError> {
    let event_fields = [
        proof.observed_workflow_run_id().is_some(),
        proof.observed_job_id().is_some(),
        proof.observed_runner_id().is_some(),
        proof.observed_runner_name().is_some(),
    ];
    let event_complete = event_fields.iter().all(|present| *present);
    let event_absent = event_fields.iter().all(|present| !present);
    let rest_fields = [
        proof.observed_attempt().is_some(),
        proof.observed_actions_job_id().is_some(),
    ];
    let rest_complete = rest_fields.iter().all(|present| *present);
    let rest_absent = rest_fields.iter().all(|present| !present);
    let diagnostics_path = proof.diagnostics_relative_path();
    let expected_path = format!("launch-{}/runner-diagnostics.tar", proof.launch_id());
    if !proof.diagnostics_redacted()
        || !proof.diagnostics_retained()
        || !valid_digest(proof.diagnostics_sha256())
        || !valid_diagnostics_path(proof.diagnostics_relative_path())
        || (!event_complete && !event_absent)
        || (!rest_complete && !rest_absent)
        || !diagnostics_path.is_empty() && diagnostics_path != expected_path
        || proof
            .observed_workflow_run_id()
            .is_some_and(|value| value <= 0)
        || proof.observed_attempt().is_some_and(|value| value <= 0)
        || proof
            .observed_actions_job_id()
            .is_some_and(|value| value <= 0)
        || proof.observed_runner_id().is_some_and(|value| value <= 0)
        || proof
            .observed_job_id()
            .is_some_and(|value| !valid_opaque(value, 256))
        || proof
            .observed_runner_name()
            .is_some_and(|value| !safe_token(value, 64))
        || proof
            .observed_runner_name()
            .is_some_and(|value| value != proof.expected_runner_name())
    {
        return Err(HostError::Journal);
    }
    validate_dispositions(proof)
}

fn validate_dispositions<P: PhysicalCleanupProof>(proof: &P) -> Result<(), HostError> {
    let post_actions = proof.post_actions();
    let cleanup = proof.cleanup_disposition();
    let not_run = matches!(&post_actions, PostActionDisposition::NotRun);
    let empty_receipt = proof.diagnostics_relative_path().is_empty()
        && proof.diagnostics_bytes() == 0
        && proof.diagnostics_sha256() == EMPTY_SHA256;
    if not_run != empty_receipt
        || not_run != proof.diagnostics_source_absent()
        || (not_run
            && proof.runner_start_observation() != super::RunnerStartObservation::NeverStarted)
    {
        return Err(HostError::Journal);
    }
    if not_run
        && (proof.observed_workflow_run_id().is_some()
            || proof.observed_job_id().is_some()
            || proof.observed_runner_id().is_some()
            || proof.observed_runner_name().is_some())
    {
        return Err(HostError::Journal);
    }
    if let PostActionDisposition::Interrupted { reason_class } = &post_actions
        && !reason_class_token(reason_class)
    {
        return Err(HostError::Journal);
    }
    if let CleanupDisposition::Interrupted { reason_class } = &cleanup
        && !reason_class_token(reason_class)
    {
        return Err(HostError::Journal);
    }
    match (cleanup, post_actions) {
        (
            CleanupDisposition::Completed,
            PostActionDisposition::Completed | PostActionDisposition::NotRun,
        )
        | (CleanupDisposition::Interrupted { .. }, _) => Ok(()),
        _ => Err(HostError::Journal),
    }
}

fn encode_resources<P: PhysicalCleanupProof>(proof: &P) -> Result<String, HostError> {
    let mut resources = Vec::new();
    for (prefix, values) in [
        ("container", proof.removed_child_container_ids()),
        ("network", proof.removed_child_network_ids()),
        ("volume", proof.absent_volume_names()),
    ] {
        if values.len() > 256 || values.iter().any(|value| !safe_token(value, 256)) {
            return Err(HostError::Journal);
        }
        resources.extend(values.iter().map(|value| format!("{prefix}:{value}")));
    }
    resources.sort_unstable();
    if resources.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(HostError::Journal);
    }
    let encoded = resources.join("\n");
    if encoded.len() > 64 * 1024 {
        return Err(HostError::Journal);
    }
    Ok(encoded)
}

fn container_id(value: &str) -> bool {
    (12..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_opaque(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn reason_class_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn post_action_fields(value: &PostActionDisposition) -> (&'static str, Option<String>) {
    match value {
        PostActionDisposition::Completed => ("completed", None),
        PostActionDisposition::NotRun => ("not_run", None),
        PostActionDisposition::Interrupted { reason_class } => {
            ("interrupted", Some(reason_class.clone()))
        }
        PostActionDisposition::Unknown => ("unknown", None),
    }
}

fn cleanup_fields(value: &CleanupDisposition) -> (&'static str, Option<String>) {
    match value {
        CleanupDisposition::Completed => ("completed", None),
        CleanupDisposition::Interrupted { reason_class } => {
            ("interrupted", Some(reason_class.clone()))
        }
    }
}

fn safe_token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_diagnostics_path(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    if value.len() > 512 || Path::new(value).is_absolute() || value.contains('\\') {
        return false;
    }
    value.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    })
}
