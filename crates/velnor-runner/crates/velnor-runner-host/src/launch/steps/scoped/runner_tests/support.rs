use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::{
    AssignmentKey, CapacityLimits, ConnectionJournal, CreatedScaleSetResponse, Journal,
    ScaleSetCreateRequest,
};
use crate::launch::steps::source::test_receipt;
use crate::launch::{
    SelectedEngineContext, VerifiedImageIdentity, VerifiedWorkerImages,
    inspect_tests::{DockerStub, http},
};

pub(super) const SET_ID: i64 = 501;
pub(super) const REQUEST_ID: i64 = 41;
const MESSAGE_ID: i64 = 77;
pub(super) const DOCKER_ROOT: &str = "/var/lib/docker";

pub(super) struct Fixture {
    _scratch: Scratch,
    pub(super) journal: Journal,
    pub(super) scope: ConnectionJournal,
    pub(super) request: crate::journal::AssignmentIdentity,
    pub(super) binding: RepositoryConnection,
}

impl Fixture {
    pub(super) async fn open(label: &str) -> Result<Self, String> {
        let scratch = Scratch::new(label)?;
        let journal = Journal::open(&scratch.file())
            .await
            .map_err(|error| error.to_string())?;
        journal
            .bind_engine("test-engine")
            .await
            .map_err(|error| error.to_string())?;
        journal
            .configure_capacity(CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 8_589_934_592,
            })
            .await
            .map_err(|error| error.to_string())?;
        let binding = binding()?;
        let scope = journal
            .connection_scope(&binding)
            .await
            .map_err(|error| error.to_string())?;
        let create = ScaleSetCreateRequest::new(&binding, 7).map_err(|error| error.to_string())?;
        let prepared = scope
            .prepare_scale_set_create(&create)
            .await
            .map_err(|error| error.to_string())?;
        let claim = scope
            .claim_scale_set_create(&prepared)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Scale Set create claim is absent".to_owned())?;
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
            .map_err(|error| error.to_string())?;
        scope
            .bind_engine_context("test-engine", "unix:///var/run/docker.sock", "linux/amd64")
            .await
            .map_err(|error| error.to_string())?;
        let request = scope
            .assignment_identity(AssignmentKey::AssignedRequest {
                request_id: REQUEST_ID,
            })
            .await
            .map_err(|error| error.to_string())?;
        let source_binding = scope
            .source_authorization_binding()
            .await
            .map_err(|error| error.to_string())?;
        let receipt = test_receipt(
            &source_binding,
            REQUEST_ID,
            MESSAGE_ID,
            verified_source(),
            SourcePolicyMode::Repository,
        );
        scope
            .record_source_authorization(&request, &receipt)
            .await
            .map_err(|error| error.to_string())?;
        Ok(Self {
            _scratch: scratch,
            journal,
            scope,
            request,
            binding,
        })
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Result<Self, String> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-scoped-adapter-{}-{label}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) struct FakeLane {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    pub(super) methods: Vec<Method>,
    pub(super) paths: Vec<String>,
}

impl FakeLane {
    pub(super) fn new(replies: impl IntoIterator<Item = Result<Exchange, TransportFail>>) -> Self {
        Self {
            replies: replies.into_iter().collect(),
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

impl crate::launch::Lane for FakeLane {
    fn on_admin(&mut self) -> Result<(), crate::scale_set::EnsureError> {
        Ok(())
    }

    fn on_queue(&mut self) -> Result<(), crate::scale_set::EnsureError> {
        Ok(())
    }

    fn use_github_api(&mut self) -> Result<(), crate::scale_set::EnsureError> {
        Ok(())
    }
}

pub(super) fn exchange(status: u16, body: &str) -> Exchange {
    Exchange {
        status,
        body: body.as_bytes().to_vec(),
    }
}

fn binding() -> Result<RepositoryConnection, String> {
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
        ConnectionId::parse("scoped-adapter").map_err(|error| error.to_string())?,
        101,
        "owner/repository".to_owned(),
        KeychainReference::parse("keychain:velnor/scoped-adapter")
            .map_err(|error| error.to_string())?,
        selector,
        policy,
    )
    .map_err(|error| error.to_string())
}

fn verified_source() -> velnor_runner_github::workflow_run::VerifiedWorkflowSource {
    use velnor_runner_github::workflow_run::{VerifiedJobWorkflowRef, VerifiedWorkflowSource};

    let workflow_ref = "owner/repository/.github/workflows/build.yml@refs/heads/main".to_owned();
    VerifiedWorkflowSource {
        repository_id: 101,
        owner_name: "owner".to_owned(),
        repository_name: "repository".to_owned(),
        workflow_run_id: 900,
        event_name: "push".to_owned(),
        head_branch: Some("main".to_owned()),
        head_sha: "a".repeat(40),
        job_workflow_ref: workflow_ref.clone(),
        job_workflow: VerifiedJobWorkflowRef {
            full_ref: workflow_ref,
            owner_name: "owner".to_owned(),
            repository_name: "repository".to_owned(),
            workflow_path: ".github/workflows/build.yml".to_owned(),
            reference: "refs/heads/main".to_owned(),
            kind: velnor_runner_github::workflow_run::WorkflowRefKind::Branch,
        },
        workflow_source_sha: None,
        run_attempt: 1,
    }
}

pub(super) fn test_images(root_digest: &str) -> VerifiedWorkerImages {
    crate::launch::verified_images::test_worker_images_for(
        "test-engine",
        root_digest,
        "aarch64",
        "linux/amd64",
    )
}

pub(super) fn selected_engine(
    images: &VerifiedWorkerImages,
) -> Result<SelectedEngineContext, String> {
    SelectedEngineContext::bind(
        &crate::journal::EngineContext {
            engine_id: images.engine_id().to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
            platform: images.platform().to_owned(),
        },
        images,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn docker_identity_stub(images: &VerifiedWorkerImages) -> Result<DockerStub, String> {
    let response_set = || {
        vec![
            http(
                200,
                &engine_info(images.engine_id(), DOCKER_ROOT, "aarch64"),
            ),
            http(
                200,
                &image_inspect(images, "velnor-runner", images.runner()),
            ),
            http(200, &image_inspect(images, "velnor-dind", images.dind())),
            http(
                200,
                &engine_info(images.engine_id(), DOCKER_ROOT, "aarch64"),
            ),
        ]
    };
    let mut responses = response_set();
    responses.extend(response_set());
    DockerStub::open(responses)
}

pub(super) fn assert_runner_name(name: &str) -> Result<(), String> {
    if name.len() != 33
        || !name.starts_with('v')
        || !name[1..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("runner name did not use one lower-case 128-bit nonce".to_owned());
    }
    Ok(())
}

pub(super) fn engine_info(engine_id: &str, root: &str, architecture: &str) -> String {
    json!({
        "ID": engine_id,
        "OSType": "linux",
        "Architecture": architecture,
        "DockerRootDir": root
    })
    .to_string()
}

pub(super) fn image_inspect(
    images: &VerifiedWorkerImages,
    name: &str,
    identity: &VerifiedImageIdentity,
) -> String {
    let reference = format!(
        "{name}:v{}-{}",
        images.source_sha(),
        identity.archive_sha256()
    );
    let index = identity
        .image_index_digest()
        .expect("worker image fixture has an index");
    let tag = reference.split_once(':').expect("image has a tag").1;
    json!({
        "Id": identity.runtime_id(),
        "Os": "linux",
        "Architecture": "amd64",
        "RepoTags": [reference],
        "RepoDigests": [format!("docker.io/library/{name}@{index}")],
        "Descriptor": {
            "digest": index,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "size": identity.image_index_size().expect("worker index has size"),
            "annotations": {
                "io.containerd.image.name": format!("docker.io/library/{reference}"),
                "org.opencontainers.image.ref.name": tag
            }
        }
    })
    .to_string()
}
