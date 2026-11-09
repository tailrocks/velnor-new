// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{error::Error, fmt::Write as _, fs, process::Command};
use velnor_actions_contract::ExecutionMode;
use velnor_actions_orchestrator::{GenerateOptions, prepare, render_staged_tree_with};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RECEIPT: &str = "2d133dc7766edd85d88b9ec8d132f075ebad232849500277ef3e6f5f93beefce";

pub(super) fn assert_preview_render(root: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let path = root.join(".velnor/generator.lock");
    let original = fs::read_to_string(&path)?;
    let lock = preview_lock(&original)?;
    fs::write(&path, lock)?;
    let prepared = prepare(root)?;
    assert_declared_inputs_generate(&prepared)?;
    let tree = render_staged_tree_with(&prepared, Some(ExecutionMode::Both))?;
    let workflow = tree.get(WORKFLOW_PATH).ok_or("missing preview workflow")?;
    assert!(
        tree.get(".github/actions/velnor-preview-setup/action.yml")
            .is_none()
    );
    assert!(
        workflow.len() <= 500_000,
        "workflow exceeds GitHub size limit"
    );
    let yaml: Value = serde_saphyr::from_str(workflow)?;
    assert_identity_steps(&tree, &yaml)?;
    assert_executor_behavior(&yaml)?;
    let seal: Value = serde_saphyr::from_str(
        tree.get(velnor_actions_contract::workflow::state_seal::STATE_SEAL_WORKFLOW_PATH)
            .ok_or("missing preview state seal")?,
    )?;
    assert_job_identity(&tree, &seal, "plan")?;
    for id in ["state-producer-crates-a-b", "state-producer-crates-a-c"] {
        assert_job_identity(&tree, &seal, id)?;
    }
    assert_preview_action_retired(root)?;
    fs::write(path, original)?;
    Ok(())
}

fn preview_lock(original: &str) -> Result<String, Box<dyn Error>> {
    let mut lock = original.to_owned();
    write!(
        &mut lock,
        "\n[planner-preview]\nschema = 3\nrepository = \"tailrocks/velnor-new\"\nversion = \"0.1.75\"\nsource_sha = \"{SOURCE}\"\ntag = \"sdk-preview-{SOURCE}\"\nrelease_id = 10\n"
    )?;
    for (role, workflow, id) in [
        (
            "source",
            ".github/workflows/product-release-generator.yml",
            11,
        ),
        (
            "draft",
            ".github/workflows/product-release-generator.yml",
            11,
        ),
        (
            "signer",
            ".github/workflows/product-release-generator.yml",
            11,
        ),
        ("ci", ".github/workflows/ci.yml", 12),
        (
            "qualification",
            ".github/workflows/source-qualification.yml",
            13,
        ),
    ] {
        let git_ref = if matches!(role, "source" | "draft" | "signer") {
            format!("refs/tags/sdk-preview-{SOURCE}")
        } else {
            "refs/heads/main".to_owned()
        };
        write!(
            &mut lock,
            "{role}_workflow_path = \"{workflow}\"\n{role}_workflow_ref = \"tailrocks/velnor-new/{workflow}@{git_ref}\"\n{role}_workflow_sha = \"{SOURCE}\"\n{role}_run_id = {id}\n{role}_run_attempt = 1\n"
        )?;
    }
    for (role, target, original, retained, job) in [
        ("linux", "x86_64-unknown-linux-gnu", 21, 23, 25),
        ("darwin", "aarch64-apple-darwin", 26, 27, 28),
    ] {
        write!(
            &mut lock,
            "{role}_payload_target = \"{target}\"\n{role}_payload_name = \"velnor-actions-{target}\"\n{role}_payload_size = 12345\n{role}_payload_sha256 = \"{}\"\n{role}_producer_run_id = 20\n{role}_producer_run_attempt = 1\n{role}_producer_payload_artifact_id = {original}\n{role}_producer_payload_archive_sha256 = \"{}\"\n{role}_candidate_payload_artifact_id = {retained}\n{role}_qualification_job_id = {job}\n",
            "c".repeat(64),
            "e".repeat(64)
        )?;
    }
    write!(
        &mut lock,
        "receipt_sha256 = \"{RECEIPT}\"\nbundle_sha256 = \"{}\"\ncandidate_source_artifact_id = 22\npublisher_run_attempt = 1\npublisher_job_id = 24\n",
        "d".repeat(64)
    )?;
    Ok(lock)
}

fn assert_declared_inputs_generate(
    prepared: &velnor_actions_orchestrator::GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    if prepared.config.checks.iter().any(|check| {
        check.selection == velnor_actions_contract::config::CheckSelection::DeclaredInputs
    }) {
        let output = tempfile::tempdir()?;
        let destination = output.path().join("preview");
        velnor_actions_orchestrator::generate(
            prepared,
            &velnor_actions_orchestrator::GenerateOptions {
                output_dir: Some(destination.clone()),
            },
        )?;
        assert!(destination.join(".github/workflows/ci.yml").is_file());
    }
    Ok(())
}

fn assert_preview_action_retired(root: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let old = root.join(".github/actions/velnor-preview-setup/action.yml");
    let custom = root.join(".github/actions/custom/action.yml");
    fs::create_dir_all(old.parent().ok_or("old action parent")?)?;
    fs::create_dir_all(custom.parent().ok_or("custom action parent")?)?;
    fs::write(
        &old,
        format!(
            "{}0.1.75; edit .velnor/config.toml and regenerate.\nname: old preview\n",
            velnor_actions_contract::MARKER_PREFIX
        ),
    )?;
    fs::write(&custom, b"name: custom\n")?;
    let prepared = prepare(root)?;
    velnor_actions_orchestrator::generate(&prepared, &GenerateOptions::default())?;
    assert!(!old.exists(), "obsolete generated preview action remains");
    assert_eq!(fs::read(&custom)?, b"name: custom\n");
    let workflow = fs::read_to_string(root.join(WORKFLOW_PATH))?;
    assert!(workflow.contains("Stage protected preview launcher"));
    assert!(!workflow.contains("uses: ./.github/actions/velnor-preview-setup"));

    fs::create_dir_all(old.parent().ok_or("old action parent")?)?;
    fs::write(&old, b"name: custom preview\n")?;
    let prepared = prepare(root)?;
    velnor_actions_orchestrator::generate(&prepared, &GenerateOptions::default())?;
    assert_eq!(fs::read(&old)?, b"name: custom preview\n");
    assert_eq!(fs::read(&custom)?, b"name: custom\n");
    Ok(())
}

fn assert_setup_script(setup: &Value) -> Result<(), Box<dyn Error>> {
    assert!(setup.get("uses").is_none());
    let script = setup["run"].as_str().ok_or("missing inline setup")?;
    assert!(script.len() <= 21_000);
    let helper_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../velnor-actions-workflow-renderer/src/native_helper");
    let integrity = Command::new("/usr/bin/python3")
        .args(["-I", "-c", "import base64,json,pathlib,sys,zlib; p=json.loads(zlib.decompress(base64.b64decode(sys.argv[1].split(chr(34))[1]))); d=pathlib.Path(sys.argv[2]); b=pathlib.Path(sys.argv[3]); assert p[:2] == [(d/'source_preview_setup.py').read_text(),b.read_text()]; identity=json.loads(bytes.fromhex(sys.argv[1].split(chr(34))[-2])); assert identity['channel']=='source-preview' and identity['source_sha']==sys.argv[4] and identity['receipt_sha256']==sys.argv[5]", script])
        .arg(helper_dir)
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../velnor-actions-mise/src/source_preview/source_preview_bootstrap.py"))
        .arg(SOURCE)
        .arg(RECEIPT)
        .status()?;
    assert!(
        integrity.success(),
        "inline launcher differs from protected source"
    );
    let candidate = tempfile::tempdir()?;
    let action = candidate
        .path()
        .join(".github/actions/velnor-preview-setup");
    fs::create_dir_all(&action)?;
    fs::write(
        action.join("action.yml"),
        "runs:\n  using: composite\n  steps:\n    - run: touch CANARY\n      shell: bash\n",
    )?;
    let temp = tempfile::tempdir()?;
    let result = Command::new("bash")
        .arg("-c")
        .arg(script)
        .current_dir(candidate.path())
        .env_clear()
        .env("RUNNER_TEMP", temp.path())
        .env("GH_TOKEN", "canary-token")
        .output()?;
    assert!(
        !result.status.success(),
        "missing GitHub identity must fail"
    );
    assert!(!candidate.path().join("CANARY").exists());
    let (before_last_quote, suffix) = script.rsplit_once('"').ok_or("missing identity")?;
    let (prefix, _) = before_last_quote
        .rsplit_once('"')
        .ok_or("missing identity")?;
    let malformed = format!("{prefix}\"00\"{suffix}");
    let temp = tempfile::tempdir()?;
    let result = Command::new("bash")
        .arg("-c")
        .arg(malformed)
        .current_dir(candidate.path())
        .env_clear()
        .env("RUNNER_TEMP", temp.path())
        .env("GH_TOKEN", "canary-token")
        .output()?;
    assert!(
        !result.status.success(),
        "malformed protected identity must fail"
    );
    assert!(!candidate.path().join("CANARY").exists());
    Ok(())
}

fn assert_executor_behavior(yaml: &Value) -> Result<(), Box<dyn Error>> {
    let steps = yaml["jobs"]["plan"]["steps"]
        .as_array()
        .ok_or("missing plan steps")?;
    let setup = steps
        .iter()
        .find(|step| step["name"] == "Stage protected preview launcher")
        .ok_or("missing preview setup")?;
    let script = setup["run"].as_str().ok_or("missing setup script")?;
    let directory = tempfile::tempdir()?;
    let executor = directory.path().join("source_preview_exec.py");
    let output = Command::new("/usr/bin/python3")
        .args(["-I", "-c", "import base64,json,pathlib,sys,zlib; p=json.loads(zlib.decompress(base64.b64decode(sys.argv[1].split(chr(34))[1]))); pathlib.Path(sys.argv[2]).write_text(p[2])", script])
        .arg(&executor)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(fs::read(&executor)?) {
        write!(digest, "{byte:02x}")?;
    }
    assert!(
        steps.iter().any(|step| step.to_string().contains(&digest)),
        "loader does not authenticate emitted executor"
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../velnor-actions-workflow-renderer");
    let keys = serde_json::to_string(
        &velnor_actions_contract::config::project_tool_resolver::PROJECT_TOOL_KEYS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
    )?;
    let mut scenarios = vec!["source_preview_exec_policy_scenarios.py"];
    if cfg!(target_os = "linux") {
        scenarios.push("source_preview_exec_scenarios.py");
    }
    for scenario in scenarios {
        let output = Command::new("python3")
            .arg("-B")
            .arg(root.join("tests").join(scenario))
            .arg(&root)
            .arg(&keys)
            .arg(&executor)
            .output()?;
        assert!(
            output.status.success(),
            "{scenario}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn assert_identity_steps(
    tree: &velnor_actions_workflow_renderer::render::RenderedTree,
    yaml: &Value,
) -> Result<(), Box<dyn Error>> {
    for id in [
        "plan",
        "required",
        "check-maintenance-helpers__hosted",
        "check-maintenance-helpers__local",
    ] {
        assert_job_identity(tree, yaml, id)?;
    }
    Ok(())
}

fn assert_job_identity(
    tree: &velnor_actions_workflow_renderer::render::RenderedTree,
    yaml: &Value,
    id: &str,
) -> Result<(), Box<dyn Error>> {
    let steps = yaml["jobs"][id]["steps"]
        .as_array()
        .ok_or_else(|| format!("{id}: missing steps"))?;
    let mut flattened = Vec::new();
    for step in steps {
        if step["uses"] == "./.github/actions/check-maintenance-helpers" {
            let action = tree
                .get(".github/actions/check-maintenance-helpers/action.yml")
                .ok_or("missing shared check action")?;
            let composite: Value = serde_saphyr::from_str(action)?;
            flattened.extend(
                composite["runs"]["steps"]
                    .as_array()
                    .ok_or("missing composite steps")?
                    .iter()
                    .cloned(),
            );
        } else {
            flattened.push(step.clone());
        }
    }
    let steps = &flattened;
    let setup_index = steps
        .iter()
        .position(|step| step["name"] == "Stage protected preview launcher")
        .ok_or_else(|| format!("{id}: missing protected setup: {steps:?}"))?;
    assert_setup_script(&steps[setup_index])?;
    let identity_index = steps
        .iter()
        .position(|step| {
            let text = step.to_string();
            [
                "source-preview",
                "0.1.75",
                "x86_64-unknown-linux-gnu",
                SOURCE,
                RECEIPT,
                &"c".repeat(64),
            ]
            .iter()
            .all(|value| text.contains(value))
        })
        .ok_or_else(|| format!("{id}: missing complete selected identity"))?;
    assert!(
        setup_index < identity_index,
        "{id}: identity used before setup"
    );
    Ok(())
}
