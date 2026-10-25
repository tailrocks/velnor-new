//! Source evidence for one lane after a successful Qualification task.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::workflow::{Plan, Trust, WorkflowEvent};
use velnor_actions_contract::{ObligationDecision, PlanGenerator, canonical_json_bytes};

use self::context::{AuthenticatedProducerContext, read_authenticated_context};
use self::reports::{SuccessfulLaneReports, read_successful_lane_reports};
use self::types::{QualificationSourceEvidence, QualificationSourceEvidenceDocument};
use crate::OrchestratorError;
use crate::internal::internal;
use crate::prepare::{GenerationPreparation, prepare};
use crate::qualification_cache_runtime::closure::{
    ObservedLaneClosure, PostTaskClosureContext, recompute_post_task_closure, require_plan_match,
};

#[path = "qualification_source_evidence_context.rs"]
mod context;
#[path = "qualification_source_evidence/receipt_writer.rs"]
mod receipt_writer;
#[path = "qualification_source_evidence_reports.rs"]
mod reports;
#[path = "qualification_source_evidence_types.rs"]
mod types;

/// Fixed internal operation that writes one source evidence file.
pub const SOURCE_EVIDENCE_OP: &str = "write-qualification-source-evidence-v1";
/// Fixed operation that validates and writes one completed producer receipt.
pub const CACHE_RECEIPT_OP: &str = "write-qualification-cache-receipt-v1";
/// Fixed file name uploaded with one lane's task reports.
pub(crate) const SOURCE_EVIDENCE_FILENAME: &str = "qualification-source-evidence.json";
/// Generated step name used by Rust, `ToFu`, and named-check lanes.
pub(crate) const SOURCE_EVIDENCE_STEP_NAME: &str = "Record Qualification source evidence";

/// Write current source, closure, and report facts for one lane.
///
/// The document is incomplete until cache steps give action evidence and backend evidence.
/// This operation does not say that the workflow run finished successfully.
///
/// # Errors
///
/// If the necessary checks of the dispatch, plan, source tree, or report files give unsatisfactory results, give an error.
/// If the necessary checks of the source evidence file path give unsatisfactory results, give an error.
pub fn write_qualification_source_evidence() -> Result<(), OrchestratorError> {
    let runner_temp = required_path("RUNNER_TEMP")?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    validate_runner_directories(&runner_temp, &run_key, None)?;
    let plan = crate::task_report::load_plan(&run_key, &runner_temp)?;
    require_qualification_plan(&plan)?;

    let workspace = required_path("GITHUB_WORKSPACE")?;
    let prepared = prepare(&workspace)?;
    let authenticated = read_authenticated_context(&prepared)?;
    validate_plan_context(&plan, &prepared, &authenticated)?;
    crate::select::verify_checkout(&prepared.root, plan.event, &plan.head)?;

    let generator = crate::internal_plan::default_generator_with_phase_timings(None);
    if generator.version != plan.generator.version
        || generator.target != plan.generator.target
        || generator.sha256 != plan.generator.sha256
    {
        return Err(internal("qualification_source_generator_mismatch"));
    }
    if authenticated.run_key != run_key {
        return Err(internal("qualification_source_run_key_mismatch"));
    }
    if authenticated.runner_temp != runner_temp {
        return Err(internal("qualification_source_runner_temp_mismatch"));
    }

    let entries = job_entries(&plan, &authenticated.job_id)?;
    let mut evidence = Vec::with_capacity(entries.len());
    for entry in entries {
        validate_runner_directories(&runner_temp, &run_key, Some(&entry.matrix_key))?;
        let obligation = unique_execute_obligation(&plan, &entry.task_id)?;
        let reports = read_successful_lane_reports(&runner_temp, &plan, entry, &entry.task_digest)?;
        let observed = recompute_lane_closure(&prepared, &plan, entry)?;
        require_plan_match(&observed, entry, obligation)?;
        evidence.push((entry, observed, reports));
    }

    write_job_sidecars(&runner_temp, &plan, &authenticated, &generator, evidence)
}

/// Validate downloaded lane and owner evidence, then write one producer receipt.
///
/// # Errors
///
/// Returns an error if the staged plan, authenticated runner, reports, or cache evidence do not match.
pub fn write_qualification_cache_receipt() -> Result<PathBuf, OrchestratorError> {
    receipt_writer::write_receipt()
}

fn write_job_sidecars(
    runner_temp: &Path,
    plan: &Plan,
    authenticated: &AuthenticatedProducerContext,
    generator: &PlanGenerator,
    evidence: Vec<(
        &velnor_actions_contract::MatrixEntry,
        ObservedLaneClosure,
        SuccessfulLaneReports,
    )>,
) -> Result<(), OrchestratorError> {
    for (entry, observed, reports) in evidence {
        write_sidecar(
            runner_temp,
            plan,
            entry,
            authenticated,
            generator,
            &observed,
            reports,
        )?;
    }
    Ok(())
}

/// Make the generated helper step used only by Qualification consumers.
///
/// # Errors
///
/// Give a render error when the helper environment cannot include each necessary value.
pub(crate) fn source_evidence_step(
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<velnor_actions_contract::Step, OrchestratorError> {
    let env = crate::matrix_step::task_step_env(
        catalog,
        &std::collections::BTreeMap::from([(
            velnor_actions_workflow_renderer::steps::INTERNAL_OP_ENV.to_owned(),
            SOURCE_EVIDENCE_OP.to_owned(),
        )]),
        false,
    )?;
    velnor_actions_workflow_renderer::shell_step(
        SOURCE_EVIDENCE_STEP_NAME,
        vec![crate::matrix_step::helper_path_for_version()],
        env,
    )
    .map_err(OrchestratorError::from)
}

fn require_qualification_plan(plan: &Plan) -> Result<(), OrchestratorError> {
    if plan.event != WorkflowEvent::Qualification
        || plan.trust != Trust::Pr
        || plan.qualification.is_none()
    {
        return Err(internal("qualification_source_plan_required"));
    }
    Ok(())
}

fn validate_plan_context(
    plan: &Plan,
    prepared: &GenerationPreparation,
    authenticated: &AuthenticatedProducerContext,
) -> Result<(), OrchestratorError> {
    if plan.qualification.as_ref() != Some(&authenticated.dispatch)
        || plan.head != authenticated.dispatch.source_sha
        || plan.runner.label != prepared.runner_label
        || plan
            .qualification
            .as_ref()
            .is_none_or(|context| context.default_branch != prepared.default_branch)
    {
        return Err(internal("qualification_source_plan_context_mismatch"));
    }
    Ok(())
}

fn job_entries<'a>(
    plan: &'a Plan,
    job_id: &str,
) -> Result<Vec<&'a velnor_actions_contract::MatrixEntry>, OrchestratorError> {
    let entries: Vec<_> = plan
        .matrix
        .include
        .iter()
        .filter(|entry| entry.job_id == job_id)
        .collect();
    if entries.is_empty() {
        return Err(internal("qualification_source_job_missing"));
    }
    let artifact_id = &entries[0].artifact_id;
    let platform = &entries[0].planned_platform;
    let mut matrix_keys = std::collections::BTreeSet::new();
    let mut task_ids = std::collections::BTreeSet::new();
    for entry in &entries {
        if entry.artifact_id != *artifact_id || entry.planned_platform != *platform {
            return Err(internal("qualification_source_job_entry_mismatch"));
        }
        if !matrix_keys.insert(entry.matrix_key.as_str())
            || !task_ids.insert(entry.task_id.as_str())
        {
            return Err(internal("qualification_source_job_entry_duplicate"));
        }
    }
    Ok(entries)
}

fn unique_execute_obligation<'a>(
    plan: &'a Plan,
    task_id: &str,
) -> Result<&'a velnor_actions_contract::PlanObligation, OrchestratorError> {
    let mut obligations = plan
        .obligations
        .iter()
        .filter(|obligation| obligation.task_id == task_id);
    let obligation = obligations
        .next()
        .ok_or_else(|| internal("qualification_source_obligation_missing"))?;
    if obligations.next().is_some() || obligation.decision != ObligationDecision::Execute {
        return Err(internal("qualification_source_obligation_not_executed"));
    }
    Ok(obligation)
}

fn recompute_lane_closure(
    prepared: &GenerationPreparation,
    plan: &Plan,
    entry: &velnor_actions_contract::MatrixEntry,
) -> Result<ObservedLaneClosure, OrchestratorError> {
    recompute_post_task_closure(
        &PostTaskClosureContext {
            root: &prepared.root,
            discovery: &prepared.discovery,
            catalog: &prepared.catalog,
            generator: &plan.generator,
            run_key: &plan.run_key,
            logical_runner_label: &plan.runner.label,
        },
        &entry.matrix_key,
        entry,
    )
}

fn write_sidecar(
    runner_temp: &Path,
    plan: &Plan,
    entry: &velnor_actions_contract::MatrixEntry,
    authenticated: &AuthenticatedProducerContext,
    generator: &PlanGenerator,
    observed: &ObservedLaneClosure,
    reports: SuccessfulLaneReports,
) -> Result<(), OrchestratorError> {
    validate_runner_directories(runner_temp, &plan.run_key, Some(&entry.matrix_key))?;
    let obligation = unique_execute_obligation(plan, &entry.task_id)?;
    let document = QualificationSourceEvidenceDocument {
        schema: 1,
        producer: authenticated.producer.clone(),
        evidence: QualificationSourceEvidence {
            plan_id: plan.plan_id.clone(),
            matrix_key: entry.matrix_key.clone(),
            job_id: entry.job_id.clone(),
            task_id: entry.task_id.clone(),
            planned_task_digest: entry.task_digest.clone(),
            planned_input_digest: entry.input_digest.clone(),
            planned_closure_digest: obligation.closure_digest.clone(),
            source_sha: authenticated.dispatch.source_sha.clone(),
            generator: generator.clone(),
            closure: observed.clone(),
            task_report: reports.task,
            matrix_report: reports.matrix,
            cache_evidence_complete: false,
        },
    };
    let bytes = canonical_json_bytes(&document).map_err(crate::internal::internal_contract)?;
    let path = runner_temp
        .join("velnor")
        .join(&plan.run_key)
        .join(&entry.matrix_key)
        .join(SOURCE_EVIDENCE_FILENAME);
    crate::exclusive_write::write_private_exclusive(&path, &bytes, "qualification_source_evidence")
}

fn validate_runner_directories(
    runner_temp: &Path,
    run_key: &str,
    matrix_key: Option<&str>,
) -> Result<(), OrchestratorError> {
    if !runner_temp.is_absolute() {
        return Err(internal("qualification_source_runner_temp_not_absolute"));
    }
    let mut directories = vec![runner_temp.to_path_buf()];
    let velnor_dir = runner_temp.join("velnor");
    directories.push(velnor_dir.clone());
    let run_dir = velnor_dir.join(run_key);
    directories.push(run_dir.clone());
    if let Some(matrix_key) = matrix_key {
        let matrix_dir = run_dir.join(matrix_key);
        directories.push(matrix_dir.clone());
        directories.push(matrix_dir.join("tasks"));
    }
    for path in directories {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| internal("qualification_source_directory_missing"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(internal("qualification_source_directory_untrusted"));
        }
    }
    Ok(())
}

fn required_path(name: &str) -> Result<PathBuf, OrchestratorError> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal(&format!("qualification_source_missing_{name}")))
}

#[cfg(test)]
#[path = "qualification_source_evidence_tests.rs"]
mod tests;
