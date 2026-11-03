// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_contract::{Job, Step, StepKind, WorkflowPolicy};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::non_input_checkout::{
    NON_INPUT_GUARD_STEP, SPARSE_CHECKOUT_KEY, SPARSE_CONE_MODE_KEY,
};

use super::one_job_per_crate::mbx_repo_for;
use super::scoped_workspace::TestResult;
use super::tools_producers::POLICIES;
use super::workflow_configuration_fixture::{self, git};

const EXCLUSION: &str = "/*\n!/docs/**";

const RUST_SUITE: &str = "\n[[stacks.rust.suites]]\nid = \"proof-suite\"\nprofile = \"default\"\ntriggers = [\"pull_request\", \"push\"]\nrequired = true\npackage = \"demo\"\nmanifest_path = \"crates/demo/Cargo.toml\"\n";

const STANDALONE_SUITE: &str = "\n[[stacks.rust.suites]]\nid = \"nightly-suite\"\nprofile = \"default\"\ntriggers = [\"workflow_dispatch\"]\npackage = \"demo\"\nmanifest_path = \"crates/demo/Cargo.toml\"\n";

const NAMED_CHECK: &str = "\n[[checks]]\nid = \"unit\"\ntask = \"fixture-check\"\ninputs = [\"crates/demo/Cargo.toml\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n";

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

fn checkout_steps(job: &Job) -> Vec<(usize, &Step)> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@"))
        })
        .collect()
}

fn with_value<'a>(step: &'a Step, key: &str) -> Option<&'a str> {
    match &step.kind {
        StepKind::Action { with, .. } => with.get(key).map(String::as_str),
        _ => None,
    }
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

fn rust_repo(policy: WorkflowPolicy, non_inputs: bool) -> Result<TempDir, Box<dyn Error>> {
    let repo = mbx_repo_for(true, policy)?;
    let root = repo.path();
    fs::remove_dir_all(root.join("src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/demo\"]\nresolver = \"2\"\n",
    )?;
    fs::create_dir_all(root.join("crates/demo/src"))?;
    fs::write(
        root.join("crates/demo/Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("crates/demo/src/lib.rs"), "pub fn f() {}\n")?;
    let config_path = repo.path().join(".velnor/config.toml");
    let mut config = fs::read_to_string(&config_path)?;
    config.push_str("\n[stacks.rust]\n");
    if non_inputs {
        config.push_str("non_inputs = [\"docs/**\"]\n");
    }
    config.push_str(RUST_SUITE);
    config.push_str(STANDALONE_SUITE);
    config.push_str(NAMED_CHECK);
    fs::write(
        root.join("mise.toml"),
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    fs::write(&config_path, config)?;
    fs::create_dir_all(repo.path().join("docs"))?;
    fs::write(repo.path().join("docs/a.md"), "a\n")?;
    Ok(repo)
}

#[test]
fn every_rust_job_checkout_excludes_non_inputs_and_other_jobs_do_not() -> TestResult {
    let mut seen = BTreeSet::new();
    for policy in POLICIES {
        let repo = rust_repo(policy, true)?;
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
        let omitted = rust_repo(policy, false)?;
        let explicit = rust_repo(policy, false)?;
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

fn guard_run(root: &Path) -> Result<std::process::Output, Box<dyn Error>> {
    let repo = rust_repo(WorkflowPolicy::ConsumerV1, true)?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    let job = jobs
        .values()
        .find(|job| {
            job.steps
                .iter()
                .any(|step| step.name == NON_INPUT_GUARD_STEP)
        })
        .ok_or("no rendered guard step")?;
    let guard = job
        .steps
        .iter()
        .find(|step| step.name == NON_INPUT_GUARD_STEP)
        .ok_or("guard step missing")?;
    let StepKind::Shell { run, .. } = &guard.kind else {
        return Err("guard is not a shell step".into());
    };
    let (program, args) = run.split_first().ok_or("empty guard command")?;
    Ok(Command::new(program)
        .args(args)
        .current_dir(root)
        .output()?)
}

fn tracked_tree() -> Result<TempDir, Box<dyn Error>> {
    let tree = tempfile::tempdir()?;
    let root = tree.path();
    git(&["init", "-b", "testmain"], root)?;
    for (path, body) in [
        ("docs/a.md", "a\n"),
        ("docs/sub/b.md", "b\n"),
        ("docsx/c.md", "c\n"),
        ("apps/a1/docs/d.md", "d\n"),
        ("crates/x/src/lib.rs", "pub fn f() {}\n"),
    ] {
        let file = root.join(path);
        fs::create_dir_all(file.parent().ok_or("no parent")?)?;
        fs::write(file, body)?;
    }
    git(&["add", "-A"], root)?;
    git(&["commit", "-m", "tree"], root)?;
    Ok(tree)
}

fn git_output(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = workflow_configuration_fixture::git_fixture::command(root)?
        .args(args)
        .output()?;
    Ok(String::from_utf8(output.stdout)?)
}

fn apply_exclusion(root: &Path) -> TestResult {
    git(&["config", "core.sparseCheckout", "true"], root)?;
    let info = git_output(root, &["rev-parse", "--git-path", "info/sparse-checkout"])?;
    fs::create_dir_all(root.join(info.trim()).parent().ok_or("no parent")?)?;
    fs::write(root.join(info.trim()), format!("{EXCLUSION}\n"))?;
    git(&["read-tree", "-mu", "HEAD"], root)?;
    Ok(())
}

#[test]
fn rendered_exclusion_removes_non_inputs_on_a_real_git_tree() -> TestResult {
    let tree = tracked_tree()?;
    let root = tree.path();
    apply_exclusion(root)?;
    for gone in ["docs/a.md", "docs/sub/b.md"] {
        assert!(!root.join(gone).exists(), "{gone} survived the exclusion");
    }
    for kept in ["docsx/c.md", "apps/a1/docs/d.md", "crates/x/src/lib.rs"] {
        assert!(root.join(kept).exists(), "{kept} was over-matched");
    }
    assert_eq!(git_output(root, &["status", "--porcelain"])?, "");
    Ok(())
}

#[test]
fn absence_guard_fails_when_a_non_input_is_present() -> TestResult {
    let tree = tracked_tree()?;
    let root = tree.path();
    let present = guard_run(root)?;
    assert!(!present.status.success(), "guard passed with docs present");
    let stderr = String::from_utf8(present.stderr)?;
    assert!(stderr.contains("non_input_present:docs/"), "{stderr}");
    apply_exclusion(root)?;
    let absent = guard_run(root)?;
    assert!(
        absent.status.success(),
        "{}",
        String::from_utf8_lossy(&absent.stderr)
    );
    let only_lookalikes = tempfile::tempdir()?;
    let lookalikes = only_lookalikes.path();
    git(&["init", "-b", "testmain"], lookalikes)?;
    fs::create_dir_all(lookalikes.join("docsx"))?;
    fs::write(lookalikes.join("docsx/c.md"), "c\n")?;
    git(&["add", "-A"], lookalikes)?;
    git(&["commit", "-m", "lookalike"], lookalikes)?;
    assert!(guard_run(lookalikes)?.status.success());
    Ok(())
}
