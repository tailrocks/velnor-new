//! P06 detection cases: wrappers, Nextest config, overrides, combos.
//!
//! Owned by the P06 detection builder; wired into `velnor_orchestrator` by
//! the parent with one `mod` line. Uses `crate::impl_common` fixtures.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use velnor_actions_contract::Plan;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{
    GenerateOptions, GenerationPreparation, generate, plan_internal, prepare,
};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, plan_for,
};

/// Repository wrapper line: inline-table spelling with the shim env.
const WRAPPER_INLINE: &str =
    "wrappers = { cargo = { command = \"mbx\", env = { MBX_CARGO_SHIM_MODE = \"1\" } } }\n";

/// Nextest config with `[profile.ci]`.
const NEXTEST_CI: &str = "[profile.ci]\nretries = 0\n";

/// Nextest config without `[profile.ci]`.
const NEXTEST_OTHER: &str = "[profile.linux]\nretries = 1\n";

/// Write `content` to `relative` under `root`, creating parents.
fn write_file(root: &Path, relative: &str, content: &str) -> TestResult {
    let target = root.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, content)?;
    Ok(())
}

/// Write an executable task with `content` at `.mise/tasks/<name>`.
fn write_task(root: &Path, name: &str, content: &str) -> TestResult {
    let task = root.join(".mise/tasks").join(name);
    fs::create_dir_all(task.parent().ok_or("task parent")?)?;
    fs::write(&task, content)?;
    #[cfg(unix)]
    fs::set_permissions(&task, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// Plan after two commits touching crate sources (mirrors intake helper).
fn plan_after_commits(root: &Path) -> Result<Plan, Box<dyn std::error::Error>> {
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let lib = fs::read_to_string(root.join("src/lib.rs"))?;
    fs::write(root.join("src/lib.rs"), format!("{lib}pub fn g() {{}}\n"))?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    Ok(plan)
}

/// `run` string of the first entry whose task kind is `kind`.
fn entry_run<'a>(plan: &'a Plan, kind: &str) -> Result<&'a str, std::io::Error> {
    plan.matrix
        .include
        .iter()
        .find(|entry| entry.task_id.split('/').nth(3) == Some(kind))
        .map(|entry| entry.run.as_str())
        .ok_or_else(|| std::io::Error::other(format!("missing {kind} entry")))
}

#[test]
fn wrapper_only_detects_mbx() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_file(root, "mise.toml", WRAPPER_INLINE)?;
    let prep = prepare(root)?;
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(workspace.profile.compile_driver.as_str(), "mbx");
    assert_eq!(workspace.profile.driver_source.as_str(), "detected");
    assert_eq!(workspace.profile.test_runner.as_str(), "cargo_test");
    assert_eq!(workspace.profile.evidence.len(), 1);
    let sighting = &workspace.profile.evidence[0];
    assert_eq!(sighting.path, "mise.toml");
    assert_eq!(sighting.line, 1);
    assert!(
        sighting
            .command_or_setting
            .contains("wrappers.cargo.command")
    );
    assert!(workspace.findings.is_empty());
    let plan = plan_for(&prep)?;
    assert!(plan.contains("mbx compile driver (detected)"), "{plan}");
    Ok(())
}

#[test]
fn dot_mise_toml_labeled_correctly() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_file(root, ".mise.toml", "[tools.rust]\nmr_boxington = true\n")?;
    let prep = prepare(root)?;
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(workspace.profile.compile_driver.as_str(), "mbx");
    assert_eq!(workspace.profile.evidence.len(), 1);
    assert_eq!(workspace.profile.evidence[0].path, ".mise.toml");
    Ok(())
}

#[test]
fn nextest_config_selects_ci_or_default() -> TestResult {
    for (content, profile) in [(NEXTEST_CI, "ci"), (NEXTEST_OTHER, "default")] {
        let repo = make_repo(config_with_branch())?;
        let root = repo.path();
        write_file(root, ".config/nextest.toml", content)?;
        let prep = prepare(root)?;
        let workspace = &prep.discovery.workspaces[0];
        assert_eq!(workspace.profile.test_runner.as_str(), "cargo_nextest");
        assert_eq!(workspace.profile.runner_source.as_str(), "detected");
        assert_eq!(workspace.profile.nextest_profile.as_str(), profile);
        assert_eq!(
            workspace.profile.nextest_config.as_deref(),
            Some(".config/nextest.toml")
        );
        assert_eq!(workspace.profile.compile_driver.as_str(), "cargo");
        assert!(workspace.findings.is_empty());
        let plan = plan_for(&prep)?;
        let line = format!("Nextest profile .: {profile} (.config/nextest.toml)");
        assert!(plan.contains(&line), "{plan}");
    }
    Ok(())
}

#[test]
fn single_axis_override_without_duplication() -> TestResult {
    let repo = make_repo(
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks.rust]\ncompile_driver = \"mbx\"\n",
    )?;
    let root = repo.path();
    write_file(root, ".config/nextest.toml", NEXTEST_CI)?;
    let prep = prepare(root)?;
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(workspace.profile.compile_driver.as_str(), "mbx");
    assert_eq!(workspace.profile.driver_source.as_str(), "declared");
    assert_eq!(workspace.profile.test_runner.as_str(), "cargo_nextest");
    assert_eq!(workspace.profile.runner_source.as_str(), "detected");

    let repo = make_repo(
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks.rust]\ntest_runner = \"cargo_test\"\n",
    )?;
    let root = repo.path();
    write_file(root, "mise.toml", WRAPPER_INLINE)?;
    let prep = prepare(root)?;
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(workspace.profile.compile_driver.as_str(), "mbx");
    assert_eq!(workspace.profile.driver_source.as_str(), "detected");
    assert_eq!(workspace.profile.test_runner.as_str(), "cargo_test");
    assert_eq!(workspace.profile.runner_source.as_str(), "declared");
    Ok(())
}

#[test]
fn no_evidence_defaults_to_cargo() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(workspace.profile.compile_driver.as_str(), "cargo");
    assert_eq!(workspace.profile.test_runner.as_str(), "cargo_test");
    assert_eq!(workspace.profile.nextest_profile.as_str(), "default");
    assert_eq!(workspace.profile.nextest_config, None);
    assert!(workspace.profile.evidence.is_empty());
    let text = prep.discovery.recommendations.join("\n");
    assert!(text.contains("nextest_recommendation"), "{text}");
    assert!(text.contains("persist_evidence"), "{text}");
    Ok(())
}

#[test]
fn root_pair_wrapper_conflict_fails() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_file(root, "mise.toml", WRAPPER_INLINE)?;
    write_file(
        root,
        ".mise.toml",
        "[wrappers.cargo]\ncommand = \"sccache\"\n",
    )?;
    let err = err_of(prepare(root).map(|_| ()), "wrapper conflict")?;
    assert!(
        err.to_string().contains("ambiguous_compile_driver"),
        "conflict: {err}"
    );
    Ok(())
}

#[test]
fn nested_workspace_detects_own_wrapper_and_config() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_file(root, "mise.toml", "[tools]\nrust = \"1.98.1\"\n")?;
    write_file(
        root,
        "sub/Cargo.toml",
        "[package]\nname = \"sub\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    write_file(root, "sub/src/lib.rs", "pub fn f() {}\n")?;
    write_file(root, "sub/mise.toml", WRAPPER_INLINE)?;
    write_file(root, "sub/.config/nextest.toml", NEXTEST_CI)?;
    let prep = prepare(root)?;
    assert_eq!(prep.discovery.workspaces.len(), 2);
    let nested = prep
        .discovery
        .workspaces
        .iter()
        .find(|workspace| workspace.record.workspace_root == "sub")
        .ok_or("missing nested workspace")?;
    assert_eq!(nested.profile.compile_driver.as_str(), "mbx");
    assert_eq!(nested.profile.test_runner.as_str(), "cargo_nextest");
    assert_eq!(nested.profile.nextest_profile.as_str(), "ci");
    assert_eq!(
        nested.profile.nextest_config.as_deref(),
        Some("sub/.config/nextest.toml")
    );
    let outer = prep
        .discovery
        .workspaces
        .iter()
        .find(|workspace| workspace.record.workspace_root.is_empty())
        .ok_or("missing root workspace")?;
    assert_eq!(outer.profile.compile_driver.as_str(), "cargo");
    assert_eq!(outer.profile.test_runner.as_str(), "cargo_test");
    assert_eq!(outer.profile.nextest_config, None);
    Ok(())
}

#[test]
fn misleading_names_and_comments_not_evidence() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_file(
        root,
        "README.md",
        "# Demo\nRun `mbx test` or `cargo nextest run`.\n",
    )?;
    write_file(
        root,
        "docs/plan.md",
        "Consider wrappers.cargo.command = \"mbx\".\n",
    )?;
    write_file(
        root,
        "mise.toml",
        "# wrappers.cargo.command = \"mbx\"\n[tools]\nrust = \"1.98.1\"\n",
    )?;
    write_task(root, "nextest", "#!/bin/sh\necho hi\n")?;
    write_task(root, "mbx-test", "#!/bin/sh\necho hi\n")?;
    let prep = prepare(root)?;
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(workspace.profile.compile_driver.as_str(), "cargo");
    assert_eq!(workspace.profile.test_runner.as_str(), "cargo_test");
    assert!(
        workspace.profile.evidence.is_empty(),
        "names/comments are not evidence: {:?}",
        workspace.profile.evidence
    );
    assert!(workspace.findings.is_empty());
    Ok(())
}

#[test]
fn repeated_generation_stable() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_file(root, "mise.toml", WRAPPER_INLINE)?;
    write_file(root, ".config/nextest.toml", NEXTEST_CI)?;
    let before = plan_for(&prepare(root)?)?;
    assert!(before.contains("mbx compile driver (detected)"), "{before}");
    let parent = tempfile::TempDir::new()?;
    let first = preview_bytes(&prepare(root)?, &parent, "one")?;
    let second = preview_bytes(&prepare(root)?, &parent, "two")?;
    assert_eq!(first, second, "generate twice byte-identical");
    generate(&prepare(root)?, &GenerateOptions::default())?;
    assert_eq!(plan_for(&prepare(root)?)?, before, "plan stable");
    Ok(())
}

/// Preview-generate `prep` into `parent/name`; return both trees' bytes.
fn preview_bytes(
    prep: &GenerationPreparation,
    parent: &tempfile::TempDir,
    name: &str,
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let preview = parent.path().join(name);
    generate(
        prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    let workflow = fs::read(preview.join(".github/workflows/ci.yml"))?;
    let actionlint = fs::read(preview.join(".github/actionlint.yaml"))?;
    Ok((workflow, actionlint))
}

#[test]
fn four_combos_argv() -> TestResult {
    let catalog = ToolCatalog::pinned();
    let rust = catalog.tool_spec(PinnedTool::Rust);
    let mbx = catalog.tool_spec(PinnedTool::MrBoxington);
    let nextest = catalog.tool_spec(PinnedTool::Nextest);
    for (use_mbx, use_nextest) in [(false, false), (false, true), (true, false), (true, true)] {
        let repo = make_repo(config_with_branch())?;
        let root = repo.path();
        if use_mbx {
            write_file(root, "mise.toml", WRAPPER_INLINE)?;
        }
        if use_nextest {
            write_file(root, ".config/nextest.toml", NEXTEST_CI)?;
        }
        let prep = prepare(root)?;
        let workspace = &prep.discovery.workspaces[0];
        let driver = if use_mbx { "mbx" } else { "cargo" };
        let runner = if use_nextest {
            "cargo_nextest"
        } else {
            "cargo_test"
        };
        assert_eq!(workspace.profile.compile_driver.as_str(), driver);
        assert_eq!(workspace.profile.test_runner.as_str(), runner);
        let profile = workspace.profile.nextest_profile.as_str();
        assert_eq!(profile, if use_nextest { "ci" } else { "default" });
        let config = workspace.profile.nextest_config.as_deref();
        assert_eq!(config, use_nextest.then_some(".config/nextest.toml"));
        let plan = plan_after_commits(root)?;
        let kind = if use_nextest { "nextest" } else { "test" };
        let run = entry_run(&plan, kind)?.to_owned();
        assert!(run.starts_with("mise "), "{run}");
        assert!(run.contains(&rust), "{run}");
        assert_eq!(run.contains(&mbx), use_mbx, "{run}");
        assert_eq!(run.contains(&nextest), use_nextest, "{run}");
        let program = if use_mbx { "-- mbx " } else { "-- cargo " };
        assert!(run.contains(program), "{run}");
        let payload = ["test --locked", "nextest run"][usize::from(use_nextest)];
        assert!(run.contains(payload), "{run}");
        assert!(run.contains("--locked"), "{run}");
        assert!(!run.contains("--profile") || use_nextest, "{run}");
        let doctest = entry_run(&plan, "doctest")?.to_owned();
        assert!(doctest.contains("--doc"), "{doctest}");
        assert!(doctest.contains(program), "{doctest}");
        assert!(!doctest.contains("--profile"), "{doctest}");
    }
    Ok(())
}
