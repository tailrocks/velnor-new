//! A busy slot does not acknowledge a scale assignment.

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail, create_session,
};

use super::inspect_tests::{DockerStub, launch_row, within};
use super::steps::{Idle, idle};
use super::{Ready, drive_ready};
use crate::IntentState;
use crate::launch_harness::{Mode, Script, absent, assigned_wait, available, open};

const SESSION: &[u8] = br#"{"sessionId":"sess","messageQueueUrl":"https://queues.example/messages","messageQueueAccessToken":"queue-token"}"#;

struct SessionTransport;

impl Transport for SessionTransport {
    fn exchange(&mut self, _request: &SessionRequest) -> Result<Exchange, TransportFail> {
        Ok(Exchange {
            status: 200,
            body: SESSION.to_vec(),
        })
    }
}

fn queue_session() -> Result<QueueSession, String> {
    create_session(&mut SessionTransport, 1, "owner", "admin-token").map_err(|err| err.to_string())
}

#[tokio::test]
async fn busy_slot_does_not_ack_scale_assignment() -> Result<(), String> {
    let (scratch, journal) = open("busy-scale").await?;
    launch_row(&journal).await?;
    let polls = [
        assigned_wait(11, 1),
        available(&[4]),
        assigned_wait(12, 0),
        Poll::Empty,
        available(&[4, 5]),
    ];
    let classes = [
        Idle::Scale,
        Idle::Launch,
        Idle::Ack,
        Idle::Empty,
        Idle::Blocked,
    ];
    for (polled, class) in polls.iter().zip(classes) {
        assert_eq!(idle(polled), class);
    }
    let stub = DockerStub::open(Vec::new())?;
    let session = queue_session()?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let mut started_any = false;
    for polled in &polls {
        let started = within(
            drive_ready(
                &mut script,
                Ready {
                    set_id: 1,
                    session: &session,
                    admin_token: "admin-token",
                    path: "queues/messages".to_owned(),
                    polled,
                },
                &journal,
                &stub.docker,
                1,
            ),
            "busy drive",
        )
        .await?
        .map_err(|err| err.to_string())?;
        started_any |= started.is_some();
    }
    stub.finish().await?;
    assert!(!started_any);
    assert_eq!(script.calls, [] as [&str; 0]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Pending);
    absent(&scratch.file())
}
