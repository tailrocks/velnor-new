// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;

use velnor_actions_contract::{Job, StepKind};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::non_input_checkout::{
    NON_INPUT_GUARD_STEP, SPARSE_CHECKOUT_KEY, SPARSE_CONE_MODE_KEY,
};

use super::non_inputs_checkout_support::{checkout_steps, rust_repo, with_value};
use super::scoped_workspace::TestResult;
use super::tools_producers::POLICIES;

const EXCLUSION: &str = "/*\n!/docs/**";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Rust,
    Other,
}

const RUST_PREFIXES: [&str; 5] = ["rust-", "check-", "suite-", "dependency-seed", "msrv"];

const RUST_IDS: [&str; 3] = ["ledger-cli", "cargo-deny", "cargo-machete"];

const OTHER_PREFIXES: [&str; 7] = [
    "tofu-",
    "tools-producer-",
    "providers-producer-",
    "bun-",
    "dbt-",
    "image-",
    "task-",
];

const OTHER_IDS: [&str; 5] = ["plan", "required", "required-gate", "alint", "jactionlint"];

fn classify(id: &str) -> Result<Kind, Box<dyn Error>> {
    if RUST_IDS.contains(&id) || RUST_PREFIXES.iter().any(|prefix| id.starts_with(prefix)) {
        Ok(Kind::Rust)
    } else if OTHER_IDS.contains(&id) || OTHER_PREFIXES.iter().any(|prefix| id.starts_with(prefix))
    {
        Ok(Kind::Other)
    } else {
        Err(format!("job kind {id} has no row in the Rust ownership table").into())
    }
}

fn kind_label(id: &str) -> String {
    RUST_IDS
        .iter()
        .chain(OTHER_IDS.iter())
        .find(|exact| **exact == id)
        .map(|exact| (*exact).to_owned())
        .or_else(|| {
            RUST_PREFIXES
                .iter()
                .chain(OTHER_PREFIXES.iter())
                .find(|prefix| id.starts_with(*prefix))
                .map(|prefix| (*prefix).to_owned())
        })
        .unwrap_or_else(|| id.to_owned())
}

fn assert_job_checkout(label: &str, id: &str, job: &Job, expected: Kind) -> TestResult {
    let guards = job
        .steps
        .iter()
        .filter(|step| step.name == NON_INPUT_GUARD_STEP)
        .count();
    for (at, checkout) in checkout_steps(job) {
        let sparse = with_value(checkout, SPARSE_CHECKOUT_KEY);
        match expected {
            Kind::Rust => {
                assert_eq!(
                    sparse,
                    Some(EXCLUSION),
                    "{label}/{id}: checkout lacks the exclusion"
                );
                assert_eq!(
                    with_value(checkout, SPARSE_CONE_MODE_KEY),
                    Some("false"),
                    "{label}/{id}"
                );
                assert!(
                    checkout.condition.is_none(),
                    "{label}/{id}: conditional checkout"
                );
                let guard = job
                    .steps
                    .get(at + 1)
                    .ok_or_else(|| format!("{label}/{id}: no step after the checkout"))?;
                assert_eq!(
                    guard.name, NON_INPUT_GUARD_STEP,
                    "{label}/{id}: guard missing"
                );
                assert!(guard.condition.is_none(), "{label}/{id}: conditional guard");
                assert!(matches!(guard.kind, StepKind::Shell { .. }), "{label}/{id}");
            }
            Kind::Other => {
                assert_eq!(sparse, None, "{label}/{id}: non-Rust job excludes paths");
                assert_eq!(guards, 0, "{label}/{id}: non-Rust job carries the guard");
            }
        }
    }
    Ok(())
}

fn assert_jobs(
    label: &str,
    jobs: &BTreeMap<String, Job>,
    seen: &mut BTreeSet<String>,
) -> TestResult {
    for (id, job) in jobs {
        if checkout_steps(job).is_empty() {
            continue;
        }
        let kind = classify(id)?;
        assert_job_checkout(label, id, job, kind)?;
        seen.insert(format!("{kind:?}:{}", kind_label(id)));
    }
    Ok(())
}

#[test]
fn every_rust_job_checkout_excludes_non_inputs_and_other_jobs_do_not() -> TestResult {
    let mut seen = BTreeSet::new();
    for policy in POLICIES {
        let repo = rust_repo(policy, Some("docs/**"))?;
        let jobs = finalized_jobs(&prepare(repo.path())?)?;
        assert_jobs(&format!("{policy:?}"), &jobs, &mut seen)?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let suites = tree
            .get(".github/workflows/suites.yml")
            .ok_or("standalone suites workflow is missing")?;
        assert!(
            suites.contains(&format!(
                "sparse-checkout: \"{}\"",
                EXCLUSION.replace('\n', "\\n")
            )),
            "{policy:?}: {suites}"
        );
        assert!(suites.contains(NON_INPUT_GUARD_STEP), "{policy:?}");
    }
    for expected in [
        "Rust:rust-",
        "Rust:ledger-cli",
        "Rust:cargo-deny",
        "Rust:cargo-machete",
        "Rust:suite-",
        "Rust:check-",
        "Rust:dependency-seed",
        "Other:plan",
        "Other:required",
        "Other:tools-producer-",
        "Other:tofu-",
    ] {
        assert!(
            seen.contains(expected),
            "fixture never rendered {expected}: {seen:?}"
        );
    }
    Ok(())
}

#[test]
fn default_non_inputs_is_empty_and_changes_nothing() -> TestResult {
    for policy in POLICIES {
        let omitted = rust_repo(policy, None)?;
        let explicit = rust_repo(policy, None)?;
        let config_path = explicit.path().join(".velnor/config.toml");
        let config = fs::read_to_string(&config_path)?;
        fs::write(
            &config_path,
            config.replacen("[stacks.rust]\n", "[stacks.rust]\nnon_inputs = []\n", 1),
        )?;
        let left = render_staged_tree(&prepare(omitted.path())?)?;
        let right = render_staged_tree(&prepare(explicit.path())?)?;
        assert_eq!(left.files.len(), right.files.len(), "{policy:?}");
        for (a, b) in left.files.iter().zip(&right.files) {
            assert_eq!(a.path, b.path, "{policy:?}");
            assert_eq!(a.bytes, b.bytes, "{policy:?}: {}", a.path);
            assert!(
                !a.bytes.contains("sparse-checkout"),
                "{policy:?}: {}",
                a.path
            );
            assert!(
                !a.bytes.contains(NON_INPUT_GUARD_STEP),
                "{policy:?}: {}",
                a.path
            );
        }
    }
    Ok(())
}
