// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use velnor_runner_github::RunnerGroup;

use super::{ControlState, SetupReceiptInput};
use crate::config::{HostConfig, RepositoryConnection};
use crate::connect::SetOwnership;
use crate::controller_ipc::wire::{SessionObservationStatus, SetupReceiptStatus};
use crate::launch::steady::PollObservation;
use crate::scale_set::EnsuredSet;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const ENGINE: &str = "engine-test";
const ROOT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CONNECTION: &str = "conn-test-1";
const NONCE: &str = "0123456789abcdef0123456789abcdef";
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
    fn with_published_setup() -> TestResult<Self> {
        let config = HostConfig::parse(CONFIG)?;
        let binding = config.connections().first().ok_or("connection")?.clone();
        let state = ControlState::new(&config);
        state.set_admission_ready(true);
        state.replace_setup_receipts(vec![SetupReceiptInput {
            set: EnsuredSet {
                id: 7,
                ownership: SetOwnership::Created,
                name: binding.scale_set.name().to_owned(),
                runner_group_id: 1,
                disable_update: false,
                labels: binding.scale_set.labels().to_vec(),
            },
            binding: binding.clone(),
            groups: vec![RunnerGroup {
                id: 1,
                name: "Default".to_owned(),
                is_default: true,
            }],
            captured_at_unix_ms: 1,
            session_intent_id: 1,
            session_id: "session-1".to_owned(),
            engine_id: ENGINE.to_owned(),
            docker_root_digest: ROOT.to_owned(),
        }])?;
        Ok(Self { state, binding })
    }

    fn setup_status(&self) -> TestResult<SetupReceiptStatus> {
        Ok(self
            .state
            .setup_receipt_status(CONNECTION, ENGINE, ROOT)?
            .status)
    }

    fn record_complete_poll(&self, session_id: &str) -> TestResult {
        self.state.record_session_poll_observation(
            &self.binding,
            7,
            1,
            session_id,
            Some(PollObservation::Complete {
                available_events: 0,
                statistics: None,
            }),
        )?;
        Ok(())
    }
}

#[test]
fn a_failed_poll_keeps_the_published_setup_receipt() -> TestResult {
    let fixture = Fixture::with_published_setup()?;
    fixture.state.invalidate_session_poll(CONNECTION)?;
    fixture.record_complete_poll("session-1")?;
    assert_eq!(fixture.setup_status()?, SetupReceiptStatus::Ready);
    let census = fixture.state.start_connection_census(
        CONNECTION,
        NONCE,
        Duration::from_secs(5),
        ENGINE,
        ROOT,
    )?;
    assert_eq!(census.setup_status, SetupReceiptStatus::Ready);
    assert!(census.setup.is_some());
    assert_eq!(census.observation.status, SessionObservationStatus::Pending);
    Ok(())
}

#[test]
fn a_changed_session_still_drops_the_published_setup_receipt() -> TestResult {
    let fixture = Fixture::with_published_setup()?;
    fixture.record_complete_poll("session-1")?;
    fixture
        .state
        .invalidate_if_session_changed(&fixture.binding, 7, 1, "session-2")?;
    assert_eq!(fixture.setup_status()?, SetupReceiptStatus::Pending);
    Ok(())
}
