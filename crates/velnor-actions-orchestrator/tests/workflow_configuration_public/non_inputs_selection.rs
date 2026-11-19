// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;

use serde_json::json;
use velnor_actions_contract::{Plan, plan_skips_work};

use super::bun_planning_fixture::{ChangedMember, make_bun_planning_repo};
use super::scoped_workspace::{
    CORE_CLOSURE, DOCS_CONFIG, FORCED_AND_DEPENDENTS, PACKAGES, PlanRun, ScopedRepo, TestResult,
    commit, crate_conclusions, generate_lockfile, merge_verdict, miss_reasons, package_manifest,
    plan, plan_request, put, scoped_workspace, set, union, verdict_passed,
};

type FailClosedCase = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
    BTreeSet<String>,
);

const DOCS: [(&str, &str); 2] = [
    ("docs/guide.md", "changed guide\n"),
    ("docs/data/schema.json", "{\"changed\":true}\n"),
];

fn change_and_plan(
    repo: &ScopedRepo,
    event: &str,
    files: &[(&str, &str)],
) -> Result<PlanRun, Box<dyn Error>> {
    for (path, body) in files {
        put(repo.root(), path, body)?;
    }
    let tip = commit(repo.root(), "change")?;
    plan(repo.root(), event, Some(&repo.base), &tip)
}

fn docs_and(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut files = DOCS.to_vec();
    files.extend_from_slice(extra);
    files
}

fn reported(
    run: &PlanRun,
    flipped: Option<&str>,
) -> Result<BTreeMap<String, &'static str>, Box<dyn Error>> {
    crate_conclusions(run, &|id, disposition| match (disposition, flipped) {
        ("run", Some(target)) if target == id => "skipped",
        ("not_affected", Some(target)) if target == id => "success",
        ("run", _) => "success",
        _ => "skipped",
    })
}

#[test]
fn docs_only_pull_request_skips_every_rust_job_and_required_stays_green() -> TestResult {
    for event in ["pull_request", "local"] {
        let repo = scoped_workspace(DOCS_CONFIG)?;
        let run = if event == "local" {
            for (path, body) in DOCS {
                fs::write(repo.root().join(path), body)?;
            }
            plan(repo.root(), event, None, &repo.base)?
        } else {
            change_and_plan(&repo, event, &DOCS)?
        };
        assert_eq!(
            run.with_disposition("not_affected")?,
            set(&PACKAGES),
            "{event}: every Rust job must be not_affected"
        );
        assert!(run.with_disposition("run")?.is_empty(), "{event}");
        assert_eq!(
            run.not_affected_output()?,
            ",rust-a1,rust-a2,rust-a3,rust-a4,rust-a5,rust-a6,rust-a7,rust-a8,rust-core,rust-util,",
            "{event}"
        );
        let warnings = run.warnings();
        assert!(
            !warnings
                .iter()
                .any(|w| w == "unclassified_files:all_changed"),
            "{event}: {warnings:?}"
        );
        assert!(!run.reasons().contains("undeclared_reads"), "{event}");
        let skipped = reported(&run, None)?;
        assert!(skipped.values().all(|conclusion| *conclusion == "skipped"));
        let report = merge_verdict(&run, event_for_merge(event), &skipped)?;
        assert!(verdict_passed(&report)?, "{event}: {report}");
        assert!(miss_reasons(&report).is_empty(), "{event}: {report}");
        let wrong = reported(&run, Some("rust-a1"))?;
        let report = merge_verdict(&run, event_for_merge(event), &wrong)?;
        assert!(!verdict_passed(&report)?, "{event}: {report}");
        assert!(miss_reasons(&report).contains("gate_mismatch"), "{report}");
    }
    Ok(())
}

fn event_for_merge(event: &str) -> &str {
    if event == "local" {
        "local"
    } else {
        "pull_request"
    }
}

#[test]
fn docs_plus_one_crate_file_selects_crate_dependents_and_forced_packages() -> TestResult {
    let cases: [(&str, &str, BTreeSet<String>); 2] = [
        (
            "core",
            "libs/core/src/lib.rs",
            union(&[&CORE_CLOSURE, &FORCED_AND_DEPENDENTS]),
        ),
        (
            "a1",
            "apps/a1/src/lib.rs",
            union(&[&["a1"], &FORCED_AND_DEPENDENTS]),
        ),
    ];
    for (name, path, expected_run) in cases {
        let repo = scoped_workspace(DOCS_CONFIG)?;
        let run = change_and_plan(
            &repo,
            "pull_request",
            &docs_and(&[(path, "pub fn g() {}\n")]),
        )?;
        let expected_idle: BTreeSet<String> =
            set(&PACKAGES).difference(&expected_run).cloned().collect();
        assert_eq!(run.with_disposition("run")?, expected_run, "{name}");
        assert_eq!(
            run.with_disposition("not_affected")?,
            expected_idle,
            "{name}"
        );
        let target = format!(
            "rust-{}",
            expected_run.iter().next().ok_or("empty run set")?
        );
        let lazy = reported(&run, Some(&target))?;
        let report = merge_verdict(&run, "pull_request", &lazy)?;
        assert!(!verdict_passed(&report)?, "{name}: {report}");
        assert!(
            miss_reasons(&report).contains("planned_job_skipped"),
            "{name}: {report}"
        );
    }
    Ok(())
}

#[test]
fn empty_diff_runs_no_forced_package() -> TestResult {
    let repo = scoped_workspace(DOCS_CONFIG)?;
    let tip = commit(repo.root(), "empty")?;
    let run = plan(repo.root(), "pull_request", Some(&repo.base), &tip)?;
    assert_eq!(run.with_disposition("not_affected")?, set(&PACKAGES));
    assert!(run.with_disposition("run")?.is_empty());
    assert!(
        run.warnings()
            .iter()
            .any(|w| w == "no_affected_files:all_unchanged")
    );
    assert!(!run.reasons().contains("undeclared_reads"));
    Ok(())
}

#[test]
fn paths_outside_non_inputs_keep_fail_closed_selection() -> TestResult {
    let cases: [FailClosedCase; 6] = [
        (
            "unowned README.md",
            docs_and(&[("README.md", "changed\n")]),
            "unclassified_files:all_changed",
            set(&PACKAGES),
        ),
        (
            "workflow file",
            docs_and(&[(".github/workflows/x.yml", "name: x\n")]),
            "global_config_changed:selecting_all",
            set(&PACKAGES),
        ),
        (
            "Cargo.lock",
            docs_and(&[]),
            "cargo_lock_changed:all_changed",
            set(&PACKAGES),
        ),
        (
            "velnor config",
            docs_and(&[(".velnor/extra.toml", "x = 1\n")]),
            "global_config_changed:selecting_all",
            set(&PACKAGES),
        ),
        (
            "docsx is not docs",
            vec![("docsx/a.md", "a\n")],
            "unclassified_files:all_changed",
            set(&PACKAGES),
        ),
        (
            "docs dir inside a package",
            vec![("apps/a1/docs/a.md", "a\n")],
            "undeclared_reads",
            union(&[&["a1"], &FORCED_AND_DEPENDENTS]),
        ),
    ];
    for (name, files, evidence, expected_run) in cases {
        let repo = scoped_workspace(DOCS_CONFIG)?;
        if name == "Cargo.lock" {
            let lock = fs::read_to_string(repo.root().join("Cargo.lock"))?;
            fs::write(repo.root().join("Cargo.lock"), format!("{lock}\n"))?;
        }
        let run = change_and_plan(&repo, "pull_request", &files)?;
        assert_eq!(run.with_disposition("run")?, expected_run, "{name}");
        let seen = run
            .warnings()
            .iter()
            .any(|w| w == evidence || w.starts_with(&format!("{evidence}:")))
            || run.reasons().contains(evidence);
        assert!(seen, "{name}: {evidence} not in {:?}", run.warnings());
    }
    Ok(())
}

#[test]
fn push_merge_group_and_dispatch_stay_full() -> TestResult {
    for event in ["push", "merge_group", "qualification"] {
        for empty in [false, true] {
            let repo = scoped_workspace(DOCS_CONFIG)?;
            if !empty {
                for (path, body) in DOCS {
                    put(repo.root(), path, body)?;
                }
            }
            let tip = commit(repo.root(), "change")?;
            let run = if event == "qualification" {
                qualification_plan(&repo, &tip)?
            } else {
                plan(repo.root(), event, Some(&repo.base), &tip)?
            };
            let label = format!("{event} empty={empty}");
            assert_eq!(run.with_disposition("run")?, set(&PACKAGES), "{label}");
            assert!(run.with_disposition("not_affected")?.is_empty(), "{label}");
            assert!(
                run.warnings()
                    .iter()
                    .any(|w| w == &format!("full_run_event:{event}:all_changed")),
                "{label}: {:?}",
                run.warnings()
            );
            assert!(run.not_affected_output()?.is_empty(), "{label}");
            let plan: Plan = serde_json::from_value(run.value["plan"].clone())?;
            assert!(!plan_skips_work(&plan), "{label}");
        }
    }
    Ok(())
}

#[test]
fn non_inputs_never_hides_another_stacks_claim() -> TestResult {
    let bun = make_bun_planning_repo(ChangedMember::Web)?;
    let root = bun.repo.path();
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?;
    fs::write(
        &config_path,
        format!("{config}\n[stacks.rust]\nnon_inputs = [\"packages/ui/**\"]\n"),
    )?;
    put(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/x\"]\nresolver = \"2\"\n",
    )?;
    let (manifest, body) = package_manifest("crates/x", "x", "");
    put(root, &manifest, &body)?;
    put(root, "crates/x/src/lib.rs", "pub fn f() {}\n")?;
    generate_lockfile(root)?;
    let base = commit(root, "add rust crate and non_inputs")?;
    let manifest_path = root.join("packages/ui/package.json");
    let mut package: serde_json::Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    package["description"] = json!("changed under a Rust non_input");
    fs::write(
        &manifest_path,
        format!("{}\n", serde_json::to_string(&package)?),
    )?;
    let tip = commit(root, "change the Bun member under a Rust non_input")?;
    let run = plan(root, "pull_request", Some(&base), &tip)?;
    let jobs = run.jobs()?;
    assert!(!jobs.contains_key("rust-x"), "{jobs:?}");
    for id in ["rust-x__hosted", "rust-x__local"] {
        assert_eq!(
            jobs.get(id).map(String::as_str),
            Some("not_affected"),
            "{jobs:?}"
        );
    }
    for id in ["bun-fixture-ui", "bun-fixture-web"] {
        assert_eq!(
            jobs.get(id).map(String::as_str),
            Some("run"),
            "{id}: {jobs:?}"
        );
    }
    Ok(())
}

fn qualification_plan(repo: &ScopedRepo, tip: &str) -> Result<PlanRun, Box<dyn Error>> {
    let repository = "tailrocks/velnor-new";
    let request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "r190001-a1",
        "base": repo.base,
        "head": tip,
        "event": "qualification",
        "root": repo.root().to_string_lossy(),
        "repository": repository,
        "qualification": {
            "campaign": "fixture",
            "phase": "cold",
            "repository": repository,
            "default_branch": "testmain",
            "git_ref": "refs/heads/testmain",
            "ref_protected": true,
            "workflow_ref": format!("{repository}/.github/workflows/ci.yml@refs/heads/testmain"),
            "workflow_sha": tip,
            "source_sha": tip,
            "run_id": 190_001,
            "run_attempt": 1,
        },
    });
    plan_request(&request)
}
