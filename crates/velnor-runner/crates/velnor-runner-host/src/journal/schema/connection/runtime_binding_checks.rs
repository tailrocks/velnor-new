// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::Journal;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const COLUMNS: &str = "connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform";

const PARTIAL_TRIPLES: [&str; 6] = [
    "7, NULL, 'created', NULL, NULL, NULL",
    "NULL, NULL, 'created', NULL, NULL, NULL",
    "7, 'set', NULL, NULL, NULL, NULL",
    "NULL, NULL, NULL, 'eng', NULL, NULL",
    "NULL, NULL, NULL, 'eng', 'unix:///docker.sock', NULL",
    "NULL, NULL, NULL, NULL, 'unix:///docker.sock', 'linux/amd64'",
];

const COMPLETE_TRIPLES: [&str; 3] = [
    "NULL, NULL, NULL, NULL, NULL, NULL",
    "7, 'set', 'created', NULL, NULL, NULL",
    "7, 'set', 'adopted', 'eng', 'unix:///docker.sock', 'linux/amd64'",
];

fn connection() -> TestResult<RepositoryConnection> {
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

#[tokio::test]
async fn a_partly_filled_scale_set_or_engine_triple_is_rejected() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let journal = Journal::open(&state.path().join("launch.db")).await?;
    journal.connection_scope(&connection()?).await?;
    let conn = journal.connection().await?;
    for triple in PARTIAL_TRIPLES {
        let insert = format!(
            "INSERT INTO connection_runtime_bindings ({COLUMNS}) VALUES ('conn-test-1', {triple})"
        );
        assert!(
            conn.execute(insert.as_str(), ()).await.is_err(),
            "accepted partial triple {triple}"
        );
    }
    for triple in COMPLETE_TRIPLES {
        let insert = format!(
            "INSERT INTO connection_runtime_bindings ({COLUMNS}) VALUES ('conn-test-1', {triple})"
        );
        conn.execute(insert.as_str(), ()).await?;
        conn.execute(
            "DELETE FROM connection_runtime_bindings WHERE connection_id = 'conn-test-1'",
            (),
        )
        .await?;
    }
    Ok(())
}
