//! Job starts stay closed until trusted guest metrics are available.

use super::super::{StartTurn, start_turn};
use super::{ready, rest, zero_assignment_session};
use crate::launch::capacity::{install_job_capacity, job_capacity};
use crate::launch::drive::GuestAdmission;
use crate::launch::inspect_tests::DockerStub;
use crate::launch::resource_capacity::{GuestTotals, OccupiedResources, calculate};
use crate::launch_harness::{Mode, Script, absent, assigned_wait, open};
use crate::worker::{ResourceBudget, Started, test_resource_budget};
use crate::{IntentState, Outcome};

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
    assert_eq!(requests, [] as [std::string::String; 0]);
    assert_eq!(script.calls, [] as [&str; 0]);
    assert_eq!(workers, [] as [crate::worker::Started; 0]);
    assert_eq!(
        journal.rows().await.map_err(|error| error.to_string())?,
        [] as [crate::IntentRow; 0]
    );
    absent(&scratch.file())
}

#[tokio::test]
async fn no_fit_capacity_keeps_header_one_but_blocks_start() -> Result<(), String> {
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let pair = budget.pair();
    no_fit_case(
        "cpu-capacity-denied",
        GuestTotals {
            cpus: 1,
            memory_bytes: pair.memory_bytes,
        },
        budget,
    )
    .await?;
    no_fit_case(
        "memory-capacity-denied",
        GuestTotals {
            cpus: 8,
            memory_bytes: pair.memory_bytes - 1,
        },
        budget,
    )
    .await
}

#[tokio::test]
async fn safe_idless_redelivery_cannot_start_when_guest_cannot_fit_a_pair() -> Result<(), String> {
    let (scratch, journal) = open("safe-idless-zero-fit-start").await?;
    let (row, _) = journal
        .begin_launch("m112")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let pair = budget.pair();
    let capacity = calculate(
        GuestTotals {
            cpus: 1,
            memory_bytes: pair.memory_bytes,
        },
        OccupiedResources {
            permits: 1,
            nano_cpus: pair.cpu_millicores * 1_000_000,
            memory_bytes: pair.memory_bytes,
        },
        budget,
        8,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(capacity.total(), 1);
    assert!(!capacity.permits_start());
    let _capacity = install_job_capacity(capacity.poll_header());

    let session = zero_assignment_session()?;
    let polled = assigned_wait(112, 1);
    let docker = DockerStub::open(Vec::new())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let mut workers: Vec<Started> = Vec::new();
    let mut rest = rest();
    rest.static_capacity = capacity.permits_start();

    let result = start_turn(
        &mut script,
        &mut workers,
        StartTurn {
            ready: ready(&session, &polled),
            journal: &journal,
            docker: &docker.docker,
            capacity: job_capacity(),
            rest,
            stop: false,
        },
    )
    .await;
    let requests = docker.finish().await?;

    assert_eq!(result, Ok(false));
    assert_eq!(requests, [] as [std::string::String; 0]);
    assert_eq!(script.calls, [] as [&str; 0]);
    assert_eq!(workers, [] as [crate::worker::Started; 0]);
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert!(rows[0].worker_volume.is_none());
    absent(&scratch.file())
}

async fn no_fit_case(name: &str, guest: GuestTotals, budget: ResourceBudget) -> Result<(), String> {
    let capacity = calculate(guest, OccupiedResources::default(), budget, 8)
        .map_err(|error| error.to_string())?;
    assert_eq!(capacity.total(), 0);
    assert!(!capacity.permits_start());
    let _capacity = install_job_capacity(capacity.poll_header());
    assert_eq!(job_capacity(), 1);

    let (scratch, journal) = open(name).await?;
    let session = zero_assignment_session()?;
    let polled = assigned_wait(90, 1);
    let docker = DockerStub::open(Vec::new())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let mut workers: Vec<Started> = Vec::new();
    let mut rest = rest();
    rest.static_capacity = capacity.permits_start();

    let result = start_turn(
        &mut script,
        &mut workers,
        StartTurn {
            ready: ready(&session, &polled),
            journal: &journal,
            docker: &docker.docker,
            capacity: job_capacity(),
            rest,
            stop: false,
        },
    )
    .await;
    let requests = docker.finish().await?;

    assert_eq!(result, Ok(false));
    assert_eq!(requests, [] as [std::string::String; 0]);
    assert_eq!(script.calls, [] as [&str; 0]);
    assert_eq!(workers, [] as [crate::worker::Started; 0]);
    assert_eq!(
        journal.rows().await.map_err(|error| error.to_string())?,
        [] as [crate::IntentRow; 0]
    );
    absent(&scratch.file())
}
