use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use velnor_actions_contract::{
    GitHubEvidenceAcquisitionV1, GitHubEvidenceDocumentKindV1, GitHubEvidenceDocumentV1,
    GitHubEvidenceFetchSpecV1, GitHubEvidenceRepositoryV1, GitHubEvidenceRunAttemptV1,
    GitHubEvidenceSnapshotV1, canonical_json_bytes,
};

use crate::OrchestratorError;
use crate::api::github_evidence_http::{
    EvidenceRoute, GitHubEvidenceRead, GitHubRepository, NativeGitHubEvidenceRead,
};
use crate::internal::internal;

#[path = "github_evidence_collect_api.rs"]
mod api;
#[path = "github_evidence_collect_artifacts.rs"]
mod artifacts;
#[path = "github_evidence_collect_bundle.rs"]
mod bundle;
#[path = "github_evidence_collect_context.rs"]
mod context;
#[path = "github_evidence_collect_time.rs"]
mod time;
#[path = "github_evidence_collect_validate.rs"]
mod validate;
#[path = "github_evidence_collect_workflows.rs"]
mod workflows;

const MAX_API_PAGES: u32 = 64;
const MAX_TOTAL_RUNS: u64 = 2_000;
const FETCH_DEADLINE: Duration = Duration::from_secs(600);
const PAGE_SIZE: u64 = 100;

pub(super) fn run() -> Result<(), OrchestratorError> {
    let invocation = Invocation::from_env()?;
    let reader = NativeGitHubEvidenceRead;
    let bundle = bundle::BundleRoot::create()?;
    let mut collector = Collector::new(&reader, invocation, bundle);
    collector.capture_repository()?;
    let workflow_ids = collector.capture_workflow_inventory()?;
    let workflow_runs = collector.capture_workflows(&workflow_ids)?;
    collector.capture_artifacts(&workflow_runs)?;
    collector.finish()
}

struct Invocation {
    spec: GitHubEvidenceFetchSpecV1,
    token: String,
    repository: GitHubRepository,
    repository_id: u64,
    run_id: u64,
    run_attempt: u32,
    ref_name: String,
    sha: String,
    window: time::EvidenceWindow,
}

struct Collector<'a, R: GitHubEvidenceRead> {
    reader: &'a R,
    invocation: Invocation,
    bundle: bundle::BundleRoot,
    repository: Option<GitHubEvidenceRepositoryV1>,
    documents: Vec<GitHubEvidenceDocumentV1>,
    attempts: Vec<GitHubEvidenceRunAttemptV1>,
    artifacts: Vec<velnor_actions_contract::GitHubEvidenceArtifactCaptureV1>,
    workflow_inventory: BTreeMap<String, u64>,
    total_bytes: u64,
    total_unpacked_bytes: u64,
    deadline: Instant,
}

pub(super) struct EvidenceDocumentIdentity {
    kind: GitHubEvidenceDocumentKindV1,
    workflow_path: Option<String>,
    run_id: Option<u64>,
    attempt: Option<u32>,
    artifact_id: Option<u64>,
    page: Option<u32>,
    path: String,
}

impl<'a, R: GitHubEvidenceRead> Collector<'a, R> {
    fn new(reader: &'a R, invocation: Invocation, bundle: bundle::BundleRoot) -> Self {
        Self {
            reader,
            invocation,
            bundle,
            repository: None,
            documents: Vec::new(),
            attempts: Vec::new(),
            artifacts: Vec::new(),
            workflow_inventory: BTreeMap::new(),
            total_bytes: 0,
            total_unpacked_bytes: 0,
            deadline: Instant::now() + FETCH_DEADLINE,
        }
    }

    pub(super) fn read_json<T: DeserializeOwned>(
        &self,
        route: &EvidenceRoute,
    ) -> Result<(Vec<u8>, T), OrchestratorError> {
        self.check_deadline()?;
        let body = self.reader.json(
            &self.invocation.repository,
            route,
            &self.invocation.token,
            self.deadline,
        )?;
        let parsed = api::decode(&body)?;
        Ok((body, parsed))
    }

    pub(super) fn capture_document(
        &mut self,
        identity: EvidenceDocumentIdentity,
        body: &[u8],
    ) -> Result<u32, OrchestratorError> {
        let size_bytes = u64::try_from(body.len()).map_err(|_| internal("github_evidence_size"))?;
        if self.documents.len() >= velnor_actions_contract::MAX_GITHUB_EVIDENCE_DOCUMENTS
            || size_bytes == 0
            || size_bytes > velnor_actions_contract::MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES
            || self.total_bytes.saturating_add(size_bytes)
                > velnor_actions_contract::MAX_GITHUB_EVIDENCE_BUNDLE_BYTES
        {
            return Err(internal("github_evidence_document_bound"));
        }
        self.bundle.write_new(&identity.path, body)?;
        let index = u32::try_from(self.documents.len())
            .map_err(|_| internal("github_evidence_document_count"))?;
        self.documents.push(GitHubEvidenceDocumentV1 {
            kind: identity.kind,
            workflow_path: identity.workflow_path,
            run_id: identity.run_id,
            attempt: identity.attempt,
            artifact_id: identity.artifact_id,
            page: identity.page,
            path: identity.path,
            size_bytes,
            sha256: bundle::sha256_hex(body),
        });
        self.total_bytes = self.total_bytes.saturating_add(size_bytes);
        Ok(index)
    }

    pub(super) fn workflow_source_map(
        &self,
    ) -> BTreeMap<String, velnor_actions_contract::GitHubEvidenceWorkflowRefV1> {
        self.invocation
            .spec
            .workflow_sources
            .iter()
            .cloned()
            .map(|source| (source.path.clone(), source))
            .collect()
    }

    pub(super) fn check_deadline(&self) -> Result<(), OrchestratorError> {
        if Instant::now() >= self.deadline {
            Err(internal("github_evidence_deadline"))
        } else {
            Ok(())
        }
    }

    fn capture_repository(&mut self) -> Result<(), OrchestratorError> {
        let (body, repository): (Vec<u8>, api::RepositoryResponse) =
            self.read_json(&EvidenceRoute::Repository)?;
        if repository.id != self.invocation.repository_id
            || repository.default_branch != self.invocation.spec.default_branch
            || !repository.full_name.eq_ignore_ascii_case(&format!(
                "{}/{}",
                self.invocation.repository.owner, self.invocation.repository.name
            ))
            || repository.name != self.invocation.repository.name
            || repository.owner.login != self.invocation.repository.owner
        {
            return Err(internal("github_evidence_repository_identity"));
        }
        self.capture_document(
            EvidenceDocumentIdentity {
                kind: GitHubEvidenceDocumentKindV1::Repository,
                workflow_path: None,
                run_id: None,
                attempt: None,
                artifact_id: None,
                page: None,
                path: "documents/repository.json".to_owned(),
            },
            &body,
        )?;
        self.repository = Some(GitHubEvidenceRepositoryV1 {
            id: repository.id,
            owner: repository.owner.login,
            name: repository.name,
            default_branch: repository.default_branch,
        });
        Ok(())
    }

    fn capture_workflow_inventory(&mut self) -> Result<BTreeMap<String, u64>, OrchestratorError> {
        self.collect_workflow_inventory()
    }

    fn capture_workflows(
        &mut self,
        workflow_ids: &BTreeMap<String, u64>,
    ) -> Result<Vec<GitHubEvidenceRunAttemptV1>, OrchestratorError> {
        self.collect_workflows(workflow_ids)
    }

    fn capture_artifacts(
        &mut self,
        workflow_runs: &[GitHubEvidenceRunAttemptV1],
    ) -> Result<(), OrchestratorError> {
        self.collect_artifacts(workflow_runs)
    }

    fn finish(self) -> Result<(), OrchestratorError> {
        self.write_snapshot()
    }

    fn write_snapshot(self) -> Result<(), OrchestratorError> {
        let repository = self
            .repository
            .clone()
            .ok_or_else(|| internal("github_evidence_repository_missing"))?;
        let spec_bytes = canonical_json_bytes(&self.invocation.spec)
            .map_err(|_| internal("github_evidence_spec_canonical"))?;
        let snapshot = GitHubEvidenceSnapshotV1 {
            schema: velnor_actions_contract::GITHUB_EVIDENCE_SNAPSHOT_SCHEMA,
            spec_sha256: bundle::sha256_hex(&spec_bytes),
            repository,
            acquisition: GitHubEvidenceAcquisitionV1 {
                run_id: self.invocation.run_id,
                run_attempt: self.invocation.run_attempt,
                collector_ref: self.invocation.ref_name,
                collector_sha: self.invocation.sha,
                captured_at: self.invocation.window.captured_at_text(),
                window_start: self.invocation.window.start_text(),
                window_end: self.invocation.window.end_text(),
            },
            workflow_sources: self.invocation.spec.workflow_sources.clone(),
            documents: self.documents,
            run_attempts: self.attempts,
            artifacts: self.artifacts,
        };
        snapshot
            .validate_for_spec(&self.invocation.spec)
            .map_err(|reason| internal(&format!("github_evidence_snapshot_invalid:{reason}")))?;
        let index = canonical_json_bytes(&snapshot)
            .map_err(|_| internal("github_evidence_snapshot_serialize"))?;
        if index.is_empty()
            || index.len() as u64 > velnor_actions_contract::MAX_GITHUB_EVIDENCE_DOCUMENT_BYTES
            || self.total_bytes.saturating_add(index.len() as u64)
                > velnor_actions_contract::MAX_GITHUB_EVIDENCE_BUNDLE_BYTES
        {
            return Err(internal("github_evidence_index_bound"));
        }
        self.bundle.write_new("index.json", &index)?;
        self.bundle.finish();
        Ok(())
    }
}
