use super::{ControlState, handle_request};
use crate::config::{HostConfig, RepositoryConnection};
use crate::config_test_support::{basic_connection, host_config};
use crate::controller_ipc::wire::Request;
use crate::journal::Journal;
use std::fs;
use std::path::PathBuf;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

#[path = "server/doctor_report_tests.rs"]
mod doctor_report_tests;
#[path = "server/readiness_tests.rs"]
mod readiness_tests;
#[path = "server/session_observation_tests.rs"]
mod session_observation_tests;
#[path = "server/setup_receipt_tests.rs"]
mod setup_receipt_tests;

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

#[test]
fn poll_receipts_require_every_exact_binding_and_config_replacement_clears_them()
-> Result<(), String> {
    let binding = test_binding()?;
    let connection = basic_connection(
        "essential",
        101,
        "donbeave/essential-mac",
        "essential",
        "essential-scale-set",
    );
    let text = host_config(&[connection], 1);
    let config = HostConfig::parse(&text).map_err(|error| error.to_string())?;
    let state = ControlState::new(&config);
    if state
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("The controller reported sessions before a successful poll.".to_owned());
    }
    state
        .record_session_poll(&binding, 17, 23, "session-1")
        .map_err(|error| error.to_string())?;
    let receipts = state
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The controller omitted a successful session poll.".to_owned())?;
    if receipts.len() != 1
        || receipts[0].connection_id != binding.connection_id.as_str()
        || receipts[0].scale_set_id != 17
        || receipts[0].session_intent_id != 23
        || receipts[0].session_id != "session-1"
        || receipts[0].generation != 1
    {
        return Err("The poll receipt did not retain the exact session identity.".to_owned());
    }
    let first_observed_at = receipts[0].observed_at;
    state
        .record_session_poll(&binding, 17, 23, "session-1")
        .map_err(|error| error.to_string())?;
    let receipts = state
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The controller omitted its next successful poll.".to_owned())?;
    if receipts[0].generation != 2 || receipts[0].observed_at < first_observed_at {
        return Err("Successful polls did not advance the recorded generation.".to_owned());
    }
    let successful_observation = receipts[0].observed_at;
    assert_pending_poll_expiry_and_replacement(&state, &binding, &config, successful_observation)
}

fn assert_pending_poll_expiry_and_replacement(
    state: &ControlState,
    binding: &RepositoryConnection,
    config: &HostConfig,
    successful_observation: std::time::Instant,
) -> Result<(), String> {
    state
        .begin_session_poll(
            binding,
            17,
            23,
            "session-1",
            std::time::Instant::now() + std::time::Duration::from_millis(20),
        )
        .map_err(|error| error.to_string())?;
    let pending = state
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "A bounded same-session poll dropped its last successful receipt.".to_owned()
        })?;
    if pending[0].generation != 2 || pending[0].observed_at != successful_observation {
        return Err("Starting a poll refreshed the previous receipt timestamp.".to_owned());
    }
    std::thread::sleep(std::time::Duration::from_millis(25));
    if state
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("An expired poll attempt kept its prior receipt valid.".to_owned());
    }
    state
        .invalidate_if_session_changed(binding, 17, 23, "replacement-session")
        .map_err(|error| error.to_string())?;
    if state
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("The controller retained a replaced session receipt.".to_owned());
    }
    state
        .record_session_poll(binding, 17, 23, "replacement-session")
        .map_err(|error| error.to_string())?;
    state
        .replace_config(config)
        .map_err(|error| error.to_string())?;
    if state
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("The controller retained receipts after configuration replacement.".to_owned());
    }
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
    let readiness = status.readiness;
    let status = status.doctor_probe.ok_or("status response is missing")?;
    if status.state != "queued"
        || status.request_id != started.request_id
        || status.report.is_some()
        || readiness != Some(crate::readiness::Readiness::Reconciling)
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
