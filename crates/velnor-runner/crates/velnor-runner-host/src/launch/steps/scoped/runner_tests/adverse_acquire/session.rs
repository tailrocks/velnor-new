use crate::journal::ConnectionJournal;
use crate::journal::HeaderAllocation;
use crate::launch::steps::scoped::runner::tests::support::{Fixture, SET_ID};
use velnor_runner_github::{
    Exchange, Method, QueueSession, SessionRequest, Transport, TransportFail,
};

#[derive(Default)]
struct SessionApi;

impl Transport for SessionApi {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Post);
        Ok(Exchange {
            status: 200,
            body: br#"{"sessionId":"session-no-acquire-replay","messageQueueUrl":"https://queue.example.invalid/","messageQueueAccessToken":"queue-token"}"#.to_vec(),
        })
    }
}

pub(super) async fn prepare(
    fixture: &Fixture,
) -> Result<(QueueSession, i64, HeaderAllocation), String> {
    fixture
        .journal
        .bootstrap_header_authority()
        .await
        .map_err(|error| error.to_string())?;
    let prepared = fixture
        .scope
        .prepare_session_create("0123456789abcdef0123456789abcdef")
        .await
        .map_err(|error| error.to_string())?;
    let claim = fixture
        .scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Session claim is absent.".to_owned())?;
    let bound = fixture
        .scope
        .record_created_session(&claim, "session-no-acquire-replay", 0)
        .await
        .map_err(|error| error.to_string())?;
    let session = session()?;
    Ok((session, bound.intent_id(), bound.allocation().clone()))
}

pub(super) fn session() -> Result<QueueSession, String> {
    let mut api = SessionApi;
    velnor_runner_github::create_session(&mut api, SET_ID, "velnor-host", "admin-token")
        .map_err(|error| error.to_string())
}

pub(super) async fn reopened_allocation(
    scope: &ConnectionJournal,
    intent_id: i64,
) -> Result<HeaderAllocation, String> {
    scope
        .session_header_allocation(intent_id)
        .await
        .map_err(|error| error.to_string())
}
