//! Adoption provenance remains valid through the ordinary cleanup proof.

use std::path::PathBuf;

use crate::journal::{
    CleanupStopPolicy, Journal, JournalDockerDaemonBinding, LegacyLaunchAdoption,
    RunnerStartObservation,
};

use super::proof::{RUNNER_ID, TestProof};
use super::{complete_outer_removals, drain_children, record_child_inventory, reserve};
use crate::journal::tests::Scratch;
use crate::journal::tests::actions_reconciliation_tests::{reconciliation, started_launch};

struct AdoptedFixture {
    _scratch: Scratch,
    path: PathBuf,
    journal: Journal,
    launch_id: i64,
    binding: JournalDockerDaemonBinding,
    proof: TestProof,
    post_actions: crate::journal::PostActionDisposition,
}

async fn adopted_fixture(label: &str) -> Result<AdoptedFixture, String> {
    let scratch = Scratch::new(label).map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let launch_id = started_launch(&journal)
        .await
        .map_err(|error| format!("Started event: {error}"))?;
    journal
        .record_actions_job_reconciliation(launch_id, &reconciliation(9015, Some("failure")))
        .await
        .map_err(|error| format!("REST reconciliation: {error}"))?;
    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "adopted launch fixture row is missing".to_owned())?;
    let binding = JournalDockerDaemonBinding::new("/run/docker.sock", "logical-engine-a")
        .map_err(|error| error.to_string())?;
    if journal
        .adopt_legacy_launch_on_engine(&row, &binding)
        .await
        .map_err(|error| error.to_string())?
        != LegacyLaunchAdoption::Adopted
    {
        return Err("launch fixture was not adopted".to_owned());
    }
    let post_actions = crate::journal::PostActionDisposition::Interrupted {
        reason_class: "job_interrupted".to_owned(),
    };
    let proof = TestProof::new(
        launch_id,
        row.runner_name.ok_or("runner name is missing")?,
        row.worker_volume.ok_or("worker volume is missing")?,
        row.outer_network_name
            .ok_or("outer network name is missing")?,
        post_actions.clone(),
    )
    .with_observed_actions(88, "opaque-scale-set-job", 2, 9015);
    Ok(AdoptedFixture {
        _scratch: scratch,
        path,
        journal,
        launch_id,
        binding,
        proof,
        post_actions,
    })
}

#[tokio::test]
async fn adopted_launch_can_be_cleaned_and_reopened() -> Result<(), String> {
    let AdoptedFixture {
        _scratch,
        path,
        journal,
        launch_id,
        binding,
        proof,
        post_actions,
    } = adopted_fixture("adopted-launch-cleanup").await?;
    for (attempt, actions_job_id) in [(3, 9015), (2, 9016)] {
        let mut stale = proof.identity();
        stale.observed_attempt = Some(attempt);
        stale.observed_actions_job_id = Some(actions_job_id);
        assert!(
            journal
                .begin_cleanup(&stale, &post_actions, &CleanupStopPolicy::RequireStopped)
                .await
                .is_err(),
            "cleanup must reject a stale Actions attempt or job identity"
        );
    }
    journal
        .begin_cleanup(
            &proof.identity(),
            &post_actions,
            &CleanupStopPolicy::RequireStopped,
        )
        .await
        .map_err(|error| format!("begin cleanup: {error}"))?;
    journal
        .record_cleanup_runner_start(launch_id, RUNNER_ID, RunnerStartObservation::MayHaveStarted)
        .await
        .map_err(|error| format!("record cleanup runner start: {error}"))?;
    record_child_inventory(&journal, launch_id, &proof)
        .await
        .map_err(|error| format!("child inventory: {error}"))?;
    drain_children(&journal, launch_id)
        .await
        .map_err(|error| format!("drain children: {error}"))?;
    drop(journal);

    let journal = Journal::open(&path)
        .await
        .map_err(|error| format!("in-progress adoption cleanup did not reopen: {error}"))?;
    complete_outer_removals(&journal, launch_id)
        .await
        .map_err(|error| format!("outer removals: {error}"))?;
    journal
        .record_physical_cleanup(&proof)
        .await
        .map_err(|error| format!("physical cleanup proof: {error}"))?;
    assert!(reserve(&journal, "next-generation").await?.is_some());
    drop(journal);

    let reopened = Journal::open(&path)
        .await
        .map_err(|error| format!("completed adoption cleanup did not reopen: {error}"))?;
    let row = reopened
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "cleaned adopted launch row is missing".to_owned())?;
    assert!(row.cleanup_proven);
    assert_eq!(
        reopened
            .launch_daemon_binding(launch_id)
            .await
            .map_err(|error| error.to_string())?,
        Some(binding)
    );
    assert_eq!(
        reopened
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        1
    );
    Ok(())
}

#[tokio::test]
async fn cleanup_flag_without_completed_proof_rejects_reopen() -> Result<(), String> {
    let AdoptedFixture {
        _scratch,
        path,
        journal,
        launch_id,
        proof,
        post_actions,
        ..
    } = adopted_fixture("adopted-cleanup-inconsistent").await?;
    journal
        .begin_cleanup(
            &proof.identity(),
            &post_actions,
            &CleanupStopPolicy::RequireStopped,
        )
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);

    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute(
        "UPDATE intents SET cleanup_proven = 1 WHERE id = ?1",
        [launch_id],
    )
    .await
    .map_err(|error| error.to_string())?;
    drop(conn);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}
