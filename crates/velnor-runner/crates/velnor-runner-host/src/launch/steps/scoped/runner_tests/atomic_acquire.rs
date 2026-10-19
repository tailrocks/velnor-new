use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::journal::{
    AssignmentKey, CapacityLimits, OperationalCapacityLimits, RequestAcquireState,
    fixture_provisioning_profile,
};
use crate::launch::Drive;
use crate::launch::resource_probe::StartPermit;
use crate::launch::steps::ScopedProvisioningOutcome;
use crate::scale_set::EnsureError;
use velnor_runner_github::Method;

use super::support::{
    FakeLane, Fixture, REQUEST_ID, SET_ID, exchange, selected_engine, test_images,
};

#[tokio::test]
async fn scoped_acquire_returns_durable_single_use_continuation_and_rejects_replay()
-> Result<(), String> {
    let fixture = Fixture::open("atomic-scoped-acquire").await?;
    prepare_capacity(&fixture).await?;
    fixture
        .journal
        .configure_storage_profile(fixture_provisioning_profile())
        .await
        .map_err(|error| error.to_string())?;
    let state = ActiveHostState::new(&fixture.binding)?;
    let drive = drive(&fixture, &state.path);
    let permit =
        StartPermit::test_fixture_with_available_memory(&"a".repeat(32), 32 * 1024 * 1024 * 1024);
    let expected = selected_engine(&test_images(permit.docker_root_digest()))?;
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Authorized)
    );
    let current_profile = fixture
        .scope
        .current_storage_profile()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the storage profile is absent".to_owned())?;
    if permit
        .storage_provisioning_admission(
            &current_profile,
            expected.engine_id(),
            expected.docker_root_digest(),
        )
        .is_none()
    {
        return Err("the fresh permit did not satisfy the current storage profile".to_owned());
    }
    let poll = crate::launch_harness::available(&[REQUEST_ID]);
    let mut lane = FakeLane::new([Ok(exchange(200, r#"{"count":1,"value":[41]}"#))]);

    let outcome = crate::launch::steps::acquire_scoped(&mut lane, &drive, &poll, permit, &expected)
        .await
        .map_err(|error| error.to_string())?;
    let ScopedProvisioningOutcome::Acquired(token) = outcome else {
        return Err("the acquired request did not return its continuation".to_owned());
    };
    assert_eq!(token.request().request_id(), Some(REQUEST_ID));
    assert!(token.reservation_id() > 0);
    assert!(token.intent_id() > 0);
    assert_eq!(token.sample_generation_id().len(), 32);
    assert_eq!(token.resource_budget().runner().nano_cpus, 1_000_000_000);
    assert_eq!(token.charge(), (2_000, 8_589_934_592));
    let physical_name = token.runner_name();
    let nonce = physical_name
        .strip_prefix('v')
        .ok_or_else(|| "runner name lacks its version prefix".to_owned())?;
    if !lower_hex(nonce, 32) {
        return Err("runner name does not contain a stable 128-bit nonce".to_owned());
    }
    assert!(matches!(
        token.physical_assignment().key(),
        AssignmentKey::Remote { subject, .. } if subject == nonce
    ));
    assert_eq!(lane.methods, [Method::Post]);
    assert!(lane.paths[0].ends_with("/acquirejobs"));
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Acquired)
    );
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        1
    );

    let replay = crate::launch::steps::acquire_scoped(
        &mut lane,
        &drive,
        &poll,
        StartPermit::test_fixture_with_available_memory(&"b".repeat(32), 32 * 1024 * 1024 * 1024),
        &expected,
    )
    .await;
    assert!(matches!(replay, Err(EnsureError::Uncertain)));
    assert_eq!(lane.methods, [Method::Post]);
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn uncertain_scoped_acquire_keeps_charge_and_sends_no_replay() -> Result<(), String> {
    let fixture = Fixture::open("atomic-scoped-acquire-uncertain").await?;
    prepare_capacity(&fixture).await?;
    fixture
        .journal
        .configure_storage_profile(crate::journal::fixture_provisioning_profile())
        .await
        .map_err(|error| error.to_string())?;
    let state = ActiveHostState::new(&fixture.binding)?;
    let drive = drive(&fixture, &state.path);
    let permit =
        StartPermit::test_fixture_with_available_memory(&"c".repeat(32), 32 * 1024 * 1024 * 1024);
    let expected = selected_engine(&test_images(permit.docker_root_digest()))?;
    let poll = crate::launch_harness::available(&[REQUEST_ID]);
    let mut lane = FakeLane::new([Err(velnor_runner_github::TransportFail::Timeout)]);

    assert!(
        crate::launch::steps::acquire_scoped(&mut lane, &drive, &poll, permit, &expected)
            .await
            .is_err()
    );
    assert_eq!(lane.methods, [Method::Post]);
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Uncertain)
    );
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        1
    );

    let replay = crate::launch::steps::acquire_scoped(
        &mut lane,
        &drive,
        &poll,
        StartPermit::test_fixture_with_available_memory(&"d".repeat(32), 32 * 1024 * 1024 * 1024),
        &expected,
    )
    .await;
    assert!(replay.is_err());
    assert_eq!(lane.methods, [Method::Post]);
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        1
    );
    Ok(())
}

async fn prepare_capacity(fixture: &Fixture) -> Result<(), String> {
    fixture
        .journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 4_000,
                memory_bytes: 32 * 1024 * 1024 * 1024,
            },
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 256 * 1024 * 1024,
            },
        )
        .await
        .map_err(|error| error.to_string())
}

struct ActiveHostState {
    path: PathBuf,
}

impl ActiveHostState {
    fn new(binding: &crate::config::RepositoryConnection) -> Result<Self, String> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let index = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-scoped-admission-{}-{index}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        let connection = toml::to_string(binding).map_err(|error| error.to_string())?;
        let connection = connection
            .replace("[scale_set]", "[connections.scale_set]")
            .replace("[source_policy]", "[connections.source_policy]");
        let config = format!(
            "schema = 2\n[[connections]]\n{connection}\n[host]\nmax_jobs = 1\n[host.resources]\nrunner_cpu_millicores = 1000\nrunner_memory_bytes = 4294967296\ndind_cpu_millicores = 1000\ndind_memory_bytes = 4294967296\noperational_memory_reserve_bytes = 1\n[docker]\ncontext = \"orbstack\"\nplatform = \"linux/amd64\"\nendpoint = \"unix:///var/run/docker.sock\"\n"
        );
        std::fs::write(path.join("host.toml"), config).map_err(|error| error.to_string())?;
        Ok(Self { path })
    }
}

impl Drop for ActiveHostState {
    fn drop(&mut self) {
        let _cleanup = std::fs::remove_dir_all(&self.path);
    }
}

fn drive(fixture: &Fixture, state: &std::path::Path) -> Drive {
    Drive {
        set_id: SET_ID,
        queue_path: "https://queue.example.invalid/".to_owned(),
        queue_token: "queue-token".to_owned(),
        admin_token: "admin-token".to_owned(),
        docker_engine_id: Some("test-engine".to_owned()),
        owner: "owner".to_owned(),
        repo: "repository".to_owned(),
        pat: "test-token".to_owned(),
        connection: Some(fixture.binding.clone()),
        authority_policy: Some(fixture.authority_policy.clone()),
        host_journal: Some(fixture.journal.clone()),
        scoped_journal: Some(fixture.scope.clone()),
        allocated_header: 1,
        source_authorized: true,
        admission_state: Some(state.to_path_buf()),
    }
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
