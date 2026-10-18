use velnor_runner_github::{
    Exchange, QueueSession, SessionRequest, Transport, TransportFail, create_session,
};

use super::drive::GuestAdmission;
use super::inspect_tests::{DockerStub, within};
use super::{Ready, Rest, drive_ready};
use crate::launch_harness::{Mode, Script, available, open};
use crate::scale_set::EnsureError;
use crate::worker::{
    ResourceBudget, guest_image::test_support::TestGuestImage, test_resource_budget,
};

const SESSION: &[u8] = br#"{"sessionId":"session","messageQueueUrl":"https://queue.example/messages","messageQueueAccessToken":"queue-token"}"#;

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
    create_session(&mut SessionTransport, 1, "owner", "admin-token")
        .map_err(|error| error.to_string())
}

#[tokio::test]
async fn incomplete_source_stops_scoped_launch_before_docker() -> Result<(), String> {
    let (scratch, journal) = open("source-before-docker").await?;
    let fixture = TestGuestImage::new().map_err(|error| error.to_string())?;
    let connection = &fixture.connection;
    let scope = journal
        .connection_scope(connection)
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(Vec::new())?;
    let session = queue_session()?;
    let polled = available(&[4]);
    let mut lane = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let budget: ResourceBudget = test_resource_budget().map_err(|error| error.to_string())?;

    let result = within(
        drive_ready(
            &mut lane,
            Ready {
                set_id: 1,
                session: &session,
                admin_token: "admin-token",
                path: "queues/messages".to_owned(),
                polled: &polled,
            },
            &journal,
            &stub.docker,
            1,
            Rest {
                owner: "acme",
                repo: "project",
                pat: "pat",
                resource_budget: Some(budget),
                static_capacity: true,
                guest_admission: GuestAdmission::FreshSample,
                connection: Some(connection),
                host_journal: Some(&journal),
                release_read_secret: None,
                scoped_journal: Some(&scope),
                allocated_header: 1,
                source_authorized: false,
                admission_state: None,
                worker_images: Some(&fixture.images),
                selected_engine: Some(&fixture.selected_engine),
                guest_image: Some(&fixture.file),
            },
        ),
        "source authorization",
    )
    .await?;
    let requests = stub.finish().await?;

    assert_eq!(
        result,
        Err(EnsureError::SourceBlocked {
            reason: "source_metadata_missing",
        })
    );
    assert!(requests.is_empty(), "Docker received a request");
    assert!(
        lane.calls.is_empty(),
        "the Scale Set lane received a request"
    );
    assert!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    drop(scope);
    drop(scratch);
    Ok(())
}
