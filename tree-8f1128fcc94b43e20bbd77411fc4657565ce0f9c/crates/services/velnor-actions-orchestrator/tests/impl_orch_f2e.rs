//! F2 generate/consumer/qualify/authority acceptance.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as Json;
use tempfile::TempDir;
use velnor_actions_orchestrator::{
    GenerateOptions, GenerateReport, GenerationPreparation, PlanOutputMode, generate,
    plan_internal, plan_outputs, prepare, publish_plan_files,
};
use velnor_actions_orchestrator_core::OrchestratorError;

use crate::impl_common::{
    TestResult, config_with_branch, fixture_manifest_json, git, git_line, make_repo,
    plan_for_source_change, snapshot, without_ambient_identity,
};

/// Preview-generate `prep` into `dir`.
fn preview_into(
    prep: &GenerationPreparation,
    dir: PathBuf,
) -> Result<GenerateReport, OrchestratorError> {
    generate(
        prep,
        &GenerateOptions {
            output_dir: Some(dir),
        },
    )
}

/// Fixture accepted by the Velnor-repository identity check (local origin).
fn make_velnor_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let git_config = repo.path().join(".git/config");
    let mut text = fs::read_to_string(&git_config)?;
    text.push_str("[remote \"origin\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n");
    fs::write(&git_config, text)?;
    Ok(repo)
}

/// Raw `plan-v1` response for a two-commit source change.
fn plan_response_for_source_change() -> Result<(TempDir, String), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    crate::impl_common::git(&["add", "."], root)?;
    crate::impl_common::git(&["commit", "-m", "one"], root)?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    crate::impl_common::git(&["add", "."], root)?;
    crate::impl_common::git(&["commit", "-m", "two"], root)?;
    let base = crate::impl_common::git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = crate::impl_common::git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({"schema": 1, "run_key": "local", "base": base, "head": head, "event": "pull_request", "root": root.display().to_string()});
    Ok((repo, plan_internal(&request.to_string())?))
}

#[test]
fn plan_and_workflow_ids_agree() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.matrix.include.is_empty());
    for entry in &plan.matrix.include {
        assert_eq!(
            velnor_actions_contract::matrix_id_for_task_group("rust", &entry.task_id)?,
            entry.id,
            "entry id derives from task id"
        );
    }
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    assert_eq!(plan.runner.label, prep.runner_label, "runner parity");
    let parent = TempDir::new()?;
    let report = preview_into(&prep, parent.path().join("preview"))?;
    assert_eq!(
        report.files_written,
        [
            ".github/AGENTS.md",
            ".github/CLAUDE.md",
            ".github/actionlint.yaml",
            ".github/actions/velnor-tool-seed/action.yml",
            ".github/workflows/ci.yml"
        ]
    );
    Ok(())
}

#[test]
fn source_build_consumer_gate_registered() -> TestResult {
    use velnor_actions_orchestrator::consumer_acquire_step_with_manifest;
    let version = env!("CARGO_PKG_VERSION");
    let err =
        consumer_acquire_step_with_manifest("ubuntu-26.04", version, None).expect_err("None fails");
    let text = err.to_string();
    assert!(text.contains("consumer_requires_release_install"), "{text}");
    assert!(text.contains("official"), "{text}");
    let step = consumer_acquire_step_with_manifest(
        "ubuntu-26.04",
        version,
        Some(&fixture_manifest_json()),
    )?;
    assert_eq!(step.name, "Acquire Velnor");
    Ok(())
}

#[test]
fn qualify_argv_runs_artifact_only_registered() -> TestResult {
    let argv = velnor_actions_orchestrator_core::qualify_argv_staged()?;
    assert_eq!(argv[0], "sh");
    let script = argv.join(" ");
    assert!(
        script.contains("$RUNNER_TEMP/velnor/candidate/velnor-actions"),
        "{script}"
    );
    assert!(script.contains("plan"), "{script}");
    assert!(script.contains("generate"), "{script}");
    let lower = script.to_lowercase();
    for marker in ["cargo", "mbx", "rustc", "mise", "build"] {
        assert!(!lower.contains(marker), "rebuild marker {marker}: {script}");
    }
    Ok(())
}

#[test]
fn plan_json_matches_github_outputs() -> TestResult {
    let (_repo, response) = plan_response_for_source_change()?;
    let outputs = plan_outputs(&response, PlanOutputMode::Static)?;
    let velnor_dir = TempDir::new()?;
    let dir = publish_plan_files(&response, velnor_dir.path())?;
    assert_eq!(dir, velnor_dir.path().join("local"));
    assert_eq!(
        dir.join("plan.json"),
        velnor_actions_orchestrator_core::decisions::plan_json_path(velnor_dir.path(), "local")?
    );
    let plan_json = fs::read_to_string(dir.join("plan.json"))?;
    let matrix_json = fs::read_to_string(dir.join("matrix.json"))?;
    assert_eq!(
        matrix_json, outputs.matrix,
        "matrix.json agrees with GITHUB_OUTPUT"
    );
    let plan_value: Json = serde_json::from_str(&plan_json)?;
    let response_value: Json = serde_json::from_str(&response)?;
    let matrix_value: Json = serde_json::from_str(&matrix_json)?;
    assert_eq!(plan_value["matrix"], response_value["matrix"]);
    assert_eq!(plan_value["matrix"], matrix_value);
    Ok(())
}

#[test]
fn malformed_toolchain_recommends_without_writes() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("mise.toml"), "[tools]\n")?;
    fs::write(root.join("rust-toolchain.toml"), "[[[\n")?;
    let before = snapshot(root)?;
    let prep = prepare(root)?;
    let parent = TempDir::new()?;
    let report = preview_into(&prep, parent.path().join("preview"))?;
    let text = report.recommendations.join("\n");
    assert!(text.contains("tooling_input_invalid"), "{text}");
    assert!(text.contains("rust-toolchain.toml"), "{text}");
    assert!(text.contains("continues with its pinned tools"), "{text}");
    assert_eq!(report.files_written.len(), 5);
    for rel in ["mise.toml", "rust-toolchain.toml", ".velnor/config.toml"] {
        assert_eq!(
            before.get(rel).map(|(bytes, _)| bytes),
            snapshot(root)?.get(rel).map(|(bytes, _)| bytes),
            "{rel} untouched"
        );
    }
    Ok(())
}

#[test]
fn missing_lock_recommends_without_refresh() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let report = generate(&prep, &GenerateOptions { output_dir: None })?;
    let text = report.recommendations.join("\n");
    assert!(text.contains("mise.lock not found"), "{text}");
    assert!(text.contains("will not create or refresh it"), "{text}");
    assert!(
        !repo.path().join("mise.lock").exists(),
        "no implicit refresh"
    );
    Ok(())
}

#[test]
fn conflicting_tool_pins_recommend() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::write(
        repo.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.84.0\"\n",
    )?;
    fs::write(
        repo.path().join("mise.toml"),
        "[tools]\nrust = \"1.85.0\"\n",
    )?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let report = preview_into(&prep, parent.path().join("preview"))?;
    let text = report.recommendations.join("\n");
    assert!(text.contains("conflicting_tool_values"), "{text}");
    assert!(text.contains("1.84.0"), "{text}");
    Ok(())
}

#[test]
fn in_place_generate_preserves_config() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let config_before = fs::read(root.join(".velnor/config.toml"))?;
    let before = snapshot(root)?;
    let prep = prepare(root)?;
    generate(&prep, &GenerateOptions { output_dir: None })?;
    assert_eq!(fs::read(root.join(".velnor/config.toml"))?, config_before);
    let after = snapshot(root)?;
    for (rel, (bytes, _)) in &before {
        if rel.starts_with(".github") {
            continue;
        }
        assert_eq!(
            after.get(rel).map(|(got, _)| got),
            Some(bytes),
            "{rel} preserved"
        );
    }
    Ok(())
}

#[test]
fn authority_order_gates_pipeline() -> TestResult {
    let bad = make_repo("schema = \n")?;
    git(&["add", "."], bad.path())?;
    git(&["commit", "-m", "bad"], bad.path())?;
    let head = git_line(&["rev-parse", "HEAD"], bad.path())?;
    let request = serde_json::json!({"schema": 1, "run_key": "local", "base": None::<String>, "head": head, "event": "push", "root": bad.path().display().to_string()});
    let err = plan_internal(&request.to_string()).expect_err("bad config gates first");
    assert!(err.to_string().contains("config.toml"), "{err}");
    let missing = serde_json::json!({"schema": 1, "run_key": "local", "base": None::<String>, "head": head, "event": "push", "root": "/nonexistent-velnor-root-xyz"});
    assert!(
        plan_internal(&missing.to_string()).is_err(),
        "root gates before config"
    );
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let occupied = parent.path().join("preview");
    fs::create_dir_all(&occupied)?;
    fs::write(occupied.join("keep.txt"), "keep\n")?;
    let err = preview_into(&prep, occupied.clone()).expect_err("occupied preview refused");
    assert!(err.to_string().contains("preview_refused"), "{err}");
    assert_eq!(fs::read(occupied.join("keep.txt"))?, b"keep\n");
    Ok(())
}

#[test]
fn lint_freshness_and_advisory_run_every_generate() -> TestResult {
    use velnor_actions_workflow_jobs::closure::CHECK_GENERATED_NAME;
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    let report = preview_into(&prep, preview.clone())?;
    let yaml = fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
    assert!(yaml.contains("actionlint"), "lint job");
    assert!(yaml.contains(CHECK_GENERATED_NAME), "freshness gate");
    assert!(
        !report.recommendations.is_empty(),
        "advisory every generate"
    );
    Ok(())
}

#[test]
fn validator_commands_stay_velnor_only() -> TestResult {
    without_ambient_identity("validator_commands_stay_velnor_only", || {
        let velnor_config = "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n";
        let velnor = make_velnor_repo(velnor_config)?;
        let prep = prepare(velnor.path())?;
        let parent = TempDir::new()?;
        let preview = parent.path().join("preview");
        preview_into(&prep, preview.clone())?;
        let yaml = fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
        assert!(yaml.contains("cargo deny"), "velnor policy job");
        let consumer = make_repo(config_with_branch())?;
        let prep = prepare(consumer.path())?;
        let preview = parent.path().join("consumer");
        preview_into(&prep, preview.clone())?;
        let yaml = fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
        assert!(!yaml.contains("cargo deny"), "consumer has no policy job");
        Ok(())
    })
}

#[test]
fn workflow_steps_stay_sequential() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let first = parent.path().join("first");
    let second = parent.path().join("second");
    preview_into(&prep, first.clone())?;
    preview_into(&prep, second.clone())?;
    for rel in [".github/actionlint.yaml", ".github/workflows/ci.yml"] {
        assert_eq!(
            fs::read(first.join(rel))?,
            fs::read(second.join(rel))?,
            "{rel} deterministic"
        );
    }
    let yaml = fs::read_to_string(first.join(".github/workflows/ci.yml"))?;
    for line in yaml.lines() {
        if line.contains("run:") {
            assert!(!has_bare_ampersand(line), "no background step: {line}");
        }
    }
    Ok(())
}

/// True when `line` holds a `&` outside a `&&` chain (background marker).
fn has_bare_ampersand(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        *byte == b'&'
            && bytes.get(index.wrapping_sub(1)) != Some(&b'&')
            && bytes.get(index + 1) != Some(&b'&')
    })
}

#[test]
fn committed_workflow_carries_generator_gates() -> TestResult {
    use velnor_actions_workflow_jobs::closure::CHECK_GENERATED_NAME;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()?;
    for rel in [".github/workflows/ci.yml", ".github/actionlint.yaml"] {
        let text = fs::read_to_string(root.join(rel))?;
        assert!(
            text.starts_with("# Generated by Velnor Actions "),
            "{rel} carries the generator marker"
        );
    }
    let yaml = fs::read_to_string(root.join(".github/workflows/ci.yml"))?;
    assert!(yaml.contains("actionlint"), "committed lint job");
    assert!(
        yaml.contains(CHECK_GENERATED_NAME),
        "committed freshness gate"
    );
    Ok(())
}

#[test]
fn plan_files_publish_locally_only() -> TestResult {
    let (_repo, response) = plan_response_for_source_change()?;
    let velnor_dir = TempDir::new()?;
    let dir = publish_plan_files(&response, velnor_dir.path())?;
    let mut files = Vec::new();
    let mut pending = vec![velnor_dir.path().to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path.strip_prefix(velnor_dir.path())?.to_path_buf());
            }
        }
    }
    files.sort();
    assert_eq!(
        files,
        [
            Path::new("local/matrix.json").to_path_buf(),
            Path::new("local/plan.json").to_path_buf()
        ]
    );
    assert_eq!(dir, velnor_dir.path().join("local"));
    Ok(())
}
