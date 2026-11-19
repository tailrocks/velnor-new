// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ScaleSetDeleteStatus, start};
use crate::config::{HostConfig, RepositoryConnection};
use crate::controller_ipc::server::ControlState;
use crate::scale_set::delete::tests::fixture;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const OTHER_TOKEN: &str = "fedcba9876543210fedcba9876543210";
const NONCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn control_state(
    binding: &RepositoryConnection,
) -> Result<ControlState, Box<dyn std::error::Error>> {
    let text = crate::scale_set::delete::tests::CONFIG;
    let config = HostConfig::parse(text)?;
    assert_eq!(config.connections().first(), Some(binding));
    let state = ControlState::new(&config);
    state.set_admission_ready(true);
    Ok(state)
}

fn drain_marker(token: &str) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir(dir.path().join("connection-drain"))?;
    std::fs::write(
        dir.path().join("connection-drain").join("conn-test-1"),
        token,
    )?;
    Ok(dir)
}

#[tokio::test]
async fn a_created_set_is_queued_for_the_main_loop_and_an_adopted_one_is_not() -> TestResult {
    let created = fixture(true).await?;
    let state = control_state(&created.binding)?;
    let marker = drain_marker(TOKEN)?;
    let status = start(
        &created.journal,
        &state,
        marker.path(),
        &created.binding,
        TOKEN,
        NONCE,
    )
    .await?;
    assert_eq!(status, ScaleSetDeleteStatus::Pending);
    assert_eq!(
        state.pending_scale_set_delete("conn-test-1")?.as_deref(),
        Some(NONCE)
    );

    let adopted = fixture(false).await?;
    let state = control_state(&adopted.binding)?;
    let status = start(
        &adopted.journal,
        &state,
        marker.path(),
        &adopted.binding,
        TOKEN,
        NONCE,
    )
    .await?;
    assert_eq!(status, ScaleSetDeleteStatus::NotOwned);
    assert_eq!(state.pending_scale_set_delete("conn-test-1")?, None);
    Ok(())
}

#[tokio::test]
async fn a_request_without_the_current_drain_token_or_admission_is_stale() -> TestResult {
    let created = fixture(true).await?;
    let state = control_state(&created.binding)?;
    let marker = drain_marker(TOKEN)?;
    let wrong = start(
        &created.journal,
        &state,
        marker.path(),
        &created.binding,
        OTHER_TOKEN,
        NONCE,
    )
    .await?;
    assert_eq!(wrong, ScaleSetDeleteStatus::Stale);
    state.set_admission_ready(false);
    let closed = start(
        &created.journal,
        &state,
        marker.path(),
        &created.binding,
        TOKEN,
        NONCE,
    )
    .await?;
    assert_eq!(closed, ScaleSetDeleteStatus::Stale);
    assert_eq!(state.pending_scale_set_delete("conn-test-1")?, None);
    Ok(())
}
