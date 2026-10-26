// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, JobDisposition, MatrixEntry, ObligationDecision, Plan,
    PlanGenerator, PlanJob, PlanMatrix, PlanObligation, PlanRunner, PlannedPlatform,
    RunnerSelection, Trust, WorkflowEvent, digest_b3, matrix_json_bytes, plan_id_for_run,
    plan_json_bytes, run_key_for_ci, task_id_for_stack,
};

use super::TestResult;

pub(super) const RUN_ID: &str = "731947";
pub(super) const RUN_ATTEMPT: &str = "1";
pub(super) const PR_EVENT: &str = r#"{"pull_request":{"head":{"repo":{"fork":false}}}}"#;

pub(super) struct Scenario {
    temp: TempDir,
    pub(super) run_key: String,
    pub(super) run_dir: PathBuf,
    pub(super) event_path: PathBuf,
    pub(super) plan: Plan,
}

impl Scenario {
    pub(super) fn new(specs: &[(&str, &str)]) -> TestResult<Self> {
        let temp = TempDir::new()?;
        let run_key = run_key_for_ci(RUN_ID.parse()?, 1);
        let plan = make_plan(specs, &run_key)?;
        let run_dir = temp.path().join("velnor").join(&run_key);
        fs::create_dir_all(&run_dir)?;
        fs::write(run_dir.join("plan.json"), plan_json_bytes(&plan)?)?;
        fs::write(
            run_dir.join("matrix.json"),
            matrix_json_bytes(&plan.matrix)?,
        )?;
        let event_path = temp.path().join("pull-request.json");
        fs::write(&event_path, PR_EVENT)?;
        Ok(Self {
            temp,
            run_key,
            run_dir,
            event_path,
            plan,
        })
    }

    pub(super) fn root(&self) -> &Path {
        self.temp.path()
    }

    pub(super) fn task_digest(&self, task_id: &str) -> TestResult<&str> {
        self.plan
            .obligations
            .iter()
            .find(|obligation| obligation.task_id == task_id)
            .map(|obligation| obligation.task_digest.as_str())
            .ok_or_else(|| std::io::Error::other("task obligation missing").into())
    }
}

pub(super) fn task_id(kind: &str) -> TestResult<String> {
    Ok(task_id_for_stack("rust", "demo", kind, "default", None)?)
}

fn make_plan(specs: &[(&str, &str)], run_key: &str) -> TestResult<Plan> {
    let platform = PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")?;
    let mut entries = Vec::with_capacity(specs.len());
    let mut obligations = Vec::with_capacity(specs.len());
    let mut task_ids = Vec::with_capacity(specs.len());
    let mut job_ids = BTreeSet::new();
    for (kind, job_id) in specs {
        let (entry, obligation) = entry_and_obligation(kind, job_id, run_key, &platform)?;
        entries.push(entry);
        task_ids.push(obligation.task_id.clone());
        obligations.push(obligation);
        job_ids.insert(*job_id);
    }
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    obligations.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    task_ids.sort();
    let jobs = job_ids
        .into_iter()
        .map(|job_id| PlanJob {
            job_id: job_id.to_owned(),
            disposition: JobDisposition::Run,
        })
        .collect();
    let plan = Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.to_owned(),
        plan_id: plan_id_for_run(run_key)?,
        base: None,
        head: "a".repeat(40),
        event: WorkflowEvent::PullRequest,
        qualification: None,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        generator: PlanGenerator {
            version: "0.1.55".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations,
        matrix: PlanMatrix { include: entries },
        task_ids,
        jobs,
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    plan.validate()?;
    Ok(plan)
}

fn entry_and_obligation(
    kind: &str,
    job_id: &str,
    run_key: &str,
    platform: &PlannedPlatform,
) -> TestResult<(MatrixEntry, PlanObligation)> {
    let task_id = task_id(kind)?;
    let task_digest = digest_b3(task_id.as_bytes());
    let input_digest = digest_b3(format!("input:{task_id}").as_bytes());
    let closure_digest = digest_b3(format!("closure:{task_id}").as_bytes());
    let entry = MatrixEntry::derive(
        "rust",
        &task_id,
        "true",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(kind.to_owned(), ExecuteTaskRef::Single(task_id.clone()))]),
        },
        &input_digest,
        run_key,
        job_id,
        platform.clone(),
    )?;
    let obligation = PlanObligation {
        task_id,
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest,
        input_digest,
        closure_digest,
        job_id: job_id.to_owned(),
    };
    Ok((entry, obligation))
}
