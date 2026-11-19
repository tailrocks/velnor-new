// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ControlState, ScaleSetDeleteOutcome, ScaleSetDeleteStatus};
use crate::config::HostConfig;

const FIRST: &str = "0123456789abcdef0123456789abcdef";
const SECOND: &str = "fedcba9876543210fedcba9876543210";
const CONNECTION: &str = "conn-test-1";
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

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn a_queued_delete_is_pending_until_the_main_loop_finishes_it() -> TestResult {
    let state = ControlState::new(&HostConfig::parse(CONFIG)?);
    assert_eq!(
        state.queue_scale_set_delete(CONNECTION, FIRST)?,
        ScaleSetDeleteStatus::Pending
    );
    assert_eq!(
        state.queue_scale_set_delete(CONNECTION, FIRST)?,
        ScaleSetDeleteStatus::Pending
    );
    assert_eq!(
        state.pending_scale_set_delete(CONNECTION)?.as_deref(),
        Some(FIRST)
    );
    state.finish_scale_set_delete(CONNECTION, FIRST, ScaleSetDeleteOutcome::Refused(403))?;
    assert_eq!(state.pending_scale_set_delete(CONNECTION)?, None);
    assert_eq!(
        state.scale_set_delete_status(CONNECTION, FIRST)?,
        ScaleSetDeleteStatus::Refused { status: 403 }
    );
    assert_eq!(
        state.queue_scale_set_delete(CONNECTION, FIRST)?,
        ScaleSetDeleteStatus::Refused { status: 403 }
    );
    Ok(())
}

#[test]
fn a_newer_request_supersedes_and_a_stale_nonce_learns_nothing() -> TestResult {
    let state = ControlState::new(&HostConfig::parse(CONFIG)?);
    assert_eq!(
        state.scale_set_delete_status(CONNECTION, FIRST)?,
        ScaleSetDeleteStatus::Stale
    );
    let _ = state.queue_scale_set_delete(CONNECTION, FIRST)?;
    let _ = state.queue_scale_set_delete(CONNECTION, SECOND)?;
    state.finish_scale_set_delete(CONNECTION, FIRST, ScaleSetDeleteOutcome::Deleted)?;
    assert_eq!(
        state.scale_set_delete_status(CONNECTION, SECOND)?,
        ScaleSetDeleteStatus::Pending
    );
    assert_eq!(
        state.scale_set_delete_status(CONNECTION, FIRST)?,
        ScaleSetDeleteStatus::Stale
    );
    Ok(())
}
