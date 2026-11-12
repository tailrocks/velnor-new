// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;

#[path = "maintenance_input_validation.rs"]
mod validation;

use super::scoped_workspace::{
    A5_COMPLETE_RULE, PACKAGES, PlanRun, TestResult, commit, plan, put, scoped_workspace, set,
};

const INVENTORY_TEST: &str =
    "crates/velnor-archive-guard/tests/golden_collector_public/tree_inventory.rs";
const PLANNER_TEST: &str = "crates/velnor-actions-orchestrator/tests/workflow_configuration_public/maintenance_check_inputs.rs";
const ORACLE_SELECTION_TEST: &str = "crates/velnor-actions-orchestrator/tests/workflow_configuration_public/oracle_input_selection.rs";
const VALIDATION_TEST: &str = "crates/velnor-actions-orchestrator/tests/workflow_configuration_public/maintenance_input_validation.rs";
const CARGO_ARTIFACT_KIND: &str = "crates/velnor-actions-freshness/src/cargo_artifact_kind.rs";
const REPO_VERIFICATION_DOC: &str = "docs/implemented/ci-maintenance/repo-verification.md";
const RUST_QUALITY_CONTRACT: &str = "docs/proposed/ci-maintenance/rust-quality-contract.md";
const BUN_CONTRACT: &str = "docs/proposed/bun-contract.md";
const DOCUMENTATION_NON_INPUT: &str = "docs/**";
const FIXTURE_CHECK_SOURCE: &str = "libs/core/src/lib.rs";

#[test]
fn documentation_outside_maintenance_is_unaffected_and_absent_from_rust_checkout() -> TestResult {
    let root = repository_root()?;
    let inputs = maintenance_inputs(&root)?;
    let non_inputs = maintenance_non_inputs(&root)?;
    for path in [
        "docs/reviews/ci-performance.md",
        "docs/implemented/named-mise-checks.md",
    ] {
        assert_planner_skips_documentation(path, "docs/**", &inputs, &non_inputs)?;
    }
    for operation in ["rename", "delete"] {
        let repo = maintenance_fixture(&inputs, &non_inputs)?;
        let path = "docs/reviews/selection.md";
        put(repo.root(), path, "before\n")?;
        let base = commit(repo.root(), "base documentation")?;
        if operation == "rename" {
            std::fs::rename(
                repo.root().join(path),
                repo.root().join("docs/reviews/renamed.md"),
            )?;
        } else {
            std::fs::remove_file(repo.root().join(path))?;
        }
        let head = commit(repo.root(), "change documentation")?;
        let run = plan(repo.root(), "pull_request", Some(&base), &head)?;
        assert!(run.with_disposition("run")?.is_empty(), "{operation}");
    }
    Ok(())
}

#[test]
fn maintenance_check_inputs_cover_sources_and_schedule_on_exact_changes() -> TestResult {
    let root = repository_root()?;
    let inputs = maintenance_inputs(&root)?;
    let non_inputs = maintenance_non_inputs(&root)?;
    assert!(inputs.contains(INVENTORY_TEST));
    assert!(inputs.contains(PLANNER_TEST));
    assert!(inputs.contains(VALIDATION_TEST));
    assert!(inputs.contains(ORACLE_SELECTION_TEST));
    assert!(inputs.contains(CARGO_ARTIFACT_KIND));
    assert!(inputs.contains(".mise-version"));
    assert!(inputs.contains("Cargo.lock"));
    assert!(inputs.contains("mise.toml"));
    assert!(!inputs.contains(REPO_VERIFICATION_DOC));
    assert!(!inputs.contains(RUST_QUALITY_CONTRACT));
    assert!(!inputs.contains(BUN_CONTRACT));
    assert_eq!(
        non_inputs,
        BTreeSet::from([DOCUMENTATION_NON_INPUT.to_owned()])
    );
    validation::assert_dedicated_document_directories(&root)?;
    validation::assert_source_closure(&root, &inputs)?;
    validation::assert_metadata_input_closures(&root)?;

    for (path, glob) in [
        (REPO_VERIFICATION_DOC, DOCUMENTATION_NON_INPUT),
        (RUST_QUALITY_CONTRACT, DOCUMENTATION_NON_INPUT),
        (BUN_CONTRACT, DOCUMENTATION_NON_INPUT),
    ] {
        assert_planner_skips_documentation(path, glob, &inputs, &non_inputs)?;
    }
    assert_local_documentation_keeps_named_check(&inputs, &non_inputs)?;
    for path in [FIXTURE_CHECK_SOURCE, "libs/util/src/lib.rs"] {
        assert_planner_selects_check_input(path, &inputs, &non_inputs)?;
    }
    for path in ["mise.toml", ".mise-version", "Cargo.lock"] {
        assert_planner_selects_check_input(path, &inputs, &non_inputs)?;
    }
    assert_planner_selects_check_input("mise.lock", &inputs, &non_inputs)?;
    assert_planner_selects_check_input(".velnor/config.toml", &inputs, &non_inputs)?;
    assert_declared_tool_change_selects_all_rust(&inputs, &non_inputs)?;
    assert_empty_diff_skips_check(&inputs, &non_inputs)?;
    for event in ["push", "merge_group", "qualification"] {
        assert_full_event_selects_check(event, &inputs, &non_inputs)?;
    }
    assert_missing_base_selects_check(&inputs, &non_inputs)?;
    Ok(())
}

fn repository_root() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?)
}

fn maintenance_inputs(root: &Path) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(root.join(".velnor/config.toml"))?;
    let config: toml::Value = toml::from_str(&source)?;
    let checks = config["checks"].as_array().ok_or("missing checks")?;
    let maintenance = checks
        .iter()
        .find(|check| check["id"].as_str() == Some("maintenance-helpers"))
        .ok_or("missing maintenance-helpers check")?;
    let inputs = maintenance["inputs"]
        .as_array()
        .ok_or("missing check inputs")?;
    inputs
        .iter()
        .map(|input| {
            input
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "maintenance input is not a string".into())
        })
        .collect()
}

fn maintenance_non_inputs(root: &Path) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(root.join(".velnor/config.toml"))?;
    let config: toml::Value = toml::from_str(&source)?;
    config["stacks"]["rust"]["non_inputs"]
        .as_array()
        .ok_or("missing Rust non_inputs")?
        .iter()
        .map(|glob| {
            glob.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "Rust non_input is not a string".into())
        })
        .collect()
}

fn assert_planner_skips_documentation(
    path: &str,
    glob: &str,
    inputs: &BTreeSet<String>,
    non_inputs: &BTreeSet<String>,
) -> TestResult {
    let repo = maintenance_fixture(inputs, non_inputs)?;
    let base = commit(repo.root(), "base documentation state")?;
    put(repo.root(), path, "changed\n")?;
    let head = commit(repo.root(), "change documentation")?;
    let run = plan(repo.root(), "pull_request", Some(&base), &head)?;
    let jobs = run.jobs()?;
    assert_eq!(
        jobs.get("check-maintenance-helpers").map(String::as_str),
        Some("not_affected"),
        "documentation changes must not schedule maintenance helpers"
    );
    assert!(
        run.with_disposition("run")?.is_empty(),
        "documentation changes must not schedule Rust jobs"
    );
    assert!(
        run.value["plan"]["matrix"]["include"]
            .as_array()
            .ok_or("missing matrix entries")?
            .is_empty(),
        "documentation changes must not create executable matrix entries"
    );
    validation::assert_documentation_checkout(repo.root(), path, glob, FIXTURE_CHECK_SOURCE)?;
    Ok(())
}

fn assert_planner_selects_check_input(
    path: &str,
    inputs: &BTreeSet<String>,
    non_inputs: &BTreeSet<String>,
) -> TestResult {
    let repo = maintenance_fixture(inputs, non_inputs)?;
    let original = match path {
        "mise.toml" => fixture_mise_config(),
        FIXTURE_CHECK_SOURCE | "libs/util/src/lib.rs" => "pub fn f() {}\n".to_owned(),
        ".mise-version" => "2026.1.0\n".to_owned(),
        "Cargo.lock" | ".velnor/config.toml" => std::fs::read_to_string(repo.root().join(path))?,
        "mise.lock" => std::fs::read_to_string(repository_root()?.join(path))?,
        _ => return Err(format!("unexpected check control path: {path}").into()),
    };
    put(repo.root(), path, &original)?;
    if path != "mise.toml" && path != FIXTURE_CHECK_SOURCE && path != "libs/util/src/lib.rs" {
        let base = commit(repo.root(), "base check input state")?;
        put(repo.root(), path, &format!("{original}\n"))?;
        let head = commit(repo.root(), "change maintenance input")?;
        let run = plan(repo.root(), "pull_request", Some(&base), &head)?;
        assert_check_runs(&run)?;
        if matches!(path, "Cargo.lock" | "mise.lock" | ".velnor/config.toml") {
            assert_eq!(run.with_disposition("run")?, set(&PACKAGES));
        }
        return Ok(());
    }
    let base = commit(repo.root(), "base check input state")?;
    let changed = match path {
        "mise.toml" => {
            "[tasks.fixture-check]\nrun = \"true && true\"\n\n[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n"
        }
        FIXTURE_CHECK_SOURCE | "libs/util/src/lib.rs" => "pub fn changed() {}\n",
        _ => unreachable!(),
    };
    put(repo.root(), path, changed)?;
    let head = commit(repo.root(), "change maintenance input")?;
    let run = plan(repo.root(), "pull_request", Some(&base), &head)?;
    assert_check_runs(&run)?;
    if path == "mise.toml" {
        assert!(
            !run.with_disposition("run")?.contains("a5"),
            "a task-only Mise edit must not force a complete-input Rust package"
        );
    }
    if path == FIXTURE_CHECK_SOURCE {
        assert_eq!(
            run.jobs()?.get("rust-core").map(String::as_str),
            Some("run"),
            "changing a check-owned Rust source must also run its Rust package"
        );
    }
    if path == "libs/util/src/lib.rs" {
        assert_eq!(
            run.jobs()?.get("rust-util").map(String::as_str),
            Some("run"),
            "an unrelated Rust change must keep its package selected"
        );
    }
    Ok(())
}

fn assert_declared_tool_change_selects_all_rust(
    inputs: &BTreeSet<String>,
    non_inputs: &BTreeSet<String>,
) -> TestResult {
    let repo = maintenance_fixture(inputs, non_inputs)?;
    let original = std::fs::read_to_string(repo.root().join("mise.toml"))?;
    let base = commit(repo.root(), "base declared tool version")?;
    let changed = original.replace(
        "cargo-nextest\" = \"0.9.151\"",
        "cargo-nextest\" = \"0.9.150\"",
    );
    assert_ne!(
        original, changed,
        "fixture nextest version declaration is missing"
    );
    put(repo.root(), "mise.toml", &changed)?;
    let head = commit(repo.root(), "change declared tool version")?;
    let run = plan(repo.root(), "pull_request", Some(&base), &head)?;
    assert_check_runs(&run)?;
    assert_eq!(run.with_disposition("run")?, set(&PACKAGES));
    Ok(())
}

fn assert_empty_diff_skips_check(
    inputs: &BTreeSet<String>,
    non_inputs: &BTreeSet<String>,
) -> TestResult {
    let repo = maintenance_fixture(inputs, non_inputs)?;
    let head = commit(repo.root(), "empty diff head")?;
    let run = plan(repo.root(), "pull_request", Some(&head), &head)?;
    assert_eq!(
        run.jobs()?
            .get("check-maintenance-helpers")
            .map(String::as_str),
        Some("not_affected"),
        "an empty pull request diff must not schedule maintenance helpers"
    );
    assert!(
        run.with_disposition("run")?.is_empty(),
        "an empty pull request diff must not schedule Rust jobs"
    );
    Ok(())
}

fn assert_local_documentation_keeps_named_check(
    inputs: &BTreeSet<String>,
    non_inputs: &BTreeSet<String>,
) -> TestResult {
    let repo = maintenance_fixture(inputs, non_inputs)?;
    put(
        repo.root(),
        REPO_VERIFICATION_DOC,
        "local documentation edit\n",
    )?;
    let run = plan(repo.root(), "local", None, &repo.base)?;
    assert_check_runs(&run)
}

fn assert_full_event_selects_check(
    event: &str,
    inputs: &BTreeSet<String>,
    non_inputs: &BTreeSet<String>,
) -> TestResult {
    let repo = maintenance_fixture(inputs, non_inputs)?;
    let head = commit(repo.root(), "full event head")?;
    let run = if event == "qualification" {
        super::scoped_workspace::qualification_plan_without_base(&repo, &head)?
    } else {
        plan(repo.root(), event, None, &head)?
    };
    assert_check_runs(&run)
}

fn assert_missing_base_selects_check(
    inputs: &BTreeSet<String>,
    non_inputs: &BTreeSet<String>,
) -> TestResult {
    let repo = maintenance_fixture(inputs, non_inputs)?;
    let head = commit(repo.root(), "missing base head")?;
    let run = plan(repo.root(), "pull_request", None, &head)?;
    assert_check_runs(&run)
}

fn maintenance_fixture(
    inputs: &BTreeSet<String>,
    non_inputs: &BTreeSet<String>,
) -> Result<super::scoped_workspace::ScopedRepo, Box<dyn std::error::Error>> {
    assert!(inputs.contains(".mise-version"));
    assert!(inputs.contains("Cargo.lock"));
    let serialized_non_inputs = non_inputs
        .iter()
        .map(|glob| toml::Value::String(glob.clone()).to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let check_inputs = inputs
        .iter()
        .cloned()
        .chain([FIXTURE_CHECK_SOURCE.to_owned()])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|input| toml::Value::String(input).to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let config = format!(
        "\n[stacks.rust]\nnon_inputs = [{serialized_non_inputs}]\n{A5_COMPLETE_RULE}\n[discovery]\nexclude = [\"crates/**\"]\n[[checks]]\nid = \"maintenance-helpers\"\ntask = \"fixture-check\"\ninputs = [{check_inputs}]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n"
    );
    let mut repo = scoped_workspace(&config)?;
    for input in inputs {
        if !repo.root().join(input).exists() {
            put(repo.root(), input, "")?;
        }
    }
    put(
        repo.root(),
        "fixtures/opentofu-goldens/cases/dogfood/plan.txt",
        "oracle fixture\n",
    )?;
    put(repo.root(), REPO_VERIFICATION_DOC, "verification fixture\n")?;
    put(repo.root(), RUST_QUALITY_CONTRACT, "quality fixture\n")?;
    put(repo.root(), ".mise-version", "2026.1.0\n")?;
    let rust_toolchain = std::fs::read_to_string(repository_root()?.join("rust-toolchain.toml"))?;
    put(repo.root(), "rust-toolchain.toml", &rust_toolchain)?;
    let lock = std::fs::read_to_string(repository_root()?.join("mise.lock"))?;
    put(repo.root(), "mise.lock", &lock)?;
    put(repo.root(), "mise.toml", &fixture_mise_config())?;
    repo.base = commit(repo.root(), "maintenance fixture baseline")?;
    Ok(repo)
}

fn fixture_mise_config() -> String {
    "[tasks.fixture-check]\nrun = \"true\"\n\n[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n".to_owned()
}

fn assert_check_runs(run: &PlanRun) -> TestResult {
    assert_eq!(
        run.jobs()?
            .get("check-maintenance-helpers")
            .map(String::as_str),
        Some("run"),
        "changing a declared maintenance input must schedule its check"
    );
    Ok(())
}
