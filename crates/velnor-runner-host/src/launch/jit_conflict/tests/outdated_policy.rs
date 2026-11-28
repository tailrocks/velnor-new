// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    JitOutcome, TestResult,
    case::{Case, Schema},
};
use crate::error::HostError;

#[tokio::test(flavor = "multi_thread")]
async fn a_journal_holding_a_schema_1_policy_asks_to_remove_launch_db() -> TestResult<()> {
    let case = Case::new(Schema::Current, JitOutcome::Interrupted, false).await?;
    let connection = case.journal.connection().await?;
    connection
        .execute("DROP TRIGGER execution_policy_immutable", ())
        .await?;
    connection
        .execute(
            "UPDATE connection_job_authority_policies SET policy_json = replace(policy_json, '\"schema_version\":2', '\"schema_version\":1')",
            (),
        )
        .await?;
    drop(connection);
    let refused = case.reopen(Schema::Current).await.err();
    let error = refused
        .as_ref()
        .and_then(|error| error.downcast_ref::<HostError>())
        .copied();
    assert_eq!(
        error,
        Some(HostError::JournalPolicySchema {
            found: 1,
            expected: 2
        })
    );
    let message = HostError::JournalPolicySchema {
        found: 1,
        expected: 2,
    }
    .to_string();
    assert!(message.contains("launch.db"), "{message}");
    assert!(message.contains("remove launch.db"), "{message}");
    Ok(())
}
