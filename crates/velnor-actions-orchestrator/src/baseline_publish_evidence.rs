//! Publication proof: current successful gate and authenticated parent.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{
    BaselineProof, FinalReport, FinalStatus, ManifestTaskProof, ObligationDecision, Plan,
    canonical_json_bytes, digest_b3, parse_strict_json,
};

use super::PublishRequest;
use crate::OrchestratorError;
use crate::cover::Signals;
use crate::cover::revalidate::{MergeAnchorExpectations, revalidate_coverage_with_anchors};
use crate::internal::{internal, internal_contract};
use crate::merge::BaselineManifest;
use crate::merge::required_evidence::BaselineTaskEntry;

/// Read bounded strict evidence; symlinks, duplicates and missing files refuse.
fn read_evidence<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, OrchestratorError> {
    let text = crate::retrieve_reports::read_staged_text(
        path,
        crate::retrieve_reports::MAX_RETRIEVE_PLAN_BYTES,
    )
    .map_err(|reason| internal(&format!("publish_refused:evidence:{reason}")))?;
    let raw = parse_strict_json(&text).map_err(internal_contract)?;
    serde_json::from_value(raw).map_err(|_| internal("publish_refused:malformed_evidence"))
}

/// Require this run's passed final report before any direct or carried proof.
fn check_report(plan: &Plan, report: &FinalReport) -> Result<(), OrchestratorError> {
    report.validate().map_err(internal_contract)?;
    let mut expected: Vec<_> = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.report_id.clone())
        .collect();
    expected.sort();
    let mut artifacts: Vec<_> = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.artifact_id.clone())
        .collect();
    artifacts.sort();
    artifacts.dedup();
    let executed = plan
        .obligations
        .iter()
        .filter(|ob| ob.decision == ObligationDecision::Execute)
        .count();
    let covered = plan.obligations.len() - executed;
    let expected_status = if plan.obligations.is_empty() {
        FinalStatus::NoWork
    } else {
        FinalStatus::Passed
    };
    let passed = report.status == expected_status;
    let counts = &report.counts;
    let bound = report.run_key == plan.run_key
        && report.report_id
            == velnor_actions_contract::final_report_id_for_run(&plan.run_key)
                .map_err(internal_contract)?
        && report.plan_id == plan.plan_id
        && report.expected_report_ids == expected
        && report.downloaded_artifact_ids == artifacts
        && usize::try_from(counts.selected).ok() == Some(plan.task_ids.len())
        && counts
            .executed
            .checked_add(counts.empty_partition)
            .and_then(|count| usize::try_from(count).ok())
            == Some(executed)
        && usize::try_from(counts.covered).ok() == Some(covered)
        && counts.reused == 0
        && counts.failed == 0
        && counts.cancelled == 0
        && counts.blocked == 0
        && counts.not_run == 0;
    if !passed || !bound {
        return Err(internal("publish_refused:final_report_mismatch"));
    }
    check_report_jobs(plan, report)?;
    Ok(())
}

/// Required owners must actually succeed, or deliberately skip with coverage.
fn check_report_jobs(plan: &Plan, report: &FinalReport) -> Result<(), OrchestratorError> {
    use velnor_actions_contract::JobConclusion;
    let mut seen = BTreeSet::new();
    for job in &report.required_job_results {
        let obligations: Vec<_> = plan
            .obligations
            .iter()
            .filter(|ob| ob.job_id == job.job_id)
            .collect();
        let skipped = velnor_actions_contract::is_crate_job_id(&job.job_id)
            && !obligations.is_empty()
            && obligations
                .iter()
                .all(|ob| ob.decision == ObligationDecision::CoveredByTrustedBaseline);
        if velnor_actions_contract::validate_job_id(&job.job_id).is_err()
            || !seen.insert(job.job_id.as_str())
            || !(job.conclusion == JobConclusion::Success
                || (job.conclusion == JobConclusion::Skipped && skipped))
        {
            return Err(internal("publish_refused:final_job_mismatch"));
        }
    }
    if !seen.contains("plan")
        || plan
            .obligations
            .iter()
            .any(|ob| !seen.contains(ob.job_id.as_str()))
    {
        return Err(internal("publish_refused:final_job_mismatch"));
    }
    Ok(())
}

/// Revalidate exact protected parent evidence before forwarding executions.
pub(super) fn current_report(plan: &Plan, runner_temp: &Path) -> Result<(), OrchestratorError> {
    if plan
        .obligations
        .iter()
        .any(|ob| ob.decision == ObligationDecision::ReusedFromTaskCache)
    {
        return Err(internal("publish_refused:unproven_reuse"));
    }
    let run_dir = runner_temp.join("velnor").join(&plan.run_key);
    let report: FinalReport = read_evidence(&run_dir.join("final-report.json"))?;
    check_report(plan, &report)
}

/// Service-qualified immutable current-run artifact, or proven absence.
pub(super) fn lookup_existing(
    request: &PublishRequest,
    plan: &Plan,
    run_id: u64,
    dir: &Path,
) -> Result<Option<BaselineManifest>, OrchestratorError> {
    let repo = request
        .repository
        .as_deref()
        .and_then(crate::origin::validate_repository_slug)
        .ok_or_else(|| internal("publish_refused:repository_unanchored"))?;
    let branch = request
        .default_branch
        .as_deref()
        .ok_or_else(|| internal("publish_refused:unprotected_ref"))?;
    crate::retrieve_baseline::retrieve_existing_publication(
        &velnor_actions_mise::ToolCatalog::pinned(),
        dir,
        plan,
        run_id,
        &repo,
        branch,
    )
    .map(|found| found.map(|acquired| acquired.manifest().clone()))
    .map_err(|reason| internal(&format!("publish_refused:existing_baseline:{reason}")))
}

/// Original successful attempt is reusable only for the full exact current identity.
pub(super) fn qualify_existing(
    request: &PublishRequest,
    plan: &Plan,
    manifest: &BaselineManifest,
    run_id: u64,
    run_attempt: u64,
) -> Result<(), OrchestratorError> {
    let compat =
        crate::cover_compat::baseline_compat_for_plan(plan).map_err(|reason| internal(&reason))?;
    let bound = manifest.run_id == run_id
        && manifest.run_attempt <= run_attempt
        && manifest.generator_version == plan.generator.version
        && manifest.generator_sha256 == plan.generator.sha256
        && manifest.compatibility_id == compat
        && manifest.tasks.len() == plan.obligations.len()
        && plan.obligations.iter().all(|ob| {
            manifest.tasks.iter().any(|task| {
                task.task_id == ob.task_id
                    && task.task_digest == ob.task_digest
                    && task.input_digest == ob.input_digest
                    && task.closure_digest == ob.closure_digest
                    && task
                        .proof
                        .as_ref()
                        .is_some_and(|proof| ob.execution_identity.matches_proof(proof))
            })
        });
    if !bound {
        return Err(internal("publish_refused:existing_baseline_mismatch"));
    }
    super::self_check(request, manifest)
}

/// Revalidate exact protected parent evidence before forwarding executions.
pub(super) fn publication_evidence(
    request: &PublishRequest,
    plan: &Plan,
    runner_temp: &Path,
) -> Result<Option<BaselineManifest>, OrchestratorError> {
    let run_dir = runner_temp.join("velnor").join(&plan.run_key);
    if !crate::covered_tasks::plan_has_covered(plan) {
        return Ok(None);
    }
    let repo = request
        .repository
        .as_deref()
        .ok_or_else(|| internal("publish_refused:repository_unanchored"))?;
    let plan_value =
        serde_json::to_value(plan).map_err(|_| internal("publish_refused:plan_encode"))?;
    let prior = acquire_parent(&run_dir, |destination| {
        crate::retrieve_baseline::retrieve_planned_baseline_to(
            &velnor_actions_mise::ToolCatalog::pinned(),
            destination,
            &plan_value,
            repo,
        )
    })?;
    qualify_parent(request, plan, &prior)?;
    Ok(Some(prior))
}

/// Fetch into a new namespace: plan-staged parent bytes cannot supply trust.
pub(super) fn acquire_parent(
    run_dir: &Path,
    fetch: impl FnOnce(&Path) -> bool,
) -> Result<BaselineManifest, OrchestratorError> {
    let fetched =
        tempfile::tempdir_in(run_dir).map_err(|_| internal("publish_refused:parent_stage"))?;
    if !fetch(fetched.path()) {
        return Err(internal("publish_refused:parent_unavailable"));
    }
    read_evidence(&fetched.path().join(super::BASELINE_FILENAME))
}

/// Revalidation after service-bound acquisition; also the pure test seam.
pub(super) fn qualify_parent(
    request: &PublishRequest,
    plan: &Plan,
    prior: &BaselineManifest,
) -> Result<(), OrchestratorError> {
    let repo = request
        .repository
        .as_deref()
        .ok_or_else(|| internal("publish_refused:repository_unanchored"))?;
    let anchors = MergeAnchorExpectations {
        repository_slug: crate::origin::validate_repository_slug(repo),
        protected_ref: request.git_ref.clone(),
        workflow_path: Some(velnor_actions_workflow_renderer::render::WORKFLOW_PATH.to_owned()),
        ci_strict_anchors: true,
    };
    let mut signals = Signals::default();
    let mut reasons = BTreeSet::new();
    revalidate_coverage_with_anchors(
        plan,
        Some(prior),
        &mut signals,
        &mut reasons,
        &anchors,
        crate::cover_baseline::unix_now(),
    );
    if signals.planning_failed {
        return Err(internal("publish_refused:unverified_parent"));
    }
    Ok(())
}

/// Every obligation gets direct execution or a parent-bound preserved proof.
pub(super) fn published_tasks(
    plan: &Plan,
    run_id: u64,
    prior: Option<&BaselineManifest>,
) -> Result<Vec<BaselineTaskEntry>, OrchestratorError> {
    let mut tasks = Vec::new();
    for obligation in &plan.obligations {
        let task = match obligation.decision {
            ObligationDecision::Execute => BaselineTaskEntry {
                task_id: obligation.task_id.clone(),
                task_digest: obligation.task_digest.clone(),
                input_digest: obligation.input_digest.clone(),
                closure_digest: obligation.closure_digest.clone(),
                proof_run_id: run_id,
                observed_run_id: run_id,
                external_data: None,
                proof: Some(
                    ManifestTaskProof::new(
                        &obligation.task_id,
                        &obligation.task_digest,
                        &obligation.input_digest,
                        obligation.execution_identity.graph_digest(),
                        obligation.execution_identity.toolchain_id(),
                        obligation.execution_identity.mbx_digest(),
                        obligation.execution_identity.platform_id(),
                        obligation.execution_identity.profile(),
                        run_id,
                    )
                    .map_err(internal_contract)?,
                ),
                carried_from: None,
            },
            ObligationDecision::CoveredByTrustedBaseline => {
                let parent = prior.ok_or_else(|| internal("publish_refused:unverified_parent"))?;
                let mut entry = parent
                    .tasks
                    .iter()
                    .find(|task| task.task_id == obligation.task_id)
                    .cloned()
                    .ok_or_else(|| internal("publish_refused:missing_parent_task"))?;
                entry.observed_run_id = run_id;
                let digest = digest_b3(&canonical_json_bytes(parent).map_err(internal_contract)?);
                entry.carried_from = Some(
                    BaselineProof::new(
                        &parent.source_commit,
                        parent.run_id,
                        parent.artifact_id,
                        &parent.artifact_name,
                        &digest,
                    )
                    .map_err(internal_contract)?,
                );
                entry
            }
            ObligationDecision::ReusedFromTaskCache => {
                return Err(internal("publish_refused:unproven_reuse"));
            }
        };
        tasks.push(task);
    }
    tasks.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    Ok(tasks)
}
