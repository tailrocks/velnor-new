use std::time::Duration;

use super::{Discovery, JobCapacity, OccupiedResources, calculate, discover_after, guest_totals};
use crate::EnsureError;
use crate::launch::inspect_tests::{DockerStub, hanging, http, journal};
use crate::worker::test_resource_budget;

#[path = "resource_capacity_edge_tests.rs"]
mod edge_tests;

const ENGINE: &str = "resource-engine";
const RUNNER_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIND_ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const VOLUME: &str = "w0123456789abcdef0123456789abcdef";

#[test]
fn configured_pair_cost_and_occupied_limits_bound_static_capacity() -> Result<(), String> {
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let guest =
        guest_totals(Some(18), Some(128 * 1024 * 1024 * 1024)).ok_or("guest totals missing")?;
    let occupied = OccupiedResources {
        permits: 1,
        nano_cpus: 4_000_000_000,
        memory_bytes: 8 * GIB,
    };

    let capacity = calculate(guest, occupied, budget, 8).map_err(|error| error.to_string())?;

    assert_eq!(capacity.total(), 4);
    Ok(())
}

#[test]
fn exact_old_container_limits_and_changed_current_budget_use_checked_totals() -> Result<(), String>
{
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let guest =
        guest_totals(Some(5), Some(8 * 1024 * 1024 * 1024)).ok_or("guest totals missing")?;
    let occupied = OccupiedResources {
        permits: 1,
        nano_cpus: 4_000_000_000,
        memory_bytes: 8 * GIB,
    };

    let capacity = calculate(guest, occupied, budget, 8).map_err(|error| error.to_string())?;

    assert_eq!(capacity.total(), 1);
    assert!(capacity.permits_start());
    assert_eq!(
        calculate(
            guest,
            OccupiedResources {
                nano_cpus: u64::MAX,
                ..occupied
            },
            budget,
            8,
        ),
        Ok(JobCapacity {
            total: 1,
            pair_fits: true,
        })
    );
    Ok(())
}

#[test]
fn occupied_permits_do_not_turn_impossible_guest_totals_into_a_start_permit() -> Result<(), String>
{
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let pair = budget.pair();
    let occupied = OccupiedResources {
        permits: 1,
        nano_cpus: pair.cpu_millicores * 1_000_000,
        memory_bytes: pair.memory_bytes,
    };
    let pair_memory = i64::try_from(pair.memory_bytes).map_err(|_| "pair memory overflow")?;
    for guest in [
        guest_totals(Some(1), Some(pair_memory)).ok_or("cpu totals missing")?,
        guest_totals(Some(8), Some(pair_memory - 1)).ok_or("memory totals missing")?,
    ] {
        let capacity = calculate(guest, occupied, budget, 8).map_err(|error| error.to_string())?;
        assert_eq!(capacity.total(), 1);
        assert_eq!(capacity.poll_header(), 1);
        assert!(!capacity.permits_start());
    }
    Ok(())
}

#[tokio::test]
async fn exact_owned_pair_uses_inspected_limits_and_bound_engine() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-pair").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    let (row, _) = journal
        .begin_launch("m100")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker_volume(row, VOLUME)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(row, Some(RUNNER_ID), Some(DIND_ID))
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![
        http(200, &docker_info(18, 128 * GIB, ENGINE)),
        http(
            200,
            &container(RUNNER_ID, VOLUME, "runner", 1_000_000_000, 2 * GIB),
        ),
        http(
            200,
            &container(DIND_ID, VOLUME, "dind", 3_000_000_000, 6 * GIB),
        ),
    ])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_secs(1),
    )
    .await;
    let requests = stub.finish().await?;

    assert_eq!(
        capacity,
        Discovery::Available(JobCapacity {
            total: 4,
            pair_fits: true
        })
    );
    assert_eq!(requests.len(), 3);
    assert!(requests[1].contains(&format!("{VOLUME}-runner")));
    assert!(requests[2].contains(&format!("{VOLUME}-dind")));
    Ok(())
}

#[tokio::test]
async fn engine_identity_mismatch_fails_closed_before_accounting() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-engine-mismatch").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(200, &docker_info(18, 128 * GIB, "other-engine"))])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_secs(1),
    )
    .await;
    let requests = stub.finish().await?;

    assert_eq!(
        capacity,
        Discovery::Untrusted(capacity_error("docker capacity"))
    );
    assert_eq!(requests.len(), 1);
    Ok(())
}

#[tokio::test]
async fn docker_info_failure_cannot_fall_back_to_the_configured_ceiling() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-info-failure").await?;
    let stub = DockerStub::open(vec![http(500, r#"{"message":"private detail"}"#)])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_secs(1),
    )
    .await;
    let requests = stub.finish().await?;

    assert_eq!(
        capacity,
        Discovery::Untrusted(capacity_error("docker capacity"))
    );
    assert_eq!(requests.len(), 1);
    assert!(!format!("{capacity:?}").contains("private detail"));
    Ok(())
}

#[tokio::test]
async fn absent_half_is_charged_at_current_limit_only_after_exact_404() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-absent-half").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    let (row, _) = journal
        .begin_launch("m101")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker_volume(row, VOLUME)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(row, Some(RUNNER_ID), None)
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![
        http(200, &docker_info(5, 8 * GIB, ENGINE)),
        http(
            200,
            &container(RUNNER_ID, VOLUME, "runner", 1_000_000_000, 2 * GIB),
        ),
        http(404, r#"{"message":"not found"}"#),
    ])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_secs(1),
    )
    .await;
    stub.finish().await?;

    assert_eq!(
        capacity,
        Discovery::Available(JobCapacity {
            total: 1,
            pair_fits: true
        })
    );
    Ok(())
}

#[tokio::test]
async fn recorded_id_absence_requires_both_name_and_id_not_found() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-ambiguous-absent-half").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    let (row, _) = journal
        .begin_launch("m105")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker_volume(row, VOLUME)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(row, Some(RUNNER_ID), Some(DIND_ID))
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![
        http(200, &docker_info(18, 128 * GIB, ENGINE)),
        http(
            200,
            &container(RUNNER_ID, VOLUME, "runner", 1_000_000_000, 2 * GIB),
        ),
        http(404, r#"{"message":"not found"}"#),
        http(500, r#"{"message":"private engine detail"}"#),
    ])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_secs(1),
    )
    .await;
    let requests = stub.finish().await?;

    assert_eq!(capacity, Discovery::Unavailable);
    assert_eq!(requests.len(), 4);
    Ok(())
}

#[tokio::test]
async fn missing_limits_or_unowned_containers_fail_closed() -> Result<(), String> {
    let (_scratch, journal) = journal("resource-invalid-limits").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    let (row, _) = journal
        .begin_launch("m102")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker_volume(row, VOLUME)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(row, Some(RUNNER_ID), None)
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![
        http(200, &docker_info(18, 128 * GIB, ENGINE)),
        http(
            200,
            &format!(
                r#"{{"Id":"{RUNNER_ID}","Name":"/{VOLUME}-runner","Config":{{"Labels":{{"velnor.volume":"{VOLUME}","velnor.worker":"{VOLUME}","velnor.role":"runner"}}}},"HostConfig":{{"Memory":2147483648,"MemorySwap":2147483648}}}}"#
            ),
        ),
    ])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_secs(1),
    )
    .await;
    stub.finish().await?;

    assert_eq!(capacity, Discovery::Unavailable);
    Ok(())
}

#[tokio::test]
async fn verified_engine_inspect_timeout_preserves_capacity_unavailable_state() -> Result<(), String>
{
    let (_scratch, journal) = journal("resource-verified-inspect-timeout").await?;
    journal
        .bind_engine(ENGINE)
        .await
        .map_err(|error| error.to_string())?;
    let (row, _) = journal
        .begin_launch("m109")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker_volume(row, VOLUME)
        .await
        .map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![
        http(200, &docker_info(18, 128 * GIB, ENGINE)),
        hanging(),
    ])?;

    let capacity = discover_after(
        &stub.docker,
        &journal,
        test_resource_budget().map_err(|error| error.to_string())?,
        8,
        Duration::from_millis(300),
    )
    .await;
    drop(stub);

    assert_eq!(capacity, Discovery::Unavailable);
    Ok(())
}

fn docker_info(cpus: u32, memory: u64, engine: &str) -> String {
    format!(r#"{{"ID":"{engine}","NCPU":{cpus},"MemTotal":{memory}}}"#)
}

fn container(id: &str, volume: &str, role: &str, cpus: i64, memory: u64) -> String {
    format!(
        r#"{{"Id":"{id}","Name":"/{volume}-{role}","Config":{{"Labels":{{"velnor.volume":"{volume}","velnor.worker":"{volume}","velnor.role":"{role}"}}}},"HostConfig":{{"NanoCpus":{cpus},"Memory":{memory},"MemorySwap":{memory}}}}}"#
    )
}

fn capacity_error(step: &'static str) -> EnsureError {
    EnsureError::Unexpected { status: 0, step }
}

const GIB: u64 = 1024 * 1024 * 1024;
