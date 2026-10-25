use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::error::HostError;
use crate::journal::{
    AssignedAcquireResult, CapacityLimits, ConnectionJournal, EngineContext, Journal,
    RequestPhysicalReservationResult, ReservationRequest, fixture_provisioning_profile,
    fixture_storage_provisioning_admission,
};
use crate::launch::{StartPermit, VerifiedWorkerImages};
use crate::worker::WorkerIdentityNames;

use super::super::super::BootstrapContainerRole;
use super::super::PairCreate;
use super::create_spec;

const ENGINE_ID: &str = "selected-engine";
const NONCE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct Fixture {
    _scratch: crate::launch_harness::Scratch,
    scope: ConnectionJournal,
    reservation_id: i64,
    plan: crate::journal::StorageProvisioningPlan,
    images: VerifiedWorkerImages,
}

async fn fixture(label: &str) -> Result<Fixture, HostError> {
    let scratch = crate::launch_harness::Scratch::new(label)?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    journal
        .configure_storage_profile(fixture_provisioning_profile())
        .await?;
    journal.bind_engine(ENGINE_ID).await?;

    let scope = connection_scope(&journal).await?;

    let permit = StartPermit::test_fixture_with_engine_id(&"a".repeat(32), ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let assignment = scope
        .assignment_identity(crate::journal::AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: NONCE.to_owned(),
        })
        .await?;
    let request =
        crate::journal::request_lifecycle::tests::helpers::acquired_physical_fixture_request(
            &scope,
        )
        .await?;
    let reservation = scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &assignment,
                engine_id: ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: 8_589_934_592,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
            &request,
        )
        .await?;
    let RequestPhysicalReservationResult::AcquireClaimed {
        reservation_id,
        intent_id,
    } = reservation
    else {
        return Err(HostError::Journal);
    };
    scope
        .record_request_acquire(
            &request,
            AssignedAcquireResult::Returned(vec![request.request_id().ok_or(HostError::Identity)?]),
        )
        .await?;
    let (begun_intent_id, fresh) = scope.begin_launch(&assignment).await?;
    if fresh || begun_intent_id != intent_id {
        return Err(HostError::Journal);
    }
    let plan = scope
        .storage_provisioning_plan(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let images = crate::launch::verified_images::test_worker_images_for_root("/var/lib/docker");
    validate_engine_context(&images)?;
    Ok(Fixture {
        _scratch: scratch,
        scope,
        reservation_id,
        plan,
        images,
    })
}

fn validate_engine_context(images: &VerifiedWorkerImages) -> Result<(), HostError> {
    let engine_context = EngineContext {
        engine_id: ENGINE_ID.to_owned(),
        endpoint: "unix:///var/run/docker.sock".to_owned(),
        platform: "linux/amd64".to_owned(),
    };
    let _selected = crate::launch::SelectedEngineContext::bind(&engine_context, images)?;
    Ok(())
}

async fn connection_scope(journal: &Journal) -> Result<ConnectionJournal, HostError> {
    let connection = RepositoryConnection::try_new(
        ConnectionId::parse("pair-builder").expect("valid connection ID"),
        701,
        "owner/repo".to_owned(),
        KeychainReference::parse("keychain:velnor/pair-builder").expect("valid Keychain reference"),
        ScaleSetSelector::try_new(
            "integration",
            &["velnor".to_owned(), "integration".to_owned()],
        )
        .expect("valid Scale Set selector"),
        SourcePolicy::try_new(
            SourcePolicyMode::Repository,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )
        .expect("valid source policy"),
    )
    .expect("valid repository connection");
    let scope = journal.connection_scope(&connection).await?;
    let request = crate::journal::ScaleSetCreateRequest::new(&connection, 7)?;
    let prepared = scope.prepare_scale_set_create(&request).await?;
    let claim = scope
        .claim_scale_set_create(&prepared)
        .await?
        .ok_or(HostError::Journal)?;
    let response = crate::journal::CreatedScaleSetResponse::verified(
        &request,
        1_701,
        "integration",
        connection.scale_set.labels(),
        true,
    )?;
    scope.record_scale_set_created(claim, response).await?;
    scope
        .bind_engine_context(ENGINE_ID, "unix:///var/run/docker.sock", "linux/amd64")
        .await?;
    let config = crate::config::HostConfig::test_with_job_authority_policy(&connection)?;
    scope.install_job_authority_policy(&config).await?;

    Ok(scope)
}

fn create(
    fixture: &Fixture,
    role: BootstrapContainerRole,
    network: &str,
) -> Result<PairCreate, HostError> {
    let names = WorkerIdentityNames::from_nonce(NONCE)?;
    let context = spec_context(fixture, &names)?;
    create_spec(
        role,
        &format!("velnor-test-{role:?}").to_ascii_lowercase(),
        network.to_owned(),
        &fixture.images,
        &context,
    )
}

fn spec_context<'a>(
    fixture: &'a Fixture,
    names: &'a WorkerIdentityNames,
) -> Result<super::PairSpecContext<'a>, HostError> {
    Ok(super::PairSpecContext {
        plan: &fixture.plan,
        names,
        connection_id: fixture.scope.connection_id().as_str(),
        budget: crate::worker::test_resource_budget()?,
    })
}

#[tokio::test]
async fn runner_and_dind_specs_use_separate_privilege_and_finite_limits() -> Result<(), HostError> {
    let fixture = fixture("pair-spec").await?;
    assert!(fixture.reservation_id > 0);
    let names = WorkerIdentityNames::from_nonce(NONCE)?;
    let dind_id = "d".repeat(64);
    let runner = create(
        &fixture,
        BootstrapContainerRole::Runner,
        &format!("container:{dind_id}"),
    )?;
    let dind = create(
        &fixture,
        BootstrapContainerRole::Dind,
        &format!("velnor-{}-private", names.remote_runner_name()?),
    )?;
    assert_pair_config(&runner, &dind, &dind_id, &names)
}

fn assert_pair_config(
    runner: &PairCreate,
    dind: &PairCreate,
    dind_id: &str,
    names: &WorkerIdentityNames,
) -> Result<(), HostError> {
    let runner_host = runner
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Identity)?;
    let dind_host = dind
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Identity)?;
    assert_eq!(runner.options.platform, "linux/amd64");
    assert_eq!(dind.options.platform, "linux/amd64");
    assert_pair_ports(runner_host, dind_host, dind_id, names)?;
    assert_runner_security(runner_host);
    assert_dind_security_is_unmodified(dind_host);
    assert_pair_limits(runner_host, dind_host);
    assert_pair_process_settings(runner, dind);
    assert_pair_logs(runner_host, dind_host)?;
    assert_pair_tmpfs(runner_host, dind_host);
    Ok(())
}

fn assert_pair_ports(
    runner_host: &bollard::models::HostConfig,
    dind_host: &bollard::models::HostConfig,
    dind_id: &str,
    names: &WorkerIdentityNames,
) -> Result<(), HostError> {
    assert_eq!(runner_host.privileged, Some(false));
    assert_eq!(dind_host.privileged, Some(true));
    assert_eq!(runner_host.readonly_rootfs, Some(true));
    assert_eq!(dind_host.readonly_rootfs, Some(true));
    assert_eq!(runner_host.publish_all_ports, Some(false));
    assert_eq!(dind_host.publish_all_ports, Some(false));
    assert!(
        runner_host
            .port_bindings
            .as_ref()
            .is_none_or(std::collections::HashMap::is_empty)
    );
    assert!(
        dind_host
            .port_bindings
            .as_ref()
            .is_none_or(std::collections::HashMap::is_empty)
    );
    let runner_network = format!("container:{dind_id}");
    let dind_network = format!("velnor-{}-private", names.remote_runner_name()?);
    assert_eq!(
        runner_host.network_mode.as_deref(),
        Some(runner_network.as_str())
    );
    assert_eq!(
        dind_host.network_mode.as_deref(),
        Some(dind_network.as_str())
    );
    Ok(())
}

fn assert_runner_security(runner_host: &bollard::models::HostConfig) {
    assert_eq!(runner_host.cap_drop, Some(vec!["ALL".to_owned()]));
    assert_eq!(
        runner_host.cap_add,
        Some(
            [
                "CHOWN",
                "DAC_OVERRIDE",
                "FOWNER",
                "FSETID",
                "KILL",
                "SETGID",
                "SETUID",
                "SETPCAP",
                "NET_BIND_SERVICE",
                "NET_RAW",
                "SYS_CHROOT",
                "MKNOD",
                "AUDIT_WRITE",
                "SETFCAP",
            ]
            .map(str::to_owned)
            .to_vec()
        )
    );
    assert_eq!(
        runner_host.security_opt,
        Some(vec![crate::worker::seccomp::runner_security_opt()])
    );
}

fn assert_dind_security_is_unmodified(dind_host: &bollard::models::HostConfig) {
    assert!(dind_host.cap_drop.as_ref().is_none_or(Vec::is_empty));
    assert!(dind_host.cap_add.as_ref().is_none_or(Vec::is_empty));
    assert!(dind_host.security_opt.as_ref().is_none_or(Vec::is_empty));
}

fn assert_pair_limits(
    runner_host: &bollard::models::HostConfig,
    dind_host: &bollard::models::HostConfig,
) {
    assert_eq!(runner_host.pids_limit, Some(1_024));
    assert_eq!(dind_host.pids_limit, Some(2_048));
    assert_eq!(runner_host.memory, Some(2_147_483_648));
    assert_eq!(dind_host.memory, Some(6_442_450_944));
    assert_eq!(runner_host.memory_swap, runner_host.memory);
    assert_eq!(dind_host.memory_swap, dind_host.memory);
}

fn assert_pair_process_settings(runner: &PairCreate, dind: &PairCreate) {
    assert_eq!(runner.config.open_stdin, Some(true));
    assert_eq!(runner.config.stdin_once, Some(true));
    assert_eq!(runner.config.tty, Some(false));
    assert_eq!(dind.config.open_stdin, Some(false));
    assert_eq!(dind.config.stdin_once, Some(false));
    assert_eq!(dind.config.tty, Some(false));
    assert_eq!(
        runner.config.env.as_deref(),
        Some(["DOCKER_HOST=tcp://localhost:2375".to_owned()].as_slice())
    );
    assert!(dind.config.env.as_ref().is_none_or(Vec::is_empty));
}

fn assert_pair_logs(
    runner_host: &bollard::models::HostConfig,
    dind_host: &bollard::models::HostConfig,
) -> Result<(), HostError> {
    for host in [runner_host, dind_host] {
        let log = host.log_config.as_ref().ok_or(HostError::Identity)?;
        assert_eq!(log.typ.as_deref(), Some("json-file"));
        assert_eq!(
            log.config
                .as_ref()
                .and_then(|values| values.get("max-size"))
                .map(String::as_str),
            Some("10m")
        );
        assert_eq!(
            log.config
                .as_ref()
                .and_then(|values| values.get("max-file"))
                .map(String::as_str),
            Some("3")
        );
    }
    Ok(())
}

fn assert_pair_tmpfs(
    runner_host: &bollard::models::HostConfig,
    dind_host: &bollard::models::HostConfig,
) {
    assert!(runner_host.tmpfs.as_ref().is_some_and(|mounts| {
        mounts
            .get("/tmp")
            .is_some_and(|value| value.contains("nr_inodes="))
            && mounts
                .get("/dev/shm")
                .is_some_and(|value| value.contains("nr_inodes="))
    }));
    assert!(dind_host.tmpfs.as_ref().is_some_and(|mounts| {
        mounts
            .get("/tmp")
            .is_some_and(|value| value.contains("nr_inodes="))
            && mounts
                .get("/dev/shm")
                .is_some_and(|value| value.contains("nr_inodes="))
    }));
}

#[tokio::test]
async fn pair_specs_reject_public_or_unpaired_network_modes() -> Result<(), HostError> {
    let fixture = fixture("pair-network-mode").await?;
    assert!(create(&fixture, BootstrapContainerRole::Runner, "host").is_err());
    assert!(
        create(
            &fixture,
            BootstrapContainerRole::Runner,
            "container:dind-id"
        )
        .is_err()
    );
    assert!(create(&fixture, BootstrapContainerRole::Dind, "bridge").is_err());
    Ok(())
}
