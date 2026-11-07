// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path, process::Command};

use serde_json::Value;

use super::BIN;

pub(super) struct Generated {
    pub inventory: Value,
    pub required: Vec<String>,
    pub manifest: String,
    pub script: String,
}

pub(super) fn generate(root: &Path, tools: &Path) -> Result<Generated, Box<dyn Error>> {
    let output = command(tools)?
        .args(["generate", "--mode", "both"])
        .current_dir(root)
        .output()?;
    assert!(
        output.status.success(),
        "CLI generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    read_generated(root)
}

fn read_generated(root: &Path) -> Result<Generated, Box<dyn Error>> {
    let workflow: Value =
        serde_saphyr::from_str(&fs::read_to_string(root.join(".github/workflows/ci.yml"))?)?;
    let inventory = workflow["jobs"]["plan"]["steps"]
        .as_array()
        .ok_or("Plan steps missing")?
        .iter()
        .find_map(|step| step["env"]["VELNOR_LANE_INVENTORY_JSON"].as_str())
        .ok_or("CLI generation did not emit canonical Both lane inventory")?;
    let required = serde_json::from_value(workflow["jobs"]["required"]["needs"].clone())?;
    let action = fs::read_dir(root.join(".github/actions"))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("declared-task-")
        })
        .ok_or("declared task action missing")?;
    let action: Value =
        serde_saphyr::from_str(&fs::read_to_string(action.path().join("action.yml"))?)?;
    let script = action["runs"]["steps"]
        .as_array()
        .and_then(|steps| steps.iter().find_map(|step| step["run"].as_str()))
        .ok_or("declared task action script missing")?;
    Ok(Generated {
        inventory: serde_json::from_str(inventory)?,
        required,
        manifest: fs::read_to_string(
            root.join(velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH),
        )?,
        script: script.to_owned(),
    })
}

pub(super) fn invoke(
    tools: &Path,
    operation: &str,
    request: &Value,
    expected_success: bool,
) -> Result<Value, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join(format!("{operation}-request.json"));
    fs::write(&path, serde_json::to_vec(request)?)?;
    let output = command(tools)?
        .env("VELNOR_INTERNAL_OP", operation)
        .env("VELNOR_REQUEST_FILE", &path)
        .env("RUNNER_TEMP", temp.path())
        .env("GITHUB_OUTPUT", temp.path().join("outputs"))
        .output()?;
    assert_eq!(
        output.status.success(),
        expected_success,
        "CLI {operation}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = velnor_actions_orchestrator::response_path_for(&path)?;
    Ok(serde_json::from_slice(&fs::read(response)?)?)
}

fn command(tools: &Path) -> Result<Command, Box<dyn Error>> {
    let path = std::env::join_paths([
        tools.join("bin"),
        tools.join("cargo/bin"),
        "/usr/bin".into(),
        "/bin".into(),
    ])?;
    let mut command = Command::new(BIN);
    command
        .env_clear()
        .env("PATH", path)
        .env("CARGO_HOME", tools.join("cargo"))
        .env("RUSTUP_HOME", tools.join("rustup"))
        .env("MISE_DATA_DIR", tools.join("mise-data"))
        .env("MISE_NO_CONFIG", "1")
        .env("MISE_NO_ENV", "1")
        .env("MISE_NO_HOOKS", "1")
        .env("MISE_AUTO_INSTALL", "0")
        .env("MISE_EXEC_AUTO_INSTALL", "false")
        .env("MISE_OFFLINE", "1")
        .env("CARGO_NET_OFFLINE", "true");
    Ok(command)
}

pub(super) fn emit_ledger(
    tools: &Path,
    root: &Path,
    runner: &Path,
    head: &str,
    job: &str,
) -> Result<Value, Box<dyn Error>> {
    let payload = runner.join("push-event.json");
    fs::write(
        &payload,
        serde_json::to_vec(&serde_json::json!({"after": head}))?,
    )?;
    let output = command(tools)?
        .current_dir(root)
        .env("VELNOR_INTERNAL_OP", "emit-group-ledger-v1")
        .env("RUNNER_TEMP", runner)
        .env("GITHUB_JOB", job)
        .env("GITHUB_RUN_ID", "190003")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_EVENT_PATH", payload)
        .env("GITHUB_SHA", head)
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_WORKFLOW_SHA", head)
        .env("VELNOR_JOB_STATUS", "success")
        .env_remove("VELNOR_MATRIX_KEY")
        .output()?;
    assert!(
        output.status.success(),
        "CLI ledger emission failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = String::from_utf8(output.stdout)?;
    Ok(serde_json::from_slice(&fs::read(path.trim())?)?)
}
