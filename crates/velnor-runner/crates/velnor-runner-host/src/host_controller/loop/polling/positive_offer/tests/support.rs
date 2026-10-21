use std::path::PathBuf;

use bollard::Docker;
use std::os::unix::net::UnixListener;
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use crate::config::{
    ConnectionId, HostConfig, KeychainReference, RepositoryConnection, ScaleSetSelector,
    SourcePolicy, SourcePolicyMode, WorkflowEvent,
};
use crate::journal::{
    CapacityLimits, ConnectionJournal, CreatedScaleSetResponse, HeaderAllocation, Journal,
    ScaleSetCreateRequest,
};
use crate::launch::{IntakeCompositionProvider, ReleaseReadSecret};
use crate::worker::guest_image::test_support::TestGuestImage;

const SCALE_SET_ID: i64 = 10_101;

pub(super) struct MissingSourceFixture {
    _scratch: Scratch,
    guest: TestGuestImage,
    journal: Journal,
    config: HostConfig,
    policy: crate::config::ApprovedJobAuthorityPolicy,
    scope: ConnectionJournal,
    docker: Docker,
    docker_listener: UnixListener,
    docker_path: PathBuf,
    guest_image: crate::worker::guest_image::GuestImageFile,
    state: PathBuf,
    provider: IntakeCompositionProvider,
    secret: std::sync::Arc<ReleaseReadSecret>,
}

impl MissingSourceFixture {
    pub(super) async fn new() -> Result<Self, String> {
        let scratch = Scratch::new("missing-source")?;
        let guest = TestGuestImage::new().map_err(|error| error.to_string())?;
        let (journal, scope) = open_journal(&scratch, &guest.connection).await?;
        bind_scope(&journal, &scope, &guest.connection).await?;
        let config = HostConfig::test_with_job_authority_policy(&guest.connection)
            .map_err(|error| error.to_string())?;
        scope
            .install_job_authority_policy(&config)
            .await
            .map_err(|error| error.to_string())?;
        let policy = config
            .validated_job_authority_policy(&guest.connection.connection_id)
            .map_err(|error| error.to_string())?
            .cloned()
            .ok_or_else(|| "The test authority policy is missing.".to_owned())?;
        let state = scratch.path().join("state");
        std::fs::create_dir(&state).map_err(|error| error.to_string())?;
        let guest_image =
            crate::worker::guest_image::write_guest_image(&state, &guest.connection, &guest.images)
                .map_err(|error| error.to_string())?;
        let docker_path = socket_path("missing-source")?;
        let docker_listener =
            UnixListener::bind(&docker_path).map_err(|error| error.to_string())?;
        docker_listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let docker = Docker::connect_with_socket(
            docker_path
                .to_str()
                .ok_or_else(|| "Docker socket path is not UTF-8".to_owned())?,
            120,
            bollard::API_DEFAULT_VERSION,
        )
        .map_err(|error| error.to_string())?;
        let provider =
            IntakeCompositionProvider::new(&guest.selected_engine, guest.images.source_sha());
        Ok(Self {
            _scratch: scratch,
            guest,
            journal,
            config,
            policy,
            scope,
            docker,
            docker_listener,
            docker_path,
            guest_image,
            state,
            provider,
            secret: std::sync::Arc::new(ReleaseReadSecret::test_token()),
        })
    }

    pub(super) async fn admit_missing_source(
        &self,
        link: &mut crate::listen::Link,
        polled: &velnor_runner_github::Poll,
    ) -> Result<crate::launch::PositiveAdmission, crate::scale_set::EnsureError> {
        let context = super::super::PositivePollContext {
            config: &self.config,
            journal: &self.journal,
            docker: &self.docker,
            selected_engine: &self.guest.selected_engine,
            worker_images: &self.guest.images,
            state: &self.state,
            release_read_secret: &self.secret,
            admission_ready: true,
            start_permit_provider: Some(&self.provider),
        };
        let budget = self
            .config
            .resource_budget()
            .map_err(|_| crate::scale_set::EnsureError::AdmissionClosed)?;
        let input = super::PositiveOfferInputs {
            context: &context,
            connection: &self.guest.connection,
            policy: &self.policy,
            budget,
            set_id: SCALE_SET_ID,
            scoped: &self.scope,
            guest_image: &self.guest_image,
            pat: "test-pat",
            allocated_header: 1,
        };
        super::admit_positive_offer(
            &input,
            link,
            polled,
            None,
            "https://queue.example.invalid/messages".to_owned(),
            "queue-token".to_owned(),
            "admin-token".to_owned(),
        )
        .await
    }

    pub(super) fn assert_no_docker_request(&self) -> Result<(), String> {
        match self.docker_listener.accept() {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(error.to_string()),
            Ok(_) => Err("Admission sent an unexpected Docker request.".to_owned()),
        }
    }

    pub(super) async fn reservations_empty(&self) -> Result<bool, String> {
        self.scope
            .physical_runner_reservations()
            .await
            .map(|items| items.is_empty())
            .map_err(|error| error.to_string())
    }

    pub(super) fn cleanup(self) -> Result<(), String> {
        let Self {
            docker_path, state, ..
        } = self;
        std::fs::remove_file(docker_path).map_err(|error| error.to_string())?;
        std::fs::remove_dir_all(state).map_err(|error| error.to_string())?;
        Ok(())
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Result<Self, String> {
        let path =
            std::env::temp_dir().join(format!("vpo-{}-{}", label, uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

async fn open_journal(
    scratch: &Scratch,
    connection: &RepositoryConnection,
) -> Result<(Journal, ConnectionJournal), String> {
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await
        .map_err(|error| error.to_string())?;
    let scope = journal
        .connection_scope(connection)
        .await
        .map_err(|error| error.to_string())?;
    Ok((journal, scope))
}

async fn bind_scope(
    journal: &Journal,
    scope: &ConnectionJournal,
    connection: &RepositoryConnection,
) -> Result<(), String> {
    journal
        .bootstrap_header_authority()
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("test-engine")
        .await
        .map_err(|error| error.to_string())?;
    scope
        .bind_engine_context("test-engine", "unix:///var/run/docker.sock", "linux/amd64")
        .await
        .map_err(|error| error.to_string())?;
    record_scale_set(scope, connection).await
}

async fn record_scale_set(
    scope: &ConnectionJournal,
    connection: &RepositoryConnection,
) -> Result<(), String> {
    let request = ScaleSetCreateRequest::new(connection, 7).map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_scale_set_create(&request)
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_scale_set_create(&prepared)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The Scale Set creation claim is missing.".to_owned())?;
    let response = CreatedScaleSetResponse::verified(
        &request,
        SCALE_SET_ID,
        request.selector_name(),
        connection.scale_set.labels(),
        true,
    )
    .map_err(|error| error.to_string())?;
    scope
        .record_scale_set_created(claim, response)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn socket_path(label: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(format!(
        "/tmp/vpo-{}-{}.sock",
        label,
        uuid::Uuid::new_v4().simple()
    ));
    if path.as_os_str().len() >= 100 {
        return Err("The test Docker socket path is too long.".to_owned());
    }
    Ok(path)
}

pub(super) struct PollFixture {
    _scratch: Scratch,
    pub(super) scope: ConnectionJournal,
    pub(super) session_intent_id: i64,
    pub(super) allocation: HeaderAllocation,
    pub(super) session: velnor_runner_github::QueueSession,
}

pub(super) async fn poll_fixture() -> Result<PollFixture, String> {
    let scratch = Scratch::new("population-only")?;
    let binding = repository_binding()?;
    let (journal, scope) = open_journal(&scratch, &binding).await?;
    bind_scope(&journal, &scope, &binding).await?;
    let prepared = scope
        .prepare_session_create("0123456789abcdef0123456789abcdef")
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The queue session creation claim is missing.".to_owned())?;
    let bound = scope
        .record_created_session(&claim, "session-scoped-poll", 1)
        .await
        .map_err(|error| error.to_string())?;
    let mut transport = SessionResponse;
    let session =
        velnor_runner_github::create_session(&mut transport, SCALE_SET_ID, "owner", "admin-token")
            .map_err(|error| error.to_string())?;
    Ok(PollFixture {
        _scratch: scratch,
        scope,
        session_intent_id: bound.intent_id(),
        allocation: bound.allocation().clone(),
        session,
    })
}

struct SessionResponse;

fn repository_binding() -> Result<RepositoryConnection, String> {
    let selector = ScaleSetSelector::try_new(
        "integration",
        &["velnor".to_owned(), "integration".to_owned()],
    )
    .map_err(|error| error.to_string())?;
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;
    RepositoryConnection::try_new(
        ConnectionId::parse("scoped-poll-test").map_err(|error| error.to_string())?,
        101,
        "owner/repository".to_owned(),
        KeychainReference::parse("keychain:velnor/scoped-poll-test")
            .map_err(|error| error.to_string())?,
        selector,
        policy,
    )
    .map_err(|error| error.to_string())
}

impl Transport for SessionResponse {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        if request.method != Method::Post {
            return Err(TransportFail::Reset);
        }
        Ok(Exchange {
            status: 200,
            body: br#"{"sessionId":"session-scoped-poll","messageQueueUrl":"https://queue.example.invalid/","messageQueueAccessToken":"queue-token"}"#.to_vec(),
        })
    }
}
