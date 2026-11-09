// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::scoped_workspace::{
    DOCS_CONFIG, TestResult, commit, generate_lockfile, plan, put, scoped_workspace,
};
use std::fs;

fn family_workspace(
    config: &str,
) -> Result<super::scoped_workspace::ScopedRepo, Box<dyn std::error::Error>> {
    let repo = scoped_workspace(config)?;
    let root = repo.root();
    let path = root.join(".velnor/config.toml");
    let text = fs::read_to_string(&path)?.replace(
        "default_branch = \"testmain\"",
        "policy = \"velnor-repository-v1\"\ndefault_branch = \"main\"",
    );
    fs::write(path, text)?;
    super::workflow_configuration_fixture::git(&["branch", "-m", "main"], root)?;
    super::workflow_configuration_fixture::git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
        root,
    )?;
    fs::write(
        root.join(".velnor/generator.lock"),
        include_str!("../../../../.velnor/generator.lock"),
    )?;
    fs::write(
        root.join(".velnor/controller-approvals.json"),
        include_str!("../../../../.velnor/controller-approvals.json"),
    )?;
    Ok(repo)
}

fn family_qualification_plan_without_base(
    repo: &super::scoped_workspace::ScopedRepo,
    head: &str,
) -> Result<super::scoped_workspace::PlanRun, Box<dyn std::error::Error>> {
    let repository = "tailrocks/velnor-new";
    super::scoped_workspace::plan_request(&serde_json::json!({
        "schema": 1, "op": "plan-v1", "run_key": "r190001-a1", "base": null,
        "head": head, "event": "qualification", "root": repo.root().to_string_lossy(),
        "repository": repository,
        "qualification": {
            "campaign": "fixture", "phase": "cold", "repository": repository,
            "default_branch": "main", "git_ref": "refs/heads/main", "ref_protected": true,
            "workflow_ref": format!("{repository}/.github/workflows/ci.yml@refs/heads/main"),
            "workflow_sha": head, "source_sha": head, "run_id": 190_001, "run_attempt": 1,
        },
    }))
}

const HELPER_INPUTS: &str = r#"
[[stacks.rust.inputs]]
globs = ["apps/a1/**", "family-guard/**", "family-guarded/**", "images/dind/fixture-unrelated.txt", "images/resource-probe/fixture-unrelated.txt", "scripts/generator-release/source-qualification.sh", "scripts/generator-release/cargo-git-handoff.py", "scripts/generator-release/cargo-source-handoff.sh"]
packages = ["a1"]
complete = true
"#;

#[test]
fn runner_family_obligation_tracks_inputs_and_skips_docs_only() -> TestResult {
    let mut repo = family_workspace(&format!(
        "{DOCS_CONFIG}\n[runner_image_family]\n{HELPER_INPUTS}"
    ))?;
    let root = repo.root();
    populate_family_fixture(root)?;
    repo.base = commit(root, "runner family baseline")?;
    for (path, expected) in [
        ("images/runner/ubuntu-26.04/Dockerfile", "run"),
        ("images/dind/Dockerfile", "run"),
        ("images/resource-probe/Dockerfile", "run"),
        ("images/runner/ubuntu-26.04/entrypoint.sh", "run"),
        ("images/dind/entrypoint.sh", "run"),
        (
            "images/resource-probe/fixture-unrelated.txt",
            "not_affected",
        ),
        ("family/probe/src/lib.rs", "run"),
        ("family/host/src/lib.rs", "run"),
        ("family/cli/src/lib.rs", "run"),
        (".velnor/runner.lock", "run"),
        ("scripts/generator-release/cargo-git-handoff.py", "run"),
        ("scripts/generator-release/cargo-source-handoff.sh", "run"),
        ("scripts/generator-release/source-qualification.sh", "run"),
        ("apps/a1/src/lib.rs", "not_affected"),
        ("docs/guide.md", "not_affected"),
    ] {
        let root = repo.root();
        super::workflow_configuration_fixture::git(&["checkout", "--detach", &repo.base], root)?;
        let content = if path.ends_with("/Dockerfile") {
            "FROM scratch\nCOPY input /input\nLABEL changed=true\n"
        } else if std::path::Path::new(path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        {
            "pub fn f() { let _ = 1; }\n"
        } else {
            "changed\n"
        };
        put(root, path, content)?;
        let head = commit(root, "change")?;
        let run = plan(root, "pull_request", Some(&repo.base), &head)?;
        assert_eq!(
            run.jobs()?
                .get("image-runner-family-linux-amd64")
                .map(String::as_str),
            Some(expected),
            "{path}"
        );
        if path.starts_with("scripts/generator-release/") {
            for job in [
                "rust-velnor-resource-probe",
                "rust-velnor-runner-host",
                "rust-velnor-actions-cli",
            ] {
                assert_eq!(
                    run.jobs()?.get(job).map(String::as_str),
                    Some("not_affected"),
                    "{path} selected {job}"
                );
            }
        }
        if path == "docs/guide.md" {
            let push = plan(root, "push", Some(&repo.base), &head)?;
            assert_eq!(
                push.jobs()?
                    .get("image-runner-family-linux-amd64")
                    .map(String::as_str),
                Some("run")
            );
            let dispatch = family_qualification_plan_without_base(&repo, &head)?;
            assert_eq!(
                dispatch
                    .jobs()?
                    .get("image-runner-family-linux-amd64")
                    .map(String::as_str),
                Some("run")
            );
        }
    }
    Ok(())
}

#[test]
fn real_runner_dockerfiles_skip_synthetic_unrelated_input() -> TestResult {
    let mut repo = family_workspace(&format!(
        "{DOCS_CONFIG}\n[runner_image_family]\n{HELPER_INPUTS}"
    ))?;
    let root = repo.root();
    populate_family_fixture(root)?;
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for path in [
        "images/runner/ubuntu-26.04/Dockerfile",
        "images/dind/Dockerfile",
        "images/resource-probe/Dockerfile",
        "images/dind/entrypoint.sh",
    ] {
        fs::copy(project.join(path), root.join(path))?;
    }
    put(root, "images/dind/fixture-unrelated.txt", "initial\n")?;
    repo.base = commit(root, "real runner Dockerfiles")?;
    for (path, expected) in [
        ("images/dind/fixture-unrelated.txt", "not_affected"),
        ("images/dind/entrypoint.sh", "run"),
        ("images/resource-probe/Dockerfile", "run"),
    ] {
        let root = repo.root();
        super::workflow_configuration_fixture::git(&["checkout", "--detach", &repo.base], root)?;
        let mut text = fs::read_to_string(root.join(path))?;
        text.push_str("\n# fixture change\n");
        put(root, path, &text)?;
        let head = commit(root, "change one context input")?;
        let run = plan(root, "pull_request", Some(&repo.base), &head)?;
        assert_eq!(
            run.jobs()?
                .get("image-runner-family-linux-amd64")
                .map(String::as_str),
            Some(expected),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn invalid_archive_guard_manifest_selects_family_conservatively() -> TestResult {
    for variant in ["malformed", "missing", "symlink"] {
        let mut repo = family_workspace(&format!(
            "{DOCS_CONFIG}\n[runner_image_family]\n{HELPER_INPUTS}"
        ))?;
        let root = repo.root();
        populate_family_fixture(root)?;
        let manifest = root.join("scripts/archive-guard-inputs.txt");
        match variant {
            "malformed" => fs::write(&manifest, "tree ../escape\n")?,
            "missing" => fs::remove_file(&manifest)?,
            "symlink" => {
                fs::remove_file(&manifest)?;
                std::os::unix::fs::symlink("missing", &manifest)?;
            }
            _ => unreachable!(),
        }
        repo.base = commit(root, "invalid archive guard manifest")?;
        let root = repo.root();
        put(root, "apps/a1/src/lib.rs", "pub fn changed() {}\n")?;
        let head = commit(root, "unrelated change")?;
        let run = plan(root, "pull_request", Some(&repo.base), &head)?;
        assert_eq!(
            run.jobs()?
                .get("image-runner-family-linux-amd64")
                .map(String::as_str),
            Some("run"),
            "{variant}"
        );
    }
    Ok(())
}

#[test]
fn unsupported_family_dockerfile_selects_family_conservatively() -> TestResult {
    let mut repo = family_workspace(&format!(
        "{DOCS_CONFIG}\n[runner_image_family]\n{HELPER_INPUTS}"
    ))?;
    let root = repo.root();
    populate_family_fixture(root)?;
    let path = root.join("images/resource-probe/Dockerfile");
    let mut dockerfile = fs::read_to_string(&path)?;
    dockerfile.push_str("\nENV FAMILY_FIXTURE=1\n");
    fs::write(path, dockerfile)?;
    repo.base = commit(root, "unsupported family Dockerfile")?;
    let root = repo.root();
    put(
        root,
        "images/resource-probe/fixture-unrelated.txt",
        "changed\n",
    )?;
    let head = commit(root, "change unrelated input")?;
    let run = plan(root, "pull_request", Some(&repo.base), &head)?;
    assert_eq!(
        run.jobs()?
            .get("image-runner-family-linux-amd64")
            .map(String::as_str),
        Some("run")
    );
    Ok(())
}

#[test]
fn archive_guard_tree_changes_select_runner_family_without_sibling_fallback() -> TestResult {
    let mut repo = family_workspace(&format!(
        "{DOCS_CONFIG}\n[runner_image_family]\n{HELPER_INPUTS}"
    ))?;
    populate_family_fixture(repo.root())?;
    repo.base = commit(repo.root(), "archive guard baseline")?;
    for (path, expected) in [
        ("family-guard/new.rs", "run"),
        ("family-guarded/new.rs", "not_affected"),
        ("scripts/archive-guard-inputs.txt", "run"),
    ] {
        let root = repo.root();
        super::workflow_configuration_fixture::git(&["checkout", "--detach", &repo.base], root)?;
        put(root, path, "changed\n")?;
        let head = commit(root, "change archive guard input")?;
        let run = plan(root, "pull_request", Some(&repo.base), &head)?;
        assert_eq!(
            run.jobs()?
                .get("image-runner-family-linux-amd64")
                .map(String::as_str),
            Some(expected),
            "{path}"
        );
    }
    for operation in ["remove", "rename"] {
        let root = repo.root();
        super::workflow_configuration_fixture::git(&["checkout", "--detach", &repo.base], root)?;
        let existing = root.join("family-guard/existing.rs");
        if operation == "remove" {
            fs::remove_file(existing)?;
        } else {
            fs::rename(existing, root.join("family-guard/renamed.rs"))?;
        }
        let head = commit(root, operation)?;
        let run = plan(root, "pull_request", Some(&repo.base), &head)?;
        assert_eq!(
            run.jobs()?
                .get("image-runner-family-linux-amd64")
                .map(String::as_str),
            Some("run"),
            "{operation}"
        );
    }
    Ok(())
}

fn populate_family_fixture(root: &std::path::Path) -> TestResult {
    let cargo = fs::read_to_string(root.join("Cargo.toml"))?;
    let cargo = cargo.replace(
        "]\nresolver",
        ", \"family/probe\", \"family/host\", \"family/cli\"]\nresolver",
    );
    put(root, "Cargo.toml", &cargo)?;
    for (path, name) in [
        ("probe", "velnor-resource-probe"),
        ("host", "velnor-runner-host"),
        ("cli", "velnor-actions-cli"),
    ] {
        put(
            root,
            &format!("family/{path}/Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        put(
            root,
            &format!("family/{path}/src/lib.rs"),
            "pub fn f() {}\n",
        )?;
    }
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for context in [
        "images/runner/ubuntu-26.04",
        "images/dind",
        "images/resource-probe",
    ] {
        let dockerfile = fs::read_to_string(project.join(context).join("Dockerfile"))?;
        put(root, &format!("{context}/Dockerfile"), &dockerfile)?;
    }
    put(
        root,
        "images/runner/ubuntu-26.04/entrypoint.sh",
        "initial\n",
    )?;
    put(root, "images/dind/entrypoint.sh", "initial\n")?;
    put(
        root,
        "images/resource-probe/fixture-unrelated.txt",
        "initial\n",
    )?;
    put(root, ".velnor/runner.lock", "initial\n")?;
    put(
        root,
        "scripts/archive-guard-inputs.txt",
        "tree family-guard\n",
    )?;
    put(root, "family-guard/existing.rs", "initial\n")?;
    put(root, "family-guarded/existing.rs", "initial\n")?;
    put(root, "scripts/check-runner-image-candidate.sh", "initial\n")?;
    generate_lockfile(root)?;
    copy_locked_tools(root)?;
    Ok(())
}

pub(super) fn copy_locked_tools(root: &std::path::Path) -> TestResult {
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in ["mise.toml", "mise.lock", "rust-toolchain.toml"] {
        std::fs::copy(project.join(name), root.join(name))?;
    }
    for name in velnor_actions_workflow_renderer::verification_jobs::FAMILY_PREFETCH_SOURCES {
        let destination = root.join(name);
        std::fs::create_dir_all(destination.parent().ok_or("missing parent")?)?;
        std::fs::copy(project.join(name), destination)?;
    }
    Ok(())
}
