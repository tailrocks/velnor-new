//! Job starts stay closed until trusted guest metrics are available.

use super::super::{StartTurn, start_turn};
use super::{ready, rest, zero_assignment_session};
use crate::launch::drive::GuestAdmission;
use crate::launch::inspect_tests::DockerStub;
use crate::launch_harness::{Mode, Script, absent, assigned_wait, open};
use crate::worker::Started;

#[tokio::test]
async fn unavailable_guest_sample_blocks_acquire_and_worker_start() -> Result<(), String> {
    let (scratch, journal) = open("turn-unavailable-guest-sample").await?;
    let session = zero_assignment_session()?;
    let polled = assigned_wait(90, 1);
    let docker = DockerStub::open(Vec::new())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let mut workers: Vec<Started> = Vec::new();
    let mut rest = rest();
    rest.guest_admission = GuestAdmission::Unavailable;

    let result = start_turn(
        &mut script,
        &mut workers,
        StartTurn {
            ready: ready(&session, &polled),
            journal: &journal,
            docker: &docker.docker,
            capacity: 2,
            rest,
            stop: false,
        },
    )
    .await;
    let requests = docker.finish().await?;

    assert_eq!(result, Ok(false));
    assert!(requests.is_empty());
    assert!(script.calls.is_empty());
    assert!(workers.is_empty());
    assert!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    absent(&scratch.file())
}
