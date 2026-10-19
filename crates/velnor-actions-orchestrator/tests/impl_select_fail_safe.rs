//! Affected-selection fail-safes: false negatives review 2 reproduced.
//!
//! Same ten-crate fixture as `impl_select_affected_jobs`: `util` is used
//! only by `a2`; `core` by `a1`, `a3`, `a4`, `a5`, and `a6`. Each case
//! asserts the exact running crate-job set.

use std::path::Path;

use velnor_actions_contract::Plan;

use crate::impl_common::{TestResult, git, git_line};
use crate::impl_select::{commit, plan_event, plan_pr};
use crate::impl_select_affected_jobs::{PACKAGES, assert_runs, fixture, put};

/// Dependents of `core`, `core` included.
const CORE_CLOSURE: [&str; 6] = ["a1", "a3", "a4", "a5", "a6", "core"];

/// Fixture plus `setup` (base) and `edit` (head), planned as a pull request.
fn plan_with(
    extra_config: &str,
    setup: impl FnOnce(&Path) -> TestResult,
    edit: impl FnOnce(&Path) -> TestResult,
) -> Result<Plan, Box<dyn std::error::Error>> {
    let (dir, _) = fixture(extra_config, false)?;
    setup(dir.path())?;
    let base = commit(dir.path(), "setup")?;
    edit(dir.path())?;
    let head = commit(dir.path(), "change")?;
    Ok(plan_pr(dir.path(), Some(&base), &head)?.0)
}

fn has_warning(plan: &Plan, warning: &str) -> bool {
    plan.warnings.iter().any(|w| w == warning)
}

const ROOT_TOFU: &str = "\n[stacks.tofu]\nroots = [\".\"]\n";

#[test]
fn root_tofu_project_never_hides_rust_changes() -> TestResult {
    let plan = plan_with(
        ROOT_TOFU,
        |root| put(root, "main.tf", "terraform {}\n"),
        |root| put(root, "libs/core/src/lib.rs", "pub fn g() {}\n"),
    )?;
    assert_runs(&plan, &CORE_CLOSURE);
    Ok(())
}

#[test]
fn root_tofu_project_keeps_unowned_paths_running_everything() -> TestResult {
    let plan = plan_with(
        ROOT_TOFU,
        |root| put(root, "main.tf", "terraform {}\n"),
        |root| put(root, "docs/notes.md", "changed\n"),
    )?;
    assert_runs(&plan, &PACKAGES);
    assert!(
        has_warning(&plan, "unclassified_files:all_changed"),
        "{:?}",
        plan.warnings
    );
    Ok(())
}

#[test]
fn tofu_root_inside_a_crate_still_selects_the_crate() -> TestResult {
    let plan = plan_with(
        "\n[stacks.tofu]\nroots = [\"apps/a1/infra\"]\n",
        |root| put(root, "apps/a1/infra/main.tf", "terraform {}\n"),
        |root| put(root, "apps/a1/infra/main.tf", "terraform {}\n# x\n"),
    )?;
    assert_runs(&plan, &["a1"]);
    Ok(())
}

#[test]
fn undeclared_include_into_a_sibling_runs_the_reader() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(root, "libs/util/data.txt", "a\n")?;
            put(
                root,
                "apps/a7/src/lib.rs",
                "pub const D: &str = include_str!(\"../../../libs/util/data.txt\");\n",
            )
        },
        |root| put(root, "libs/util/data.txt", "b\n"),
    )?;
    assert_runs(&plan, &["a2", "a7", "util"]);
    assert!(
        has_warning(
            &plan,
            "undeclared_reads:a7:apps/a7/src/lib.rs:reads:libs/util/data.txt"
        ),
        "{:?}",
        plan.warnings
    );
    Ok(())
}

#[test]
fn declared_include_is_covered_and_narrows_again() -> TestResult {
    let rule = "\n[[stacks.rust.inputs]]\nglobs = [\"libs/util/data.txt\"]\npackages = [\"a7\"]\n";
    let setup = |root: &Path| {
        put(root, "libs/util/data.txt", "a\n")?;
        put(
            root,
            "apps/a7/src/lib.rs",
            "pub const D: &str = include_str!(\"../../../libs/util/data.txt\");\n",
        )
    };
    let plan = plan_with(rule, setup, |root| {
        put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n")
    })?;
    assert_runs(&plan, &["a1"]);
    let plan = plan_with(rule, setup, |root| put(root, "libs/util/data.txt", "b\n"))?;
    assert_runs(&plan, &["a2", "a7", "util"]);
    Ok(())
}

#[test]
fn manifest_dir_read_of_a_deleted_sibling_fixture_runs_the_reader() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(root, "libs/util/fixtures/x.json", "{}\n")?;
            put(
                root,
                "apps/a7/tests/t.rs",
                "#[test] fn t(){ std::fs::read(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../libs/util/fixtures/x.json\")).unwrap(); }\n",
            )
        },
        |root| {
            Ok(std::fs::remove_file(
                root.join("libs/util/fixtures/x.json"),
            )?)
        },
    )?;
    assert_runs(&plan, &["a2", "a7", "util"]);
    Ok(())
}

#[test]
fn unbounded_manifest_dir_reads_run_until_inputs_are_declared() -> TestResult {
    let reader = |root: &Path| {
        put(
            root,
            "apps/a7/tests/t.rs",
            "#[test] fn t(){ let _ = std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../..\"); }\n",
        )
    };
    let edit = |root: &Path| put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n");
    let plan = plan_with("", reader, edit)?;
    assert_runs(&plan, &["a1", "a7"]);
    assert!(
        has_warning(
            &plan,
            "undeclared_reads:a7:apps/a7/tests/t.rs:manifest_dir_escape"
        ),
        "{:?}",
        plan.warnings
    );
    // An input rule alone is not a statement that the inputs are complete.
    let partial = "\n[[stacks.rust.inputs]]\nglobs = [\"libs/util/**\"]\npackages = [\"a7\"]\n";
    let plan = plan_with(partial, reader, edit)?;
    assert_runs(&plan, &["a1", "a7"]);
    let complete = "\n[[stacks.rust.inputs]]\nglobs = [\"libs/util/**\"]\npackages = [\"a7\"]\ncomplete = true\n";
    let plan = plan_with(complete, reader, edit)?;
    assert_runs(&plan, &["a1"]);
    let plan = plan_with(complete, reader, |root| {
        put(root, "libs/util/src/lib.rs", "pub fn g() {}\n")
    })?;
    assert_runs(&plan, &["a2", "a7", "util"]);
    Ok(())
}

#[test]
fn build_script_library_dependents_run() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(root, "libs/util/proto.txt", "a\n")?;
            put(
                root,
                "libs/core/build.rs",
                "fn main(){ println!(\"cargo:rerun-if-changed=../util/proto.txt\"); }\n",
            )
        },
        |root| put(root, "libs/util/proto.txt", "b\n"),
    )?;
    let mut expected = CORE_CLOSURE.to_vec();
    expected.extend(["a2", "util"]);
    assert_runs(&plan, &expected);
    Ok(())
}

#[test]
fn forced_packages_and_dependents_run_on_an_empty_change_set() -> TestResult {
    let (dir, _) = fixture("", false)?;
    put(dir.path(), "libs/core/build.rs", "fn main() {}\n")?;
    let head = commit(dir.path(), "build script")?;
    let (plan, _) = plan_pr(dir.path(), Some(&head), &head)?;
    assert_runs(&plan, &CORE_CLOSURE);
    assert!(
        has_warning(&plan, "no_affected_files:all_unchanged"),
        "{:?}",
        plan.warnings
    );
    Ok(())
}

#[test]
fn merge_group_runs_everything() -> TestResult {
    let (dir, base) = fixture("", false)?;
    put(dir.path(), "apps/a1/src/lib.rs", "pub fn g() {}\n")?;
    let head = commit(dir.path(), "change")?;
    let (plan, warnings) = plan_event(dir.path(), Some(&base), &head, "merge_group")?;
    assert_runs(&plan, &PACKAGES);
    assert!(
        warnings
            .iter()
            .any(|w| w == "full_run_event:merge_group:all_changed")
    );
    Ok(())
}

#[test]
fn workflow_dispatch_qualification_runs_everything() -> TestResult {
    let (dir, _) = fixture("", false)?;
    let root = dir.path();
    put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n")?;
    let head = commit(root, "change")?;
    let context = serde_json::json!({
        "campaign": "select-test",
        "phase": "cold",
        "repository": "owner/project",
        "default_branch": "testmain",
        "git_ref": "refs/heads/testmain",
        "ref_protected": true,
        "workflow_ref": "owner/project/.github/workflows/ci.yml@refs/heads/testmain",
        "workflow_sha": head,
        "source_sha": head,
        "run_id": 7,
        "run_attempt": 1,
    });
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "r7-a1",
        "base": null,
        "head": head,
        "event": "qualification",
        "qualification": context,
        "root": root.display().to_string(),
        "repository": "owner/project",
    });
    let response = velnor_actions_orchestrator::plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    assert_runs(&plan, &PACKAGES);
    assert!(has_warning(
        &plan,
        "full_run_event:qualification:all_changed"
    ));
    Ok(())
}

#[test]
fn fork_pull_requests_select_affected_work() -> TestResult {
    let (dir, base) = fixture("", false)?;
    put(dir.path(), "apps/a1/src/lib.rs", "pub fn g() {}\n")?;
    let head = commit(dir.path(), "change")?;
    let (plan, _) = plan_event(dir.path(), Some(&base), &head, "fork")?;
    assert_runs(&plan, &["a1"]);
    Ok(())
}

#[test]
fn dirty_worktree_runs_everything() -> TestResult {
    let (dir, base) = fixture("", false)?;
    put(dir.path(), "apps/a1/src/lib.rs", "pub fn g() {}\n")?;
    let head = commit(dir.path(), "change")?;
    put(dir.path(), "libs/util/src/lib.rs", "pub fn dirty() {}\n")?;
    let (plan, warnings) = plan_pr(dir.path(), Some(&base), &head)?;
    assert_runs(&plan, &PACKAGES);
    assert!(
        warnings.iter().any(|w| w == "dirty_worktree:all_changed"),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn stale_merge_ref_runs_everything() -> TestResult {
    let (dir, base) = fixture("", false)?;
    let root = dir.path();
    git(&["checkout", "-b", "topic"], root)?;
    put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n")?;
    let head = commit(root, "topic change")?;
    git(&["checkout", "testmain"], root)?;
    put(root, "apps/a2/src/lib.rs", "pub fn base_moved() {}\n")?;
    let moved = commit(root, "base moved")?;
    git(&["merge", "--no-ff", "-m", "merge", &head], root)?;
    let merge = git_line(&["rev-parse", "HEAD"], root)?;
    assert_ne!(merge, head);
    // The merge checkout's first parent is the moved base: binding the
    // change set to the stale `base` would test a tree the diff never saw.
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_runs(&plan, &PACKAGES);
    assert!(
        warnings
            .iter()
            .any(|w| w == "comparison_unavailable:stale_merge_ref:all_changed"),
        "{warnings:?}"
    );
    let (plan, _) = plan_pr(root, Some(&moved), &head)?;
    assert_runs(&plan, &["a1"]);
    Ok(())
}

#[test]
fn reads_of_unowned_paths_never_force() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(
                root,
                "apps/a7/src/lib.rs",
                "pub const D: &str = include_str!(\"../../../docs/notes.md\");\n",
            )
        },
        |root| put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n"),
    )?;
    // A change to `docs/notes.md` is unowned and runs everything already.
    assert_runs(&plan, &["a1"]);
    Ok(())
}
