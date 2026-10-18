use velnor_runner_github::{
    Exchange, Method, ParsedBatch, SessionRequest, Transport, TransportFail,
};

use crate::IntentState;
use crate::launch::{Drive, Lane};
use crate::launch_harness::{ctx, open};
use crate::scale_set::EnsureError;

#[derive(Default)]
struct UnauthorizedAck {
    requests: Vec<(Method, String)>,
}

impl Transport for UnauthorizedAck {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.requests.push((request.method, request.path.clone()));
        Ok(Exchange {
            status: 401,
            body: Vec::new(),
        })
    }
}

impl Lane for UnauthorizedAck {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }

    fn on_queue(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }

    fn use_github_api(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
}

#[tokio::test]
async fn ack_401_is_not_replayed_and_leaves_the_intent_uncertain() -> Result<(), String> {
    let (_scratch, journal) = open("ack-401-no-replay").await?;
    let (intent, _) = journal
        .begin_launch("ack-401")
        .await
        .map_err(|error| error.to_string())?;
    let mut lane = UnauthorizedAck::default();
    let context: Drive = ctx();
    let batch = ParsedBatch {
        message_id: 59,
        raw_body: String::new(),
        statistics: None,
        jobs: Vec::new(),
    };

    assert!(matches!(
        super::ack_bound(&mut lane, &context, &batch, &journal, intent).await,
        Err(EnsureError::Uncertain)
    ));
    assert_eq!(
        lane.requests,
        [(Method::Delete, "queues/messages/59".to_owned())]
    );
    assert_eq!(
        journal
            .read(intent)
            .await
            .map_err(|error| error.to_string())?,
        IntentState::Uncertain
    );
    Ok(())
}

#[tokio::test]
async fn legacy_acquire_401_is_not_replayed_and_leaves_the_intent_uncertain() -> Result<(), String>
{
    let (_scratch, journal) = open("legacy-acquire-401-no-replay").await?;
    let (intent, _) = journal
        .begin_launch("acquire-401")
        .await
        .map_err(|error| error.to_string())?;
    let mut lane = UnauthorizedAck::default();
    let context: Drive = ctx();
    let error = super::taken(&mut lane, &context, 41).expect_err("401 must stop the acquire");

    assert_eq!(lane.requests.len(), 1);
    assert_eq!(lane.requests[0].0, Method::Post);
    assert!(lane.requests[0].1.ends_with("/acquirejobs"));
    assert!(matches!(
        super::fail_acquire(&journal, intent, error).await,
        Err(EnsureError::Uncertain)
    ));
    assert_eq!(
        journal
            .read(intent)
            .await
            .map_err(|error| error.to_string())?,
        IntentState::Uncertain
    );
    Ok(())
}
