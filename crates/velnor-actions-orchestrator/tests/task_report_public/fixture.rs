// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use velnor_actions_contract::TOFU_JOB_ID_PREFIX;
use velnor_actions_contract::{
    EdgeKind, ExecuteTaskIds, ExecuteTaskRef, JobDisposition, MatrixEntry, ObligationDecision,
    Plan, PlanGenerator, PlanJob, PlanMatrix, PlanObligation, PlanRunner, PlannedPlatform,
    ProposedTask, RunnerSelection, TaskEdge, Trust, WorkflowEvent, assign_crate_job_ids, digest_b3,
    matrix_json_bytes, plan_id_for_run, plan_json_bytes, run_key_for_ci, task_id_for_stack,
};
use velnor_actions_mise::GH_VERSION;
use velnor_actions_tofu::{TofuTaskGroup, TofuTaskKind, entry_metadata_for_task, propose_task};

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

    pub(super) fn tofu_provider() -> TestResult<Self> {
        let temp = TempDir::new()?;
        let run_key = run_key_for_ci(RUN_ID.parse()?, 1);
        let root = temp.path().join("infra");
        fs::create_dir_all(&root)?;
        fs::write(
            root.join(".terraform.lock.hcl"),
            "provider \"registry.opentofu.org/hashicorp/random\" {\n  version = \"3.6.0\"\n  hashes = [\"h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"]\n}\n",
        )?;
        let plan = make_tofu_plan(&run_key)?;
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

    pub(super) fn task_digest(&self, task_id: &str) -> TestResult<&str> {
        self.plan
            .obligations
            .iter()
            .find(|obligation| obligation.task_id == task_id)
            .map(|obligation| obligation.task_digest.as_str())
            .ok_or_else(|| std::io::Error::other("task obligation missing").into())
    }

    pub(super) fn stage_generator_wire_field(&self, field: &str) -> TestResult<()> {
        let path = self.run_dir.join("plan.json");
        let mut plan: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
        if plan["schema"] != 2 {
            return Err(io::Error::other("schema-2 plan required").into());
        }
        let source = &plan["generator"];
        let mut generator = serde_json::Map::new();
        for name in ["version", "target"] {
            generator.insert(name.to_owned(), source[name].clone());
        }
        generator.insert(field.to_owned(), "a".repeat(64).into());
        plan["generator"] = generator.into();
        fs::write(path, serde_json::to_vec(&plan)?)?;
        Ok(())
    }
}

#[cfg(unix)]
pub(super) fn install_fake_gh(scenario: &Scenario) -> TestResult<PathBuf> {
    let mise_data_dir = scenario.root().join("mise-data");
    let executable = mise_data_dir
        .join("installs")
        .join("gh")
        .join(GH_VERSION)
        .join("bin")
        .join("gh");
    fs::create_dir_all(
        executable
            .parent()
            .ok_or_else(|| io::Error::other("fake gh executable parent missing"))?,
    )?;
    fs::write(
        &executable,
        "#!/bin/sh\n\
set -eu\n\
[ \"$#\" -eq 9 ] || exit 64\n\
[ \"$1\" = run ] || exit 65\n\
[ \"$2\" = download ] || exit 66\n\
[ \"$3\" = \"$GITHUB_RUN_ID\" ] || exit 67\n\
[ \"$4\" = --name ] || exit 68\n\
[ \"$6\" = --dir ] || exit 69\n\
[ \"$8\" = --repo ] || exit 70\n\
[ \"$9\" = \"$GITHUB_REPOSITORY\" ] || exit 71\n\
printf '%s\\t%s\\t%s\\n' \"$3\" \"$5\" \"$9\" >> \"$VELNOR_TEST_GH_LOG\"\n\
for source in \"$VELNOR_TEST_ARTIFACT_PAYLOAD\"/*; do\n\
    [ -d \"$source\" ] || continue\n\
    matrix_key=${source##*/}\n\
    mkdir -p \"$7/$matrix_key/tasks\"\n\
    cp \"$source/matrix-report.json\" \"$7/$matrix_key/matrix-report.json\"\n\
    for task in \"$source/tasks\"/*; do\n\
        [ -f \"$task\" ] || continue\n\
        cp \"$task\" \"$7/$matrix_key/tasks/\"\n\
    done\n\
done\n",
    )?;
    let mut permissions = fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions)?;
    Ok(mise_data_dir)
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

fn make_tofu_plan(run_key: &str) -> TestResult<Plan> {
    let proposals = tofu_proposals()?;
    let package = ("infra".to_owned(), "infra".to_owned(), "default".to_owned());
    let job_ids = assign_crate_job_ids(&BTreeSet::from([package.clone()]), TOFU_JOB_ID_PREFIX);
    let job_id = job_ids
        .get(&(package.0, package.2))
        .cloned()
        .ok_or_else(|| io::Error::other("tofu job ID missing"))?;
    let validate_dependency = proposals[1]
        .depends_on
        .first()
        .cloned()
        .ok_or_else(|| io::Error::other("validate dependency missing"))?;
    if validate_dependency != proposals[0].task_id {
        return Err(io::Error::other("validate dependency did not match init task").into());
    }
    let platform = PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")?;
    let TofuPlanTasks {
        entries,
        obligations,
        task_ids,
    } = tofu_plan_tasks(&proposals, run_key, &job_id, &platform)?;
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
        jobs: vec![PlanJob {
            job_id: job_id.clone(),
            disposition: JobDisposition::Run,
        }],
        warnings: Vec::new(),
        edges: vec![TaskEdge {
            from: validate_dependency,
            to: proposals[1].task_id.clone(),
            kind: EdgeKind::Data,
        }],
    };
    plan.validate()?;
    Ok(plan)
}

fn tofu_proposals() -> TestResult<[ProposedTask; 2]> {
    Ok([
        propose_task(&TofuTaskGroup {
            root: "infra".to_owned(),
            kind: TofuTaskKind::InitForValidate,
            configuration: "default".to_owned(),
            no_targets: false,
        })?,
        propose_task(&TofuTaskGroup {
            root: "infra".to_owned(),
            kind: TofuTaskKind::Validate,
            configuration: "default".to_owned(),
            no_targets: false,
        })?,
    ])
}

struct TofuPlanTasks {
    entries: Vec<MatrixEntry>,
    obligations: Vec<PlanObligation>,
    task_ids: Vec<String>,
}

fn tofu_plan_tasks(
    proposals: &[ProposedTask],
    run_key: &str,
    job_id: &str,
    platform: &PlannedPlatform,
) -> TestResult<TofuPlanTasks> {
    let mut entries = Vec::with_capacity(proposals.len());
    let mut obligations = Vec::with_capacity(proposals.len());
    let mut task_ids = Vec::with_capacity(proposals.len());
    for proposal in proposals {
        let task_digest = digest_b3(proposal.task_id.as_bytes());
        let input_digest = digest_b3(format!("input:{}", proposal.task_id).as_bytes());
        let closure_digest = digest_b3(format!("closure:{}", proposal.task_id).as_bytes());
        let metadata = entry_metadata_for_task(proposal, &[])?;
        entries.push(MatrixEntry::derive(
            &proposal.stack_id,
            &proposal.task_id,
            "true",
            &task_digest,
            metadata,
            ExecuteTaskIds {
                tasks: BTreeMap::from([(
                    proposal.task_kind.clone(),
                    ExecuteTaskRef::Single(proposal.task_id.clone()),
                )]),
            },
            &input_digest,
            run_key,
            job_id,
            platform.clone(),
        )?);
        obligations.push(PlanObligation {
            task_id: proposal.task_id.clone(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest,
            input_digest,
            closure_digest,
            job_id: job_id.to_owned(),
        });
        task_ids.push(proposal.task_id.clone());
    }
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    obligations.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    task_ids.sort();
    Ok(TofuPlanTasks {
        entries,
        obligations,
        task_ids,
    })
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
