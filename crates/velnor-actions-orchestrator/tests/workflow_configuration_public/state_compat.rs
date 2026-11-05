// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_ROOT_ENV;
use velnor_actions_contract::{Job, StepId, StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::state_roots::{declared_input_repo, plan_key_script};
use super::workflow_configuration_fixture::git;

fn plan_step_env(jobs: &BTreeMap<String, Job>) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let step = jobs
        .get("plan")
        .and_then(|plan| plan.steps.iter().find(|s| s.id == Some(StepId::CacheKeys)))
        .ok_or("plan job lost its cache keys step")?;
    match &step.kind {
        StepKind::Shell { env, .. } => Ok(env.clone()),
        _ => Err("cache keys step left its shell form".into()),
    }
}

fn resolved_env(env: &BTreeMap<String, String>) -> Vec<(String, String)> {
    env.iter()
        .map(|(name, value)| {
            let value = if value.contains("${{") {
                match name.as_str() {
                    "VELNOR_DEFAULT_REF" => "refs/heads/main",
                    "VELNOR_STATE_SNAPSHOT" => {
                        "s256-1111111111111111111111111111111111111111111111111111111111111111"
                    }
                    _ => "",
                }
            } else {
                value.as_str()
            };
            (name.clone(), value.to_owned())
        })
        .collect()
}

fn plan_outputs(
    jobs: &BTreeMap<String, Job>,
    repo: &Path,
    runner_temp: &Path,
) -> Result<String, Box<dyn Error>> {
    git(&["add", "-A"], repo)?;
    let output = runner_temp.join("plan_output");
    fs::write(&output, "")?;
    let run = Command::new("sh")
        .args(["-c", &plan_key_script(jobs)?])
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_WORKSPACE", repo)
        .env("GITHUB_OUTPUT", &output)
        .env("ImageOS", "ubuntu24")
        .envs(resolved_env(&plan_step_env(jobs)?))
        .output()?;
    if !run.status.success() {
        return Err(String::from_utf8_lossy(&run.stderr).into_owned().into());
    }
    Ok(fs::read_to_string(&output)?)
}

fn job_compat(
    jobs: &BTreeMap<String, Job>,
    repo: &Path,
    runner_temp: &Path,
    root: &str,
) -> Result<String, Box<dyn Error>> {
    let (script, env) = jobs
        .values()
        .flat_map(|job| &job.steps)
        .find_map(|step| match (&step.role, &step.kind) {
            (Some(StepRole::MbxBundleKey), StepKind::Shell { run, env })
                if env.get(MBX_BUNDLE_ROOT_ENV).map(String::as_str) == Some(root) =>
            {
                Some((run.get(2)?.clone(), env.clone()))
            }
            _ => None,
        })
        .ok_or("no crate job key step for the root")?;
    let output = runner_temp.join("job_output");
    fs::write(&output, "")?;
    let mut command = Command::new("sh");
    command
        .args(["-c", &script])
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_WORKSPACE", repo)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REF_PROTECTED", "true")
        .env("VELNOR_DEFAULT_REF", "refs/heads/main")
        .env("ImageOS", "ubuntu24");
    command.envs(resolved_env(&env));
    let run = command.output()?;
    if !run.status.success() {
        return Err(String::from_utf8_lossy(&run.stderr).into_owned().into());
    }
    let record = fs::read_to_string(runner_temp.join("velnor/state-discovery.record"))?;
    record
        .lines()
        .find_map(|line| line.strip_prefix("compat=").map(str::to_owned))
        .ok_or_else(|| "discovery record lost its compat".into())
}

#[test]
fn plan_computes_the_compat_the_crate_job_recomputes() -> Result<(), Box<dyn Error>> {
    let repo = declared_input_repo()?;
    let root = repo.path();
    let jobs = finalized_jobs(&prepare(root)?)?;
    let temp = tempfile::tempdir()?;
    let outputs = plan_outputs(&jobs, root, temp.path())?;
    for (dir, name) in [
        ("crates/nested", "state_slice_rust_nested_compat"),
        ("other", "state_slice_rust_other_compat"),
    ] {
        let planned = outputs
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")))
            .ok_or_else(|| format!("plan never published {name}"))?;
        let recomputed = job_compat(&jobs, root, temp.path(), dir)?;
        if planned != recomputed {
            return Err(format!("{dir}: plan says {planned}, the job says {recomputed}").into());
        }
    }
    Ok(())
}
