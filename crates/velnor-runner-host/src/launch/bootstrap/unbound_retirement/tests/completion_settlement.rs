// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::reservation_release_ready;
use crate::journal::{ConnectionJournal, Journal, UnboundInitializer};

use super::bound_attachments::attachment_statements;
use super::fixture::FixtureDocker;
use super::seed::{self, Pair};
use super::{BOUND, FAILED, TestResult};

const STATES: &str = "SELECT remove_state || '/' || abort_state FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1";
const PROVISIONING: &str = "SELECT bootstrap_start_state || '/' || resources_bound FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1";
const ROLES: &[&str] = &[
    "runner-root",
    "runner-home",
    "runner-work",
    "runner-tmp",
    "runner-shm",
    "shared-run",
    "dind-root",
    "dind-data",
    "dind-tmp",
    "dind-shm",
    "engine-metadata",
];

async fn seeded(
    dir: &std::path::Path,
    pairs: &[Pair],
) -> TestResult<(
    Journal,
    ConnectionJournal,
    crate::launch::SelectedEngineContext,
)> {
    let socket = dir.join("docker.sock");
    let _fixture = FixtureDocker::start(&socket, pairs)?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let journal = Journal::open(&dir.join("launch.db")).await?;
    let (scope, engine) = seed::started_pairs(&journal, &docker, pairs).await?;
    for pair in pairs.iter().copied() {
        seed::initializer(&journal, pair, "not_requested", "not_requested").await?;
        seed::unguarded(
            &journal,
            &[format!("UPDATE capacity_reservation_storage_initialization SET create_dispatch_state = 'response_received', start_state = 'started', result_state = 'confirmed', metadata_layout_digest = '{}', home_device_major = 0, home_device_minor = 1, home_inode = 1, home_mount_id = 1, work_device_major = 0, work_device_minor = 2, work_inode = 2, work_mount_id = 2 WHERE reservation_id = {}", "a".repeat(64), pair.reservation)],
        )
        .await?;
        seed::attach_handoff(&journal, pair).await?;
    }
    Ok((journal, scope, engine))
}

fn helper(pair: Pair) -> UnboundInitializer {
    UnboundInitializer {
        name: pair.helper_name(),
        id: Some(pair.helper_id()),
        operation_key: format!("{}-1", pair.reservation - 1),
    }
}

async fn settle(
    scope: &ConnectionJournal,
    engine: &crate::launch::SelectedEngineContext,
    pair: Pair,
    helper: &UnboundInitializer,
) -> Result<(), crate::error::HostError> {
    scope
        .settle_removed_storage_initializer(
            pair.reservation,
            engine.engine_id(),
            engine.docker_root_digest(),
            helper,
        )
        .await
}

async fn ready(journal: &Journal) -> TestResult<bool> {
    let connection = journal.connection().await?;
    Ok(reservation_release_ready(&connection, BOUND.reservation).await?)
}

async fn reopen(path: &std::path::Path) -> TestResult<(Journal, ConnectionJournal)> {
    let journal = Journal::open(path).await?;
    let scope = seed::scope(&journal).await?;
    Ok((journal, scope))
}

fn legal_profile_statements(reservation: i64) -> TestResult<Vec<String>> {
    use crate::journal::{
        StorageInodePolicy, StorageMedium, StoragePoolLimit, StorageProfile, StorageRole,
        StorageRoleLimit,
    };
    let generation = "b".repeat(32);
    let pool_id = "1".repeat(64);
    let pool = StoragePoolLimit::new_with_inode_policy(
        pool_id.clone(),
        StorageMedium::Tmpfs,
        StorageInodePolicy::Finite,
        10_000,
        1_000,
        0,
        0,
    )?;
    let mut roles = Vec::new();
    for role in StorageRole::PHYSICAL {
        roles.push(StorageRoleLimit::new(
            role,
            pool_id.clone(),
            StorageMedium::Tmpfs,
            100,
            10,
            100,
        )?);
    }
    for role in [StorageRole::CacheLive, StorageRole::CacheStage] {
        roles.push(StorageRoleLimit::new_alias(
            role,
            pool_id.clone(),
            StorageMedium::Tmpfs,
            StorageRole::RunnerWork,
            50,
            5,
        )?);
    }
    let profile = StorageProfile::new(generation.clone(), vec![pool], roles)?;
    let digest = profile.profile_digest();
    let policy = profile
        .controller_publication()
        .ok_or("profile publication")?
        .digest();
    let mut statements = vec![
        format!(
            "UPDATE host_storage_profiles SET profile_digest = '{digest}' WHERE generation_id = '{generation}'"
        ),
        format!(
            "UPDATE capacity_reservation_provisioning SET profile_digest = '{digest}' WHERE reservation_id = {reservation}"
        ),
        format!(
            "INSERT INTO host_storage_profile_publication (generation_id, policy_digest) VALUES ('{generation}', '{policy}')"
        ),
    ];
    for pool in profile.pools() {
        let pool_id = pool.pool_id();
        let medium = pool.medium().as_str();
        let inode_policy = pool.inode_policy().as_str();
        let max_bytes = pool.max_bytes();
        let max_inodes = pool.max_inodes();
        let reserve_bytes = pool.reserve_bytes();
        let reserve_inodes = pool.reserve_inodes();
        statements.push(format!("INSERT INTO host_storage_profile_pools (generation_id, pool_id, medium, inode_policy, max_bytes, max_inodes, reserve_bytes, reserve_inodes) VALUES ('{generation}', '{pool_id}', '{medium}', '{inode_policy}', {max_bytes}, {max_inodes}, {reserve_bytes}, {reserve_inodes})"));
    }
    for role in profile.roles() {
        let name = role.role().as_str();
        let pool_id = role.pool_id();
        let medium = role.medium().as_str();
        let max_bytes = role.max_bytes();
        let max_inodes = role.max_inodes();
        let retained = role.retained_memory_bytes();
        let alias = role
            .alias_of()
            .map_or("NULL".to_owned(), |alias| format!("'{}'", alias.as_str()));
        statements.push(format!("INSERT INTO host_storage_profile_roles (generation_id, role, pool_id, medium, max_bytes, max_inodes, retained_memory_bytes, alias_of) VALUES ('{generation}', '{name}', '{pool_id}', '{medium}', {max_bytes}, {max_inodes}, {retained}, {alias})"));
    }
    Ok(statements)
}

async fn prime_bound_release(journal: &Journal, pair: Pair) -> TestResult<()> {
    let reservation = pair.reservation;
    let digest = "a".repeat(64);
    let pool = format!("{:064x}", 1);
    let mut statements = legal_profile_statements(reservation)?;
    for index in 1..=9 {
        let id = format!("{index:064x}");
        statements.push(format!("INSERT INTO capacity_reservation_provisioning_pools (reservation_id, pool_id, scope, medium, filesystem_kind, inode_policy, max_bytes, max_inodes, reserve_bytes, reserve_inodes, filesystem_instance_digest, total_bytes, available_bytes, total_inodes, available_inodes) VALUES ({reservation}, '{id}', 'shared-disk', 'disk', 'other', 'finite', 100, 10, 0, 0, '{digest}', 100, 100, 10, 10)"));
    }
    for role in ROLES {
        statements.push(format!("INSERT INTO capacity_reservation_provisioning_roles (reservation_id, role, pool_id, medium, filesystem_kind, inode_policy, planned_filesystem_instance_digest, charge_bytes, charge_inodes, retained_memory_bytes) VALUES ({reservation}, '{role}', '{pool}', 'disk', 'other', 'finite', '{digest}', 1, 1, 0)"));
        statements.push(format!("INSERT INTO capacity_reservation_bound_storage_roles (reservation_id, role, pool_id, filesystem_instance_digest, medium, filesystem_kind, total_bytes, free_bytes, available_bytes, inode_total, inode_free, inode_available, inode_policy, owned_usage_bytes, owned_usage_inodes, charge_bytes, charge_inodes, retained_memory_bytes) VALUES ({reservation}, '{role}', '{pool}', '{digest}', 'disk', 'other', 100, 100, 100, 10, 10, 10, 'finite', 0, 0, 1, 1, 0)"));
    }
    statements.push(format!("INSERT INTO capacity_reservation_provisioning_memory (reservation_id, guest_total_bytes, guest_available_bytes) VALUES ({reservation}, 100, 100)"));
    let runner = pair.container_id("runner");
    let dind = pair.container_id("dind");
    statements.extend(attachment_statements(reservation, &runner, &dind, &digest));
    for (role, container) in [("runner", runner.as_str()), ("dind", dind.as_str())] {
        statements.push(format!("INSERT INTO capacity_reservation_bound_cgroups (reservation_id, role, container_id, path, memory_max_bytes, memory_current_bytes, cpu_quota_micros, cpu_period_micros, pids_max) VALUES ({reservation}, '{role}', '{container}', '/sys/fs/cgroup/{role}', 1073741824, 1024, 100000, 100000, 512)"));
    }
    statements.push(format!("INSERT INTO capacity_reservation_provisioning_cleanups (reservation_id, runner_container_id, dind_container_id, shared_run_volume, runner_home_volume, runner_work_volume, dind_data_volume, metadata_volume, instance_binding_digest, role_mask, containers_absent, volumes_absent, mounts_detached, cgroups_absent) SELECT s.reservation_id, s.runner_container_id, s.dind_container_id, shared.docker_name, home.docker_name, work.docker_name, data.docker_name, metadata.docker_name, s.instance_binding_digest, 2047, 1, 1, 1, 1 FROM capacity_reservation_provisioning_state AS s JOIN capacity_reservation_provisioning_objects AS shared ON shared.reservation_id = s.reservation_id AND shared.role = 'shared-run' JOIN capacity_reservation_provisioning_objects AS home ON home.reservation_id = s.reservation_id AND home.role = 'runner-home' JOIN capacity_reservation_provisioning_objects AS work ON work.reservation_id = s.reservation_id AND work.role = 'runner-work' JOIN capacity_reservation_provisioning_objects AS data ON data.reservation_id = s.reservation_id AND data.role = 'dind-data' JOIN capacity_reservation_provisioning_objects AS metadata ON metadata.reservation_id = s.reservation_id AND metadata.role = 'engine-metadata' WHERE s.reservation_id = {reservation}"));
    statements.push(format!("UPDATE capacity_reservation_provisioning_state SET storage_cleanup_proven = 1 WHERE reservation_id = {reservation}"));
    let probe = "c".repeat(32);
    let container = "d".repeat(64);
    let sha = "e".repeat(64);
    let source = "f".repeat(40);
    let image = format!("sha256:{}", "9".repeat(64));
    statements.push(format!("INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, container_id, helper_role, probe_purpose, request_sha256) SELECT '{probe}', meta.instance_id, r.engine_id, r.docker_root_digest, '{source}', '{image}', '{sha}', 'velnor-metadata-probe-{reservation}', 'resource-probe', '{sha}', 'removed', '{container}', 'resource-probe', 'capacity', '{sha}' FROM journal_meta AS meta JOIN capacity_reservations AS r ON r.id = {reservation} WHERE meta.singleton = 1"));
    statements.push(format!("INSERT INTO capacity_reservation_metadata_settlement_claims (reservation_id, generation, operation_id, connection_id, scale_set_id, instance_id, probe_operation_id, profile_generation_id, profile_digest, engine_id, docker_root_digest, instance_binding_digest) SELECT r.id, 1, r.intent_id, r.connection_id, i.scale_set_id, meta.instance_id, '{probe}', p.profile_generation_id, p.profile_digest, p.engine_id, p.docker_root_digest, s.instance_binding_digest FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id JOIN journal_meta AS meta ON meta.singleton = 1 WHERE r.id = {reservation}"));
    statements.push(format!("INSERT INTO capacity_reservation_metadata_probe_cleanups (reservation_id, generation, probe_operation_id, engine_id, docker_root_digest, operation_name, container_id, phase, lease_released) SELECT r.id, 1, '{probe}', r.engine_id, r.docker_root_digest, 'velnor-metadata-probe-{reservation}', '{container}', 'removed', 1 FROM capacity_reservations AS r WHERE r.id = {reservation}"));
    statements.push(format!("INSERT INTO capacity_reservation_metadata_settlements (reservation_id, generation, probe_operation_id, sample_generation_id, profile_generation_id, profile_digest, engine_id, docker_root_digest, observed_at_unix_ms, outcome) SELECT r.id, 1, '{probe}', '{probe}', p.profile_generation_id, p.profile_digest, p.engine_id, p.docker_root_digest, 1700000000000, 'settled' FROM capacity_reservations AS r JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id WHERE r.id = {reservation}"));
    statements.push(format!("UPDATE capacity_reservation_networks SET remove_state = 'removed' WHERE reservation_id = {reservation}"));
    seed::unguarded(journal, &statements).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_completion_removal_settles_only_its_own_reservations_initializer() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (journal, scope, engine) = seeded(dir.path(), &[FAILED, BOUND]).await?;

    settle(&scope, &engine, FAILED, &helper(FAILED)).await?;

    assert_eq!(
        seed::text(&journal, STATES, FAILED.reservation).await?,
        "removed/not_requested"
    );
    assert_eq!(
        seed::text(&journal, STATES, BOUND.reservation).await?,
        "not_requested/not_requested"
    );
    assert!(
        scope
            .unretired_storage_initializer(
                FAILED.reservation,
                engine.engine_id(),
                engine.docker_root_digest()
            )
            .await?
            .is_none()
    );
    assert!(
        scope
            .unretired_storage_initializer(
                BOUND.reservation,
                engine.engine_id(),
                engine.docker_root_digest()
            )
            .await?
            .is_some()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn settling_twice_and_after_a_crash_between_the_two_writes_converges() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("launch.db");
    let (journal, scope, engine) = seeded(dir.path(), &[BOUND]).await?;
    prime_bound_release(&journal, BOUND).await?;
    seed::unguarded(
        &journal,
        &["UPDATE capacity_reservation_storage_initialization SET remove_state = 'requested' WHERE reservation_id = 4".to_owned()],
    )
    .await?;
    assert_eq!(
        seed::text(&journal, STATES, BOUND.reservation).await?,
        "requested/not_requested"
    );
    assert_eq!(
        seed::text(&journal, PROVISIONING, BOUND.reservation).await?,
        "started/1"
    );
    assert!(!ready(&journal).await?);
    drop(scope);
    drop(journal);

    let (journal, scope) = reopen(&path).await?;
    assert_eq!(
        seed::text(&journal, STATES, BOUND.reservation).await?,
        "requested/not_requested"
    );
    assert_eq!(
        seed::text(&journal, PROVISIONING, BOUND.reservation).await?,
        "started/1"
    );
    settle(&scope, &engine, BOUND, &helper(BOUND)).await?;
    settle(&scope, &engine, BOUND, &helper(BOUND)).await?;
    assert_eq!(
        seed::text(&journal, STATES, BOUND.reservation).await?,
        "removed/not_requested"
    );
    assert_eq!(
        seed::text(&journal, PROVISIONING, BOUND.reservation).await?,
        "started/1"
    );
    drop(scope);
    drop(journal);

    let (journal, _scope) = reopen(&path).await?;
    assert!(ready(&journal).await?);
    assert_eq!(
        seed::text(&journal, PROVISIONING, BOUND.reservation).await?,
        "started/1"
    );
    assert_eq!(
        seed::text(&journal, STATES, BOUND.reservation).await?,
        "removed/not_requested"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_helper_identity_that_differs_from_the_journal_settles_nothing() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (journal, scope, engine) = seeded(dir.path(), &[FAILED, BOUND]).await?;
    for forged in [
        UnboundInitializer {
            id: Some(BOUND.helper_id()),
            ..helper(FAILED)
        },
        UnboundInitializer {
            name: BOUND.helper_name(),
            ..helper(FAILED)
        },
        UnboundInitializer {
            operation_key: "9-9".to_owned(),
            ..helper(FAILED)
        },
        UnboundInitializer {
            id: None,
            ..helper(FAILED)
        },
    ] {
        assert!(settle(&scope, &engine, FAILED, &forged).await.is_err());
    }
    assert!(
        settle(&scope, &engine, BOUND, &helper(FAILED))
            .await
            .is_err()
    );
    for pair in [FAILED, BOUND] {
        assert_eq!(
            seed::text(&journal, STATES, pair.reservation).await?,
            "not_requested/not_requested"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_initializer_remains_valid_after_journal_reopen() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("launch.db");
    let (journal, scope, engine) = seeded(dir.path(), &[BOUND]).await?;
    prime_bound_release(&journal, BOUND).await?;
    settle(&scope, &engine, BOUND, &helper(BOUND))
        .await
        .map_err(|error| format!("initializer settlement: {error}"))?;
    drop(scope);
    drop(journal);

    let (journal, _scope) = reopen(&path)
        .await
        .map_err(|error| format!("journal reload: {error}"))?;
    assert!(ready(&journal).await?);
    assert_eq!(
        seed::text(&journal, STATES, BOUND.reservation).await?,
        "removed/not_requested"
    );
    Ok(())
}
