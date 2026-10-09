use std::collections::BTreeSet;

use velnor_actions_contract::{
    canonical::digest_b3,
    ids::{plan_id_for_run, task_report_id_for_task},
};
use velnor_actions_contract_workflow::{
    ArtifactBuildRunContext, ExecuteTaskIds, MatrixEntry, NamedCheckLaneVariant,
    ObligationDecision, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage,
    PlanRunner, TaskRuntimeIdentity, TaskRuntimeReceipt, Trust, WorkflowEvent,
    canonical_plan_digest,
};
use velnor_actions_orchestrator_merge_ports::{
    ActionsAttemptArtifactView, ActionsAttemptJobView, CompleteActionsAttemptView,
    ScopedCompareRequest, TaskReportOutputFanIn,
};

pub(super) const REPOSITORY: &str = "ChainArgos/java-monorepo";
pub(super) const RUN_ID: i64 = 424_242;
pub(super) const ATTEMPT: u32 = 2;
pub(super) const REPOSITORY_ID: i64 = 12_345;

#[derive(Clone)]
pub(super) struct Job {
    pub(super) id: i64,
    pub(super) check_run_id: Option<i64>,
    pub(super) run_id: i64,
    pub(super) head_sha: String,
    pub(super) status: String,
    pub(super) conclusion: Option<String>,
}

impl ActionsAttemptJobView for Job {
    fn actions_job_id(&self) -> i64 {
        self.id
    }
    fn check_run_id(&self) -> Option<i64> {
        self.check_run_id
    }
    fn workflow_run_id(&self) -> i64 {
        self.run_id
    }
    fn head_sha(&self) -> &str {
        &self.head_sha
    }
    fn status(&self) -> &str {
        &self.status
    }
    fn conclusion(&self) -> Option<&str> {
        self.conclusion.as_deref()
    }
}

#[derive(Clone)]
pub(super) struct Artifact {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) expired: bool,
    pub(super) run_id: i64,
    pub(super) repository_id: i64,
    pub(super) head_sha: String,
}

impl ActionsAttemptArtifactView for Artifact {
    fn artifact_id(&self) -> i64 {
        self.id
    }
    fn artifact_name(&self) -> &str {
        &self.name
    }
    fn expired(&self) -> bool {
        self.expired
    }
    fn workflow_run_id(&self) -> i64 {
        self.run_id
    }
    fn repository_id(&self) -> i64 {
        self.repository_id
    }
    fn head_sha(&self) -> &str {
        &self.head_sha
    }
}

#[derive(Clone)]
pub(super) struct Provider {
    pub(super) repository_id: i64,
    pub(super) repository: String,
    pub(super) run_id: i64,
    pub(super) attempt: u32,
    pub(super) head_sha: String,
    pub(super) run_status: String,
    pub(super) run_conclusion: Option<String>,
    pub(super) jobs: Vec<Job>,
    pub(super) artifacts: Vec<Artifact>,
}

impl CompleteActionsAttemptView for Provider {
    type Job = Job;
    type Artifact = Artifact;

    fn repository_id(&self) -> i64 {
        self.repository_id
    }
    fn repository_full_name(&self) -> &str {
        &self.repository
    }
    fn workflow_run_id(&self) -> i64 {
        self.run_id
    }
    fn attempt(&self) -> u32 {
        self.attempt
    }
    fn head_sha(&self) -> &str {
        &self.head_sha
    }
    fn run_status(&self) -> &str {
        &self.run_status
    }
    fn run_conclusion(&self) -> Option<&str> {
        self.run_conclusion.as_deref()
    }
    fn jobs(&self) -> &[Self::Job] {
        &self.jobs
    }
    fn artifacts(&self) -> &[Self::Artifact] {
        &self.artifacts
    }
}

pub(super) struct Fixture {
    pub(super) plan: Plan,
    pub(super) receipts: Vec<TaskRuntimeReceipt>,
    pub(super) outputs: TaskReportOutputFanIn,
    pub(super) provider: Provider,
}

pub(super) fn fixture() -> Fixture {
    let plan = test_plan();
    let head = plan.head.clone();
    let plan_digest = canonical_plan_digest(&plan).expect("plan digest");
    Fixture {
        receipts: receipts_for(&plan, &head),
        outputs: outputs_for(&plan, &head, &plan_digest),
        provider: provider_for(&plan, &head),
        plan,
    }
}

fn test_plan() -> Plan {
    let run_key = format!("r{RUN_ID}-a{ATTEMPT}");
    let task_id = "stack/mise/demo/check/default";
    let task_digest = digest_b3(b"task");
    let mut entries = [
        lane_entry(
            &run_key,
            task_id,
            &task_digest,
            "check-demo__hosted",
            NamedCheckLaneVariant::Hosted,
        ),
        lane_entry(
            &run_key,
            task_id,
            &task_digest,
            "check-demo__local",
            NamedCheckLaneVariant::ScaleSet,
        ),
    ];
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    let head = "ab".repeat(20);
    let plan = Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key).expect("plan ID"),
        base: None,
        head: head.clone(),
        event: WorkflowEvent::PullRequest,
        runner: serde_json::from_value::<PlanRunner>(serde_json::json!({
            "label": "ubuntu-26.04",
            "selection": "latest_default"
        }))
        .expect("runner"),
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("no_entry")).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.1".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "cd".repeat(32),
        },
        packages: vec![PlanPackage {
            package_id: "demo 0.1.0".to_owned(),
            name: "demo".to_owned(),
            manifest: "Cargo.toml".to_owned(),
            selected: true,
            reasons: vec!["changed".to_owned()],
            tasks: vec![task_id.to_owned()],
        }],
        obligations: vec![PlanObligation {
            task_id: task_id.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "changed".to_owned(),
            task_digest: task_digest.clone(),
            input_digest: digest_b3(b"inputs"),
            closure_digest: digest_b3(b"closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: entries.to_vec(),
        },
        task_ids: vec![task_id.to_owned()],
        artifact_tasks: vec![],
        warnings: vec![],
        edges: vec![],
    };
    plan.validate().expect("valid fixture plan");
    plan
}

pub(super) fn receipts_for(plan: &Plan, head: &str) -> Vec<TaskRuntimeReceipt> {
    plan.matrix
        .include
        .iter()
        .map(|entry| {
            let identity = TaskRuntimeIdentity::new(
                REPOSITORY.to_owned(),
                RUN_ID.to_string(),
                ATTEMPT,
                head.to_owned(),
                format!("{REPOSITORY}/.github/workflows/ci.yml@refs/heads/main"),
                entry.job_id.clone(),
                "runner-label-is-not-used".to_owned(),
            )
            .expect("runtime identity");
            let report_id =
                task_report_id_for_task(&plan.run_key, &entry.matrix_key, &entry.task_digest)
                    .expect("report ID");
            TaskRuntimeReceipt::derive(plan, entry, &report_id, &identity).expect("receipt")
        })
        .collect::<Vec<_>>()
}

pub(super) fn outputs_for(plan: &Plan, head: &str, plan_digest: &str) -> TaskReportOutputFanIn {
    let keys = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.job_id.as_str())
        .collect::<BTreeSet<_>>();
    let producer_rows = keys
        .iter()
        .map(|job_id| {
            let entry = plan
                .matrix
                .include
                .iter()
                .find(|entry| entry.job_id == **job_id)
                .expect("plan lane for workflow job key");
            let (artifact_id, check_run_id) = ids_for_lane(entry.lane_variant.expect("typed lane"));
            serde_json::json!({
                "workflow_job_key": entry.job_id,
                "conclusion": "success",
                "artifact_id": artifact_id,
                "check_run_id": check_run_id,
            })
        })
        .collect::<Vec<_>>();
    TaskReportOutputFanIn::parse_value(serde_json::json!({
        "schema": 1,
        "origin": "github_com",
        "run": ArtifactBuildRunContext {
            repository_id: REPOSITORY_ID.to_string(),
            repository: REPOSITORY.to_owned(),
            run_id: RUN_ID.to_string(),
            run_attempt: ATTEMPT,
        },
        "head_sha": head,
        "plan_digest": plan_digest,
        "expected_workflow_job_keys": keys.iter().map(|key| (*key).to_owned()).collect::<Vec<_>>(),
        "producers": producer_rows,
    }))
    .expect("producer output sidecar")
}

pub(super) fn provider_for(plan: &Plan, head: &str) -> Provider {
    let keys = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.job_id.as_str())
        .collect::<BTreeSet<_>>();
    let jobs = keys
        .iter()
        .map(|job_id| {
            let entry = plan
                .matrix
                .include
                .iter()
                .find(|entry| entry.job_id == **job_id)
                .expect("plan lane for workflow job key");
            let (_, check_run_id) = ids_for_lane(entry.lane_variant.expect("typed lane"));
            let action_id = check_run_id + 10_000;
            Job {
                id: action_id,
                check_run_id: Some(check_run_id),
                run_id: RUN_ID,
                head_sha: head.to_owned(),
                status: "completed".to_owned(),
                conclusion: Some("success".to_owned()),
            }
        })
        .collect();
    let artifacts = keys
        .iter()
        .map(|job_id| {
            let entry = plan
                .matrix
                .include
                .iter()
                .find(|entry| entry.job_id == **job_id)
                .expect("plan lane for workflow job key");
            let (artifact_id, _) = ids_for_lane(entry.lane_variant.expect("typed lane"));
            Artifact {
                id: artifact_id,
                name: entry.artifact_id.clone(),
                expired: false,
                run_id: RUN_ID,
                repository_id: REPOSITORY_ID,
                head_sha: head.to_owned(),
            }
        })
        .collect();
    Provider {
        repository_id: REPOSITORY_ID,
        repository: REPOSITORY.to_owned(),
        run_id: RUN_ID,
        attempt: ATTEMPT,
        head_sha: head.to_owned(),
        run_status: "completed".to_owned(),
        run_conclusion: Some("success".to_owned()),
        jobs,
        artifacts,
    }
}

pub(super) fn lane_entry(
    run_key: &str,
    task_id: &str,
    task_digest: &str,
    job_id: &str,
    variant: NamedCheckLaneVariant,
) -> MatrixEntry {
    MatrixEntry::derive_for_lane(
        "mise",
        task_id,
        "mise check:all",
        task_digest,
        serde_json::json!({"check_id":"demo"}),
        ExecuteTaskIds::default(),
        &digest_b3(b"entry"),
        run_key,
        job_id,
        Some(variant),
    )
    .expect("lane entry")
}

fn ids_for_lane(variant: NamedCheckLaneVariant) -> (i64, i64) {
    match variant {
        NamedCheckLaneVariant::Hosted => (501, 601),
        NamedCheckLaneVariant::ScaleSet => (502, 602),
    }
}

pub(super) fn request(repository: &str) -> ScopedCompareRequest<'_> {
    ScopedCompareRequest {
        repository,
        run_id: RUN_ID,
        attempt: ATTEMPT,
    }
}
