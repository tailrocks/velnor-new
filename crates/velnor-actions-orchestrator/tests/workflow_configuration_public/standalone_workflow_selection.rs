// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path};

use super::scoped_workspace::{
    TestResult, commit, generate_lockfile, package_manifest, plan, put, set,
};
use super::workflow_configuration_fixture::{git, make_repo};

const WORKFLOW: &str = ".github/workflows/job-image-candidate.yml";
const ALL: [&str; 4] = [
    "velnor-actions-workflow-renderer",
    "velnor-archive-guard",
    "consumer",
    "velnor-runner-core",
];
const SELECTED: [&str; 3] = [
    "velnor-actions-workflow-renderer",
    "velnor-archive-guard",
    "consumer",
];

fn fixture(case: &str) -> Result<(tempfile::TempDir, String), Box<dyn Error>> {
    let complete = case != "incomplete_claim";
    let include_guard = case != "missing_claim";
    let runner_reads = case == "source_reader";
    let velnor_policy = case != "consumer_policy";
    let packages = if include_guard {
        "\"velnor-actions-workflow-renderer\", \"velnor-archive-guard\""
    } else {
        "\"velnor-actions-workflow-renderer\""
    };
    let policy = if velnor_policy {
        "policy = \"velnor-repository-v1\"\n"
    } else {
        ""
    };
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{policy}[stacks.rust]\n[[stacks.rust.inputs]]\nglobs = [\".github/workflows/**\"]\npackages = [{packages}]\ncomplete = {complete}\n"
    );
    let dir = make_repo(&config)?;
    let root = dir.path();
    if velnor_policy {
        git(
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/tailrocks/velnor-new.git",
            ],
            root,
        )?;
        let support = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test_support");
        fs::copy(
            support.join("generator_074.lock"),
            root.join(".velnor/generator.lock"),
        )?;
        fs::copy(
            support.join("controller_approval_074.json"),
            root.join(".velnor/controller-approvals.json"),
        )?;
    }
    fs::remove_dir_all(root.join("src"))?;
    put(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"renderer\", \"guard\", \"consumer\", \"runner\"]\nresolver = \"2\"\n",
    )?;
    for (path, name, deps) in [
        ("renderer", "velnor-actions-workflow-renderer", ""),
        ("guard", "velnor-archive-guard", ""),
        (
            "consumer",
            "consumer",
            "[dependencies]\nvelnor-archive-guard = { path = \"../guard\" }\n",
        ),
        ("runner", "velnor-runner-core", ""),
    ] {
        let (manifest, body) = package_manifest(path, name, deps);
        put(root, &manifest, &body)?;
        put(root, &format!("{path}/src/lib.rs"), "pub fn fixture() {}\n")?;
    }
    put(root, WORKFLOW, "name: candidate\n")?;
    if runner_reads {
        put(
            root,
            "runner/src/lib.rs",
            "pub const WORKFLOW: &str = include_str!(\"../../.github/workflows/job-image-candidate.yml\");\n",
        )?;
    }
    generate_lockfile(root)?;
    let base = commit(root, "base")?;
    Ok((dir, base))
}

fn selected(root: &Path, base: &str) -> Result<std::collections::BTreeSet<String>, Box<dyn Error>> {
    let head = commit(root, "change")?;
    plan(root, "pull_request", Some(base), &head)?.with_disposition("run")
}

#[test]
fn declared_standalone_workflow_selects_consumers_and_reverse_dependencies() -> TestResult {
    let (dir, base) = fixture("declared")?;
    put(dir.path(), WORKFLOW, "name: changed candidate\n")?;
    assert_eq!(selected(dir.path(), &base)?, set(&SELECTED));
    Ok(())
}

#[test]
fn standalone_workflow_selects_an_additional_source_reader() -> TestResult {
    let (dir, base) = fixture("source_reader")?;
    put(dir.path(), WORKFLOW, "name: changed candidate\n")?;
    assert_eq!(selected(dir.path(), &base)?, set(&ALL));
    Ok(())
}

#[test]
fn ambiguous_standalone_workflow_changes_remain_fail_closed() -> TestResult {
    for case in [
        "missing_claim",
        "incomplete_claim",
        "consumer_policy",
        "delete",
        "rename",
        "type",
        "unknown",
        "ci",
        "action",
        "mixed",
    ] {
        let (dir, base) = fixture(case)?;
        let root = dir.path();
        match case {
            "delete" => fs::remove_file(root.join(WORKFLOW))?,
            "rename" => fs::rename(
                root.join(WORKFLOW),
                root.join(".github/workflows/renamed.yml"),
            )?,
            "type" => {
                fs::remove_file(root.join(WORKFLOW))?;
                std::os::unix::fs::symlink("other.yml", root.join(WORKFLOW))?;
            }
            "unknown" => put(root, ".github/workflows/unknown.yml", "name: unknown\n")?,
            "ci" => put(root, ".github/workflows/ci.yml", "name: CI\n")?,
            "action" => put(root, ".github/actions/shared/action.yml", "name: action\n")?,
            "mixed" => {
                put(root, WORKFLOW, "name: changed candidate\n")?;
                put(root, ".github/workflows/ci.yml", "name: CI\n")?;
            }
            _ => put(root, WORKFLOW, "name: changed candidate\n")?,
        }
        assert_eq!(selected(root, &base)?, set(&ALL), "{case}");
    }
    Ok(())
}
