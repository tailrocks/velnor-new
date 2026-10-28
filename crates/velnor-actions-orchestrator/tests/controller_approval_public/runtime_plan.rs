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
    let consumer_config = plan_config.replace(
        "policy = \"velnor-repository-v1\"",
        "policy = \"consumer-v1\"",
    );
    fs::write(root.join(".velnor/config.toml"), &plan_config)?;
    let head = git_output(root, &["rev-parse", "HEAD"])?;
    let task = fs::read(root.join("mise.toml"))?;
    let request = plan_request(root, &head)?;
    let approved = plan_obligations(&request)?;

    let version_rotated_approvals = compiled_runtime_approvals()?;
    write_approvals(root, &version_rotated_approvals)?;
    let compiled = plan_obligations(&request)?;
    assert_all_targets_rotate(&approved, &compiled)?;

    fs::write(root.join(".velnor/config.toml"), &consumer_config)?;
    write_approvals(root, super::super::BASE_APPROVALS)?;
    let consumer = plan_obligations(&request)?;
    assert_all_targets_equal(&compiled, &consumer, "ConsumerV1 ignores compiled runtime")?;

    for target in ReleaseTarget::ALL {
        for mutation in [RuntimeMutation::Sha256, RuntimeMutation::Bytes] {
            let approvals = same_version_runtime_mutation(target, mutation)?;
            fs::write(root.join(".velnor/config.toml"), &plan_config)?;
            write_approvals(root, &approvals)?;
            let rotated = plan_obligations(&request)?;
            assert_selected_target_rotates(&approved, &rotated, target, mutation)?;

            fs::write(root.join(".velnor/config.toml"), &consumer_config)?;
            let consumer_with_stray_approval = plan_obligations(&request)?;
            assert_all_targets_equal(
                &consumer,
                &consumer_with_stray_approval,
                &format!("ConsumerV1 ignores same-version {mutation:?} rotation for {target:?}"),
            )?;
        }
    }

    assert_eq!(git_output(root, &["rev-parse", "HEAD"])?, head);
    assert_eq!(
        fs::read(root.join(".velnor/config.toml"))?,
        consumer_config.as_bytes()
    );
    assert_eq!(fs::read(root.join("mise.toml"))?, task);
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum RuntimeMutation {
    Sha256,
    Bytes,
}

fn plan_request(root: &Path, head: &str) -> Result<String, Box<dyn Error>> {
    let request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "r260001-a1",
        "base": null,
        "head": head,
        "event": "local",
        "root": root.to_string_lossy(),
    });
    Ok(serde_json::to_string(&request)?)
}

fn write_approvals(root: &Path, approvals: &str) -> Result<(), Box<dyn Error>> {
    fs::write(root.join(".velnor/controller-approvals.json"), approvals)?;
    Ok(())
}

fn same_version_runtime_mutation(
    target: ReleaseTarget,
    mutation: RuntimeMutation,
) -> Result<String, Box<dyn Error>> {
    let mut approvals: Value = serde_json::from_str(super::super::BASE_APPROVALS)?;
    let runtimes = approvals
        .pointer_mut("/active/mise_runtime")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("active runtime approvals missing"))?;
    let runtime = runtimes
        .iter_mut()
        .find(|entry| entry["target"].as_str() == Some(target.triple()))
        .ok_or_else(|| io::Error::other("runtime target approval missing"))?;
    match mutation {
        RuntimeMutation::Sha256 => {
            let current = runtime["sha256"]
                .as_str()
                .ok_or_else(|| io::Error::other("runtime SHA-256 missing"))?;
            let changed_first = if current.starts_with('0') { '1' } else { '0' };
            runtime["sha256"] = Value::String(format!("{changed_first}{}", &current[1..]));
        }
        RuntimeMutation::Bytes => {
            let current = runtime["bytes"]
                .as_u64()
                .ok_or_else(|| io::Error::other("runtime byte count missing"))?;
            runtime["bytes"] = Value::from(
                current
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("runtime byte count overflow"))?,
            );
        }
    }
    Ok(serde_json::to_string(&approvals)?)
}

fn assert_all_targets_rotate(before: &Value, after: &Value) -> Result<(), Box<dyn Error>> {
    for target in ReleaseTarget::ALL {
        let before = obligation(before, job_id(target))?;
        let after = obligation(after, job_id(target))?;
        for identity in ["task_digest", "input_digest", "closure_digest"] {
            assert_ne!(
                before[identity], after[identity],
                "version rotation must change {identity} for {target:?}"
            );
        }
    }
    Ok(())
}

fn assert_selected_target_rotates(
    before: &Value,
    after: &Value,
    selected: ReleaseTarget,
    mutation: RuntimeMutation,
) -> Result<(), Box<dyn Error>> {
    for target in ReleaseTarget::ALL {
        let before = obligation(before, job_id(target))?;
        let after = obligation(after, job_id(target))?;
        for identity in ["task_digest", "input_digest", "closure_digest"] {
            if target == selected {
                assert_ne!(
                    before[identity], after[identity],
                    "same-version {mutation:?} rotation must change {identity} for selected target {selected:?}"
                );
            } else {
                assert_eq!(
                    before[identity], after[identity],
                    "same-version {mutation:?} rotation must preserve {identity} for unselected target {target:?}"
                );
            }
        }
    }
    Ok(())
}

fn assert_all_targets_equal(
    before: &Value,
    after: &Value,
    context: &str,
) -> Result<(), Box<dyn Error>> {
    for target in ReleaseTarget::ALL {
        let before = obligation(before, job_id(target))?;
        let after = obligation(after, job_id(target))?;
        for identity in ["task_digest", "input_digest", "closure_digest"] {
            assert_eq!(
                before[identity], after[identity],
                "{context}: ConsumerV1 must preserve {identity} for {target:?}"
            );
        }
    }
    Ok(())
}

fn job_id(target: ReleaseTarget) -> &'static str {
    match target {
        ReleaseTarget::LinuxX86_64 => "check-fixture-mise-linux",
        ReleaseTarget::MacosArm64 => "check-fixture-mise-arm",
        ReleaseTarget::MacosX86_64 => "check-fixture-mise-intel",
    }
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
    Ok(format!(
        "{}\n[discovery]\nexclude = [\"Cargo.toml\"]\n{}",
        &config[..start],
        &config[end..]
    ))
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
