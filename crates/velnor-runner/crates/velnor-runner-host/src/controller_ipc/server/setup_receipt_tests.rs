use std::fs;
use std::time::{Duration, Instant};

use crate::config::{HostConfig, RepositoryConnection};
use crate::config_test_support::{basic_connection, host_config};
use crate::connect::SetOwnership;
use crate::controller_ipc::SetupReceiptInput;
use crate::controller_ipc::wire::{Request, SessionObservationStatus, SetupReceiptStatus};
use crate::journal::Journal;
use crate::launch::steady::PollObservation;
use crate::scale_set::EnsuredSet;
use velnor_runner_github::RunnerGroup;

use super::super::{ControlState, handle_request};

const ENGINE: &str = "engine-setup";
const ROOT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NONCE: &str = "0123456789abcdef0123456789abcdef";
const CAPTURED_AT_UNIX_MS: u64 = 1_700_000_000_000;

#[test]
fn setup_receipt_retains_ordered_groups_and_selected_set_from_one_snapshot() -> Result<(), String> {
    let binding = binding()?;
    let config = config()?;
    let state = ControlState::new(&config);
    state.set_admission_ready(true);
    let setup_input = input(&binding, "default group");
    state
        .replace_setup_receipts(vec![setup_input.clone()])
        .map_err(|error| error.to_string())?;

    let receipt = state
        .setup_receipt_status(binding.connection_id.as_str(), ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if receipt.status != SetupReceiptStatus::Ready {
        return Err("controller did not expose its verified setup snapshot".to_owned());
    }
    let snapshot = receipt.setup.ok_or("ready receipt omitted setup")?;
    if snapshot.revision != 1
        || snapshot.captured_at_unix_ms != CAPTURED_AT_UNIX_MS
        || snapshot.runner_groups.len() != 2
        || snapshot.runner_groups[0].name != "secondary"
        || snapshot.runner_groups[0].is_default
        || snapshot.runner_groups[1].name != "default group"
        || !snapshot.runner_groups[1].is_default
        || snapshot.selected_set.id != 17
        || snapshot.selected_set.name != binding.scale_set.name()
        || snapshot.selected_set.runner_group_id != 7
        || !snapshot.selected_set.disable_update
        || snapshot.selected_set.labels != binding.scale_set.labels()
    {
        return Err("setup receipt changed the ordered group or selected set facts".to_owned());
    }
    state
        .replace_setup_receipts(vec![setup_input])
        .map_err(|error| error.to_string())?;
    let republished = state
        .setup_receipt_status(binding.connection_id.as_str(), ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if republished
        .setup
        .is_none_or(|setup| setup.captured_at_unix_ms != CAPTURED_AT_UNIX_MS)
    {
        return Err("republishing setup facts changed their capture time".to_owned());
    }
    Ok(())
}

#[test]
fn setup_binding_mismatch_and_admission_loss_fail_closed() -> Result<(), String> {
    let binding = binding()?;
    let config = config()?;
    let state = ControlState::new(&config);
    let mut invalid = input(&binding, "default group");
    invalid.set.name.push_str("-mismatch");
    assert!(state.replace_setup_receipts(vec![invalid]).is_err());
    state.set_admission_ready(true);
    state
        .replace_setup_receipts(vec![input(&binding, "default group")])
        .map_err(|error| error.to_string())?;
    state.set_admission_ready(false);
    let receipt = state
        .setup_receipt_status(binding.connection_id.as_str(), ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if receipt.status != SetupReceiptStatus::Stale || receipt.setup.is_some() {
        return Err("admission loss retained a readable setup receipt".to_owned());
    }
    state.set_admission_ready(true);
    let after_recovery = state
        .setup_receipt_status(binding.connection_id.as_str(), ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if after_recovery.status != SetupReceiptStatus::Pending || after_recovery.setup.is_some() {
        return Err("admission recovery exposed the invalidated setup snapshot".to_owned());
    }
    Ok(())
}

#[test]
fn combined_census_binds_the_setup_revision_to_the_next_empty_poll() -> Result<(), String> {
    let binding = binding()?;
    let state = ControlState::new(&config()?);
    state.set_admission_ready(true);
    state
        .replace_setup_receipts(vec![input(&binding, "default group")])
        .map_err(|error| error.to_string())?;
    finish_poll(
        &state,
        &binding,
        Some(PollObservation::Complete {
            available_events: 1,
            statistics: None,
        }),
    )?;

    let started = state
        .start_connection_census(
            binding.connection_id.as_str(),
            NONCE,
            Duration::from_secs(2),
            ENGINE,
            ROOT,
        )
        .map_err(|error| error.to_string())?;
    if started.setup_status != SetupReceiptStatus::Ready
        || started.setup.as_ref().map(|setup| setup.revision) != Some(1)
        || started.observation.status != SessionObservationStatus::Pending
    {
        return Err("combined census did not atomically capture the setup revision".to_owned());
    }

    finish_poll(
        &state,
        &binding,
        Some(PollObservation::Complete {
            available_events: 0,
            statistics: None,
        }),
    )?;
    let complete = state
        .connection_census_status(binding.connection_id.as_str(), NONCE, ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    let poll = complete
        .observation
        .poll
        .ok_or("completed census omitted poll")?;
    if complete.setup_status != SetupReceiptStatus::Ready
        || complete.setup.as_ref().map(|setup| setup.revision) != Some(1)
        || complete.observation.status != SessionObservationStatus::Complete
        || poll
            != (PollObservation::Complete {
                available_events: 0,
                statistics: None,
            })
    {
        return Err(
            "empty poll was not preserved as zero events with unknown statistics".to_owned(),
        );
    }
    Ok(())
}

#[test]
fn setup_revision_replacement_stales_a_pending_combined_census() -> Result<(), String> {
    let binding = binding()?;
    let state = ControlState::new(&config()?);
    state.set_admission_ready(true);
    state
        .replace_setup_receipts(vec![input(&binding, "default group")])
        .map_err(|error| error.to_string())?;
    finish_poll(&state, &binding, None)?;
    state
        .start_connection_census(
            binding.connection_id.as_str(),
            NONCE,
            Duration::from_secs(2),
            ENGINE,
            ROOT,
        )
        .map_err(|error| error.to_string())?;
    state
        .replace_setup_receipts(vec![input(&binding, "default group")])
        .map_err(|error| error.to_string())?;
    let reply = state
        .connection_census_status(binding.connection_id.as_str(), NONCE, ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if reply.setup_status != SetupReceiptStatus::Stale
        || reply.observation.status != SessionObservationStatus::Stale
    {
        return Err("setup replacement did not stale the combined census".to_owned());
    }
    Ok(())
}

#[test]
fn session_and_config_replacement_invalidate_setup_receipts() -> Result<(), String> {
    let binding = binding()?;
    let config = config()?;
    let state = ControlState::new(&config);
    state.set_admission_ready(true);
    state
        .replace_setup_receipts(vec![input(&binding, "default group")])
        .map_err(|error| error.to_string())?;
    finish_poll(&state, &binding, None)?;
    state
        .invalidate_if_session_changed(&binding, 17, 23, "session-2")
        .map_err(|error| error.to_string())?;
    let after_session = state
        .setup_receipt_status(binding.connection_id.as_str(), ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if after_session.status != SetupReceiptStatus::Pending {
        return Err("session replacement retained the old setup snapshot".to_owned());
    }
    state
        .replace_setup_receipts(vec![input(&binding, "default group")])
        .map_err(|error| error.to_string())?;
    let before_failed_poll = state
        .setup_receipt_status(binding.connection_id.as_str(), ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if before_failed_poll.status != SetupReceiptStatus::Ready {
        return Err("setup receipt was not ready before failed poll invalidation".to_owned());
    }
    state
        .invalidate_session_poll(binding.connection_id.as_str())
        .map_err(|error| error.to_string())?;
    let after_failed_poll = state
        .setup_receipt_status(binding.connection_id.as_str(), ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if after_failed_poll.status == SetupReceiptStatus::Ready || after_failed_poll.setup.is_some() {
        return Err("failed poll retained a readable setup receipt".to_owned());
    }
    state
        .replace_setup_receipts(vec![input(&binding, "default group")])
        .map_err(|error| error.to_string())?;
    state
        .replace_config(&config)
        .map_err(|error| error.to_string())?;
    let after_config = state
        .setup_receipt_status(binding.connection_id.as_str(), ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if after_config.status != SetupReceiptStatus::Pending {
        return Err("config replacement retained the old setup snapshot".to_owned());
    }
    Ok(())
}

#[tokio::test]
async fn ipc_setup_and_census_requests_return_cached_facts_without_polling() -> Result<(), String> {
    let state_path = std::env::temp_dir().join(format!(
        "velnor-setup-receipt-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&state_path).map_err(|error| error.to_string())?;
    let binding = binding()?;
    let state = ControlState::new(&config()?);
    state.set_admission_ready(true);
    state
        .replace_setup_receipts(vec![input(&binding, "default group")])
        .map_err(|error| error.to_string())?;
    let journal = Journal::open(&state_path.join("launch.db"))
        .await
        .map_err(|error| format!("open: {error}"))?;

    let setup = handle_request(
        &journal,
        &state,
        &state_path,
        Request::ConnectionSetupStatus {
            connection_id: binding.connection_id.clone(),
        },
        ENGINE,
        ROOT,
    )
    .await
    .map_err(|error| format!("setup status: {error}"))?;
    if setup.setup_receipt.map(|reply| reply.status) != Some(SetupReceiptStatus::Ready) {
        return Err("IPC did not return the cached setup receipt".to_owned());
    }

    let census = handle_request(
        &journal,
        &state,
        &state_path,
        Request::ConnectionCensusStart {
            connection_id: binding.connection_id.clone(),
            nonce: NONCE.to_owned(),
            timeout_ms: 2_000,
        },
        ENGINE,
        ROOT,
    )
    .await
    .map_err(|error| format!("census start: {error}"))?;
    let census = census.connection_census.ok_or("census response missing")?;
    if census.setup_status != SetupReceiptStatus::Ready
        || census.observation.status != SessionObservationStatus::Pending
        || census.observation.poll.is_some()
    {
        return Err("IPC fabricated a poll before the normal poll loop ran".to_owned());
    }
    fs::write(state_path.join("drain"), b"draining").map_err(|error| error.to_string())?;
    let drained = handle_request(
        &journal,
        &state,
        &state_path,
        Request::ConnectionSetupStatus {
            connection_id: binding.connection_id,
        },
        ENGINE,
        ROOT,
    )
    .await
    .map_err(|error| format!("drained setup status: {error}"))?;
    if drained.setup_receipt.map(|reply| reply.status) != Some(SetupReceiptStatus::Stale) {
        return Err("drain marker did not invalidate setup status".to_owned());
    }
    fs::remove_file(state_path.join("drain")).map_err(|error| error.to_string())?;
    let after_drain = state
        .setup_receipt_status("essential", ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if after_drain.status != SetupReceiptStatus::Pending {
        return Err("drain removal revived the invalidated setup receipt".to_owned());
    }
    drop(journal);
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())?;
    Ok(())
}

fn finish_poll(
    state: &ControlState,
    binding: &RepositoryConnection,
    poll: Option<PollObservation>,
) -> Result<(), String> {
    state
        .begin_session_poll(
            binding,
            17,
            23,
            "session-1",
            Instant::now() + Duration::from_secs(10),
        )
        .map_err(|error| error.to_string())?;
    state
        .record_session_poll_observation(binding, 17, 23, "session-1", poll)
        .map_err(|error| error.to_string())
}

fn input(binding: &RepositoryConnection, default_group: &str) -> SetupReceiptInput {
    SetupReceiptInput {
        binding: binding.clone(),
        set: EnsuredSet {
            id: 17,
            ownership: SetOwnership::Adopted,
            name: binding.scale_set.name().to_owned(),
            runner_group_id: 7,
            disable_update: true,
            labels: binding.scale_set.labels().to_vec(),
        },
        groups: vec![
            RunnerGroup {
                id: 3,
                name: "secondary".to_owned(),
                is_default: false,
            },
            RunnerGroup {
                id: 7,
                name: default_group.to_owned(),
                is_default: true,
            },
        ],
        captured_at_unix_ms: CAPTURED_AT_UNIX_MS,
        session_intent_id: 23,
        session_id: "session-1".to_owned(),
        engine_id: ENGINE.to_owned(),
        docker_root_digest: ROOT.to_owned(),
    }
}

fn binding() -> Result<RepositoryConnection, String> {
    let config = config()?;
    config
        .connections()
        .first()
        .cloned()
        .ok_or_else(|| "test config had no connection".to_owned())
}

fn config() -> Result<HostConfig, String> {
    HostConfig::parse(&host_config(
        &[basic_connection(
            "essential",
            101,
            "donbeave/essential-mac",
            "essential",
            "essential-scale-set",
        )],
        1,
    ))
    .map_err(|error| error.to_string())
}
