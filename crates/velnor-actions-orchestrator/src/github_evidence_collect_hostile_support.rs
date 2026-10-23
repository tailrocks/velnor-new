use std::cell::Cell;
use std::time::Instant;

use serde_json::{Value, json};

use super::{RUN_TIME, run_summary};
use crate::OrchestratorError;
use crate::api::github_evidence_http::{
    ArtifactArchiveRequest, DownloadReceipt, EvidenceRoute, GitHubEvidenceRead, GitHubRepository,
};
use crate::internal::internal;

#[path = "github_evidence_collect_hostile_mutations.rs"]
mod mutations;

#[derive(Clone, Copy)]
pub(super) enum Mutation {
    RepositoryId,
    RepositoryName,
    WorkflowPath,
    RunWorkflowId,
    RunEvent,
    RunBranch,
    RunHead,
    AttemptRunId,
    AttemptNumber,
    AttemptEvent,
    AttemptBranch,
    AttemptHead,
    DuplicateWorkflowId,
    DuplicateRunId,
    DuplicateArtifactId,
    ArtifactExpired,
    ArtifactEmpty,
    ArtifactWrongSize,
    ArtifactWrongDigest,
    ArtifactByIdChanged,
    ArchiveReceiptMismatch,
    WorkflowRunsCountDrift,
    WorkflowRunsTruncatedSecondPage,
    ArtifactCountDrift,
    ArtifactTruncatedSecondPage,
    MalformedRepositoryBytes,
    DuplicateRepositoryKeyBytes,
}

pub(super) struct HostileReader {
    inner: super::support::FixtureReader,
    mutation: Mutation,
    page_two_seen: Cell<bool>,
}

impl HostileReader {
    pub(super) fn new(mutation: Mutation) -> Self {
        Self {
            inner: super::support::FixtureReader::default(),
            mutation,
            page_two_seen: Cell::new(false),
        }
    }

    pub(super) fn saw_second_page(&self) -> bool {
        self.page_two_seen.get()
    }

    fn json_body(&self, route: &EvidenceRoute) -> Result<Vec<u8>, OrchestratorError> {
        if matches!(route, EvidenceRoute::Repository) {
            match self.mutation {
                Mutation::MalformedRepositoryBytes => return Ok(b"{not-json".to_vec()),
                Mutation::DuplicateRepositoryKeyBytes => {
                    return Ok(br#"{"id":1,"id":1,"name":"velnor","full_name":"tailrocks/velnor","default_branch":"main","owner":{"login":"tailrocks"}}"#.to_vec());
                }
                _ => {}
            }
        }
        if let Some(body) = self.pagination_body(route)? {
            return Ok(body);
        }
        let bytes = self.inner.json(
            &GitHubRepository {
                owner: "tailrocks".to_owned(),
                name: "velnor".to_owned(),
            },
            route,
            "fixture-token",
            Instant::now() + std::time::Duration::from_secs(30),
        )?;
        let mut value: Value =
            serde_json::from_slice(&bytes).map_err(|_| internal("hostile_fixture_json_parse"))?;
        self.apply_mutation(route, &mut value);
        serde_json::to_vec(&value).map_err(|_| internal("hostile_fixture_json_encode"))
    }

    fn pagination_body(&self, route: &EvidenceRoute) -> Result<Option<Vec<u8>>, OrchestratorError> {
        let value = self
            .workflow_pagination_value(route)
            .or_else(|| self.artifact_pagination_value(route));
        value
            .map(|body| {
                serde_json::to_vec(&body).map_err(|_| internal("hostile_fixture_json_encode"))
            })
            .transpose()
    }
    fn workflow_pagination_value(&self, route: &EvidenceRoute) -> Option<Value> {
        match (self.mutation, route) {
            (
                Mutation::WorkflowRunsCountDrift | Mutation::WorkflowRunsTruncatedSecondPage,
                EvidenceRoute::WorkflowRuns {
                    workflow_id: 10,
                    page: 1,
                    ..
                },
            ) => {
                let rows = (0_u64..100)
                    .map(|index| run_summary(10_000 + index, 10, 1, RUN_TIME, "success"))
                    .collect::<Vec<_>>();
                Some(json!({ "total_count": 101, "workflow_runs": rows }))
            }
            (
                Mutation::WorkflowRunsCountDrift | Mutation::WorkflowRunsTruncatedSecondPage,
                EvidenceRoute::WorkflowRuns {
                    workflow_id: 10,
                    page: 2,
                    ..
                },
            ) => {
                self.page_two_seen.set(true);
                let total = if matches!(self.mutation, Mutation::WorkflowRunsCountDrift) {
                    102
                } else {
                    101
                };
                let rows = if matches!(self.mutation, Mutation::WorkflowRunsCountDrift) {
                    vec![
                        run_summary(10_100, 10, 1, RUN_TIME, "success"),
                        run_summary(10_101, 10, 1, RUN_TIME, "success"),
                    ]
                } else {
                    Vec::new()
                };
                Some(json!({ "total_count": total, "workflow_runs": rows }))
            }
            _ => None,
        }
    }

    fn artifact_pagination_value(&self, route: &EvidenceRoute) -> Option<Value> {
        match (self.mutation, route) {
            (
                Mutation::ArtifactCountDrift | Mutation::ArtifactTruncatedSecondPage,
                EvidenceRoute::RunArtifacts {
                    run_id: 200,
                    page: 1,
                },
            ) => {
                let rows = (0_u64..100)
                    .map(|index| {
                        json!({
                            "id": 5_000 + index,
                            "name": format!("unselected-{index}"),
                            "size_in_bytes": 1,
                            "expired": false,
                            "digest": format!("sha256:{}", "c".repeat(64)),
                            "workflow_run": null
                        })
                    })
                    .collect::<Vec<_>>();
                Some(json!({ "total_count": 101, "artifacts": rows }))
            }
            (
                Mutation::ArtifactCountDrift | Mutation::ArtifactTruncatedSecondPage,
                EvidenceRoute::RunArtifacts {
                    run_id: 200,
                    page: 2,
                },
            ) => {
                self.page_two_seen.set(true);
                let total = if matches!(self.mutation, Mutation::ArtifactCountDrift) {
                    102
                } else {
                    101
                };
                let artifacts = if matches!(self.mutation, Mutation::ArtifactCountDrift) {
                    vec![
                        json!({
                            "id": 5_100,
                            "name": "unselected-100",
                            "size_in_bytes": 1,
                            "expired": false,
                            "digest": format!("sha256:{}", "c".repeat(64)),
                            "workflow_run": null
                        }),
                        json!({
                            "id": 5_101,
                            "name": "unselected-101",
                            "size_in_bytes": 1,
                            "expired": false,
                            "digest": format!("sha256:{}", "c".repeat(64)),
                            "workflow_run": null
                        }),
                    ]
                } else {
                    Vec::new()
                };
                Some(json!({ "total_count": total, "artifacts": artifacts }))
            }
            _ => None,
        }
    }

    fn apply_mutation(&self, route: &EvidenceRoute, value: &mut Value) {
        mutations::apply(self.mutation, route, value);
    }
}

impl GitHubEvidenceRead for HostileReader {
    fn json(
        &self,
        _repo: &GitHubRepository,
        route: &EvidenceRoute,
        _token: &str,
        _deadline: Instant,
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.json_body(route)
    }

    fn archive(
        &self,
        request: ArtifactArchiveRequest<'_>,
    ) -> Result<DownloadReceipt, OrchestratorError> {
        if matches!(
            self.mutation,
            Mutation::ArtifactWrongSize | Mutation::ArtifactWrongDigest
        ) {
            let archive = crate::api::github_evidence::test_support::archive();
            let digest = super::super::bundle::sha256_hex(&archive);
            std::fs::write(request.dest, &archive)
                .map_err(|_| internal("hostile_fixture_archive_write"))?;
            return Ok(DownloadReceipt {
                size_bytes: archive.len() as u64,
                sha256: digest,
            });
        }
        let mut receipt = self.inner.archive(request)?;
        if matches!(self.mutation, Mutation::ArchiveReceiptMismatch) {
            receipt.size_bytes += 1;
        }
        Ok(receipt)
    }
}
