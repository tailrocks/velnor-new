//! Compare one measured Runner request with GitHub and the approved source graph.

use std::time::Instant;

use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::error::HostError;
use crate::journal::ConnectionJournal;
use crate::launch::execution_authorization::source::verify_live_source;
use crate::launch::execution_authorization::{AuthorizedExecutionSource, ExecutionSourceRequest};
use crate::worker::execution_authorization::VerifiedProofChannelBinding;

/// Compare a canonical request with its durable Runner event and GitHub source.
pub(crate) async fn verify_execution_source(
    journal: &ConnectionJournal,
    connection: &RepositoryConnection,
    policy: &ApprovedJobAuthorityPolicy,
    request_bytes: &[u8],
    channel: &VerifiedProofChannelBinding,
    token: &str,
    deadline: Instant,
) -> Result<AuthorizedExecutionSource, HostError> {
    let request = ExecutionSourceRequest::parse(request_bytes)?;
    let native = journal
        .native_started_source_context(&request, channel)
        .await?;
    verify_live_source(
        connection, policy, &request, channel, &native, token, deadline,
    )
}
