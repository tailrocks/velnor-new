//! Final-gate `needs` channel model (P01-3/4).
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::workflow::{
    ir::Job,
    job_output::{JobOutput, JobOutputName},
    step_identity::{StepId, StepRole},
};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_config::config::ExecutionMode;

/// Env key carrying the finalized `needs` conclusions payload.
pub const NEEDS_CHANNEL_ENV: &str = "VELNOR_NEEDS_JSON";

/// GitHub expression producing the `needs` conclusions payload.
pub const NEEDS_CHANNEL_EXPRESSION: &str = "${{ toJSON(needs) }}";

/// Env key carrying the rendered expected validator inventory.
///
/// Static JSON array of the finalized gate's `needs`, emitted beside
/// the runtime conclusions so the merge binds the required inventory
/// to the committed workflow instead of deriving it from whatever the
/// run happened to observe (a dropped validator would otherwise shrink
/// both sides of the check and pass silently).
pub const NEEDS_EXPECTED_ENV: &str = "VELNOR_NEEDS_EXPECTED";

/// Env key carrying the finalized direct report-producer job inventory.
pub const TASK_REPORT_PRODUCERS_EXPECTED_ENV: &str = "VELNOR_TASK_REPORT_PRODUCERS_EXPECTED";

/// Finalized direct report producers for an explicitly routed Both workflow.
///
/// This is graph metadata, not evidence that a provider run or artifact
/// comparison is complete. It is derived from the Required gate's direct
/// needs after lane retargeting and requires each producer's typed upload
/// role and both canonical outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskReportProducerInventory {
    /// Required-gate job ID.
    pub required_job: String,
    /// Sorted direct job IDs that expose report artifact and check-run IDs.
    pub workflow_job_keys: Vec<String>,
}

impl TaskReportProducerInventory {
    /// Derive paired report producers from the finalized direct-needs graph.
    ///
    /// Schema 1 and hosted/scale-set routes return no inventory. The caller
    /// supplies the effective typed mode produced by schema-2 expansion.
    /// # Errors
    pub fn from_finalized_jobs(
        mode: Option<ExecutionMode>,
        required_job: &str,
        jobs: &BTreeMap<String, Job>,
    ) -> Result<Option<Self>, ContractError> {
        if mode != Some(ExecutionMode::Both) {
            return Ok(None);
        }
        let gate = jobs.get(required_job).ok_or_else(|| {
            ContractError::identity(
                "task_report_producers",
                format!("missing_gate:{required_job}"),
            )
        })?;
        let direct = unique_direct_needs(required_job, &gate.needs, jobs)?;
        let mut workflow_job_keys = Vec::new();
        for job_id in &direct {
            let job = &jobs[job_id];
            if has_report_producer_marker(job) {
                validate_report_producer(job_id, job)?;
                workflow_job_keys.push(job_id.clone());
            }
        }
        reject_hidden_report_producers(&direct, jobs)?;
        if workflow_job_keys.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            required_job: required_job.to_owned(),
            workflow_job_keys,
        }))
    }

    /// Static JSON array for the finalized Required gate.
    ///
    /// # Errors
    pub fn expected_env(&self) -> Result<(String, String), ContractError> {
        let value = serde_json::to_string(&self.workflow_job_keys).map_err(|error| {
            ContractError::identity("task_report_producers.expected", error.to_string())
        })?;
        Ok((TASK_REPORT_PRODUCERS_EXPECTED_ENV.to_owned(), value))
    }
}

fn unique_direct_needs(
    required_job: &str,
    needs: &[String],
    jobs: &BTreeMap<String, Job>,
) -> Result<BTreeSet<String>, ContractError> {
    let direct: BTreeSet<String> = needs.iter().cloned().collect();
    if direct.len() != needs.len() {
        return Err(producer_error(required_job, "duplicate_direct_need"));
    }
    for job_id in &direct {
        if !jobs.contains_key(job_id) {
            return Err(producer_error(
                required_job,
                &format!("missing_direct_need:{job_id}"),
            ));
        }
    }
    Ok(direct)
}

fn has_report_producer_marker(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        step.role == Some(StepRole::CrateReportUpload) || step.id == Some(StepId::CrateReportUpload)
    }) || job.outputs.iter().any(|output| {
        matches!(
            output.name,
            JobOutputName::TaskReportArtifactId | JobOutputName::TaskReportCheckRunId
        )
    })
}

fn validate_report_producer(job_id: &str, job: &Job) -> Result<(), ContractError> {
    let upload_steps: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::CrateReportUpload))
        .collect();
    let upload_ids: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.id == Some(StepId::CrateReportUpload))
        .collect();
    let artifact_outputs: Vec<_> = job
        .outputs
        .iter()
        .filter(|output| output.name == JobOutputName::TaskReportArtifactId)
        .collect();
    let check_run_outputs: Vec<_> = job
        .outputs
        .iter()
        .filter(|output| output.name == JobOutputName::TaskReportCheckRunId)
        .collect();
    let exact_upload = upload_steps.len() == 1
        && upload_ids.len() == 1
        && upload_steps[0].id == Some(StepId::CrateReportUpload);
    let exact_outputs = artifact_outputs.len() == 1
        && check_run_outputs.len() == 1
        && artifact_outputs[0] == &JobOutput::task_report_artifact_id()
        && check_run_outputs[0] == &JobOutput::task_report_check_run_id();
    if exact_upload && exact_outputs {
        Ok(())
    } else {
        Err(producer_error(job_id, "incomplete_report_output_pair"))
    }
}

fn reject_hidden_report_producers(
    direct: &BTreeSet<String>,
    jobs: &BTreeMap<String, Job>,
) -> Result<(), ContractError> {
    for (job_id, job) in jobs {
        if has_report_producer_marker(job) && !direct.contains(job_id) {
            return Err(producer_error(job_id, "report_producer_not_direct_need"));
        }
    }
    Ok(())
}

fn producer_error(job_id: &str, reason: &str) -> ContractError {
    ContractError::identity("task_report_producers", format!("{reason}:{job_id}"))
}

/// Producer model of the final-gate `needs` channel (P01-3/4).
///
/// The required validator inventory derives from the gate job's own
/// `needs` list — the exact set `toJSON(needs)` can observe at runtime.
/// Deriving from the finalized job set instead (every job except the
/// gate) admitted downstream jobs the gate cannot need without a cycle,
/// so the merge saw `needs_inventory_mismatch` on every run; the gate
/// list is the single source of truth. The renderer emits
/// [`NEEDS_CHANNEL_ENV`] with [`NEEDS_CHANNEL_EXPRESSION`] so merge
/// conclusions match the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeedsConclusions {
    /// Required-gate job ID.
    pub required_job: String,
    /// Sorted required validator IDs (the gate's `needs`).
    pub inventory: Vec<String>,
}

impl NeedsConclusions {
    /// Derive the inventory from the gate job's `needs` list.
    ///
    /// Downstream jobs (needing the gate) and unrelated jobs are
    /// excluded: only the gate's `needs` are observable at runtime.
    /// # Errors
    pub fn from_finalized_jobs(
        required_job: &str,
        jobs: &BTreeMap<String, Job>,
    ) -> Result<Self, ContractError> {
        let Some(gate) = jobs.get(required_job) else {
            return Err(ContractError::identity(
                "needs.inventory",
                format!("missing_gate:{required_job}"),
            ));
        };
        let mut inventory = gate.needs.clone();
        inventory.sort();
        if inventory.is_empty() {
            return Err(ContractError::identity(
                "needs.inventory",
                "empty_inventory",
            ));
        }
        Ok(Self {
            required_job: required_job.to_owned(),
            inventory,
        })
    }

    /// Channel binding the renderer emits on the merge step.
    #[must_use]
    pub fn channel_env(&self) -> (String, String) {
        (
            NEEDS_CHANNEL_ENV.to_owned(),
            NEEDS_CHANNEL_EXPRESSION.to_owned(),
        )
    }

    /// Expected-inventory binding the renderer emits on the merge step.
    ///
    /// Static JSON array of the finalized inventory (sorted by
    /// construction from the ordered job map), so the merge compares
    /// observed conclusions against the committed workflow.
    #[must_use]
    pub fn expected_env(&self) -> (String, String) {
        let mut inventory = self.inventory.clone();
        inventory.sort();
        (
            NEEDS_EXPECTED_ENV.to_owned(),
            serde_json::to_string(&inventory).unwrap_or_else(|_| "[]".to_owned()),
        )
    }

    /// True when the finalized gate needs exactly this inventory.
    #[must_use]
    pub fn gate_matches(&self, jobs: &BTreeMap<String, Job>) -> bool {
        jobs.get(&self.required_job).is_some_and(|job| {
            let mut needs = job.needs.clone();
            needs.sort();
            needs == self.inventory
        })
    }
}

#[cfg(test)]
mod tests;
