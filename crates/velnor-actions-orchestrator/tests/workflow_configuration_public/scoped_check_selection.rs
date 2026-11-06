// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::scoped_workspace::{DOCS_CONFIG, TestResult, commit, plan, put, scoped_workspace};

const CHECKS: &str = "\n[[checks]]\nid = \"scoped\"\ntask = \"fixture-check\"\nselection = \"declared_inputs\"\ninputs = [\"apps/a1/src/lib.rs\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n";

#[test]
fn declared_inputs_scope_named_check_without_changing_other_jobs() -> TestResult {
    let config = format!(
        "{DOCS_CONFIG}\n[[checks]]\nid = \"scoped\"\ntask = \"fixture-check\"\nselection = \"declared_inputs\"\ninputs = [\"apps/a1/src/lib.rs\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n[[checks]]\nid = \"default\"\ntask = \"fixture-check\"\ninputs = [\"apps/a1/src/lib.rs\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n"
    );
    for (path, event, scoped, rust) in [
        ("apps/a1/src/lib.rs", "pull_request", "run", "rust-a1"),
        (
            "apps/a2/src/lib.rs",
            "pull_request",
            "not_affected",
            "rust-a2",
        ),
        (
            "apps/a2/tests/unrelated.rs",
            "pull_request",
            "not_affected",
            "rust-a2",
        ),
        ("unknown/extra.txt", "pull_request", "run", "rust-a2"),
        ("apps/a2/src/lib.rs", "push", "run", "rust-a2"),
    ] {
        let mut repo = scoped_workspace(&config)?;
        put(
            repo.root(),
            "mise.toml",
            "[tasks.fixture-check]\nrun = \"true\"\n",
        )?;
        put(repo.root(), path, "baseline\n")?;
        repo.base = commit(repo.root(), "configure checks")?;
        put(repo.root(), path, "changed\n")?;
        let head = commit(repo.root(), "change")?;
        let run = plan(repo.root(), event, Some(&repo.base), &head)?;
        let jobs = run.jobs()?;
        assert_eq!(
            jobs.get("check-scoped").map(String::as_str),
            Some(scoped),
            "{path}: {event}"
        );
        assert_eq!(
            jobs.get("check-default").map(String::as_str),
            Some("run"),
            "{path}: {event}"
        );
        assert_eq!(
            jobs.get(rust).map(String::as_str),
            Some("run"),
            "{path}: {event}"
        );
    }
    Ok(())
}

#[test]
fn scoped_check_broadens_for_task_source_and_missing_comparison() -> TestResult {
    let mut repo = scoped_workspace(&format!("{DOCS_CONFIG}{CHECKS}"))?;
    put(
        repo.root(),
        "mise.toml",
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    repo.base = commit(repo.root(), "configure check")?;
    put(
        repo.root(),
        "mise.toml",
        "[tasks.fixture-check]\nrun = \"false\"\n",
    )?;
    let head = commit(repo.root(), "change task")?;
    let run = plan(repo.root(), "pull_request", Some(&repo.base), &head)?;
    assert_eq!(
        run.jobs()?.get("check-scoped").map(String::as_str),
        Some("run")
    );
    let run = plan(repo.root(), "pull_request", None, &head)?;
    assert_eq!(
        run.jobs()?.get("check-scoped").map(String::as_str),
        Some("run")
    );
    Ok(())
}

#[test]
fn scoped_check_skips_docs_and_rejects_missing_inputs_or_unknown_policy() -> TestResult {
    let checks = CHECKS.replace("apps/a1/src/lib.rs", "apps/a1/required-data.txt");
    let mut repo = scoped_workspace(&format!("{DOCS_CONFIG}{checks}"))?;
    put(
        repo.root(),
        "mise.toml",
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    put(repo.root(), "apps/a1/required-data.txt", "required\n")?;
    repo.base = commit(repo.root(), "configure check")?;
    put(repo.root(), "docs/guide.md", "changed\n")?;
    let head = commit(repo.root(), "change docs")?;
    let run = plan(repo.root(), "pull_request", Some(&repo.base), &head)?;
    assert_eq!(
        run.jobs()?.get("check-scoped").map(String::as_str),
        Some("not_affected")
    );
    std::fs::remove_file(repo.root().join("apps/a1/required-data.txt"))?;
    let head = commit(repo.root(), "remove input")?;
    assert!(plan(repo.root(), "pull_request", Some(&repo.base), &head).is_err());

    let repo = scoped_workspace(&format!(
        "{DOCS_CONFIG}{}",
        CHECKS.replace("declared_inputs", "unknown")
    ))?;
    put(
        repo.root(),
        "mise.toml",
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    let head = commit(repo.root(), "invalid policy")?;
    assert!(plan(repo.root(), "pull_request", Some(&repo.base), &head).is_err());
    Ok(())
}

#[test]
fn stable_generator_rejects_unavailable_scoped_check_capability_before_output() -> TestResult {
    use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

    for scoped in [false, true] {
        let checks = if scoped {
            CHECKS.to_owned()
        } else {
            CHECKS.replace("selection = \"declared_inputs\"\n", "")
        };
        let repo = scoped_workspace(&format!("{DOCS_CONFIG}{checks}"))?;
        put(
            repo.root(),
            "mise.toml",
            "[tasks.fixture-check]\nrun = \"true\"\n",
        )?;
        commit(repo.root(), "configure check")?;
        let output = tempfile::tempdir()?;
        let destination = output.path().join("generated-preview");
        let result = prepare(repo.root()).and_then(|prepared| {
            generate(
                &prepared,
                &GenerateOptions {
                    output_dir: Some(destination.clone()),
                },
            )
        });
        if scoped {
            let error = result.err().ok_or("unsupported check generated output")?;
            assert!(
                error
                    .to_string()
                    .contains("declared_inputs_requires_approved_preview_controller")
            );
            assert!(!destination.exists(), "unsupported check wrote output");
        } else {
            result?;
            assert!(destination.join(".github/workflows/ci.yml").is_file());
        }
    }
    Ok(())
}
