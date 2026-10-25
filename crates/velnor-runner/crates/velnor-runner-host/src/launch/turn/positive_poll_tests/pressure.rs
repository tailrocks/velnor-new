use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use crate::journal::{
    CapacityLimits, ConnectionJournal, CreatedScaleSetResponse, HeaderAllocation, Journal,
    ScaleSetCreateRequest,
};

struct PollFixture {
    _scratch: crate::launch::turn::scoped_poll_tests::support::Scratch,
    journal: Journal,
    scope: ConnectionJournal,
    intent_id: i64,
    allocation: HeaderAllocation,
    session: velnor_runner_github::QueueSession,
}

struct SessionReply;

impl Transport for SessionReply {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Post);
        Ok(Exchange {
            status: 200,
            body: br#"{"sessionId":"session-pressure-poll","messageQueueUrl":"https://queue.example.invalid/","messageQueueAccessToken":"queue-token"}"#.to_vec(),
        })
    }
}

async fn poll_fixture() -> Result<PollFixture, String> {
    use crate::launch::turn::scoped_poll_tests::support::{Scratch, repository_connection};

    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 4,
            cpu_millicores: 4_000,
            memory_bytes: 4 * 1024 * 1024 * 1024,
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

    let binding = repository_connection()?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| error.to_string())?;
    let create = ScaleSetCreateRequest::new(&binding, 7).map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_scale_set_create(&create)
        .await
        .map_err(|error| error.to_string())?;
    let create_claim = scope
        .claim_scale_set_create(&prepared)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Scale Set create claim missing".to_owned())?;
    let created = CreatedScaleSetResponse::verified(
        &create,
        10_101,
        "integration",
        binding.scale_set.labels(),
        true,
    )
    .map_err(|error| error.to_string())?;
    scope
        .record_scale_set_created(create_claim, created)
        .await
        .map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_session_create("0123456789abcdef0123456789abcdef")
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Session create claim missing".to_owned())?;
    let bound = scope
        .record_created_session(&claim, "session-pressure-poll", 4)
        .await
        .map_err(|error| error.to_string())?;
    let mut session_reply = SessionReply;
    let session =
        velnor_runner_github::create_session(&mut session_reply, 10_101, "owner", "admin-token")
            .map_err(|error| error.to_string())?;

    Ok(PollFixture {
        _scratch: scratch,
        journal,
        scope,
        intent_id: bound.intent_id(),
        allocation: bound.allocation().clone(),
        session,
    })
}

#[path = "pressure/completion.rs"]
mod completion;
