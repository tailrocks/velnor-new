// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use super::ControlState;
use crate::config::{HostConfig, RepositoryConnection};
use crate::controller_ipc::wire::SessionObservationStatus;
use crate::launch::steady::PollObservation;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const ENGINE: &str = "engine-test";
const ROOT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const FIRST: &str = "0123456789abcdef0123456789abcdef";
const SECOND: &str = "fedcba9876543210fedcba9876543210";
const CONNECTION: &str = "conn-test-1";
const TIMEOUT: Duration = Duration::from_secs(30);
const CONFIG: &str = r#"schema = 2
[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

struct Fixture {
    state: ControlState,
    binding: RepositoryConnection,
}

impl Fixture {
    fn with_first_observation_complete() -> TestResult<Self> {
        let config = HostConfig::parse(CONFIG)?;
        let binding = config.connections().first().ok_or("connection")?.clone();
        let state = ControlState::new(&config);
        state.set_admission_ready(true);
        state.record_session_poll_observation(&binding, 7, 1, "session-1", None)?;
        let fixture = Self { state, binding };
        assert_eq!(fixture.start(FIRST)?, SessionObservationStatus::Pending);
        fixture.begin_poll()?;
        fixture.state.record_session_poll_observation(
            &fixture.binding,
            7,
            1,
            "session-1",
            Some(PollObservation::Complete {
                available_events: 0,
                statistics: None,
            }),
        )?;
        assert_eq!(fixture.status(FIRST)?, SessionObservationStatus::Complete);
        Ok(fixture)
    }

    fn start(&self, nonce: &str) -> TestResult<SessionObservationStatus> {
        Ok(self
            .state
            .start_session_observation(CONNECTION, nonce, TIMEOUT, ENGINE, ROOT)?
            .status)
    }

    fn status(&self, nonce: &str) -> TestResult<SessionObservationStatus> {
        Ok(self
            .state
            .session_observation_status(CONNECTION, nonce, ENGINE, ROOT)?
            .status)
    }

    fn begin_poll(&self) -> TestResult {
        self.state.begin_session_poll(
            &self.binding,
            7,
            1,
            "session-1",
            Instant::now() + Duration::from_secs(60),
        )?;
        Ok(())
    }

    fn age_receipt(&self) -> TestResult {
        let mut polls = self
            .state
            .session_polls
            .write()
            .map_err(|_| "session polls lock")?;
        let receipt = polls.get_mut(CONNECTION).ok_or("receipt")?;
        receipt.observed_at = Instant::now()
            .checked_sub(Duration::from_secs(6))
            .ok_or("clock")?;
        Ok(())
    }
}

#[test]
fn a_start_refused_for_a_pending_poll_keeps_the_completed_observation() -> TestResult {
    let fixture = Fixture::with_first_observation_complete()?;
    fixture.begin_poll()?;
    assert_eq!(fixture.start(SECOND)?, SessionObservationStatus::Busy);
    assert_eq!(fixture.status(FIRST)?, SessionObservationStatus::Complete);
    assert_eq!(fixture.status(SECOND)?, SessionObservationStatus::Missing);
    Ok(())
}

#[test]
fn a_start_refused_for_a_stale_receipt_keeps_the_completed_observation() -> TestResult {
    let fixture = Fixture::with_first_observation_complete()?;
    fixture.age_receipt()?;
    assert_eq!(fixture.start(SECOND)?, SessionObservationStatus::Stale);
    assert_eq!(fixture.status(FIRST)?, SessionObservationStatus::Complete);
    assert_eq!(fixture.status(SECOND)?, SessionObservationStatus::Missing);
    Ok(())
}

#[test]
fn a_start_that_can_store_a_ticket_replaces_the_completed_observation() -> TestResult {
    let fixture = Fixture::with_first_observation_complete()?;
    assert_eq!(fixture.start(SECOND)?, SessionObservationStatus::Pending);
    assert_eq!(fixture.status(SECOND)?, SessionObservationStatus::Pending);
    assert_eq!(fixture.status(FIRST)?, SessionObservationStatus::Missing);
    Ok(())
}
