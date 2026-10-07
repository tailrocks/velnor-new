//! Intake remediation cases: discovery, registry order, tool inputs, config.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_planning::DetectionStatus;
use velnor_actions_mise::{GitRequest, is_allowed_git_verb};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};
use velnor_actions_orchestrator_core::{OrchestratorError, init_config, resolve_root};

use crate::support::{
    TestResult, config_with_branch, err_of, make_repo, plan_for, snapshot, write_nextest_task,
};

/// Selected detection manifests in discovery order.
fn selected_manifests(statuses: &[DetectionStatus]) -> Vec<String> {
    statuses
        .iter()
        .filter_map(|status| match status {
            DetectionStatus::Selected(project) => Some(project.manifest.clone()),
            DetectionStatus::Ignored { .. } => None,
        })
        .collect()
}

/// Derived task ids from a fresh preparation of `root`.
fn task_ids_for(root: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let prep = prepare(root)?;
    Ok(prep
        .discovery
        .proposals
        .iter()
        .map(|task| task.task_id.clone())
        .collect())
}

/// Add an independent `sub` crate to the fixture.
fn add_sub_crate(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(root.join("sub/src"))?;
    fs::write(
        root.join("sub/Cargo.toml"),
        "[package]\nname = \"sub\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("sub/src/lib.rs"), "pub fn g() {}\n")?;
    Ok(())
}

#[test]
fn intake_tool_inputs_untouched_and_reported() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("mise.toml"), "[tools]\n")?;
    fs::write(root.join("rust-toolchain.toml"), "[toolchain]\n")?;
    fs::write(root.join("mise.lock"), "{}\n")?;
    let before = snapshot(root)?;
    let prep = prepare(root)?;
    let text = plan_for(&prep)?;
    for fragment in [
        "mise.toml is read-only input",
        "mise.lock is read-only input",
        "rust-toolchain.toml is read-only input",
    ] {
        assert!(text.contains(fragment), "missing {fragment}:\n{text}");
    }
    let report = generate(&prep, &GenerateOptions { output_dir: None })?;
    assert_eq!(report.recommendations, prep.discovery.recommendations);
    let after = snapshot(root)?;
    for tool in ["mise.toml", "mise.lock", "rust-toolchain.toml"] {
        assert_eq!(before[tool], after[tool]);
    }
    assert!(
        !root.join(".mise-version").exists(),
        "never create .mise-version"
    );
    Ok(())
}

#[test]
fn intake_consumer_default_ignores_velnor_files() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join(".velnor/version-policy.toml"), "garbage [[[\n")?;
    fs::write(root.join(".velnor/generator.lock"), "garbage {{{\n")?;
    let prep = prepare(root)?;
    assert!(matches!(
        prep.config.workflow.policy,
        WorkflowPolicy::ConsumerV1
    ));
    assert!(!prep.discovery.proposals.is_empty());
    generate(&prep, &GenerateOptions { output_dir: None })?;
    Ok(())
}

#[test]
fn intake_malformed_manifest_fails_selected_or_ignored() -> TestResult {
    for config in [
        config_with_branch(),
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks]\nignore = [\"rust\"]\n",
    ] {
        let repo = make_repo(config)?;
        fs::write(
            repo.path().join("Cargo.toml"),
            "[package\nname = \"demo\"\n",
        )?;
        let err = err_of(prepare(repo.path()), "malformed manifest fails")?;
        assert!(
            matches!(err, OrchestratorError::Detection { .. }),
            "got {err}"
        );
        assert!(
            err.to_string().contains("malformed_manifest:Cargo.toml"),
            "got {err}"
        );
    }
    let incomplete = OrchestratorError::PreparationIncomplete {
        manifest: "Cargo.toml".to_owned(),
        problem: "tooling_failed".to_owned(),
    };
    assert_eq!(
        incomplete.to_string(),
        "preparation_incomplete: Cargo.toml: tooling_failed"
    );
    assert!(!is_allowed_git_verb("fetch"), "fetch must stay rejected");
    assert!(
        GitRequest::new("fetch", Vec::new()).is_err(),
        "fetch rejected"
    );
    Ok(())
}

#[test]
fn intake_tool_changes_refresh_findings_not_tasks() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let tasks_before = task_ids_for(root)?;
    let recs_before = prepare(root)?.discovery.recommendations;
    fs::write(root.join("mise.toml"), "[tools]\nrust = \"1.0.0\"\n")?;
    fs::write(root.join("rust-toolchain.toml"), "[toolchain]\n")?;
    fs::write(root.join("mise.lock"), "{\"version\":1}\n")?;
    assert_eq!(task_ids_for(root)?, tasks_before);
    let recs_after = prepare(root)?.discovery.recommendations;
    assert_ne!(recs_after, recs_before);
    fs::write(root.join("mise.toml"), "[tools]\nrust = \"1.2.3\"\n")?;
    assert_eq!(task_ids_for(root)?, tasks_before);
    fs::write(root.join("mise.toml"), "!!! not toml {{{ \n")?;
    fs::write(root.join("rust-toolchain.toml"), "[[[ nope\n")?;
    fs::write(root.join("mise.lock"), "not json {{{\n")?;
    let err = err_of(prepare(root).map(|_| ()), "garbage wrapper")?;
    assert!(
        err.to_string().contains("wrapper_invalid"),
        "diagnostic: {err}"
    );
    fs::remove_file(root.join("mise.toml"))?;
    fs::remove_file(root.join("rust-toolchain.toml"))?;
    fs::remove_file(root.join("mise.lock"))?;
    assert_eq!(prepare(root)?.discovery.recommendations, recs_before);
    Ok(())
}

#[test]
fn intake_child_dir_invocation_writes_only_under_root() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let child = root.join("child");
    fs::create_dir_all(&child)?;
    fs::remove_file(root.join(".velnor/config.toml"))?;
    let before = snapshot(&child)?;
    let resolved = resolve_root(&child)?;
    assert_eq!(resolved, root.canonicalize()?);
    init_config(&resolved)?;
    assert_eq!(before, snapshot(&child)?, "init must not write into child");
    fs::write(root.join(".velnor/config.toml"), config_with_branch())?;
    let prep = prepare(root)?;
    generate(&prep, &GenerateOptions { output_dir: None })?;
    assert_eq!(
        before,
        snapshot(&child)?,
        "generate must not write into child"
    );
    Ok(())
}

#[test]
fn intake_not_work_tree_records_calling_cwd() -> TestResult {
    let bare = TempDir::new()?;
    let err = err_of(resolve_root(bare.path()), "outside work tree")?;
    assert!(
        matches!(err, OrchestratorError::NotWorkTree { .. }),
        "got {err}"
    );
    let message = err.to_string();
    assert!(message.contains("not_inside_work_tree"), "got {message}");
    let cwd = bare.path().display().to_string();
    assert!(message.contains(cwd.as_str()), "cwd recorded: {message}");
    let repo = make_repo(config_with_branch())?;
    assert!(resolve_root(repo.path())?.is_absolute());
    Ok(())
}

#[test]
fn intake_symlinked_cwd_resolves_to_canonical_root() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let nested = root.join("a/b");
    fs::create_dir_all(&nested)?;
    let via = root.join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&nested, &via)?;
    #[cfg(not(unix))]
    fs::create_dir_all(&via)?;
    assert_eq!(resolve_root(&via)?, root.canonicalize()?);
    Ok(())
}

#[test]
fn intake_task_ids_are_valid_grammar_and_sorted() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let mut ids: Vec<&str> = Vec::new();
    for task in &prep.discovery.proposals {
        velnor_actions_contract::validate_task_id(&task.task_id)?;
        ids.push(task.task_id.as_str());
    }
    assert!(!ids.is_empty(), "selected detection proposes tasks");
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "task ids ascending");
    Ok(())
}

#[test]
fn intake_unconfigured_suites_are_not_sharded() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    for id in task_ids_for(root)? {
        assert!(!id.contains("/shard-"), "default must not shard: {id}");
    }
    fs::write(
        root.join(".velnor/config.toml"),
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[test_sharding]\ndefault_shards = 2\n",
    )?;
    let err = err_of(prepare(root), "cargo_test shards")?.to_string();
    assert!(err.contains("cargo_test_single_obligation"), "{err}");
    write_nextest_task(root)?;
    let ids = task_ids_for(root)?;
    assert!(ids.join(" ").contains("/shard-"), "explicit shards expand");
    for id in &ids {
        velnor_actions_contract::validate_task_id(id)?;
    }
    Ok(())
}

#[test]
fn intake_generated_workflow_uses_crate_jobs_not_native_parallel() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    generate(&prep, &GenerateOptions { output_dir: None })?;
    let yml = fs::read_to_string(repo.path().join(".github/workflows/ci.yml"))?;
    assert!(yml.contains("  rust-demo:"), "crate job:\n{yml}");
    assert!(!yml.contains("strategy:"), "static graph:\n{yml}");
    for line in yml.lines() {
        let stepped = line.trim_start();
        assert!(
            !stepped.starts_with("parallel:") && !stepped.starts_with("background:"),
            "native syntax: {line}"
        );
    }
    Ok(())
}

#[test]
fn intake_detection_order_is_deterministic() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    add_sub_crate(root)?;
    let first = selected_manifests(&prepare(root)?.discovery.statuses);
    let second = selected_manifests(&prepare(root)?.discovery.statuses);
    assert_eq!(first, vec!["Cargo.toml", "sub/Cargo.toml"]);
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn intake_exclusions_apply_before_detection() -> TestResult {
    let repo = make_repo(
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[discovery]\nexclude = [\"sub/**\"]\n",
    )?;
    let root = repo.path();
    add_sub_crate(root)?;
    let prep = prepare(root)?;
    assert_eq!(
        selected_manifests(&prep.discovery.statuses),
        vec!["Cargo.toml"]
    );
    assert!(
        prep.discovery
            .proposals
            .iter()
            .all(|task| task.identity.unit_key == "root")
    );
    Ok(())
}

#[test]
fn intake_default_budgets_are_conservative_and_reported() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    assert_eq!(prep.config.workflow.max_parallel_jobs, 2);
    assert_eq!(prep.config.resources.compiler_process_budget, 2);
    assert_eq!(prep.config.resources.test_process_budget, 2);
    let text = plan_for(&prep)?;
    assert!(text.contains("Parallel:"), "budgets reported:\n{text}");
    Ok(())
}

#[test]
fn intake_detection_without_tool_files() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    assert!(!prepare(repo.path())?.discovery.proposals.is_empty());
    Ok(())
}

#[test]
fn intake_missing_tool_files_recommend_without_writes() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    let text = plan_for(&prep)?;
    for fragment in [
        "mise.toml not found",
        "mise.lock not found",
        "rust-toolchain.toml not found",
    ] {
        assert!(text.contains(fragment), "missing {fragment}:\n{text}");
    }
    generate(&prep, &GenerateOptions { output_dir: None })?;
    for tool in ["mise.toml", "mise.lock", "rust-toolchain.toml"] {
        assert!(!root.join(tool).exists(), "{tool} must never be created");
    }
    Ok(())
}

#[test]
fn intake_selected_detection_contributes_tasks() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    add_sub_crate(root)?;
    let prep = prepare(root)?;
    let mut selected = selected_manifests(&prep.discovery.statuses);
    assert!(!selected.is_empty(), "detections selected");
    selected.sort();
    let mut contributed: Vec<String> = prep
        .discovery
        .proposals
        .iter()
        .map(|task| match task.identity.unit_key.as_str() {
            "root" => "Cargo.toml".to_owned(),
            key => format!("{key}/Cargo.toml"),
        })
        .collect();
    contributed.sort();
    contributed.dedup();
    assert_eq!(contributed, selected);
    Ok(())
}
