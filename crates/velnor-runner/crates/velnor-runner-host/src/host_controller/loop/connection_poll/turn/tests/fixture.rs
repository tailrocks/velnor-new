// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, sync::Arc, time::Instant};

use bollard::{Docker, models::SystemInfo};
use tempfile::TempDir;

use crate::{
    config::HostConfig,
    connect::SetOwnership,
    controller_ipc::ControlState,
    host_controller::loop_driver::state::{
        AdminRefreshSchedule, PreparedConnection, PreparedSetup,
    },
    journal::{CapacityLimits, EngineContext, Journal, OperationalCapacityLimits},
    launch::{
        ImageAuthority, SelectedEngineContext, VerifiedWorkerImages,
        scoped_runtime::RunnerReadyRegistry,
    },
    scale_set::EnsuredSet,
    worker::guest_image::write_guest_image,
};

use super::{ConnectionPollContext, TestResult};

const CONFIG: &str = r#"schema = 2
[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

const IMAGE_ID: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const BINDING: &str = "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES ('conn-test-1', 7, 'ubuntu-26.04-scale-set', 'created', 'engine-test', 'unix:///docker.sock', 'linux/amd64')";

pub(super) struct Fixture {
    pub(super) config: HostConfig,
    pub(super) control: ControlState,
    pub(super) journal: Journal,
    pub(super) prepared: PreparedConnection,
    docker: Docker,
    engine: SelectedEngineContext,
    images: VerifiedWorkerImages,
    image_authority: Arc<ImageAuthority>,
    state: TempDir,
    _docker_socket: tokio::net::UnixListener,
}

impl Fixture {
    pub(super) async fn new() -> TestResult<Self> {
        let state = tempfile::tempdir()?;
        let config = HostConfig::parse(CONFIG)?;
        let control = ControlState::new(&config);
        let journal = Journal::open(&state.path().join("launch.db")).await?;
        journal.bind_engine("engine-test").await?;
        let binding = config.connections().first().ok_or("connection")?.clone();
        let scope = journal.connection_scope(&binding).await?;
        journal.connection().await?.execute(BINDING, ()).await?;
        journal
            .configure_capacity_with_operational_limits(
                CapacityLimits {
                    jobs: 1,
                    cpu_millicores: 6_000,
                    memory_bytes: 13 << 30,
                },
                OperationalCapacityLimits {
                    cpu_millicores: 500,
                    memory_bytes: 1 << 30,
                },
            )
            .await?;
        let engine = SelectedEngineContext::capture(
            &EngineContext {
                engine_id: "engine-test".to_owned(),
                endpoint: "unix:///docker.sock".to_owned(),
                platform: "linux/amd64".to_owned(),
            },
            &SystemInfo {
                id: Some("engine-test".to_owned()),
                docker_root_dir: Some("/var/lib/docker".to_owned()),
                os_type: Some("linux".to_owned()),
                architecture: Some("x86_64".to_owned()),
                ..SystemInfo::default()
            },
        )?;
        let images = VerifiedWorkerImages::for_test(&engine, &"1".repeat(40), IMAGE_ID, IMAGE_ID)?;
        let guest_image = write_guest_image(state.path(), &binding, &images)?;
        let prepared = prepared(binding, scope, guest_image);
        let socket = state.path().join("docker.sock");
        let docker_socket = tokio::net::UnixListener::bind(&socket)?;
        let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
        Ok(Self {
            config,
            control,
            journal,
            prepared,
            docker,
            engine,
            images,
            image_authority: Arc::new(ImageAuthority::parse("token")?),
            state,
            _docker_socket: docker_socket,
        })
    }

    pub(super) async fn poll(&mut self) -> TestResult<Option<super::PollTurnResult>> {
        let context = ConnectionPollContext {
            config: &self.config,
            journal: &self.journal,
            docker: &self.docker,
            selected_engine: &self.engine,
            worker_images: &self.images,
            state: self.state.path(),
            image_authority: &self.image_authority,
            admission_ready: true,
            control_state: &self.control,
        };
        Ok(super::poll_connection(
            &mut self.prepared,
            &context,
            &[],
            &mut super::FairnessTurn::new(false, false),
        )
        .await?)
    }

    pub(super) async fn bind_session(&mut self) -> TestResult<()> {
        let scope = &self.prepared.scope;
        let nonce = "0123456789abcdef0123456789abcdef";
        scope.prepare_session_create(nonce).await?;
        let claim = scope
            .claim_session_create(nonce)
            .await?
            .ok_or("session claim")?;
        let bound = scope
            .record_created_session(&claim, "session-test", 0)
            .await?;
        self.prepared.session_intent = bound.intent_id();
        Ok(())
    }
}

fn prepared(
    binding: crate::config::RepositoryConnection,
    scope: crate::journal::ConnectionJournal,
    guest_image: crate::worker::guest_image::GuestImageFile,
) -> PreparedConnection {
    let set = EnsuredSet {
        id: 7,
        ownership: SetOwnership::Created,
        name: binding.scale_set.name().to_owned(),
        runner_group_id: 1,
        disable_update: false,
        labels: binding.scale_set.labels().to_vec(),
    };
    PreparedConnection {
        binding,
        scope,
        set_id: 7,
        setup: PreparedSetup {
            set,
            groups: Vec::new(),
            captured_at_unix_ms: 0,
        },
        session_intent: 0,
        assigned_total: None,
        poll_cursor: 0,
        poll_retry_at: None,
        restore_retry_at: None,
        guest_image,
        completion: None,
        credentials: None,
        runner_ready: RunnerReadyRegistry::new(),
        ready_intents: BTreeMap::new(),
        authorizing: BTreeMap::new(),
        pending_bootstrap: BTreeMap::new(),
        unkeyed_bootstrap: Vec::new(),
        authorized: BTreeMap::new(),
        pending_authorization: BTreeMap::new(),
        unkeyed_authorization: Vec::new(),
        pending_acquired_recovery: BTreeMap::new(),
        unkeyed_acquired_recovery: Vec::new(),
        admin_refresh: AdminRefreshSchedule::due_at(Instant::now()),
        recovery_pass: 0,
    }
}
