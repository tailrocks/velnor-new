// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, io, path::Path, process::Command};

use serde_json::{Value, json};
use velnor_actions_contract::ReleaseTarget;
use velnor_actions_orchestrator::plan_internal;

#[test]
fn public_plan_identity_uses_effective_runtime_for_runner_target() -> Result<(), Box<dyn Error>> {
    let temp = super::super::fixture(Some(super::super::BASE_APPROVALS), super::super::BASE_LOCK)?;
    let root = temp.path();
    let plan_config = config_without_native_checks()?;
    fs::write(root.join(".velnor/config.toml"), &plan_config)?;
    let head = git_output(root, &["rev-parse", "HEAD"])?;
    let request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "r260001-a1",
        "base": null,
        "head": head.clone(),
        "event": "local",
        "root": root.to_string_lossy(),
    });
    let request = serde_json::to_string(&request)?;
    let approved = plan_obligations(&request)?;

    fs::write(
        root.join(".velnor/controller-approvals.json"),
        compiled_runtime_approvals()?,
    )?;
    let compiled = plan_obligations(&request)?;
    assert_eq!(
        git_output(root, &["rev-parse", "HEAD"])?,
        head,
        "runtime rotation must keep checkout HEAD fixed"
    );

    for job_id in [
        "check-fixture-mise-linux",
        "check-fixture-mise-arm",
        "check-fixture-mise-intel",
    ] {
        let before = obligation(&approved, job_id)?;
        let after = obligation(&compiled, job_id)?;
        for identity in ["task_digest", "input_digest", "closure_digest"] {
            assert_ne!(
                before[identity], after[identity],
                "public plan {identity} must bind the approved Mise identity for {job_id}"
            );
        }
    }

    fs::write(
        root.join(".velnor/config.toml"),
        plan_config.replace(
            "policy = \"velnor-repository-v1\"",
            "policy = \"consumer-v1\"",
        ),
    )?;
    fs::write(
        root.join(".velnor/controller-approvals.json"),
        super::super::BASE_APPROVALS,
    )?;
    let consumer = plan_obligations(&request)?;
    for job_id in [
        "check-fixture-mise-linux",
        "check-fixture-mise-arm",
        "check-fixture-mise-intel",
    ] {
        assert_eq!(
            obligation(&compiled, job_id)?["task_digest"],
            obligation(&consumer, job_id)?["task_digest"],
            "ConsumerV1 must retain its compiled runtime identity despite stray approvals for {job_id}"
        );
    }
    Ok(())
}

fn plan_obligations(request: &str) -> Result<Value, Box<dyn Error>> {
    let response = plan_internal(request)?;
    let value: Value = serde_json::from_str(&response)?;
    Ok(value["plan"]["obligations"].clone())
}

fn config_without_native_checks() -> Result<String, Box<dyn Error>> {
    let config = super::super::CONFIG;
    let start = config
        .find("[[native_checks]]")
        .ok_or_else(|| io::Error::other("fixture native check section missing"))?;
    let end = config[start..]
        .find("[[checks]]")
        .map(|offset| start + offset)
        .ok_or_else(|| io::Error::other("fixture Mise check section missing"))?;
    Ok(format!("{}{}", &config[..start], &config[end..]))
}

fn obligation<'a>(obligations: &'a Value, job_id: &str) -> Result<&'a Value, Box<dyn Error>> {
    obligations
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["job_id"].as_str() == Some(job_id))
        })
        .ok_or_else(|| io::Error::other(format!("public plan obligation missing: {job_id}")).into())
}

fn compiled_runtime_approvals() -> Result<String, Box<dyn Error>> {
    let mut value: Value = serde_json::from_str(super::super::BASE_APPROVALS)?;
    let entries = value
        .pointer_mut("/active/mise_runtime")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("active runtime approval missing"))?;
    for target in ReleaseTarget::ALL {
        let pin = velnor_actions_mise::current_mise_binary(target);
        let entry = entries
            .iter_mut()
            .find(|entry| entry["target"].as_str() == Some(target.triple()))
            .ok_or_else(|| io::Error::other("runtime target approval missing"))?;
        entry["version"] = Value::String(pin.version.to_owned());
        entry["sha256"] = Value::String(pin.sha256.to_owned());
        entry["bytes"] = Value::from(pin.bytes);
    }
    Ok(serde_json::to_string(&value)?)
}

fn git_output(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
