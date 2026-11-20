// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use crate::declared_fixture::{TestResult, rendered_text};
use crate::yaml_tree::document;

/// Jobs whose commands are a call of a checked-in script under `qualification/`: velnor-gen
/// refuses a multi-line `run`, a loop, a pipe, a `shell` key and a job `env`.
pub(super) const SCRIPTED: [&str; 10] = [
    "mbx-cache-write-hosted",
    "mbx-cache-read-hosted",
    "ports-a-hosted",
    "ports-a-scale-set",
    "ports-b-hosted",
    "ports-b-scale-set",
    "mise-pin-linux-x64",
    "stable-sources-producer-linux",
    "stable-sources-linux-warm",
    "stable-sources-linux-cold",
];

const MBX_SCRIPT: &str = "qualification/mbx-cache-roundtrip.sh";

/// Commands each script must still hold, verbatim.
const SCRIPT_KEEPS: [(&str, &[&str]); 4] = [
    (
        MBX_SCRIPT,
        &[
            "mbx build --manifest-path \"$root/Cargo.toml\"",
            "jq -e '.objects > 0' \"$RUNNER_TEMP/mbx-object-stats.json\"",
            "jq -e '.savings.cached_compilations > 0' \"$RUNNER_TEMP/mbx-reuse-stats.json\"",
            "test \"$CACHE_SAVE_REASON\" = 'workflow_dispatch'",
            "test \"$CACHE_HIT\" = 'false'",
            "mise --no-config --no-env --no-hooks exec rust@\"${VELNOR_TOOL_RUST}\" -- mbx clean",
        ],
    ),
    (
        "qualification/ports.sh",
        &[
            "docker run -d --name g4-hold -p 8080:",
            "docker port g4-hold 80 | grep -q 8080",
            "echo port-held",
            "sleep 120",
            "sleep 45",
            "docker rm -f g4-hold",
        ],
    ),
    (
        "qualification/stable-sources.sh",
        &[
            "bash scripts/generator-release/stable-source-producer.sh",
            "/usr/bin/python3 -I -S scripts/generator-release/stable-source-admission.py",
            "bash scripts/generator-release/stable-source-consumer.sh",
            "CARGO_NET_OFFLINE=true bash scripts/build-owned-archive-guard.sh --source-cache \"$CARGO_HOME\"",
            "grep -Fx \"fetch_mode=Cargo.toml:$mode\"",
            "mise --no-config --no-env --no-hooks exec gh@2.104.0 -- gh \"$@\"",
            "[ \"$VELNOR_EVIDENCE_ARTIFACT_ID\" -le 9007199254740991 ]",
        ],
    ),
    (
        "qualification/mise-pin.sh",
        &[
            "binary=\"$(command -v mise)\"",
            "test \"$actual\" = \"$MISE_SHA256\"",
        ],
    ),
];

fn repo(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn read(path: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(repo(path))?)
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

fn generated() -> Result<Value, Box<dyn Error>> {
    let config = read(".velnor/workflows.toml")?;
    Ok(document(&rendered_text(&config, "qualification")?)?)
}

fn jobs_of(workflow: &Value) -> Result<&serde_json::Map<String, Value>, Box<dyn Error>> {
    Ok(workflow
        .get("jobs")
        .and_then(Value::as_object)
        .ok_or("no generated jobs")?)
}

/// The `inputs.mode == 'x'` values a job condition names.
fn modes(condition: &str) -> BTreeSet<String> {
    condition
        .split("inputs.mode == '")
        .skip(1)
        .filter_map(|rest| rest.split('\'').next())
        .map(str::to_owned)
        .collect()
}

/// `bash qualification/<file> <args>` as a (file, args) pair.
fn script_call(run: &str) -> Option<(&str, &str)> {
    let rest = run.strip_prefix("bash qualification/")?;
    Some(rest.split_once(' ').unwrap_or((rest, "")))
}

#[test]
fn the_committed_workflow_and_the_sample_are_what_the_repository_config_renders() -> TestResult {
    let config = read(".velnor/workflows.toml")?;
    let rendered = rendered_text(&config, "qualification")?;
    assert_eq!(read(".github/workflows/qualification.yml")?, rendered);
    assert_eq!(
        read("generator/samples/qualification/view/.velnor/workflows.toml")?,
        config
    );
    assert_eq!(
        read("generator/samples/qualification/expected/.github/workflows/qualification.yml")?,
        rendered
    );
    Ok(())
}

#[test]
fn every_job_is_offered_by_the_mode_input_and_every_mode_has_a_job() -> TestResult {
    let workflow = generated()?;
    let offered: BTreeSet<String> = workflow
        .pointer("/on/workflow_dispatch/inputs/mode/options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(text)
        .collect();
    let mut named = BTreeSet::new();
    for (id, job) in jobs_of(&workflow)? {
        let condition = job.get("if").map(text).ok_or(format!("{id}: no if"))?;
        let job_modes = modes(&condition);
        assert!(!job_modes.is_empty(), "{id}: names no mode");
        assert!(
            job_modes.is_subset(&offered),
            "{id}: names an unoffered mode"
        );
        named.extend(job_modes);
    }
    assert_eq!(named, offered);
    Ok(())
}

#[test]
fn scripted_jobs_call_scripts_that_exist_with_a_phase_they_have() -> TestResult {
    let workflow = generated()?;
    let jobs = jobs_of(&workflow)?;
    let mut called = BTreeSet::new();
    for id in SCRIPTED {
        let job = jobs.get(id).ok_or(format!("{id}: no such job"))?;
        let mut calls = 0;
        for step in job
            .get("steps")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some((file, args)) = step.get("run").map(text).and_then(|run| {
                script_call(&run).map(|(file, args)| (file.to_owned(), args.to_owned()))
            }) else {
                continue;
            };
            calls += 1;
            let path = format!("qualification/{file}");
            let script = read(&path).map_err(|_| format!("{id}: {path} does not exist"))?;
            assert!(
                args.is_empty() || script.contains(&format!("  {args})")),
                "{id}: {path} has no phase {args}"
            );
            called.insert(path);
        }
        assert!(calls > 0, "{id}: calls no script");
    }
    for (path, fragments) in SCRIPT_KEEPS {
        assert!(called.contains(path), "{path}: no job calls it");
        let script = read(path)?;
        for fragment in fragments {
            assert!(script.contains(fragment), "{path} lost {fragment}");
        }
    }
    Ok(())
}

#[test]
fn the_mbx_script_parses_and_refuses_an_unknown_phase() -> TestResult {
    let script = repo("qualification/mbx-cache-roundtrip.sh");
    let parsed = std::process::Command::new("/bin/bash")
        .arg("-n")
        .arg(&script)
        .status()?;
    assert!(parsed.success());
    let unknown = std::process::Command::new("/bin/bash")
        .arg(&script)
        .arg("no-such-phase")
        .output()?;
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("no-such-phase"));
    Ok(())
}

/// Jobs whose only step is `exit 1`: a dispatch that matches one of them fails the run.
fn failing_jobs(workflow: &Value) -> Result<Vec<String>, Box<dyn Error>> {
    let mut conditions = Vec::new();
    for job in jobs_of(workflow)?.values() {
        let exits = job
            .get("steps")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|step| step.get("run").map(text).as_deref() == Some("exit 1"));
        if exits {
            conditions.push(job.get("if").map(text).ok_or("failing job: no if")?);
        }
    }
    Ok(conditions)
}

#[test]
fn a_mode_that_no_job_runs_fails_the_run_instead_of_skipping_every_job() -> TestResult {
    let workflow = generated()?;
    let offered: BTreeSet<String> = workflow
        .pointer("/on/workflow_dispatch/inputs/mode/options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(text)
        .collect();
    let guarded = failing_jobs(&workflow)?
        .iter()
        .any(|condition| condition.contains("!(") && modes(condition) == offered);
    assert!(
        guarded,
        "no job fails the run when inputs.mode is none of the offered modes"
    );
    Ok(())
}

#[test]
fn stable_sources_outside_a_non_default_branch_fails_the_run() -> TestResult {
    let workflow = generated()?;
    let guarded = failing_jobs(&workflow)?.iter().any(|condition| {
        condition.contains("inputs.mode == 'stable-sources'")
            && condition.contains("github.event.repository.default_branch")
    });
    assert!(
        guarded,
        "no job fails the run for stable-sources on the default branch"
    );
    Ok(())
}

#[test]
fn a_cancel_service_run_cancels_its_own_run_mid_step_and_a_check_job_asserts_the_cancel() -> TestResult {
    let workflow = generated()?;
    let jobs = jobs_of(&workflow)?;
    let cancel_jobs = ["cancel-service-hosted", "cancel-service-scale-set"];
    for id in cancel_jobs {
        let job = jobs.get(id).ok_or(format!("{id}: no such job"))?;
        assert_eq!(
            job.pointer("/permissions/actions"),
            Some(&Value::from("write")),
            "{id}: must hold actions: write to cancel its own run"
        );
        let runs: Vec<String> = job
            .get("steps")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|step| step.get("run").map(text))
            .collect();
        assert!(
            runs.iter()
                .any(|run| run.contains("/actions/runs/$GITHUB_RUN_ID/cancel")),
            "{id}: never cancels its own run"
        );
        assert!(
            runs.iter().any(|run| run == "exit 1"),
            "{id}: a run that is never cancelled must fail, not pass"
        );
        assert!(
            !text(job).contains("sleep 900"),
            "{id}: sleeps past its own timeout, so the timeout cancels it instead"
        );
    }
    let check = jobs
        .get("cancel-service-check")
        .ok_or("cancel-service-check: no such job")?;
    let needs = text(check.get("needs").unwrap_or(&Value::Null));
    for id in cancel_jobs {
        assert!(needs.contains(id), "cancel-service-check does not wait for {id}");
    }
    assert!(
        text(check.get("if").unwrap_or(&Value::Null)).contains("always()"),
        "cancel-service-check must run after the cancelled jobs"
    );
    let check_runs = text(check.get("steps").unwrap_or(&Value::Null));
    assert!(
        check_runs.contains("bash qualification/cancel-service-check.sh"),
        "cancel-service-check calls no checker"
    );
    let checker = read("qualification/cancel-service-check.sh")?;
    assert!(
        checker.contains("cancelled") && checker.contains("240"),
        "cancel-service-check.sh does not assert an explicit cancel under the timeout"
    );
    Ok(())
}
