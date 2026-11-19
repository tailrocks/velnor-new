// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;

use super::scoped_workspace::{
    DOCS_CONFIG, FORCED_AND_DEPENDENTS, PACKAGES, PlanRun, TestResult, commit, plan, put,
    scoped_workspace, set,
};

fn star_config() -> String {
    DOCS_CONFIG.replace("docs/**", "docs/*/**")
}

fn assert_forced_run(run: &PlanRun, with_docs: bool, label: &str) -> TestResult {
    let expected: BTreeSet<String> = set(&FORCED_AND_DEPENDENTS);
    assert_eq!(run.with_disposition("run")?, expected, "{label}");
    let idle: BTreeSet<String> = set(&PACKAGES).difference(&expected).cloned().collect();
    assert_eq!(run.with_disposition("not_affected")?, idle, "{label}");
    let warnings = run.warnings();
    assert_eq!(
        warnings
            .iter()
            .any(|warning| warning == "toolfiles_only:all_unchanged"),
        !with_docs,
        "{label}: {warnings:?}"
    );
    for forced in ["a7", "util"] {
        assert!(
            warnings
                .iter()
                .any(|warning| warning.starts_with(&format!("undeclared_reads:{forced}:"))),
            "{label}: {forced} not reported as forced in {warnings:?}"
        );
    }
    assert!(
        run.reasons().contains("affected_by_change"),
        "{label}: {:?}",
        run.reasons()
    );
    Ok(())
}

#[test]
fn tool_files_only_change_runs_forced_packages_and_dependents() -> TestResult {
    for event in ["pull_request", "local"] {
        for with_docs in [false, true] {
            let label = format!("{event} docs={with_docs}");
            let repo = scoped_workspace(DOCS_CONFIG)?;
            put(repo.root(), "mise.toml", "# fixture\n")?;
            let first = commit(repo.root(), "add mise.toml")?;
            put(repo.root(), "mise.toml", "# fixture, edited\n")?;
            if with_docs {
                put(repo.root(), "docs/guide.md", "changed guide\n")?;
            }
            let run = if event == "local" {
                plan(repo.root(), event, None, &first)?
            } else {
                let second = commit(repo.root(), "comment only")?;
                plan(repo.root(), event, Some(&first), &second)?
            };
            assert_forced_run(&run, with_docs, &label)?;
        }
    }
    Ok(())
}

#[test]
fn star_segment_glob_skips_only_what_the_checkout_drops() -> TestResult {
    let repo = scoped_workspace(&star_config())?;
    fs::write(
        repo.root().join("docs/data/schema.json"),
        "{\"changed\":true}\n",
    )?;
    let tip = commit(repo.root(), "nested docs")?;
    let run = plan(repo.root(), "pull_request", Some(&repo.base), &tip)?;
    assert_eq!(run.with_disposition("not_affected")?, set(&PACKAGES));
    assert!(run.with_disposition("run")?.is_empty());
    assert!(
        !run.warnings()
            .iter()
            .any(|warning| warning == "unclassified_files:all_changed"),
        "{:?}",
        run.warnings()
    );

    let repo = scoped_workspace(&star_config())?;
    fs::write(repo.root().join("docs/guide.md"), "changed guide\n")?;
    let tip = commit(repo.root(), "top-level docs")?;
    let run = plan(repo.root(), "pull_request", Some(&repo.base), &tip)?;
    assert!(
        run.warnings()
            .iter()
            .any(|warning| warning == "unclassified_files:all_changed"),
        "docs/guide.md is outside docs/*/** and must reach every Rust job: {:?}",
        run.warnings()
    );
    assert_eq!(run.with_disposition("run")?, set(&PACKAGES));
    Ok(())
}
