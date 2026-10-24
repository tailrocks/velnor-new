use std::sync::Arc;

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::journal::request_lifecycle::tests::helpers::{
    JobFactsReaderFixture, Scratch, TEST_ENGINE_ID, binding, created_scope,
    provision_acquired_request_with_initialization,
};
use crate::journal::{
    CapacityLimits, ConnectionJournal, EngineContext, JitConflictRecoveryClaim, Journal,
};
use crate::launch::jit_conflict::ReconcileContext;
use crate::launch::jit_conflict::remote::reconcile_admin;
use crate::launch::verified_images::test_worker_images_for;
use crate::launch::{ReleaseReadSecret, SelectedEngineContext, VerifiedWorkerImages};
use crate::listen::{Link, Secret, inert_link_for_offline_test};

pub(super) const ADMIN_TOKEN: &str = "offline-admin-fixture-token";
const GITHUB_TOKEN: &str = "offline-github-fixture-token";

pub(super) struct RecoveryFixture {
    pub(super) _scratch: Scratch,
    pub(super) journal: Journal,
    pub(super) scope: ConnectionJournal,
    pub(super) binding: crate::config::RepositoryConnection,
    pub(super) physical: JobFactsReaderFixture,
    pub(super) claim: JitConflictRecoveryClaim,
    pub(super) requested_name: String,
}

pub(super) struct ContextFixture {
    docker: crate::launch::inspect_tests::BackgroundDockerStub,
    images: VerifiedWorkerImages,
    selected: SelectedEngineContext,
    link: Link,
    github_pat: Secret,
    release_secret: Arc<ReleaseReadSecret>,
}

impl ContextFixture {
    pub(super) fn new(fixture: &RecoveryFixture) -> Result<Self, HostError> {
        let images = test_worker_images_for(
            TEST_ENGINE_ID,
            &fixture.physical.docker_root_digest,
            "x86_64",
            "linux/amd64",
        );
        let selected = SelectedEngineContext::bind(
            &EngineContext {
                engine_id: TEST_ENGINE_ID.to_owned(),
                endpoint: "unix:///var/run/docker.sock".to_owned(),
                platform: "linux/amd64".to_owned(),
            },
            &images,
        )?;
        let docker = crate::launch::inspect_tests::BackgroundDockerStub::open(Vec::new())
            .map_err(|_| HostError::Journal)?;
        Ok(Self {
            docker,
            images,
            selected,
            link: inert_link_for_offline_test(),
            github_pat: Secret::new(GITHUB_TOKEN),
            release_secret: Arc::new(ReleaseReadSecret::test_inert()),
        })
    }

    pub(super) fn context<'a>(&'a mut self, fixture: &'a RecoveryFixture) -> ReconcileContext<'a> {
        ReconcileContext {
            docker: &self.docker.docker,
            journal: &fixture.journal,
            scope: &fixture.scope,
            binding: &fixture.binding,
            set_id: fixture.claim.scale_set_id(),
            link: &mut self.link,
            github_pat: &self.github_pat,
            selected: &self.selected,
            images: &self.images,
            release_secret: &self.release_secret,
        }
    }

    pub(super) async fn finish_without_docker_requests(self) -> Result<(), HostError> {
        self.docker
            .finish_observed()
            .await
            .map(|_| ())
            .map_err(|_| HostError::Journal)
    }
}

pub(super) struct Reply {
    method: Method,
    path: &'static str,
    query: Option<String>,
    status: u16,
    body: String,
}

impl Reply {
    pub(super) fn new(
        method: Method,
        path: &'static str,
        query: Option<String>,
        status: u16,
        body: &str,
    ) -> Self {
        Self {
            method,
            path,
            query,
            status,
            body: body.to_owned(),
        }
    }
}

pub(super) struct Script {
    pub(super) expected: Vec<Reply>,
    expected_token: String,
    pub(super) seen: Vec<SeenRequest>,
}

pub(super) struct SeenRequest {
    pub(super) method: Method,
    pub(super) path: String,
}

impl Script {
    pub(super) fn new(expected: Vec<Reply>, expected_token: &str) -> Self {
        Self {
            expected,
            expected_token: expected_token.to_owned(),
            seen: Vec::new(),
        }
    }

    pub(super) fn repository_scope(binding: &RepositoryConnection, count: usize) -> Self {
        let (owner, name) = binding
            .repository
            .split_once('/')
            .expect("fixture repository");
        let body = format!(
            r#"{{"id":{},"name":"{name}","full_name":"{owner}/{name}","private":true,"owner":{{"login":"{owner}"}}}}"#,
            binding.repository_id
        );
        let expected = (0..count)
            .map(|_| Reply::new(Method::Get, "repos/owner-101/repo-101", None, 200, &body))
            .collect();
        Self::new(expected, GITHUB_TOKEN)
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        let expected = self.expected.first().expect("no extra scripted request");
        assert_eq!(request.method, expected.method);
        assert_eq!(request.path.trim_start_matches('/'), expected.path);
        assert_eq!(request.query.as_deref(), expected.query.as_deref());
        assert!(request.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization")
                && value == &format!("Bearer {}", self.expected_token)
        }));
        self.seen.push(SeenRequest {
            method: request.method,
            path: request.path.clone(),
        });
        let reply = self.expected.remove(0);
        Ok(Exchange {
            status: reply.status,
            body: reply.body.into_bytes(),
        })
    }
}

pub(super) fn runner_lookup(name: &str, scale_set_id: i64, runner_id: i64) -> Reply {
    Reply::new(
        Method::Get,
        "_apis/distributedtask/pools/0/agents",
        Some(format!("agentName={name}&api-version=6.0-preview")),
        200,
        &format!(
            r#"{{"count":1,"value":[{{"id":{runner_id},"name":"{name}","runnerScaleSetId":{scale_set_id}}}]}}"#
        ),
    )
}

pub(super) fn runner_absence(name: &str) -> Reply {
    Reply::new(
        Method::Get,
        "_apis/distributedtask/pools/0/agents",
        Some(format!("agentName={name}&api-version=6.0-preview")),
        200,
        r#"{"count":0,"value":[]}"#,
    )
}

pub(super) async fn run_reconcile<I: Transport, A: Transport>(
    context: &ReconcileContext<'_>,
    claim: &JitConflictRecoveryClaim,
    identity: &mut I,
    admin: &mut A,
) -> Result<bool, HostError> {
    reconcile_admin(context, claim, identity, admin, &Secret::new(ADMIN_TOKEN)).await
}

pub(super) async fn scalar_pair(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<(i64, i64), HostError> {
    let mut rows = connection
        .query(
            "SELECT (SELECT COUNT(*) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1 AND phase_index=3 AND phase='delete_started'), (SELECT COUNT(*) FROM physical_attempt_jit_conflict_settlements WHERE reservation_id=?1)",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok((
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
    ))
}

pub(super) async fn recovery_fixture(
    suffix: char,
    set_id: i64,
    request_id: i64,
) -> Result<RecoveryFixture, HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    Box::pin(finish_recovery_fixture(
        scratch, journal, suffix, set_id, request_id,
    ))
    .await
}

#[inline(never)]
fn finish_recovery_fixture(
    scratch: Scratch,
    journal: Journal,
    suffix: char,
    set_id: i64,
    request_id: i64,
) -> impl std::future::Future<Output = Result<RecoveryFixture, HostError>> + Send {
    Box::pin(async move {
        journal
            .configure_capacity(CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 25_769_803_776,
            })
            .await?;
        let binding = binding(&format!("jit-conflict-remote-{suffix}"), 101)?;
        let scope = created_scope(&journal, &binding, set_id).await?;
        let physical =
            provision_acquired_request_with_initialization(&scope, request_id, suffix, suffix)
                .await?;
        let requested_name = format!("v{}", suffix.to_string().repeat(32));
        let (authority_generation, authority_digest) =
            scope.test_active_authority_identity().await?;
        let jit_claim = scope
            .claim_runner_jit_for_request(
                physical.intent_id,
                physical.reservation_id,
                request_id,
                &requested_name,
                authority_generation,
                &authority_digest,
            )
            .await?
            .ok_or(HostError::Journal)?;
        scope
            .mark_runner_jit_conflict_received(
                jit_claim,
                physical.reservation_id,
                authority_generation,
                &authority_digest,
            )
            .await?;
        let candidate = scope
            .recoverable_jit_conflict_candidates()
            .await?
            .into_iter()
            .find(|candidate| candidate.reservation_id() == physical.reservation_id)
            .ok_or(HostError::Journal)?;
        let claim = scope
            .claim_jit_conflict_candidate(&candidate, 180)
            .await?
            .ok_or(HostError::Journal)?;

        record_local_absence(&scope, &claim, &physical, suffix).await?;

        Ok(RecoveryFixture {
            _scratch: scratch,
            journal,
            scope,
            binding,
            physical,
            claim,
            requested_name,
        })
    })
}

async fn record_local_absence(
    scope: &ConnectionJournal,
    claim: &JitConflictRecoveryClaim,
    physical: &JobFactsReaderFixture,
    suffix: char,
) -> Result<(), HostError> {
    use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

    let names = WorkerIdentityNames::from_nonce(&suffix.to_string().repeat(32))?;
    let local_absence = crate::launch::bootstrap::test_verified_storage_cleanup(
        physical.reservation_id,
        physical.intent_id,
        TEST_ENGINE_ID.to_owned(),
        physical.docker_root_digest.clone(),
        physical.runner_container_id.clone(),
        physical.dind_container_id.clone(),
        vec![
            names.volume_name(WorkerVolumeRole::Socket),
            names.volume_name(WorkerVolumeRole::Home),
            names.volume_name(WorkerVolumeRole::Work),
            names.volume_name(WorkerVolumeRole::DindData),
            names.volume_name(WorkerVolumeRole::RunnerMetadata),
        ],
        physical.instance_binding_digest.clone(),
        crate::journal::StorageRole::PHYSICAL.to_vec(),
        true,
        true,
        true,
        true,
    )?;
    crate::journal::request_lifecycle::tests::helpers::record_cleanup_observer_probe_for_job_facts(
        scope, physical,
    )
    .await?;
    scope
        .record_jit_conflict_local_absence(claim, local_absence)
        .await
}
