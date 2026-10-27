// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::crate_job::{reader_job_id_for, writer_job_id_for};
use velnor_actions_contract::workflow::jobs::validate_job_id;
use velnor_actions_contract::{ContractError, Job, ProposedTask, Stack, Step, StepKind, StepRole};
use velnor_actions_rust::tool_needs;

#[path = "crate_jobs_recheck.rs"]
mod recheck;
pub(crate) use recheck::{
    WRITER_RECHECK_STEP_NAME, is_recoverable_probe, rewrite_recheck_step_keys, writer_recheck_step,
};

pub(crate) fn needs(task: &ProposedTask) -> velnor_actions_rust::ToolNeeds {
    if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
        return velnor_actions_rust::ToolNeeds {
            mbx: false,
            nextest: false,
        };
    }
    tool_needs(&task.identity.compile_driver, &task.identity.test_runner)
}

pub(crate) fn is_rust(task: &ProposedTask) -> bool {
    !is_opentofu(task)
}

pub(crate) fn is_mbx(task: &ProposedTask) -> bool {
    needs(task).mbx
}

pub(crate) fn is_nextest(task: &ProposedTask) -> bool {
    needs(task).nextest
}

pub(crate) fn is_opentofu(task: &ProposedTask) -> bool {
    Stack::from_id(&task.stack_id) == Some(Stack::Tofu)
}

pub(crate) const STAGED_TOFU_JOB_CONDITION: &str = "always()";

pub(crate) fn stage_tofu_root_jobs(
    jobs: &mut [(String, Job)],
    tofu_branches: &[CombinedBranch],
    max_parallel_jobs: u32,
) {
    let width = usize::try_from(max_parallel_jobs.max(1)).unwrap_or(1);
    let mut readers: Vec<&str> = tofu_branches
        .iter()
        .map(|branch| branch.reader.as_str())
        .collect();
    let mut writers: Vec<&str> = tofu_branches
        .iter()
        .map(|branch| branch.writer.as_str())
        .collect();
    readers.sort_unstable();
    readers.dedup();
    writers.sort_unstable();
    writers.dedup();
    chain_lane(jobs, &readers, width);
    chain_lane(jobs, &writers, width);
}

fn chain_lane(jobs: &mut [(String, Job)], lane: &[&str], width: usize) {
    let width = width.max(1);
    let mut predecessors: BTreeMap<&str, &str> = BTreeMap::new();
    for (index, id) in lane.iter().enumerate() {
        if index >= width {
            predecessors.insert(id, lane[index - width]);
        }
    }
    if predecessors.is_empty() {
        return;
    }
    for (id, job) in jobs.iter_mut() {
        if let Some(previous) = predecessors.get(id.as_str()) {
            job.needs.push((*previous).to_owned());
            job.needs.sort();
            job.needs.dedup();
            job.condition = Some(chain_condition(job.condition.as_deref()));
        }
    }
}

fn chain_condition(existing: Option<&str>) -> String {
    match existing {
        Some(prior) => format!("{STAGED_TOFU_JOB_CONDITION} && ({prior})"),
        None => STAGED_TOFU_JOB_CONDITION.to_owned(),
    }
}

pub(crate) const BRANCHES_PER_GROUP: usize = 2;

pub(crate) const TRUSTED_WRITER_EVENT_GATE: &str = "github.event_name == 'push'";

pub(crate) const UNTRUSTED_READER_EVENT_GATE: &str = "github.event_name != 'push'";

const SUPPRESSED_SAVE_CONDITION: &str = "false";

const GROUP_MARKER_OPEN: char = ',';

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CombinedBranch {
    pub(crate) group: String,
    pub(crate) reader: String,
    pub(crate) writer: String,
}

pub(crate) fn combined_branch_for(group_job_id: &str) -> Result<CombinedBranch, ContractError> {
    validate_job_id(group_job_id)?;
    let reader = reader_job_id_for(group_job_id);
    let writer = writer_job_id_for(group_job_id);
    validate_job_id(&reader)?;
    validate_job_id(&writer)?;
    if reader == writer {
        return Err(ContractError::identity(
            "crate_group.branches",
            "branch_collision",
        ));
    }
    Ok(CombinedBranch {
        group: group_job_id.to_owned(),
        reader,
        writer,
    })
}

pub(crate) fn branch_job_count(group_count: usize) -> usize {
    group_count.saturating_mul(BRANCHES_PER_GROUP)
}

fn require_gate_text(gate: &str) -> Result<(), ContractError> {
    if gate.trim().is_empty() || gate.bytes().any(|byte| matches!(byte, b'\n' | b'\r')) {
        return Err(ContractError::identity(
            "crate_group.branch_condition",
            "bad_branch_gate",
        ));
    }
    Ok(())
}

fn group_marker(branch: &CombinedBranch) -> String {
    format!("{GROUP_MARKER_OPEN}{}{GROUP_MARKER_OPEN}", branch.group)
}

pub(crate) fn reader_branch_condition(
    affected_gate: Option<&str>,
) -> Result<String, ContractError> {
    match affected_gate {
        Some(gate) => {
            require_gate_text(gate)?;
            Ok(format!("({gate}) && {UNTRUSTED_READER_EVENT_GATE}"))
        }
        None => Ok(UNTRUSTED_READER_EVENT_GATE.to_owned()),
    }
}

pub(crate) fn writer_branch_condition(
    affected_gate: Option<&str>,
) -> Result<String, ContractError> {
    match affected_gate {
        Some(gate) => {
            require_gate_text(gate)?;
            Ok(format!("({gate}) && {TRUSTED_WRITER_EVENT_GATE}"))
        }
        None => Ok(TRUSTED_WRITER_EVENT_GATE.to_owned()),
    }
}

pub(crate) fn writer_branch_needs(
    external_needs: &[String],
    reader_id: &str,
) -> Result<Vec<String>, ContractError> {
    validate_job_id(reader_id)?;
    let mut needs = BTreeSet::new();
    for need in external_needs {
        validate_job_id(need)?;
        needs.insert(need.clone());
    }
    needs.insert(reader_id.to_owned());
    Ok(needs.into_iter().collect())
}

fn is_save_role(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::ToolsCacheSave
                | StepRole::CargoSourcesSave
                | StepRole::MbxBundleSave
                | StepRole::FamilySave
                | StepRole::TofuProvidersSave
        )
    )
}

fn executes(step: &Step) -> bool {
    matches!(step.kind, StepKind::TaskExecution { .. })
        || step.role == Some(StepRole::TofuProviderUse)
}

pub(crate) struct BranchJobs<'a> {
    pub(crate) reader_job: &'a str,
    pub(crate) reader: &'a Job,
    pub(crate) writer_job: &'a str,
    pub(crate) writer: &'a Job,
}

fn validate_branch_identity(
    branch: &CombinedBranch,
    jobs: &BranchJobs<'_>,
) -> Result<(), ContractError> {
    if jobs.reader_job != branch.reader || jobs.writer_job != branch.writer {
        return Err(ContractError::identity(
            "crate_group.branches",
            "branch_identity_mismatch",
        ));
    }
    Ok(())
}

fn validate_branch_modes(jobs: &BranchJobs<'_>) -> Result<(), ContractError> {
    if jobs
        .reader
        .cache_mode
        .as_ref()
        .is_none_or(|mode| !mode.is_explicit_read())
    {
        return Err(ContractError::identity(
            "job.cache_mode",
            format!("reader_requires_explicit_read:{}", jobs.reader_job),
        ));
    }
    if jobs
        .writer
        .cache_mode
        .as_ref()
        .is_none_or(|mode| !mode.grants_write())
    {
        return Err(ContractError::identity(
            "job.cache_mode",
            format!("trusted_writer_requires_cache_write:{}", jobs.writer_job),
        ));
    }
    Ok(())
}

fn branch_condition<'a>(job: &'a Job, job_id: &str, role: &str) -> Result<&'a str, ContractError> {
    job.condition.as_deref().ok_or_else(|| {
        ContractError::identity(
            "crate_group.branches",
            format!("{role}_requires_branch_condition:{job_id}"),
        )
    })
}

fn validate_branch_conditions(
    branch: &CombinedBranch,
    affected_gate: Option<&str>,
    jobs: &BranchJobs<'_>,
) -> Result<(), ContractError> {
    if !jobs.writer.needs.iter().any(|need| need == jobs.reader_job) {
        return Err(ContractError::identity(
            "crate_group.branches",
            format!("writer_requires_reader_need:{}", jobs.writer_job),
        ));
    }
    let writer_condition = branch_condition(jobs.writer, jobs.writer_job, "writer")?;
    if !writer_condition.contains(TRUSTED_WRITER_EVENT_GATE) {
        return Err(ContractError::identity(
            "crate_group.branches",
            format!("writer_requires_trusted_event:{}", jobs.writer_job),
        ));
    }
    let reader_condition = branch_condition(jobs.reader, jobs.reader_job, "reader")?;
    if !reader_condition.contains(UNTRUSTED_READER_EVENT_GATE) {
        return Err(ContractError::identity(
            "crate_group.branches",
            format!("reader_requires_untrusted_event:{}", jobs.reader_job),
        ));
    }
    if affected_gate.is_some() {
        let marker = group_marker(branch);
        if !reader_condition.contains(&marker) || !writer_condition.contains(&marker) {
            return Err(ContractError::identity(
                "crate_group.branches",
                format!("branch_requires_group_gate:{}", branch.group),
            ));
        }
    }
    Ok(())
}

fn validate_branch_execution(
    base_had_probe: bool,
    jobs: &BranchJobs<'_>,
) -> Result<(), ContractError> {
    if !jobs.reader.steps.iter().any(executes) {
        return Err(ContractError::identity(
            "crate_group.branches",
            format!("reader_requires_execution:{}", jobs.reader_job),
        ));
    }
    let Some(first_execution) = jobs.writer.steps.iter().position(executes) else {
        return Err(ContractError::identity(
            "crate_group.branches",
            format!("writer_requires_execution:{}", jobs.writer_job),
        ));
    };
    if !base_had_probe {
        return Ok(());
    }
    let Some(recheck) = jobs.writer.steps.first() else {
        return Err(ContractError::identity(
            "crate_group.branches",
            format!("writer_requires_recheck:{}", jobs.writer_job),
        ));
    };
    if recheck.name != WRITER_RECHECK_STEP_NAME || !is_recoverable_probe(recheck) {
        return Err(ContractError::identity(
            "crate_group.branches",
            format!("writer_requires_recheck:{}", jobs.writer_job),
        ));
    }
    if first_execution == 0 {
        return Err(ContractError::identity(
            "crate_group.branches",
            format!("recheck_requires_queue_slot:{}", jobs.writer_job),
        ));
    }
    Ok(())
}

fn validate_writer_reports_failure(jobs: &BranchJobs<'_>) -> Result<(), ContractError> {
    for step in &jobs.writer.steps {
        if is_save_role(step.role) && step.condition.as_deref() == Some(SUPPRESSED_SAVE_CONDITION) {
            return Err(ContractError::identity(
                "job.cache_mode",
                format!("writer_suppresses_hard_failure:{}", jobs.writer_job),
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_combined_branch(
    branch: &CombinedBranch,
    affected_gate: Option<&str>,
    base_had_probe: bool,
    jobs: &BranchJobs<'_>,
) -> Result<(), ContractError> {
    validate_branch_identity(branch, jobs)?;
    validate_branch_modes(jobs)?;
    validate_branch_conditions(branch, affected_gate, jobs)?;
    validate_branch_execution(base_had_probe, jobs)?;
    validate_writer_reports_failure(jobs)
}
