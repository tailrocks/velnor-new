// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::scale_set_delete;
use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn connection() -> Result<RepositoryConnection, Box<dyn std::error::Error>> {
    Ok(RepositoryConnection::try_new(
        ConnectionId::parse("conn-test-1")?,
        42,
        "owner/repo".to_owned(),
        KeychainReference::parse("keychain:velnor.test/conn-test-1")?,
        ScaleSetSelector::try_new(
            "ubuntu-26.04-scale-set",
            &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
        )?,
        SourcePolicy::try_new(
            SourcePolicyMode::BranchRestricted,
            vec!["main".to_owned()],
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )?,
    )?)
}

#[test]
fn an_unlinked_daemon_lock_does_not_let_the_offline_delete_run_beside_the_controller() -> TestResult
{
    let state = tempfile::tempdir()?;
    let controller = DaemonLock::try_acquire_daemon(state.path())?;
    std::fs::remove_file(state.path().join("daemon.lock"))?;
    let result = scale_set_delete(state.path(), &connection()?, "token");
    assert!(matches!(result, Err(HostError::Lock)));
    assert!(!state.path().join("daemon.lock").exists());
    assert!(controller.is_held());
    Ok(())
}
