use super::{ControlState, handle_doctor_probe, handle_request};
use crate::config::{HostConfig, RepositoryConnection};
use crate::config_test_support::{basic_connection, host_config};
use crate::controller_ipc::wire::Request;
use crate::journal::Journal;
use crate::readiness::{ResourceProbeInodePolicy, ResourceProbeReport};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

#[tokio::test]
async fn doctor_probe_returns_typed_completed_sample_without_a_boolean_success()
-> Result<(), String> {
    let (sender, mut requests) = mpsc::channel(1);
    let handler = tokio::spawn(async move { handle_doctor_probe(&sender).await });
    let request = requests
        .recv()
        .await
        .ok_or_else(|| "doctor request was not forwarded to the controller".to_owned())?;
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
    request.respond(Some(report.clone()));
    let response = handler
        .await
        .map_err(|error| format!("doctor request handler failed: {error}"))?
        .map_err(|error| format!("doctor request failed: {error}"))?;
    if response.value.is_some() || response.resource_probe != Some(report) {
        return Err("doctor response did not carry only the typed probe result".to_owned());
    }
    let bytes = response
        .clone()
        .encode()
        .map_err(|()| "typed doctor response did not encode within its bound".to_owned())?;
    let decoded = crate::controller_ipc::wire::Response::decode(&bytes)
        .map_err(|()| "typed doctor response did not decode".to_owned())?;
    if decoded.resource_probe != response.resource_probe {
        return Err("typed doctor response changed on the wire".to_owned());
    }
    Ok(())
}

#[tokio::test]
async fn detached_is_provable_while_the_host_is_draining() -> Result<(), String> {
    let state_path = test_state("global-drain-detached");
    fs::create_dir_all(&state_path).map_err(|error| error.to_string())?;
    fs::create_dir_all(state_path.join("connection-drain")).map_err(|error| error.to_string())?;
    fs::write(state_path.join("drain"), TOKEN).map_err(|error| error.to_string())?;
    fs::write(state_path.join("connection-drain/essential"), TOKEN)
        .map_err(|error| error.to_string())?;

    let binding = test_binding()?;
    let journal = Journal::open(&state_path.join("launch.db"))
        .await
        .map_err(|error| format!("open: {error}"))?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| format!("scope: {error}"))?;
    if !scope
        .retire_if_drained()
        .await
        .map_err(|error| format!("retire: {error}"))?
    {
        return Err("empty connection did not retire".to_owned());
    }
    let state = ControlState(Arc::new(RwLock::new(Vec::new())));
    let response = handle_request(
        &journal,
        &state,
        &state_path,
        Request::Detached {
            binding,
            token: TOKEN.to_owned(),
        },
    )
    .await
    .map_err(|error| format!("detach request: {error}"))?;
    if response.value != Some(true) {
        return Err("global drain incorrectly blocked exact detach proof".to_owned());
    }
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn reactivation_retries_after_crash_before_config_persist() -> Result<(), String> {
    let state_path = test_state("reactivation-retry");
    fs::create_dir_all(state_path.join("connection-drain")).map_err(|error| error.to_string())?;
    fs::write(state_path.join("connection-drain/essential"), TOKEN)
        .map_err(|error| error.to_string())?;
    let binding = test_binding()?;
    let journal = Journal::open(&state_path.join("launch.db"))
        .await
        .map_err(|error| format!("open: {error}"))?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| format!("scope: {error}"))?;
    if !scope
        .retire_if_drained()
        .await
        .map_err(|error| format!("retire: {error}"))?
    {
        return Err("empty connection did not retire".to_owned());
    }
    let state = ControlState(Arc::new(RwLock::new(Vec::new())));
    let request = Request::Reactivate {
        binding,
        token: TOKEN.to_owned(),
    };
    for attempt in 0..2 {
        let response = handle_request(&journal, &state, &state_path, request.clone())
            .await
            .map_err(|error| format!("reactivate attempt {attempt}: {error}"))?;
        if response.value != Some(true) {
            return Err(format!("reactivate attempt {attempt} was not acknowledged"));
        }
    }
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())?;
    Ok(())
}

fn test_binding() -> Result<RepositoryConnection, String> {
    let connection = basic_connection(
        "essential",
        101,
        "donbeave/essential-mac",
        "essential",
        "essential-scale-set",
    );
    let text = host_config(&[connection], 1);
    let config = HostConfig::parse(&text).map_err(|error| error.to_string())?;
    let binding = config
        .connections()
        .first()
        .cloned()
        .ok_or_else(|| "test config had no binding".to_owned())?;
    Ok(binding)
}

fn test_state(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-control-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}
