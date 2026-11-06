// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::crate_release_github::GitHubAdmissionRead;
use crate::github_review_feedback::{FeedbackQuery, FeedbackRead, capture_feedback};
use base64::Engine as _;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Write},
};
use velnor_actions_contract::{
    canonical_json_bytes,
    config::ReleaseRehearsalConfig,
    homebrew_review_completion::{
        TAP_REVIEW_COMPLETION_SCHEMA, TapCompletedReviewer, TapReviewCompletionPayload,
        TapReviewVerdict,
    },
};

type ArchiveFiles = BTreeMap<u64, Vec<u8>>;

struct Api {
    values: BTreeMap<String, Value>,
    log: Vec<u8>,
    archives: ArchiveFiles,
    source_reads: Cell<u32>,
    mutate: bool,
}
impl GitHubAdmissionRead for Api {
    fn get(&self, path: &str) -> Result<Value, OrchestratorError> {
        let mut v = self
            .values
            .get(path)
            .cloned()
            .ok_or_else(|| crate::internal::internal("fixture_route"))?;
        if path == "repos/acme/consumer/pulls/8" {
            let n = self.source_reads.get();
            self.source_reads.set(n + 1);
            if self.mutate && n >= 2 {
                v["head"]["sha"] = "f".repeat(40).into();
            }
        }
        Ok(v)
    }
    fn get_optional(&self, path: &str) -> Result<Option<Value>, OrchestratorError> {
        Ok(self.values.get(path).cloned())
    }
}
impl WitnessRead for Api {
    fn witness_job_log(&self, _: &str, _: u64) -> Result<Vec<u8>, OrchestratorError> {
        Ok(self.log.clone())
    }
    fn artifact_bytes(
        &self,
        _: &str,
        id: u64,
        _: u64,
        _: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.archives
            .get(&id)
            .cloned()
            .ok_or_else(|| crate::internal::internal("fixture_artifact"))
    }
}
impl FeedbackRead for Api {
    fn feedback_get(&self, path: &str) -> Result<Value, OrchestratorError> {
        if path == "repos/acme/candidate/pulls/7" {
            return Ok(json!({"number":7,"head":{"sha":"a".repeat(40)},"commits":1}));
        }
        if path == "repos/acme/candidate/pulls/7/commits?per_page=100&page=1" {
            return Ok(json!([{"sha":"a".repeat(40)}]));
        }
        if path.contains("?per_page=100&page=1") {
            return Ok(json!([]));
        }
        Err(crate::internal::internal("fixture_feedback"))
    }
    fn feedback_graphql(
        &self,
        q: FeedbackQuery,
        _: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        match q {
            FeedbackQuery::Threads => Ok(
                json!({"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":false}}}}}}),
            ),
            FeedbackQuery::Opinions => Ok(
                json!({"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{"nodes":[],"pageInfo":{"hasNextPage":false}}}}}}),
            ),
        }
    }
}
fn zip(name: &str, data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file(name, zip::write::SimpleFileOptions::default())?;
    z.write_all(data)?;
    Ok(z.finish()?.into_inner())
}
fn fixture() -> Result<
    (
        Api,
        ReviewedSourceRequest,
        ReviewedSourceRequest,
        WitnessRequest,
    ),
    Box<dyn std::error::Error>,
> {
    let candidate = ReviewedSourceRequest {
        repository: "acme/candidate".into(),
        repository_id: 11,
        pull_request: 7,
        head_sha: "a".repeat(40),
        default_branch: "main".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        state: ReviewedPullState::OpenHead,
    };
    let source = ReviewedSourceRequest {
        repository: "acme/consumer".into(),
        repository_id: 12,
        pull_request: 8,
        head_sha: "b".repeat(40),
        default_branch: "main".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        state: ReviewedPullState::Merged {
            merge_sha: "c".repeat(40),
            merge_tree_sha: "d".repeat(40),
        },
    };
    let witness = WitnessRequest {
        repository: source.repository.clone(),
        repository_id: 12,
        workflow_sha: "e".repeat(40),
        workflow_path: ".github/workflows/release-rehearsal.yml".into(),
        run_id: 33,
        run_attempt: 1,
        job_id: 44,
        environment: "staging".into(),
        reviewer_id: 55,
        output_bundle: true,
        metadata: Some(
            velnor_actions_contract::config::ReleaseRehearsalMetadataConfig {
                intent_config_path: "qualification/release.toml".into(),
                qualification: velnor_actions_contract::config::ReleaseQualificationPolicy {
                    candidate_repository: "acme/candidate".into(),
                    candidate_repository_id: 11,
                    candidate_default_branch: "main".into(),
                    candidate_required_workflow_path: ".github/workflows/ci.yml".into(),
                    review_proof:
                        velnor_actions_contract::config::ReleaseReviewProof::CompletionCheck {
                            completion_app_id: 80,
                            completion_check_name: "review-complete".into(),
                            reviewer_app_ids: [90].into_iter().collect(),
                        },
                    historical_manifest:
                        velnor_actions_contract::config::ReleaseHistoricalManifestPolicy {
                            version: "0.1.5".into(),
                            source_sha: "b".repeat(40),
                            sha256: "c".repeat(64),
                        },
                },
                manifest_path: "Cargo.toml".into(),
                rust_version: "1.99.1".into(),
            },
        ),
        source_sha: Some("c".repeat(40)),
        script: "verify/run.sh".into(),
        source_sha256: "1".repeat(64),
        candidate_pr: 7,
        candidate_sha: candidate.head_sha.clone(),
    };
    let mut values = BTreeMap::new();
    add_reviewed_repositories(&mut values, &candidate, &source);
    add_approval(&mut values, &witness);
    add_workflow(&mut values, &witness)?;
    add_jobs(&mut values, &witness);
    let (log, archives) = add_artifacts(&mut values, &witness)?;
    let mut api = Api {
        values,
        log: log.into_bytes(),
        archives,
        source_reads: Cell::new(0),
        mutate: false,
    };
    add_feedback(&mut api, &candidate)?;
    Ok((api, candidate, source, witness))
}

fn add_reviewed_repositories(
    values: &mut BTreeMap<String, Value>,
    candidate: &ReviewedSourceRequest,
    source: &ReviewedSourceRequest,
) {
    for (r, merged) in [(candidate, false), (source, true)] {
        let p = format!("repos/{}", r.repository);
        values.insert(
            p.clone(),
            json!({"id":r.repository_id,"full_name":r.repository,"default_branch":"main"}),
        );
        let mut pull = json!({"number":r.pull_request,"head":{"sha":r.head_sha,"repo":{"id":r.repository_id,"full_name":r.repository}},"base":{"ref":"main","repo":{"id":r.repository_id,"full_name":r.repository}},"state":"open","draft":false,"merged":false,"mergeable_state":"clean"});
        if merged {
            pull["state"] = "closed".into();
            pull["merged"] = true.into();
            pull["merged_at"] = "2026-11-05T00:00:00Z".into();
            pull["merge_commit_sha"] = "c".repeat(40).into();
        }
        values.insert(format!("{p}/pulls/{}", r.pull_request), pull);
        values.insert(format!("{p}/rules/branches/main?per_page=100&page=1"),json!([{"type":"pull_request","parameters":{"required_review_thread_resolution":true}},{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":true,"required_status_checks":[{"context":"Required"}]}}]));
        let shas = if merged {
            vec![r.head_sha.clone(), "c".repeat(40)]
        } else {
            vec![r.head_sha.clone()]
        };
        for sha in shas {
            let check = json!({"id":100,"name":"Required","head_sha":sha,"app":{"id":15368,"slug":"github-actions"},"check_suite":{"id":100},"status":"completed","conclusion":"success","started_at":"2026-11-05T00:00:00Z"});
            values.insert(
                format!("{p}/commits/{sha}/check-runs?filter=all&per_page=100&page=1"),
                json!({"total_count":1,"check_runs":[check]}),
            );
            values.insert(format!("{p}/actions/runs?head_sha={sha}&per_page=100&page=1"),json!({"total_count":1,"workflow_runs":[{"check_suite_id":100,"head_sha":sha,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success"}]}));
        }
    }
}

fn add_approval(values: &mut BTreeMap<String, Value>, witness: &WitnessRequest) {
    let p = "repos/acme/consumer";
    values.insert(
        format!("{p}/git/commits/{}", "c".repeat(40)),
        json!({"sha":"c".repeat(40),"tree":{"sha":"d".repeat(40)}}),
    );
    values.insert(
        format!("{p}/compare/{}...main", "c".repeat(40)),
        json!({"status":"ahead"}),
    );
    values.insert(
        format!("{p}/branches/main"),
        json!({"name":"main","protected":true,"commit":{"sha":witness.workflow_sha}}),
    );
    values.insert(format!("{p}/actions/runs/33/attempts/1"),json!({"id":33,"run_attempt":1,"event":"workflow_dispatch","head_sha":witness.workflow_sha,"head_branch":"main","path":witness.workflow_path,"repository":{"id":12},"head_repository":{"id":12},"status":"completed","conclusion":"success"}));
    values.insert(format!("{p}/environments/staging"),json!({"id":77,"name":"staging","can_admins_bypass":false,"deployment_branch_policy":{"protected_branches":true,"custom_branch_policies":false},"protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"type":"User","id":55}}]}]}));
    values.insert(format!("{p}/actions/runs/33/approvals"),json!([{"state":"approved","user":{"type":"User","id":55},"environments":[{"id":77,"name":"staging"}]}]));
}

fn add_workflow(
    values: &mut BTreeMap<String, Value>,
    witness: &WitnessRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::release::crate_release_pr_tree::{TreeItem, git_tree_sha};
    let p = "repos/acme/consumer";
    let config = ReleaseRehearsalConfig {
        script: witness.script.clone(),
        environment: witness.environment.clone(),
        reviewer_id: 55,
        output_bundle: true,
        metadata: witness.metadata.clone(),
    };
    let bytes = velnor_actions_workflow_renderer::release_tree::rehearsal::render(
        &config,
        env!("CARGO_PKG_VERSION"),
    )?
    .bytes
    .into_bytes();
    let blob = crate::release_policy_admission::git_blob_sha1(&bytes)?;
    let mut entry = TreeItem {
        name: "release-rehearsal.yml".into(),
        mode: "100644".into(),
        sha: blob.clone(),
    };
    for name in ["workflows", ".github", "root"] {
        let sha = git_tree_sha(&[entry.clone()])?;
        let kind = if entry.mode == "040000" {
            "tree"
        } else {
            "blob"
        };
        values.insert(format!("{p}/git/trees/{sha}"),json!({"sha":sha,"truncated":false,"tree":[{"path":entry.name,"mode":entry.mode,"type":kind,"sha":entry.sha}]}));
        entry = TreeItem {
            name: name.into(),
            mode: "040000".into(),
            sha,
        };
    }
    values.insert(format!("{p}/git/blobs/{blob}"),json!({"sha":blob,"encoding":"base64","size":bytes.len(),"content":base64::engine::general_purpose::STANDARD.encode(&bytes)}));
    values.insert(
        format!("{p}/git/commits/{}", witness.workflow_sha),
        json!({"sha":witness.workflow_sha,"tree":{"sha":entry.sha}}),
    );
    Ok(())
}

fn add_jobs(values: &mut BTreeMap<String, Value>, witness: &WitnessRequest) {
    let p = "repos/acme/consumer";
    let jobs=[("preflight",41),("gate",42),("execute",43),("witness",44),("metadata",45)].iter().map(|(name,id)|json!({"name":name,"id":id,"run_id":33,"run_attempt":1,"head_sha":witness.workflow_sha,"status":"completed","conclusion":"success"})).collect::<Vec<_>>();
    values.insert(
        format!("{p}/actions/runs/33/attempts/1/jobs?per_page=100&page=1"),
        json!({"total_count":5,"jobs":jobs}),
    );
    values.insert(format!("{p}/actions/jobs/44"),json!({"id":44,"run_id":33,"run_attempt":1,"head_sha":witness.workflow_sha,"name":"witness","status":"completed","conclusion":"success","steps":[{"name":"Bind observation artifact","status":"completed","conclusion":"success"},{"name":"Bind rehearsal output artifact","status":"completed","conclusion":"success"},{"name":"Bind metadata artifact","status":"completed","conclusion":"success"},{"name":"Bind approved metadata source","status":"completed","conclusion":"success"}]}));
}

fn add_artifacts(
    values: &mut BTreeMap<String, Value>,
    witness: &WitnessRequest,
) -> Result<(String, ArchiveFiles), Box<dyn std::error::Error>> {
    let p = "repos/acme/consumer";
    let observation = json!({"schema":1,"observation":"script-exited-successfully","repository":witness.repository,"workflow_sha":witness.workflow_sha,"run_id":"33","run_attempt":"1","script":witness.script,"source_sha256":witness.source_sha256,"environment":witness.environment,"reviewer_id":"55","unverified_candidate_pr":"7","unverified_candidate_sha":witness.candidate_sha});
    let archives = BTreeMap::from([
        (
            77,
            zip(
                "rehearsal-observation.json",
                &serde_json::to_vec(&observation)?,
            )?,
        ),
        (78, zip("rehearsal-output.tar", b"fixture-output")?),
        (79, zip("metadata.json", br#"{"packages":[]}"#)?),
    ]);
    let mut log = String::new();
    for (id, name, marker) in [
        (77, "rehearsal-observation", "VELNOR_REHEARSAL_WITNESS"),
        (
            78,
            "rehearsal-output-observation",
            "VELNOR_REHEARSAL_OUTPUT",
        ),
        (
            79,
            "approved-metadata-observation",
            "VELNOR_REHEARSAL_METADATA",
        ),
    ] {
        let data = &archives[&id];
        let digest = crate::planner::generator::sha256_hex(data);
        values.insert(format!("{p}/actions/artifacts/{id}"),json!({"id":id,"name":name,"expired":false,"size_in_bytes":data.len(),"digest":format!("sha256:{digest}"),"workflow_run":{"id":33,"repository_id":12,"head_repository_id":12,"head_sha":witness.workflow_sha}}));
        writeln!(
            log,
            "2026-11-05T00:00:00.000Z {marker} run=33 attempt=1 artifact={id} sha256={digest}"
        )?;
    }
    writeln!(
        log,
        "2026-11-05T00:00:00Z VELNOR_METADATA_SOURCE run=33 attempt=1 source={} config=qualification/release.toml tree={} sha256={}",
        "c".repeat(40),
        "d".repeat(40),
        "9".repeat(64)
    )?;
    Ok((log, archives))
}

fn add_feedback(
    api: &mut Api,
    candidate: &ReviewedSourceRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let reviewers = BTreeSet::from([90]);
    let context = FeedbackContext {
        repository: &candidate.repository,
        pull_request_number: 7,
        head_sha: &candidate.head_sha,
        completion_app_id: 80,
        completion_check_name: "review-complete",
        expected_reviewer_app_ids: &reviewers,
    };
    let observed = capture_feedback(api, &context, &[])?;
    let payload = TapReviewCompletionPayload {
        schema: TAP_REVIEW_COMPLETION_SCHEMA.into(),
        repository: candidate.repository.clone(),
        pull_request_number: 7,
        head_sha: candidate.head_sha.clone(),
        verdict: TapReviewVerdict::Complete,
        feedback_digest: observed.snapshot.canonical_digest()?,
        has_unaddressed_findings: false,
        reviewers: vec![TapCompletedReviewer {
            app_id: 90,
            head_sha: candidate.head_sha.clone(),
            evidence_digest: format!("b3-{}", "2".repeat(64)),
            complete: true,
            has_unaddressed_findings: false,
        }],
    };
    let text = String::from_utf8(canonical_json_bytes(&payload)?)?;
    let route = format!(
        "repos/acme/candidate/commits/{}/check-runs?filter=all&per_page=100&page=1",
        candidate.head_sha
    );
    let checks = &mut api.values.get_mut(&route).ok_or("check route")?["check_runs"];
    checks.as_array_mut().ok_or("checks")?.push(json!({"id":101,"name":"review-complete","app":{"id":80},"head_sha":candidate.head_sha,"status":"completed","conclusion":"success","output":{"text":text}}));
    api.values.get_mut(&route).ok_or("check route")?["total_count"] = 2.into();
    Ok(())
}

#[path = "crate_release_qualification_capture_integration_cases.rs"]
mod cases;

#[path = "crate_release_qualification_native_capture_tests.rs"]
mod native_capture;
