//! Source receipts stay bound to their exact immutable scope.

use super::*;

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
    scope
        .record_source_authorization(&assignment, &receipt)
        .await?;
    drop(scope);
    drop(journal);

    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "ALTER TABLE connection_source_authorizations DROP COLUMN receipt_binding_digest",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 8", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let (replayed_intent_id, fresh) = scope.begin_assigned_launch(&assignment, "v89").await?;
    assert_eq!(replayed_intent_id, intent_id);
    assert!(fresh);
    assert_eq!(
        scope
            .record_source_authorization(&assignment, &receipt)
            .await,
        Err(HostError::Identity)
    );
    assert!(!scope.claim_assigned_acquire(intent_id).await?);
    assert!(!scope.claim_launch_jit(intent_id).await?);
    assert!(!journal.is_drained().await?);
    Ok(())
}
