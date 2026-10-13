//! Physical cleanup is ordered, durable, and the only launch-slot release proof.

use std::num::NonZeroU32;

use velnor_runner_github::{InnerJob, InnerKind};

use crate::Outcome;
use crate::journal::{
    CapacityClaim, CleanupChildren, CleanupStopPolicy, Journal, LaunchEffectState,
    PostActionDisposition, ReplayRoute, RunnerStartObservation, ScopedLaunchIdentity,
};

use super::Scratch;

mod proof;
use proof::{
    CHILD_CONTAINER_ID, CHILD_NETWORK_ID, DIND_ID, OUTER_NETWORK_ID, RUNNER_ID, TestProof,
};

#[tokio::test]
async fn cleanup_order_survives_reopen_and_only_full_proof_releases_capacity() -> Result<(), String>
{
    let scratch = Scratch::new("ordered-physical-cleanup").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let (id, proof) = start_generation(&journal).await?;
    record_child_inventory(&journal, id, &proof).await?;
    drain_children(&journal, id).await?;

    drop(journal);
    let journal = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert!(journal.cleanup_before(id, "dind-removal").await.is_err());
    assert!(journal.cleanup_before(id, "volume-removal").await.is_err());
    assert_capacity_full(&journal).await?;

    complete_outer_removals(&journal, id).await?;
    journal
        .record_physical_cleanup(&proof)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_physical_cleanup(&proof)
        .await
        .map_err(|error| error.to_string())?;
    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or("cleanup row missing")?;
    assert!(row.cleanup_proven);
    assert_eq!(row.launch_effect, LaunchEffectState::MayHaveEffect);
    assert!(reserve(&journal, "next-generation").await?.is_some());
    Ok(())
}

async fn start_generation(journal: &Journal) -> Result<(i64, TestProof), String> {
    let id = reserve(journal, "cleanup-target")
        .await?
        .ok_or("target launch was not reserved")?;
    let runner_name = format!("g2_{id}");
    let volume = format!("worker-{id}");
    let network_name = format!("velnor-net-{id}");
    persist_generation_identity(journal, id, &runner_name, &volume, &network_name).await?;
    journal
        .finish(id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    for kind in [InnerKind::Started, InnerKind::Completed] {
        journal
            .observe_runner_event(&runner_event(kind, &runner_name))
            .await
            .map_err(|error| error.to_string())?;
    }

    let post_actions = PostActionDisposition::Interrupted {
        reason_class: "job_interrupted".to_owned(),
    };
    let proof = TestProof::new(
        id,
        runner_name.clone(),
        volume,
        network_name,
        post_actions.clone(),
    );
    journal
        .begin_cleanup(
            &proof.identity(),
            &post_actions,
            &CleanupStopPolicy::RequireStopped,
        )
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_cleanup_runner_start(id, RUNNER_ID, RunnerStartObservation::MayHaveStarted)
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .cleanup_before(id, "dind-termination")
            .await
            .is_err()
    );
    assert!(journal.cleanup_before(id, "runner-removal").await.is_err());
    assert!(
        journal
            .record_cleanup_diagnostics(id, &proof.diagnostics())
            .await
            .is_err()
    );
    Ok((id, proof))
}

async fn persist_generation_identity(
    journal: &Journal,
    id: i64,
    runner_name: &str,
    volume: &str,
    network_name: &str,
) -> Result<(), String> {
    journal
        .record_launch_effect_intent(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_launch_identity(
            id,
            Some(700),
            Some(701),
            Some(45),
            Some("requested-job"),
            runner_name,
        )
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker_volume(id, volume)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_outer_network_intent(id, network_name)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_outer_network_id(id, OUTER_NETWORK_ID)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(id, Some(RUNNER_ID), Some(DIND_ID))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_runner_start_intent(id, RUNNER_ID)
        .await
        .map_err(|error| error.to_string())
}

async fn record_child_inventory(
    journal: &Journal,
    id: i64,
    proof: &TestProof,
) -> Result<(), String> {
    checkpoint(journal, id, "runner-termination").await?;
    journal
        .cleanup_before(id, "diagnostics-retention")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_cleanup_diagnostics(id, &proof.diagnostics())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .cleanup_after(id, "diagnostics-retention")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .cleanup_before(id, "child-enumeration")
        .await
        .map_err(|error| error.to_string())?;
    let children = CleanupChildren {
        containers: vec![CHILD_CONTAINER_ID.to_owned()],
        networks: vec![CHILD_NETWORK_ID.to_owned()],
    };
    journal
        .observe_cleanup_children(id, &children)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .cleanup_after(id, "child-enumeration")
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn drain_children(journal: &Journal, id: i64) -> Result<(), String> {
    let children = CleanupChildren {
        containers: vec![CHILD_CONTAINER_ID.to_owned()],
        networks: vec![CHILD_NETWORK_ID.to_owned()],
    };
    assert!(
        journal
            .cleanup_before(id, "children-drained")
            .await
            .is_err()
    );
    let unknown = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    assert!(
        journal
            .cleanup_before(id, &format!("child-container:{unknown}"))
            .await
            .is_err()
    );
    checkpoint(
        journal,
        id,
        &format!("child-container:{CHILD_CONTAINER_ID}"),
    )
    .await?;
    assert!(
        journal
            .cleanup_before(id, "children-drained")
            .await
            .is_err()
    );
    checkpoint(journal, id, &format!("child-network:{CHILD_NETWORK_ID}")).await?;
    journal
        .cleanup_before(id, "children-drained")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_cleanup_children_drained(id, &children)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .cleanup_after(id, "children-drained")
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn complete_outer_removals(journal: &Journal, id: i64) -> Result<(), String> {
    checkpoint(journal, id, "dind-termination").await?;
    checkpoint(journal, id, "runner-removal").await?;
    checkpoint(journal, id, "dind-removal").await?;
    assert!(journal.cleanup_before(id, "volume-removal").await.is_err());
    checkpoint(journal, id, "outer-network-removal").await?;
    checkpoint(journal, id, "volume-removal").await?;
    assert_capacity_full(journal).await
}

async fn reserve(journal: &Journal, suffix: &str) -> Result<Option<i64>, String> {
    let route = ReplayRoute {
        destination: "https://api.github.com",
        registration_scope: "organization",
        owner: "velnor",
        repository: "",
        runner_group_id: 7,
        runner_group_name: "linux",
        scale_set_id: 11,
        scale_set_name: "workers",
    };
    let identity = ScopedLaunchIdentity::new(route, "session-full-id", 700, suffix_id(suffix))
        .map_err(|error| error.to_string())?;
    let capacity = NonZeroU32::new(1).ok_or("capacity")?;
    match journal
        .reserve_launch_if_accepting(&identity, capacity)
        .await
        .map_err(|error| error.to_string())?
    {
        CapacityClaim::New(id) => Ok(Some(id)),
        CapacityClaim::CapacityFull { occupied: 1, .. } => Ok(None),
        other => Err(format!("unexpected reservation result: {other:?}")),
    }
}

fn suffix_id(suffix: &str) -> i64 {
    match suffix {
        "cleanup-target" => 701,
        "next-generation" => 702,
        _ => 703,
    }
}

async fn assert_capacity_full(journal: &Journal) -> Result<(), String> {
    assert_eq!(reserve(journal, "next-generation").await?, None);
    Ok(())
}

async fn checkpoint(journal: &Journal, id: i64, key: &str) -> Result<(), String> {
    journal
        .cleanup_before(id, key)
        .await
        .map_err(|error| format!("before {key}: {error}"))?;
    journal
        .cleanup_after(id, key)
        .await
        .map_err(|error| format!("after {key}: {error}"))?;
    Ok(())
}

fn runner_event(kind: InnerKind, name: &str) -> InnerJob {
    InnerJob {
        kind,
        request_id: None,
        job_id: Some("observed-job".to_owned()),
        workflow_run_id: Some(45),
        owner_name: None,
        repository_name: None,
        event_name: None,
        labels: Vec::new(),
        runner_id: Some(99),
        runner_name: Some(name.to_owned()),
        result: None,
        fields: Vec::new(),
    }
}
