use super::{
    DockerStub, EnsureError, IntentState, Mode, Script, StartTurn, Started, absent, assigned_wait,
    http, open, ready, rest, start_turn, zero_assignment_session,
};

#[tokio::test]
async fn zero_initial_census_and_positive_poll_keep_jit_conflict_unacked() -> Result<(), String> {
    let (scratch, journal) = open("turn-census-conflict").await?;
    let session = zero_assignment_session()?;
    assert_eq!(
        session
            .statistics()
            .map(velnor_runner_github::Statistics::assigned_population),
        Some(0)
    );
    let polled = assigned_wait(91, 1);
    assert_eq!(crate::launch::idle(&polled), crate::launch::Idle::Scale);

    let docker = DockerStub::open(vec![http(
        200,
        r#"{"ID":"test-engine","DockerRootDir":"/var/lib/docker"}"#,
    )])?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::JitConflict,
    };
    let mut workers: Vec<Started> = Vec::new();
    let result = start_turn(
        &mut script,
        &mut workers,
        StartTurn {
            ready: ready(&session, &polled),
            journal: &journal,
            docker: &docker.docker,
            capacity: 2,
            rest: rest(),
            stop: false,
        },
    )
    .await;
    drop(docker);

    assert_eq!(result, Err(EnsureError::Conflict));
    assert_eq!(script.calls, ["jit"]);
    assert_eq!(workers, Vec::new());
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].subject, "m91");
    assert_eq!(rows[0].state, IntentState::Failed);
    assert!(rows[0].docker_id.is_none());
    assert!(rows[0].dind_id.is_none());
    assert!(rows[0].worker_volume.is_none());
    absent(&scratch.file())
}
