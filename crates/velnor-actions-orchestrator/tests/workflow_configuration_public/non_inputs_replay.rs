// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::scoped_workspace::{PlanRun, TestResult, commit, generate_lockfile, plan, put};
use super::topology_e797385c74::{Dep, Forcing, PACKAGES, RUNNER_WORKSPACE, RealPackage};
use super::workflow_configuration_fixture::make_repo;

const DOCS_NON_INPUTS: &str = "\n[stacks.rust]\nnon_inputs = [\"docs/**\"]\n";

const PR_473: [&str; 6] = [
    "docs/implemented/consumer-rust-setup.md",
    "docs/implemented/native-rust-crate-jobs.md",
    "docs/proposed/README.md",
    "docs/proposed/cache-contract.md",
    "docs/proposed/cache-families.md",
    "docs/proposed/compiled-state-persistence.md",
];

const PR_449: [&str; 2] = [
    "crates/velnor-actions-orchestrator/tests/workflow_configuration_public/native_artifacts_suite_gate.rs",
    "crates/velnor-actions-workflow-renderer/src/rust_suites.rs",
];

const PR_424: [&str; 14] = [
    "crates/velnor-actions-mise/src/check_command.rs",
    "crates/velnor-actions-mise/src/check_execution.rs",
    "crates/velnor-actions-orchestrator/src/check_binary.rs",
    "crates/velnor-actions-orchestrator/src/check_prepare.rs",
    "crates/velnor-actions-orchestrator/src/check_runtime.rs",
    "crates/velnor-actions-orchestrator/src/config.rs",
    "crates/velnor-actions-orchestrator/src/controller_approval.rs",
    "crates/velnor-actions-orchestrator/src/preview_producer.rs",
    "crates/velnor-actions-orchestrator/src/tap_prepare_updater.rs",
    "crates/velnor-actions-orchestrator/tests/gc1_t09.rs",
    "crates/velnor-actions-orchestrator/tests/gc1_t09_a.rs",
    "crates/velnor-actions-orchestrator/tests/gc1_t15.rs",
    "crates/velnor-actions-orchestrator/tests/gc1_t15_b.rs",
    "crates/velnor-actions-orchestrator/tests/gc1_t39.rs",
];

const NOT_AFFECTED_HOSTED: [&str; 10] = [
    "velnor-actions-actionlint",
    "velnor-actions-ansible",
    "velnor-actions-bun",
    "velnor-actions-contract",
    "velnor-actions-dbt",
    "velnor-actions-gradle",
    "velnor-actions-rust",
    "velnor-resource-probe",
    "velnor-runner-core",
    "velnor-runner-github",
];

const RUN_HOSTED: [&str; 10] = [
    "velnor-actions-cli",
    "velnor-actions-freshness",
    "velnor-actions-mise",
    "velnor-actions-orchestrator",
    "velnor-actions-tofu",
    "velnor-actions-workflow-renderer",
    "velnor-archive-guard",
    "velnor-runner-attestation",
    "velnor-runner-cli",
    "velnor-runner-host",
];

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn all_jobs() -> BTreeSet<String> {
    PACKAGES
        .iter()
        .map(|package| package.name.to_owned())
        .collect()
}

fn relative(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').collect();
    let to: Vec<&str> = to.split('/').collect();
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts = vec![".."; from.len() - shared];
    parts.extend(&to[shared..]);
    parts.join("/")
}

fn manifest(package: &RealPackage) -> String {
    let mut text = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        package.name
    );
    for (header, wanted) in [
        ("dependencies", Dep::Normal),
        ("dev-dependencies", Dep::Dev),
    ] {
        let lines: Vec<String> = package
            .deps
            .iter()
            .filter(|(_, kind)| *kind == wanted)
            .map(|(name, _)| {
                let target = PACKAGES
                    .iter()
                    .find(|candidate| candidate.name == *name)
                    .map_or("", |candidate| candidate.dir);
                format!(
                    "{name} = {{ path = \"{}\" }}",
                    relative(package.dir, target)
                )
            })
            .collect();
        if !lines.is_empty() {
            text.push_str(&["\n[", header, "]\n", &lines.join("\n"), "\n"].concat());
        }
    }
    text
}

fn source(package: &RealPackage) -> &'static str {
    match package.forcing {
        Forcing::ManifestDirEscape => {
            "pub fn up() -> Option<&'static std::path::Path> {\n    std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).parent()\n}\n"
        }
        Forcing::ReadsSibling => {
            "pub const SIBLING: &str = include_str!(\"../../velnor-actions-mise/src/lib.rs\");\n"
        }
        Forcing::None | Forcing::BuildScript => "pub fn f() {}\n",
    }
}

fn write_workspace_manifest(root: &Path, dir: &str, members: &[&RealPackage]) -> TestResult {
    let listed = members
        .iter()
        .map(|package| {
            let relative = package.dir.strip_prefix(dir).unwrap_or(package.dir);
            format!("\"{}\"", relative.trim_start_matches('/'))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let manifest = if dir.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{dir}/Cargo.toml")
    };
    put(
        root,
        &manifest,
        &format!("[workspace]\nmembers = [{listed}]\nresolver = \"2\"\n"),
    )
}

fn real_topology(extra_config: &str) -> Result<(TempDir, String), Box<dyn Error>> {
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{extra_config}"
    );
    let repo = make_repo(&config)?;
    let root = repo.path();
    fs::remove_dir_all(root.join("src"))?;
    let (runner, main): (Vec<&RealPackage>, Vec<&RealPackage>) =
        PACKAGES.iter().partition(|package| {
            package.dir.starts_with(RUNNER_WORKSPACE) && package.dir != RUNNER_WORKSPACE
        });
    write_workspace_manifest(root, "", &main)?;
    write_workspace_manifest(root, RUNNER_WORKSPACE, &runner)?;
    for package in &PACKAGES {
        put(
            root,
            &format!("{}/Cargo.toml", package.dir),
            &manifest(package),
        )?;
        put(
            root,
            &format!("{}/src/lib.rs", package.dir),
            source(package),
        )?;
        if package.forcing == Forcing::BuildScript {
            put(root, &format!("{}/build.rs", package.dir), "fn main() {}\n")?;
        }
    }
    for doc in PR_473 {
        put(root, doc, "doc\n")?;
    }
    generate_lockfile(root)?;
    let nested = root.join(RUNNER_WORKSPACE);
    generate_lockfile(&nested)?;
    let base = commit(root, "base")?;
    Ok((repo, base))
}

fn complete_rules() -> String {
    PACKAGES
        .iter()
        .filter(|package| package.forcing == Forcing::ManifestDirEscape)
        .map(|package| {
            [
                "\n[[stacks.rust.inputs]]\nglobs = [\"",
                package.dir,
                "/**\"]\npackages = [\"",
                package.name,
                "\"]\ncomplete = true\n",
            ]
            .concat()
        })
        .collect()
}

fn adopted_config() -> String {
    format!("{DOCS_NON_INPUTS}{}", complete_rules())
}

fn replay(paths: &[&str], config: &str) -> Result<PlanRun, Box<dyn Error>> {
    let (repo, base) = real_topology(config)?;
    for path in paths {
        put(repo.path(), path, "// replayed change\n")?;
    }
    let tip = commit(repo.path(), "replay")?;
    plan(repo.path(), "pull_request", Some(&base), &tip)
}

#[test]
fn replay_pr473_docs_only_hosted_inputs() -> TestResult {
    let run = replay(&PR_473, &adopted_config())?;
    let idle: BTreeSet<String> = run.with_disposition("not_affected")?.into_iter().collect();
    assert_eq!(idle, all_jobs(), "all crate jobs must be not_affected");
    assert!(run.with_disposition("run")?.is_empty());
    assert!(
        !run.warnings()
            .iter()
            .any(|w| w == "unclassified_files:all_changed")
    );
    Ok(())
}

#[test]
fn replay_narrow_hosted_inputs_stay_narrow() -> TestResult {
    for (name, paths) in [("449", PR_449.to_vec()), ("424", PR_424.to_vec())] {
        let run = replay(&paths, "")?;
        assert_eq!(
            run.with_disposition("run")?,
            names(&RUN_HOSTED),
            "PR {name}"
        );
        assert_eq!(
            run.with_disposition("not_affected")?,
            names(&NOT_AFFECTED_HOSTED),
            "PR {name}"
        );
    }
    Ok(())
}

#[test]
fn replay_docs_plus_dbt_runs_the_closure_and_the_remaining_forcing() -> TestResult {
    let mut paths = PR_473.to_vec();
    paths.push("crates/velnor-actions-dbt/src/lib.rs");
    let run = replay(&paths, &adopted_config())?;
    let expected = names(&[
        "velnor-actions-dbt",
        "velnor-actions-orchestrator",
        "velnor-actions-workflow-renderer",
        "velnor-actions-cli",
        "velnor-actions-tofu",
        "velnor-actions-mise",
        "velnor-runner-host",
        "velnor-runner-cli",
    ]);
    assert_eq!(run.with_disposition("run")?, expected, "docs plus dbt");
    let idle: BTreeSet<String> = all_jobs().difference(&expected).cloned().collect();
    assert_eq!(run.with_disposition("not_affected")?, idle, "docs plus dbt");
    Ok(())
}
