use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll, SourceMetadata};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::AssignmentKey;
use crate::launch::DeferredAuthorizedOffers;

use super::support::*;
use crate::launch::turn::scoped::{finish_scoped_poll_response, process_deferred_scoped_response};

fn available(request_id: i64) -> InnerJob {
    InnerJob {
        kind: InnerKind::Available,
        request_id: Some(request_id),
        runner_id: None,
        runner_name: None,
        result: None,
        job_id: Some("opaque-job".to_owned()),
        labels: Vec::new(),
        fields: Vec::new(),
        source: SourceMetadata::default(),
    }
}

fn event(kind: InnerKind, request_id: i64) -> InnerJob {
    InnerJob {
        kind,
        request_id: Some(request_id),
        runner_id: Some(900),
        runner_name: Some("runner-900".to_owned()),
        result: Some("Succeeded".to_owned()),
        job_id: Some("opaque-event".to_owned()),
        labels: Vec::new(),
        fields: Vec::new(),
        source: SourceMetadata::default(),
    }
}

fn batch(message_id: i64, jobs: Vec<InnerJob>) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id,
        raw_body: "native response body".to_owned(),
        statistics: None,
        jobs,
    })
}

fn native_fixture_connection() -> Result<RepositoryConnection, String> {
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
        ConnectionId::parse("native-started-intake").map_err(|error| error.to_string())?,
        101,
        "owner/repository".to_owned(),
        KeychainReference::parse("keychain:velnor/native-started-intake")
            .map_err(|error| error.to_string())?,
        selector,
        policy,
    )
    .map_err(|error| error.to_string())
}

async fn durable_offer_set(
    polled: &Poll,
    scope: &crate::journal::ConnectionJournal,
    set_id: i64,
    request_id: i64,
) -> Result<DeferredAuthorizedOffers, String> {
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|error| error.to_string())?;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let receipt = crate::launch::steps::source::test_receipt(
        &binding,
        request_id,
        match polled {
            Poll::Batch(batch) => batch.message_id,
            _ => return Err("poll was not a batch".to_owned()),
        },
        authorized_source(),
        SourcePolicyMode::Repository,
    );
    scope
        .record_source_authorization(&identity, &receipt)
        .await
        .map_err(|error| error.to_string())?;
    let connection = repository_connection()?;
    DeferredAuthorizedOffers::from_receipts(
        &connection,
        scope,
        set_id,
        polled,
        vec![(request_id, receipt)],
    )
    .await
    .map_err(|error| format!("could not bind durable offer: {error:?}"))
}

#[cfg(test)]
#[path = "deferred_mode/tests.rs"]
mod tests;
