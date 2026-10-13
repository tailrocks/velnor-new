//! Require a complete durable checkpoint chain before accepting physical cleanup.

use crate::error::HostError;

use super::super::CleanupRecord;
use super::read::{cleanup_children, cleanup_row, step_completed};

const REQUIRED_STEPS: [&str; 8] = [
    "runner-termination",
    "diagnostics-retention",
    "child-enumeration",
    "children-drained",
    "dind-termination",
    "runner-removal",
    "dind-removal",
    "volume-removal",
];

pub(super) async fn validate_ready(
    conn: &turso::Connection,
    proof: &CleanupRecord,
) -> Result<(), HostError> {
    validate_checkpoint(conn, proof, false).await
}

pub(super) async fn validate_completed(
    conn: &turso::Connection,
    proof: &CleanupRecord,
) -> Result<(), HostError> {
    validate_checkpoint(conn, proof, true).await
}

async fn validate_checkpoint(
    conn: &turso::Connection,
    proof: &CleanupRecord,
    complete: bool,
) -> Result<(), HostError> {
    let row = cleanup_row(conn, proof.launch_id)
        .await?
        .ok_or(HostError::Journal)?;
    if row.complete != complete
        || row.post_state != proof.post_action_state
        || row.post_reason != proof.post_action_reason
        || row.outer_network_name != proof.outer_network_name
        || row.outer_network_id != proof.outer_network_id
        || !row.children_drained
        || !row.diagnostics_recorded
        || row.diagnostics_relative_path.as_deref() != Some(proof.diagnostics_path.as_str())
        || row.diagnostics_sha256.as_deref() != Some(proof.diagnostics_sha256.as_str())
        || row.diagnostics_bytes != Some(proof.diagnostics_bytes)
        || row.diagnostics_redacted != Some(1)
        || row.diagnostics_retained != Some(1)
        || row.diagnostics_source_absent != Some(i64::from(proof.diagnostics_source_absent))
        || row.runner_start_observation != Some(proof.runner_start_observation)
    {
        return Err(HostError::Journal);
    }
    for key in REQUIRED_STEPS {
        if !step_completed(conn, proof.launch_id, key).await? {
            return Err(HostError::Journal);
        }
    }
    if proof.outer_network_name.is_some()
        && !step_completed(conn, proof.launch_id, "outer-network-removal").await?
    {
        return Err(HostError::Journal);
    }
    let children = cleanup_children(conn, proof.launch_id).await?;
    let mut expected = Vec::new();
    for (kind, values) in [
        ("container", &children.containers),
        ("network", &children.networks),
    ] {
        for value in values {
            let step = format!("child-{kind}:{value}");
            if !step_completed(conn, proof.launch_id, &step).await? {
                return Err(HostError::Journal);
            }
            expected.push(format!("{kind}:{value}"));
        }
    }
    expected.extend(volume_names(&proof.worker_volume)?);
    expected.sort_unstable();
    if proof.cleanup_resources.lines().collect::<Vec<_>>()
        != expected.iter().map(String::as_str).collect::<Vec<_>>()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn volume_names(base: &str) -> Result<Vec<String>, HostError> {
    if base.is_empty()
        || base.len() > 128
        || !base
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(HostError::Journal);
    }
    Ok([
        base.to_owned(),
        format!("{base}-work"),
        format!("{base}-externals"),
        format!("{base}-docker"),
        format!("{base}-home"),
        format!("{base}-tmp"),
    ]
    .into_iter()
    .map(|name| format!("volume:{name}"))
    .collect())
}
