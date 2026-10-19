use super::{ControlState, handle_request};
use crate::config::{HostConfig, RepositoryConnection};
use crate::config_test_support::{basic_connection, host_config};
use crate::controller_ipc::wire::Request;
use crate::journal::Journal;
use std::fs;
use std::path::PathBuf;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

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
    let state = ControlState::empty();
    let response = handle_request(
        &journal,
        &state,
        &state_path,
        Request::Detached {
            binding,
            token: TOKEN.to_owned(),
        },
        "engine",
        &"a".repeat(64),
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
    let state = ControlState::empty();
    let request = Request::Reactivate {
        binding,
        token: TOKEN.to_owned(),
    };
    for attempt in 0..2 {
        let response = handle_request(
            &journal,
            &state,
            &state_path,
            request.clone(),
            "engine",
            &"a".repeat(64),
        )
        .await
        .map_err(|error| format!("reactivate attempt {attempt}: {error}"))?;
        if response.value != Some(true) {
            return Err(format!("reactivate attempt {attempt} was not acknowledged"));
        }
    }
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn doctor_probe_start_and_status_use_the_durable_ticket() -> Result<(), String> {
    let state_path = test_state("doctor-probe-ipc");
    fs::create_dir_all(&state_path).map_err(|error| error.to_string())?;
    let journal = Journal::open(&state_path.join("launch.db"))
        .await
        .map_err(|error| format!("open: {error}"))?;
    journal
        .bind_engine("doctor-engine")
        .await
        .map_err(|error| format!("bind engine: {error}"))?;
    let state = ControlState::empty();
    let nonce = "1".repeat(32);
    let started = handle_request(
        &journal,
        &state,
        &state_path,
        Request::DoctorProbeStart {
            nonce: nonce.clone(),
        },
        "doctor-engine",
        &"a".repeat(64),
    )
    .await
    .map_err(|error| format!("start request: {error}"))?;
    let started = started.doctor_probe.ok_or("start response is missing")?;
    if started.state != "queued" || started.request_id.is_none() {
        return Err("start response did not return the durable queued ticket".to_owned());
    }
    let status = handle_request(
        &journal,
        &state,
        &state_path,
        Request::DoctorProbeStatus { nonce },
        "doctor-engine",
        &"a".repeat(64),
    )
    .await
    .map_err(|error| format!("status request: {error}"))?;
    let status = status.doctor_probe.ok_or("status response is missing")?;
    if status.state != "queued"
        || status.request_id != started.request_id
        || status.report.is_some()
    {
        return Err("status did not return the same queued ticket".to_owned());
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
