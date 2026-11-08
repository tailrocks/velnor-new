// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::seed_off::seed_repo;
use super::workflow_configuration_fixture::{git, git_fixture};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::Path,
    process::{Command, Output},
};
use velnor_actions_contract::workflow::producer_execution_manifest::{
    PRODUCER_EXECUTION_MANIFEST_PATH, ProducerExecutionManifest,
};
use velnor_actions_contract::{Job, TASK_EXECUTION_MANIFEST_PATH, TaskExecutionManifestV1};
use velnor_actions_orchestrator::{finalized_jobs, plan_internal, prepare, render_staged_tree};
type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const TEST_NAME: &str = "producer_execution::producer_tasks_resolve_against_the_real_plan";

pub(super) struct Fixture {
    pub(super) repo: tempfile::TempDir,
    pub(super) temp: tempfile::TempDir,
    pub(super) head: String,
    pub(super) jobs: BTreeMap<String, Job>,
    pub(super) manifest: TaskExecutionManifestV1,
    pub(super) descriptor: String,
    pub(super) plan: Value,
}

impl Fixture {
    pub(super) fn new(both: bool) -> TestResult<Self> {
        let repo = seed_repo("")?;
        let root = repo.path();
        if both {
            let path = root.join(".velnor/config.toml");
            let config = fs::read_to_string(&path)?.replace("schema = 1", "schema = 2");
            fs::write(
                path,
                format!(
                    "{config}\n[execution]\nmode = \"both\"\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n"
                ),
            )?;
        }
        let prepared = prepare(root)?;
        super::producer_execution_support::assert_conflicting_task_rejected(&prepared)?;
        let jobs = finalized_jobs(&prepared)?;
        let tree = render_staged_tree(&prepared)?;
        let manifest_text = tree
            .get(TASK_EXECUTION_MANIFEST_PATH)
            .ok_or("manifest missing")?;
        let manifest: TaskExecutionManifestV1 =
            serde_json::from_str(manifest_text.split_once('\n').ok_or("marker missing")?.1)?;
        let manifest_path = root.join(TASK_EXECUTION_MANIFEST_PATH);
        fs::create_dir_all(manifest_path.parent().ok_or("manifest parent missing")?)?;
        fs::write(manifest_path, manifest_text)?;
        let descriptor = tree
            .get(PRODUCER_EXECUTION_MANIFEST_PATH)
            .ok_or("producer manifest missing")?
            .to_owned();
        fs::write(root.join(PRODUCER_EXECUTION_MANIFEST_PATH), &descriptor)?;
        git(&["add", "-A"], root)?;
        git(&["commit", "-qm", "fixture"], root)?;
        let head = git_head(root)?;
        let mut request = json!({"schema":1,"op":"plan-v1","run_key":"r190001-a1","head":head,"event":"push","root":root});
        if both {
            let expansion = velnor_actions_contract::expand_workflow_with_lane_plan(
                &prepared.workflow.ir,
                &prepared.config,
                Some(velnor_actions_contract::ExecutionMode::Both),
            )?;
            request["lane_inventory"] =
                serde_json::to_value(expansion.lane_plan.ok_or("lane plan missing")?.inventory())?;
        }
        let response: Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
        assert!(
            response["plan"].get("producer_executions").is_none(),
            "producer authorization must not change Plan wire format"
        );
        if both {
            assert!(
                response["plan"]["matrix"]["include"]
                    .as_array()
                    .ok_or("matrix missing")?
                    .iter()
                    .any(|entry| !entry["lane_variant"].is_null()),
                "Both-mode fixture has no projected lanes"
            );
        }
        let temp = tempfile::tempdir()?;
        fs::create_dir_all(temp.path().join("velnor/r190001-a1"))?;
        let fixture = Self {
            repo,
            temp,
            head,
            jobs,
            manifest,
            descriptor,
            plan: response["plan"].clone(),
        };
        fixture.write_plan(&fixture.plan)?;
        Ok(fixture)
    }

    pub(super) fn write_plan(&self, plan: &Value) -> TestResult {
        fs::write(
            self.temp.path().join("velnor/r190001-a1/plan.json"),
            serde_json::to_vec(plan)?,
        )?;
        Ok(())
    }

    pub(super) fn command(
        &self,
        job: &str,
        task: &str,
        digest: &str,
        head: &str,
        exit: Option<&str>,
    ) -> TestResult<Command> {
        let mut command = Command::new(std::env::current_exe()?);
        let payload = self.temp.path().join("push-event.json");
        fs::write(
            &payload,
            serde_json::to_vec(
                &json!({"ref":"refs/heads/testmain", "after":head, "repository":{"default_branch":"testmain"}}),
            )?,
        )?;
        command
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env("VELNOR_PRODUCER_TEST_CHILD", "1")
            .env("GITHUB_WORKSPACE", self.repo.path())
            .env("RUNNER_TEMP", self.temp.path())
            .env("VELNOR_RUNTIME_RUNNER_TEMP", self.temp.path())
            .env("GITHUB_SHA", head)
            .env("GITHUB_EVENT_NAME", "push")
            .env("GITHUB_REF_PROTECTED", "true")
            .env("GITHUB_REF", "refs/heads/testmain")
            .env("GITHUB_EVENT_PATH", payload)
            .env("GITHUB_RUN_ID", "190001")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env("GITHUB_JOB", job)
            .env("VELNOR_TASK_ID", task)
            .env("VELNOR_EXIT_CODE", exit.unwrap_or("0"))
            .env("VELNOR_TASK_EXECUTION_DIGEST", digest)
            .env("VELNOR_GENERATOR_VERSION", &self.manifest.generator_version)
            .env("VELNOR_TOOL_RUST", velnor_actions_mise::RUST_VERSION);
        if exit.is_some() {
            command.env("VELNOR_PRODUCER_TEST_WRITE", "1");
        }
        Ok(command)
    }

    pub(super) fn run(
        &self,
        job: &str,
        task: &str,
        digest: &str,
        head: &str,
        exit: Option<&str>,
    ) -> TestResult<Output> {
        Ok(self.command(job, task, digest, head, exit)?.output()?)
    }

    pub(super) fn descriptor(&self) -> TestResult<ProducerExecutionManifest> {
        Ok(serde_json::from_str(
            self.descriptor
                .split_once('\n')
                .ok_or("descriptor marker missing")?
                .1,
        )?)
    }

    pub(super) fn write_descriptor(&self, descriptor: &ProducerExecutionManifest) -> TestResult {
        let marker = velnor_actions_contract::workflow::task_execution_manifest_marker_line(
            &self.manifest.generator_version,
        )?;
        let body = String::from_utf8(velnor_actions_contract::canonical_json_bytes(descriptor)?)?;
        fs::write(
            self.repo.path().join(PRODUCER_EXECUTION_MANIFEST_PATH),
            format!("{marker}\n{body}\n"),
        )?;
        Ok(())
    }
}

pub(super) fn git_head(root: &Path) -> TestResult<String> {
    Ok(String::from_utf8(
        git_fixture::command(root)?
            .args(["rev-parse", "HEAD"])
            .output()?
            .stdout,
    )?
    .trim()
    .to_owned())
}
