use std::fs;
use std::time::{Duration, Instant};

use crate::config::{HostConfig, RepositoryConnection};
use crate::config_test_support::{basic_connection, host_config};
use crate::controller_ipc::wire::{Request, SessionObservationStatus};
use crate::error::HostError;
use crate::journal::Journal;
use crate::launch::steady::{PollObservation, PollStatistics};

use super::{ControlState, handle_request, test_binding, test_state};

const ENGINE: &str = "engine-observation";
const ROOT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NONCE: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn only_a_poll_started_after_request_can_complete_the_ticket() -> Result<(), String> {
    let (state, binding) = ready_state()?;
    state
        .record_session_poll(&binding, 17, 23, "session-1")
        .map_err(|error| error.to_string())?;
    let started = state
        .start_session_observation(
            binding.connection_id.as_str(),
            NONCE,
            Duration::from_secs(2),
            ENGINE,
            ROOT,
        )
        .map_err(|error| error.to_string())?;
    if started.status != SessionObservationStatus::Pending {
        return Err("fresh observation request did not become pending".to_owned());
    }
    state
        .begin_session_poll(
            &binding,
            17,
            23,
            "session-1",
            Instant::now() + Duration::from_secs(10),
        )
        .map_err(|error| error.to_string())?;
    state
        .record_session_poll_observation(&binding, 17, 23, "session-1", Some(poll()))
        .map_err(|error| error.to_string())?;
    let completed = state
        .session_observation_status(binding.connection_id.as_str(), NONCE, ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if completed.status != SessionObservationStatus::Complete || completed.poll != Some(poll()) {
        return Err("the next settled poll did not complete the observation".to_owned());
    }
    Ok(())
}

#[test]
fn request_during_an_in_flight_poll_cannot_reuse_its_completion() -> Result<(), String> {
    let (state, binding) = ready_state()?;
    state
        .record_session_poll(&binding, 17, 23, "session-1")
        .map_err(|error| error.to_string())?;
    state
        .begin_session_poll(
            &binding,
            17,
            23,
            "session-1",
            Instant::now() + Duration::from_secs(10),
        )
        .map_err(|error| error.to_string())?;
    let started = state
        .start_session_observation(
            binding.connection_id.as_str(),
            NONCE,
            Duration::from_secs(2),
            ENGINE,
            ROOT,
        )
        .map_err(|error| error.to_string())?;
    if started.status != SessionObservationStatus::Busy {
        return Err("in-flight poll was not rejected as busy".to_owned());
    }
    state
        .record_session_poll_observation(&binding, 17, 23, "session-1", Some(poll()))
        .map_err(|error| error.to_string())?;
    let status = state
        .session_observation_status(binding.connection_id.as_str(), NONCE, ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if status.status != SessionObservationStatus::Missing || status.poll.is_some() {
        return Err("a pre-request poll satisfied a later request".to_owned());
    }
    Ok(())
}

#[test]
fn invalidation_engine_change_and_timeout_are_explicit() -> Result<(), String> {
    let (state, binding) = ready_state()?;
    state
        .record_session_poll(&binding, 17, 23, "session-1")
        .map_err(|error| error.to_string())?;
    state
        .start_session_observation(
            binding.connection_id.as_str(),
            NONCE,
            Duration::from_secs(2),
            ENGINE,
            ROOT,
        )
        .map_err(|error| error.to_string())?;
    state
        .invalidate_if_session_changed(&binding, 17, 23, "session-2")
        .map_err(|error| error.to_string())?;
    let ticket_reply = state
        .session_observation_status(binding.connection_id.as_str(), NONCE, ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if ticket_reply.status != SessionObservationStatus::Stale {
        return Err("session replacement did not stale the ticket".to_owned());
    }

    state
        .record_session_poll(&binding, 17, 23, "session-2")
        .map_err(|error| error.to_string())?;
    let next_nonce = "1123456789abcdef0123456789abcdef";
    state
        .start_session_observation(
            binding.connection_id.as_str(),
            next_nonce,
            Duration::from_millis(2),
            ENGINE,
            ROOT,
        )
        .map_err(|error| error.to_string())?;
    std::thread::sleep(Duration::from_millis(5));
    let timed_out = state
        .session_observation_status(binding.connection_id.as_str(), next_nonce, ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if timed_out.status != SessionObservationStatus::TimedOut {
        return Err("expired request did not return a timeout".to_owned());
    }
    state
        .record_session_poll(&binding, 17, 23, "session-2")
        .map_err(|error| error.to_string())?;
    let third_nonce = "2123456789abcdef0123456789abcdef";
    state
        .start_session_observation(
            binding.connection_id.as_str(),
            third_nonce,
            Duration::from_secs(2),
            ENGINE,
            ROOT,
        )
        .map_err(|error| error.to_string())?;
    let replaced_engine = state
        .session_observation_status(
            binding.connection_id.as_str(),
            third_nonce,
            "replacement-engine",
            ROOT,
        )
        .map_err(|error| error.to_string())?;
    if replaced_engine.status != SessionObservationStatus::Stale {
        return Err("engine replacement did not stale the ticket".to_owned());
    }
    state.set_admission_ready(false);
    let admission_lost = state
        .session_observation_status(binding.connection_id.as_str(), third_nonce, ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if admission_lost.status != SessionObservationStatus::Stale {
        return Err("admission loss did not stale the ticket".to_owned());
    }
    Ok(())
}

#[test]
fn poll_reset_and_config_replacement_stale_existing_tickets() -> Result<(), String> {
    let (state, binding) = ready_state()?;
    state
        .record_session_poll(&binding, 17, 23, "session-1")
        .map_err(|error| error.to_string())?;
    state
        .start_session_observation(
            binding.connection_id.as_str(),
            NONCE,
            Duration::from_secs(2),
            ENGINE,
            ROOT,
        )
        .map_err(|error| error.to_string())?;
    state
        .invalidate_session_poll(binding.connection_id.as_str())
        .map_err(|error| error.to_string())?;
    let reset = state
        .session_observation_status(binding.connection_id.as_str(), NONCE, ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if reset.status != SessionObservationStatus::Stale {
        return Err("poll reset did not stale the existing ticket".to_owned());
    }

    state
        .record_session_poll(&binding, 17, 23, "session-1")
        .map_err(|error| error.to_string())?;
    let next_nonce = "3123456789abcdef0123456789abcdef";
    state
        .start_session_observation(
            binding.connection_id.as_str(),
            next_nonce,
            Duration::from_secs(2),
            ENGINE,
            ROOT,
        )
        .map_err(|error| error.to_string())?;
    state
        .replace_config(&test_config()?)
        .map_err(|error| error.to_string())?;
    let replaced = state
        .session_observation_status(binding.connection_id.as_str(), next_nonce, ENGINE, ROOT)
        .map_err(|error| error.to_string())?;
    if replaced.status != SessionObservationStatus::Stale {
        return Err("config replacement did not stale the existing ticket".to_owned());
    }
    Ok(())
}

#[tokio::test]
async fn ipc_observation_requests_return_immediately_and_drain_invalidates() -> Result<(), String> {
    let state_path = test_state("session-observation");
    fs::create_dir_all(&state_path).map_err(|error| error.to_string())?;
    let binding = test_binding()?;
    let config = test_config()?;
    let state = ControlState::new(&config);
    state.set_admission_ready(true);
    state
        .record_session_poll(&binding, 17, 23, "session-1")
        .map_err(|error| error.to_string())?;
    let journal = Journal::open(&state_path.join("launch.db"))
        .await
        .map_err(|error| format!("open: {error}"))?;
    let request = Request::SessionObservationStart {
        connection_id: binding.connection_id.clone(),
        nonce: NONCE.to_owned(),
        timeout_ms: 2_000,
    };
    let started = handle_request(&journal, &state, &state_path, request, ENGINE, ROOT)
        .await
        .map_err(|error| format!("start: {error}"))?;
    if started.session_observation.map(|reply| reply.status)
        != Some(SessionObservationStatus::Pending)
    {
        return Err("IPC start did not return pending".to_owned());
    }
    fs::write(state_path.join("drain"), b"draining").map_err(|error| error.to_string())?;
    let status = handle_request(
        &journal,
        &state,
        &state_path,
        Request::SessionObservationStatus {
            connection_id: binding.connection_id.clone(),
            nonce: NONCE.to_owned(),
        },
        ENGINE,
        ROOT,
    )
    .await
    .map_err(|error| format!("status: {error}"))?;
    if status.session_observation.map(|reply| reply.status) != Some(SessionObservationStatus::Stale)
    {
        return Err("drain marker did not invalidate IPC observation".to_owned());
    }
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())?;
    Ok(())
}

fn ready_state() -> Result<(ControlState, RepositoryConnection), String> {
    let binding = test_binding()?;
    let config = test_config()?;
    let state = ControlState::new(&config);
    state.set_admission_ready(true);
    Ok((state, binding))
}

fn test_config() -> Result<HostConfig, String> {
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

fn poll() -> PollObservation {
    PollObservation::Complete {
        available_events: 2,
        statistics: Some(PollStatistics {
            available_jobs: 8,
            acquired_jobs: 7,
            assigned_jobs: 6,
            running_jobs: 5,
            registered_runners: 4,
            busy_runners: 3,
            idle_runners: 2,
        }),
    }
}

#[test]
fn invalid_timeout_and_nonce_are_rejected() {
    let (state, binding) = ready_state().expect("test state is valid");
    assert_eq!(
        state.start_session_observation(
            binding.connection_id.as_str(),
            "bad",
            Duration::from_secs(1),
            ENGINE,
            ROOT,
        ),
        Err(HostError::Frame)
    );
    assert_eq!(
        state.start_session_observation(
            binding.connection_id.as_str(),
            NONCE,
            Duration::from_secs(31),
            ENGINE,
            ROOT,
        ),
        Err(HostError::Frame)
    );
}
