use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

#[path = "support/source.rs"]
mod source;
use source::verified_source;

use velnor_runner_github::{
    Exchange, InnerJob, InnerKind, Method, ParsedBatch, Poll, SessionRequest, Transport,
    TransportFail,
};

use crate::{
    config::{
        ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
        SourcePolicyMode, WorkflowEvent,
    },
    journal::{
        AssignmentKey, CapacityLimits, ConnectionJournal, CreatedScaleSetResponse, Journal,
        OperationalCapacityLimits, RequestAcquireState, ScaleSetCreateRequest,
    },
    launch::{
        Drive, Lane, SelectedEngineContext, StartPermit, VerifiedWorkerImages,
        steps::{ScopedProvisioningOutcome, source::test_receipt},
    },
};

pub(super) const REQUEST_ID: i64 = 41;
pub(super) const MESSAGE_ID: i64 = 77;
pub(super) const SET_ID: i64 = 501;
pub(super) const ENGINE_ID: &str = "stale-acquired-engine";

pub(super) struct Fixture {
    _scratch: Scratch,
    pub(super) journal: Journal,
    pub(super) scope: ConnectionJournal,
    connection: RepositoryConnection,
    authority_policy: crate::config::ApprovedJobAuthorityPolicy,
    pub(super) request: crate::journal::AssignmentIdentity,
    admission_state: PathBuf,
}

pub(super) struct FakeLane {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    pub(super) methods: Vec<Method>,
    pub(super) paths: Vec<String>,
}

impl FakeLane {
    fn acquire_only() -> Self {
        Self {
            replies: VecDeque::from([Ok(Exchange {
                status: 200,
                body: br#"{"count":1,"value":[41]}"#.to_vec(),
            })]),
            methods: Vec::new(),
            paths: Vec::new(),
        }
    }
}

impl Transport for FakeLane {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.methods.push(request.method);
        self.paths.push(request.path.clone());
        self.replies
            .pop_front()
            .unwrap_or(Err(TransportFail::Reset))
    }
}

impl Lane for FakeLane {
    fn on_admin(&mut self) -> Result<(), crate::scale_set::EnsureError> {
        Ok(())
    }

    fn on_queue(&mut self) -> Result<(), crate::scale_set::EnsureError> {
        Ok(())
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, String> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-native-stale-acquired-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn journal_path(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    async fn open(label: &str) -> Result<Self, String> {
        let scratch = Scratch::new()?;
        let journal = Journal::open(&scratch.journal_path())
            .await
            .map_err(|error| error.to_string())?;
        journal
            .configure_storage_profile(crate::journal::fixture_provisioning_profile())
            .await
            .map_err(|error| error.to_string())?;
        journal
            .bind_engine(ENGINE_ID)
            .await
            .map_err(|error| error.to_string())?;
        journal
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
            .map_err(|error| error.to_string())?;
        let connection = connection()?;
        let scope = journal
            .connection_scope(&connection)
            .await
            .map_err(|error| error.to_string())?;
        bind_scale_set(&scope, &connection).await?;
        scope
            .bind_engine_context(ENGINE_ID, "unix:///var/run/docker.sock", "linux/amd64")
            .await
            .map_err(|error| error.to_string())?;
        let authority_policy = install_authority(&scope, &connection).await?;
        let request = scope
            .assignment_identity(AssignmentKey::AssignedRequest {
                request_id: REQUEST_ID,
            })
            .await
            .map_err(|error| error.to_string())?;
        record_source_approval(&scope, &request).await?;
        let admission_state = write_active_host_config(&scratch, label, &connection)?;
        Ok(Self {
            _scratch: scratch,
            journal,
            scope,
            connection,
            authority_policy,
            request,
            admission_state,
        })
    }

    pub(super) async fn acquire(
        label: &str,
    ) -> Result<(Self, ScopedProvisioningOutcome, FakeLane), String> {
        let fixture = Self::open(label).await?;
        let images = test_images();
        let engine = selected_engine(&images)?;
        let drive = fixture.drive();
        let poll = poll();
        let mut lane = FakeLane::acquire_only();
        let outcome = crate::launch::steps::acquire_scoped(
            &mut lane,
            &drive,
            &poll,
            StartPermit::test_fixture_with_engine_and_available_memory(
                &"a".repeat(32),
                ENGINE_ID,
                32 * 1024 * 1024 * 1024,
            ),
            &engine,
        )
        .await
        .map_err(|error| format!("The source-authorized Acquire failed: {error:?}"))?;
        if !matches!(outcome, ScopedProvisioningOutcome::Acquired(_)) {
            return Err("The source-authorized request was not acquired.".to_owned());
        }
        assert_eq!(
            fixture
                .scope
                .request_acquire_state(&fixture.request)
                .await
                .map_err(|error| error.to_string())?,
            Some(RequestAcquireState::Acquired)
        );
        assert_eq!(lane.methods, [Method::Post]);
        assert!(lane.paths[0].ends_with("/acquirejobs"));
        Ok((fixture, outcome, lane))
    }

    fn drive(&self) -> Drive {
        Drive {
            set_id: SET_ID,
            queue_path: "queues/messages".to_owned(),
            queue_token: "queue-token".to_owned(),
            admin_token: "admin-token".to_owned(),
            docker_engine_id: Some(ENGINE_ID.to_owned()),
            owner: "owner".to_owned(),
            repo: "repository".to_owned(),
            pat: "test-pat".to_owned(),
            connection: Some(self.connection.clone()),
            authority_policy: Some(self.authority_policy.clone()),
            host_journal: Some(self.journal.clone()),
            scoped_journal: Some(self.scope.clone()),
            allocated_header: 1,
            source_authorized: true,
            admission_state: Some(self.admission_state.clone()),
        }
    }
}

async fn bind_scale_set(
    scope: &ConnectionJournal,
    connection: &RepositoryConnection,
) -> Result<(), String> {
    let create = ScaleSetCreateRequest::new(connection, 7).map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_scale_set_create(&create)
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_scale_set_create(&prepared)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The Scale Set create claim is absent.".to_owned())?;
    let response = CreatedScaleSetResponse::verified(
        &create,
        SET_ID,
        create.selector_name(),
        create.selector_labels(),
        true,
    )
    .map_err(|error| error.to_string())?;
    scope
        .record_scale_set_created(claim, response)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn install_authority(
    scope: &ConnectionJournal,
    connection: &RepositoryConnection,
) -> Result<crate::config::ApprovedJobAuthorityPolicy, String> {
    let config = crate::config::HostConfig::test_with_job_authority_policy(connection)
        .map_err(|error| error.to_string())?;
    scope
        .install_job_authority_policy(&config)
        .await
        .map_err(|error| error.to_string())?;
    config
        .validated_job_authority_policy(&connection.connection_id)
        .map_err(|error| error.to_string())?
        .cloned()
        .ok_or_else(|| "The test authority policy is absent.".to_owned())
}

async fn record_source_approval(
    scope: &ConnectionJournal,
    request: &crate::journal::AssignmentIdentity,
) -> Result<(), String> {
    let source_binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    scope
        .record_source_authorization(
            request,
            &test_receipt(
                &source_binding,
                REQUEST_ID,
                MESSAGE_ID,
                verified_source(),
                SourcePolicyMode::Repository,
            ),
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn write_active_host_config(
    scratch: &Scratch,
    label: &str,
    connection: &RepositoryConnection,
) -> Result<PathBuf, String> {
    let path = scratch.0.join(format!("{label}-state"));
    std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    let mut connection_toml = toml::to_string(connection).map_err(|error| error.to_string())?;
    connection_toml = connection_toml
        .replace("[scale_set]", "[connections.scale_set]")
        .replace("[source_policy]", "[connections.source_policy]");
    let host_config = format!(
        "schema = 2\n[[connections]]\n{connection_toml}\n[host]\nmax_jobs = 1\n[host.resources]\nrunner_cpu_millicores = 1000\nrunner_memory_bytes = 4294967296\ndind_cpu_millicores = 1000\ndind_memory_bytes = 4294967296\noperational_memory_reserve_bytes = 1\n[docker]\ncontext = \"orbstack\"\nplatform = \"linux/amd64\"\nendpoint = \"unix:///var/run/docker.sock\"\n"
    );
    std::fs::write(path.join("host.toml"), host_config).map_err(|error| error.to_string())?;
    Ok(path)
}

pub(super) fn selected_engine(
    images: &VerifiedWorkerImages,
) -> Result<SelectedEngineContext, String> {
    SelectedEngineContext::bind(
        &crate::journal::EngineContext {
            engine_id: ENGINE_ID.to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
            platform: images.platform().to_owned(),
        },
        images,
    )
    .map_err(|error| error.to_string())
}

fn poll() -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: MESSAGE_ID,
        raw_body: String::new(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Available,
            request_id: Some(REQUEST_ID),
            job_id: None,
            labels: Vec::new(),
            runner_id: None,
            runner_name: None,
            result: None,
            fields: Vec::new(),
            source: velnor_runner_github::SourceMetadata::default(),
        }],
    })
}

fn connection() -> Result<RepositoryConnection, String> {
    let labels = ["velnor".to_owned(), "integration".to_owned()];
    let selector =
        ScaleSetSelector::try_new("integration", &labels).map_err(|error| error.to_string())?;
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;
    RepositoryConnection::try_new(
        ConnectionId::parse("native-stale-acquired").map_err(|error| error.to_string())?,
        101,
        "owner/repository".to_owned(),
        KeychainReference::parse("keychain:velnor/native-stale-acquired")
            .map_err(|error| error.to_string())?,
        selector,
        policy,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn test_images() -> VerifiedWorkerImages {
    let permit = StartPermit::test_fixture_with_engine_id(&"a".repeat(32), ENGINE_ID);
    crate::launch::verified_images::test_worker_images_for(
        ENGINE_ID,
        permit.docker_root_digest(),
        "aarch64",
        "linux/amd64",
    )
}
