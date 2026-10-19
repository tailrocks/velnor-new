//! End-to-end `plan-v1` crate-job dispositions for affected pull requests.
//!
//! Fixture: two libraries (`core`, `util`) and eight apps (`a1`..`a8`) with
//! a real offline lockfile. `util` is used only by `a2`; `core` by the
//! other apps through normal, dev, target-specific, and optional edges.
//! `a8` declares `protos/**` and `libs/core/data/**` as inputs. Each case
//! asserts the exact set of crate jobs the plan schedules (`run`); every
//! other crate job must be `not_affected`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::{JobDisposition, ObligationDecision, Plan};

use crate::impl_common::{TestResult, config_with_branch, fixture_manifest_json, git};
use crate::impl_select::{commit, plan_pr, plan_push};

/// Every crate job of the fixture, by package name.
pub(crate) const PACKAGES: [&str; 10] = [
    "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "core", "util",
];

/// Write `body` to `root/path`, creating parents.
pub(crate) fn put(root: &Path, path: &str, body: &str) -> TestResult {
    let file = root.join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(file, body)?;
    Ok(())
}

/// Committed fixture repository plus its base commit.
pub(crate) fn fixture(
    extra_config: &str,
    build_script: bool,
) -> Result<(TempDir, String), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    put(
        root,
        ".velnor/config.toml",
        &format!(
            "{}\n[[stacks.rust.inputs]]\nglobs = [\"protos/**/*.proto\", \"libs/core/data/**\"]\npackages = [\"a8\"]\n{extra_config}",
            config_with_branch()
        ),
    )?;
    put(
        root,
        ".velnor/release-manifest.json",
        &fixture_manifest_json(),
    )?;
    let members = PACKAGES
        .iter()
        .map(|name| {
            if name.starts_with('a') {
                format!("\"apps/{name}\"")
            } else {
                format!("\"libs/{name}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    put(
        root,
        "Cargo.toml",
        &format!("[workspace]\nmembers = [{members}]\nresolver = \"2\"\n"),
    )?;
    let core = "core = { path = \"../../libs/core\" }\n";
    let tables = [
        ("libs/core", "core", String::new()),
        ("libs/util", "util", String::new()),
        ("apps/a1", "a1", format!("[dependencies]\n{core}")),
        ("apps/a2", "a2", "[dependencies]\nutil = { path = \"../../libs/util\" }\n".to_owned()),
        ("apps/a3", "a3", format!("[dev-dependencies]\n{core}")),
        ("apps/a4", "a4", format!("[dependencies]\n{core}")),
        ("apps/a5", "a5", format!("[target.'cfg(unix)'.dependencies]\n{core}")),
        (
            "apps/a6",
            "a6",
            "[features]\nextra = [\"dep:core\"]\n[dependencies]\ncore = { path = \"../../libs/core\", optional = true }\n"
                .to_owned(),
        ),
        ("apps/a7", "a7", String::new()),
        ("apps/a8", "a8", String::new()),
    ];
    for (path, name, deps) in &tables {
        put(
            root,
            &format!("{path}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{deps}"
            ),
        )?;
        put(root, &format!("{path}/src/lib.rs"), "pub fn f() {}\n")?;
    }
    if build_script {
        put(root, "apps/a7/build.rs", "fn main() {}\n")?;
    }
    put(root, "libs/core/data/schema.sql", "select 1;\n")?;
    put(root, "protos/v1/a8.proto", "syntax = \"proto3\";\n")?;
    put(root, "docs/notes.md", "notes\n")?;
    let status = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .status()?;
    assert!(status.success(), "generate-lockfile failed");
    let base = commit(root, "base")?;
    Ok((dir, base))
}

/// Crate jobs the plan schedules (`run`), by package name.
pub(crate) fn running(plan: &Plan) -> BTreeSet<String> {
    let mut running = BTreeSet::new();
    for job in &plan.jobs {
        let Some(name) = job.job_id.strip_prefix("rust-") else {
            continue;
        };
        if job.disposition == JobDisposition::Run {
            running.insert(name.to_owned());
        }
    }
    running
}

/// Assert the exact running crate jobs; everything else is skipped.
pub(crate) fn assert_runs(plan: &Plan, expected: &[&str]) {
    let expected: BTreeSet<String> = expected.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(running(plan), expected, "{:?}", plan.jobs);
    for job in &plan.jobs {
        let Some(name) = job.job_id.strip_prefix("rust-") else {
            continue;
        };
        if !expected.contains(name) {
            assert_eq!(job.disposition, JobDisposition::NotAffected, "{job:?}");
        }
    }
    let crate_jobs = plan
        .jobs
        .iter()
        .filter(|job| job.job_id.starts_with("rust-"))
        .count();
    assert_eq!(crate_jobs, PACKAGES.len(), "{:?}", plan.jobs);
    for obligation in &plan.obligations {
        let runs = obligation.decision == ObligationDecision::Execute;
        let job_runs = plan
            .jobs
            .iter()
            .any(|job| job.job_id == obligation.job_id && job.disposition == JobDisposition::Run);
        assert_eq!(runs, job_runs, "{obligation:?}");
    }
    for entry in &plan.matrix.include {
        assert!(
            running(plan)
                .iter()
                .any(|name| entry.job_id == format!("rust-{name}"))
                || !entry.job_id.starts_with("rust-"),
            "{entry:?}"
        );
    }
}

/// Apply one change, commit, and plan it as a pull request.
fn plan_change(edit: impl FnOnce(&Path) -> TestResult) -> Result<Plan, Box<dyn std::error::Error>> {
    let (dir, base) = fixture("", false)?;
    edit(dir.path())?;
    let head = commit(dir.path(), "change")?;
    Ok(plan_pr(dir.path(), Some(&base), &head)?.0)
}

#[test]
fn library_used_by_one_app_runs_exactly_two_jobs() -> TestResult {
    let plan = plan_change(|root| put(root, "libs/util/src/lib.rs", "pub fn g() {}\n"))?;
    assert_runs(&plan, &["a2", "util"]);
    Ok(())
}

#[test]
fn one_app_runs_one_job() -> TestResult {
    let plan = plan_change(|root| put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n"))?;
    assert_runs(&plan, &["a1"]);
    Ok(())
}

#[test]
fn shared_library_runs_every_dependent_kind() -> TestResult {
    let plan = plan_change(|root| put(root, "libs/core/src/lib.rs", "pub fn g() {}\n"))?;
    assert_runs(&plan, &["a1", "a3", "a4", "a5", "a6", "core"]);
    Ok(())
}

#[test]
fn lockfile_change_runs_everything() -> TestResult {
    let plan = plan_change(|root| {
        let lock = fs::read_to_string(root.join("Cargo.lock"))?;
        fs::write(root.join("Cargo.lock"), format!("{lock}\n"))?;
        Ok(())
    })?;
    assert_runs(&plan, &PACKAGES);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w == "cargo_lock_changed:all_changed")
    );
    Ok(())
}

#[test]
fn unowned_file_runs_everything() -> TestResult {
    let plan = plan_change(|root| put(root, "docs/notes.md", "changed\n"))?;
    assert_runs(&plan, &PACKAGES);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w == "unclassified_files:all_changed")
    );
    Ok(())
}

#[test]
fn declared_proto_input_runs_its_consumer() -> TestResult {
    let plan = plan_change(|root| put(root, "protos/v1/a8.proto", "syntax = \"proto3\"; //\n"))?;
    assert_runs(&plan, &["a8"]);
    Ok(())
}

#[test]
fn cross_crate_declared_input_runs_owner_and_reader() -> TestResult {
    let plan = plan_change(|root| put(root, "libs/core/data/schema.sql", "select 2;\n"))?;
    assert_runs(&plan, &["a1", "a3", "a4", "a5", "a6", "a8", "core"]);
    Ok(())
}

#[test]
fn build_script_packages_always_run() -> TestResult {
    let (dir, base) = fixture("", true)?;
    put(dir.path(), "apps/a1/src/lib.rs", "pub fn g() {}\n")?;
    let head = commit(dir.path(), "change")?;
    let plan = plan_pr(dir.path(), Some(&base), &head)?.0;
    assert_runs(&plan, &["a1", "a7"]);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w == "undeclared_reads:a7:build_script"),
        "{:?}",
        plan.warnings
    );
    Ok(())
}

#[test]
fn push_runs_everything() -> TestResult {
    let (dir, base) = fixture("", false)?;
    put(dir.path(), "apps/a1/src/lib.rs", "pub fn g() {}\n")?;
    let head = commit(dir.path(), "change")?;
    let (plan, warnings) = plan_push(dir.path(), Some(&base), &head)?;
    assert_runs(&plan, &PACKAGES);
    assert!(
        warnings
            .iter()
            .any(|w| w == "full_run_event:push:all_changed")
    );
    Ok(())
}

#[test]
fn unknown_input_package_fails_configuration() -> TestResult {
    let (dir, base) = fixture(
        "\n[[stacks.rust.inputs]]\nglobs = [\"x/**\"]\npackages = [\"ghost\"]\n",
        false,
    )?;
    let error = plan_pr(dir.path(), Some(&base), &base).expect_err("unknown package rejected");
    assert!(
        error.to_string().contains("unknown_package:ghost"),
        "{error}"
    );
    Ok(())
}
