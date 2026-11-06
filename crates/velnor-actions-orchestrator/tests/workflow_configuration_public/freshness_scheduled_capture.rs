// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tempfile::TempDir;
use velnor_actions_contract::{FRESHNESS_CRON_WEEKLY, WorkflowPolicy};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use super::maintenance_features::owner_feature_repo;
use super::workflow_configuration_fixture;

const FRESHNESS_PATH: &str = ".github/workflows/freshness.yml";
const PROBE_LOG_PATH: &str = "${{ runner.temp }}/velnor/freshness/probe.log";

fn generate_preview(repo: &TempDir, destination: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let preparation = prepare(repo.path())?;
    let report = generate(
        &preparation,
        &GenerateOptions {
            output_dir: Some(destination.to_path_buf()),
            ..GenerateOptions::default()
        },
    )?;
    Ok(report.files_written)
}

fn parse_workflow(path: &Path) -> Result<Value, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    Ok(serde_saphyr::from_str(&text)?)
}

fn steps(workflow: &Value) -> Result<&[Value], Box<dyn Error>> {
    workflow["jobs"]["freshness"]["steps"]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| "freshness job has no steps".into())
}

fn step_named<'a>(steps: &'a [Value], name: &str) -> Result<(usize, &'a Value), Box<dyn Error>> {
    steps
        .iter()
        .enumerate()
        .find(|(_, step)| step["name"].as_str() == Some(name))
        .ok_or_else(|| format!("freshness step {name} is missing").into())
}

fn permissions_are_read_only(value: &Value) -> bool {
    value.as_object().is_some_and(|permissions| {
        permissions.len() == 1
            && permissions.get("contents").and_then(Value::as_str) == Some("read")
    })
}

#[test]
fn public_generation_captures_repository_probe_and_omits_consumer_capture()
-> Result<(), Box<dyn Error>> {
    let repository = owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        false,
        "default",
        "default",
    )?;
    let repository_preview = tempfile::tempdir()?;
    let repository_output = repository_preview.path().join("generated");
    let repository_files = generate_preview(&repository, &repository_output)?;
    assert!(repository_files.iter().any(|path| path == FRESHNESS_PATH));

    let workflow = parse_workflow(&repository_output.join(FRESHNESS_PATH))?;
    assert_eq!(
        workflow["on"]["schedule"],
        json!([{"cron": FRESHNESS_CRON_WEEKLY}])
    );
    assert_eq!(workflow["on"]["workflow_dispatch"], json!({}));
    assert_eq!(workflow["jobs"]["freshness"]["runs-on"], "ubuntu-26.04");
    assert!(permissions_are_read_only(&workflow["permissions"]));
    assert!(permissions_are_read_only(
        &workflow["jobs"]["freshness"]["permissions"]
    ));

    let generated_steps = steps(&workflow)?;
    let (checkout_index, checkout) = step_named(generated_steps, "Checkout")?;
    assert_eq!(checkout["with"]["persist-credentials"], "false");
    let (setup_index, _) = step_named(generated_steps, "Setup Mise")?;
    let (install_index, install) = step_named(generated_steps, "Install Rust toolchain")?;
    let install_run = install["run"]
        .as_str()
        .ok_or("Rust install step has no shell command")?;
    assert!(
        install_run.contains("mise")
            && install_run.contains("install")
            && install_run.contains("rust[profile=minimal]@"),
        "{install_run}"
    );
    let (verify_index, _) = step_named(generated_steps, "Verify Rust channel manifest")?;
    let (probe_index, probe) = step_named(generated_steps, "Check upstream freshness")?;
    assert!(
        checkout_index < setup_index
            && setup_index < install_index
            && install_index < verify_index
            && verify_index < probe_index
    );
    let probe_run = probe["run"]
        .as_str()
        .ok_or("probe step has no shell command")?;
    assert!(probe_run.contains("mise --no-config --no-env --no-hooks exec rust@"));
    assert!(probe_run.contains("bash scripts/check-freshness.sh --check-upstream"));
    assert_eq!(probe["env"]["GITHUB_TOKEN"], "");
    assert_eq!(probe["env"]["GH_TOKEN"], "");

    let upload_actions: Vec<_> = generated_steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            step["uses"].as_str() == Some(velnor_actions_actionlint::UPLOAD_ARTIFACT_ACTION_USES)
        })
        .collect();
    assert_eq!(upload_actions.len(), 1);
    let (upload_index, upload) = upload_actions[0];
    assert!(upload_index > probe_index);
    assert_eq!(upload["if"], "always()");
    assert_eq!(upload["with"]["path"], PROBE_LOG_PATH);
    assert_eq!(upload["with"]["if-no-files-found"], "error");
    assert_eq!(upload["with"]["retention-days"], "30");
    let artifact_name = upload["with"]["name"]
        .as_str()
        .ok_or("freshness artifact has no name")?;
    assert!(artifact_name.contains("${{ github.run_id }}"));
    assert!(artifact_name.contains("${{ github.run_attempt }}"));

    let consumer = workflow_configuration_fixture::make_repo(
        &workflow_configuration_fixture::config_with_verification_branch(None),
    )?;
    let consumer_preview = tempfile::tempdir()?;
    let consumer_output = consumer_preview.path().join("generated");
    let consumer_files = generate_preview(&consumer, &consumer_output)?;
    assert!(!consumer_files.iter().any(|path| path == FRESHNESS_PATH));
    assert!(!consumer_output.join(FRESHNESS_PATH).exists());
    Ok(())
}

#[test]
fn generated_probe_capture_retains_the_real_missing_inventory_failure() -> Result<(), Box<dyn Error>>
{
    let repository = owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        false,
        "default",
        "default",
    )?;
    let preview = tempfile::tempdir()?;
    let generated_root = preview.path().join("generated");
    generate_preview(&repository, &generated_root)?;
    let workflow = parse_workflow(&generated_root.join(FRESHNESS_PATH))?;
    let (_, probe) = step_named(steps(&workflow)?, "Check upstream freshness")?;
    let run = probe["run"]
        .as_str()
        .ok_or("probe step has no shell command")?;

    let scripts = repository.path().join("scripts");
    fs::create_dir_all(&scripts)?;
    fs::write(
        scripts.join("check-freshness.sh"),
        include_str!("../../../../scripts/check-freshness.sh"),
    )?;
    let inventory = repository.path().join(".velnor/freshness-inventory.json");
    assert!(!inventory.exists());

    let runner_temp = tempfile::tempdir()?;
    let output = Command::new("bash")
        .args(["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", run])
        .current_dir(repository.path())
        .env("RUNNER_TEMP", runner_temp.path())
        .output()?;
    assert!(!output.status.success());

    let saved_output = fs::read(runner_temp.path().join("velnor/freshness/probe.log"))?;
    let printed_output = String::from_utf8(output.stdout)?;
    assert!(printed_output.contains("check-freshness: missing inventory:"));
    assert!(printed_output.contains(&inventory.display().to_string()));
    assert_eq!(saved_output, printed_output.as_bytes());
    Ok(())
}
