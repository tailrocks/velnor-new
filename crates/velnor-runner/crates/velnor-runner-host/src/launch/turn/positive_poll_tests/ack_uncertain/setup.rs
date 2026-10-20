use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::{
    CapacityLimits, ConnectionJournal, CreatedScaleSetResponse, HeaderAllocation, Journal,
    ScaleSetCreateRequest,
};
use crate::launch::turn::scoped_poll_tests::support::Scratch;

pub(super) async fn active_scope(
    scratch: &Scratch,
) -> Result<(ConnectionJournal, i64, HeaderAllocation), String> {
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bootstrap_header_authority()
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("test-engine")
        .await
        .map_err(|error| error.to_string())?;
    let binding = repository_binding()?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| error.to_string())?;
    record_scale_set(&scope, &binding).await?;
    let (intent_id, allocation) = record_session(&scope).await?;
    Ok((scope, intent_id, allocation))
}

fn repository_binding() -> Result<RepositoryConnection, String> {
    let selector = ScaleSetSelector::try_new(
        "integration",
        &["velnor".to_owned(), "integration".to_owned()],
    )
    .map_err(|error| error.to_string())?;
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;
    RepositoryConnection::try_new(
        ConnectionId::parse("scoped-poll-test").map_err(|error| error.to_string())?,
        101,
        "owner/repository".to_owned(),
        KeychainReference::parse("keychain:velnor/scoped-poll-test")
            .map_err(|error| error.to_string())?,
        selector,
        policy,
    )
    .map_err(|error| error.to_string())
}

async fn record_scale_set(
    scope: &ConnectionJournal,
    binding: &RepositoryConnection,
) -> Result<(), String> {
    let create = ScaleSetCreateRequest::new(binding, 7).map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_scale_set_create(&create)
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_scale_set_create(&prepared)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The claim for Scale Set creation is missing.".to_owned())?;
    let created = CreatedScaleSetResponse::verified(
        &create,
        10_101,
        "integration",
        binding.scale_set.labels(),
        true,
    )
    .map_err(|error| error.to_string())?;
    scope
        .record_scale_set_created(claim, created)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn record_session(scope: &ConnectionJournal) -> Result<(i64, HeaderAllocation), String> {
    let prepared = scope
        .prepare_session_create("0123456789abcdef0123456789abcdef")
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The claim for session creation is missing.".to_owned())?;
    let bound = scope
        .record_created_session(&claim, "session-scoped-poll", 1)
        .await
        .map_err(|error| error.to_string())?;
    Ok((bound.intent_id(), bound.allocation().clone()))
}
