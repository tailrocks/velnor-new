//! Source receipts keep their exact, unchanged scope.

use super::*;

use std::path::Path;

#[path = "v8_fixture.rs"]
mod v8_fixture;

#[tokio::test]
async fn a_receipt_cannot_cross_connection_policy_or_scale_set_scope() -> Result<(), HostError> {
    let (_scratch, _journal, [broad, narrow, other]) = policy_scopes().await?;
    let (broad_assignment, _) = reserve_assigned(&broad, 88).await?;
    let (narrow_assignment, _) = reserve_assigned(&narrow, 88).await?;
    let (other_assignment, _) = reserve_assigned(&other, 88).await?;
    let broad_receipt = scoped_receipt(
        &broad,
        88,
        121,
        source(101, 910),
        SourcePolicyMode::Repository,
    )
    .await?;
    let narrow_receipt = scoped_receipt(
        &narrow,
        88,
        121,
        source(101, 910),
        SourcePolicyMode::BranchRestricted,
    )
    .await?;

    assert_eq!(
        narrow
            .record_source_authorization(&narrow_assignment, &broad_receipt)
            .await,
        Err(HostError::Identity)
    );
    assert_eq!(
        other
            .record_source_authorization(&other_assignment, &narrow_receipt)
            .await,
        Err(HostError::Identity)
    );
    assert_eq!(
        broad
            .record_source_authorization(&broad_assignment, &broad_receipt)
            .await?,
        SourceAuthorizationResult::Recorded
    );
    let wrong_set = crate::journal::AssignmentIdentity::new(
        narrow.connection_id(),
        599,
        AssignmentKey::AssignedRequest { request_id: 88 },
    )?;
    assert_eq!(
        narrow
            .record_source_authorization(&wrong_set, &narrow_receipt)
            .await,
        Err(HostError::Identity)
    );
    Ok(())
}

async fn policy_scopes() -> Result<(Scratch, Journal, [ConnectionJournal; 3]), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 3,
            cpu_millicores: 12_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let broad_policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )?;
    let narrow_policy = SourcePolicy::try_new(
        SourcePolicyMode::BranchRestricted,
        vec!["main".to_owned()],
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )?;
    let other_branch_policy = SourcePolicy::try_new(
        SourcePolicyMode::BranchRestricted,
        vec!["main".to_owned(), "release".to_owned()],
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )?;
    let broad_binding = binding_with_policy("broad", 101, "donbeave/essential-mac", broad_policy)?;
    let narrow_binding =
        binding_with_policy("narrow", 101, "donbeave/essential-mac", narrow_policy)?;
    let other_binding = binding_with_policy(
        "other-policy",
        101,
        "donbeave/essential-mac",
        other_branch_policy,
    )?;
    let broad = created_scope(&journal, &broad_binding, 501).await?;
    let narrow = created_scope(&journal, &narrow_binding, 502).await?;
    let other = created_scope(&journal, &other_binding, 503).await?;
    Ok((scratch, journal, [broad, narrow, other]))
}

#[tokio::test]
async fn version_eight_receipts_remain_unbound_after_migration() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 4_000,
            memory_bytes: 8_589_934_592,
        })
        .await?;
    let binding = binding("legacy", 101, "donbeave/essential-mac")?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (assignment, intent_id) = reserve_assigned(&scope, 89).await?;
    let receipt = scoped_receipt(
        &scope,
        89,
        122,
        source(101, 911),
        SourcePolicyMode::Repository,
    )
    .await?;
    let source_json = crate::journal::source_authorization::encode_source_receipt(&receipt, 101)?;
    let identity = String::from_utf8(
        binding
            .canonical_identity_bytes()
            .map_err(|_| HostError::Journal)?,
    )
    .map_err(|_| HostError::Journal)?;
    let assignment_key = assignment.storage_key();
    let selector_labels =
        serde_json::to_string(binding.scale_set.labels()).map_err(|_| HostError::Journal)?;
    let payload = LegacyAuthorization {
        connection_id: binding.connection_id.as_str(),
        identity,
        binding_digest: scope.binding_digest().to_owned(),
        source_digest: digest(source_json.as_bytes()),
        source_json,
        intent_id,
        assignment_key,
        repository_id: 101,
        repository: binding.repository.clone(),
        selector_name: binding.scale_set.name().to_owned(),
        selector_labels,
        runner_group_id: 7,
        scale_set_id: 501,
        request_id: 89,
        message_id: 122,
    };
    drop(scope);
    drop(journal);

    let legacy_path = scratch.0.join("legacy-v8.db");
    v8_fixture::write(&legacy_path, &payload).await?;
    assert_migrated_receipt_stays_closed(&legacy_path, &binding, &assignment, intent_id).await
}

pub(super) struct LegacyAuthorization<'a> {
    pub(super) connection_id: &'a str,
    pub(super) identity: String,
    pub(super) binding_digest: String,
    pub(super) source_digest: String,
    pub(super) source_json: String,
    pub(super) intent_id: i64,
    pub(super) assignment_key: String,
    pub(super) repository_id: i64,
    pub(super) repository: String,
    pub(super) selector_name: String,
    pub(super) selector_labels: String,
    pub(super) runner_group_id: i64,
    pub(super) scale_set_id: i64,
    pub(super) request_id: i64,
    pub(super) message_id: i64,
}

async fn assert_migrated_receipt_stays_closed(
    path: &Path,
    binding: &RepositoryConnection,
    assignment: &crate::journal::AssignmentIdentity,
    intent_id: i64,
) -> Result<(), HostError> {
    let journal = Journal::open(path).await?;
    let scope = journal.connection_scope(binding).await?;
    let (replayed_intent_id, fresh) = scope.begin_assigned_launch(assignment, "v89").await?;
    assert_eq!(replayed_intent_id, intent_id);
    assert!(fresh);
    assert_null_receipt_binding(&journal, binding).await?;
    assert!(!scope.claim_request_acquire(assignment).await?);
    assert!(!journal.is_drained().await?);
    Ok(())
}

async fn assert_null_receipt_binding(
    journal: &Journal,
    binding: &RepositoryConnection,
) -> Result<(), HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT receipt_binding_digest FROM connection_source_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
            (binding.connection_id.as_str(), 501, 89),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    assert_eq!(
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<Option<String>>(0)
            .map_err(|_| HostError::Journal)?,
        None
    );
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    let mut value = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _written = write!(value, "{byte:02x}");
    }
    value
}
