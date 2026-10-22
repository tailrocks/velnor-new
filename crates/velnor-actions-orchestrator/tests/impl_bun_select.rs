//! End-to-end `plan-v1` dispositions of Bun member jobs.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{JobDisposition, Plan};

use crate::impl_bun::{LOCK, MISE, ROOT_PACKAGE};
use crate::impl_common::TestResult;
use crate::impl_select::{commit, plan_pr, plan_push};
use crate::impl_select_affected_jobs::{fixture, put};

const BUN: &str = "
[stacks.bun]
roots = [\"frontend\"]

[[stacks.bun.inputs]]
globs = [\"shared/**\"]
members = [\"@chainargos/ui\"]
complete = true
";

/// Rust fixture plus the Bun workspace, committed as the base.
fn bun_fixture() -> Result<(tempfile::TempDir, String), Box<dyn std::error::Error>> {
    bun_fixture_with("")
}

/// [`bun_fixture`] with extra `[stacks.bun]` configuration appended.
fn bun_fixture_with(
    extra: &str,
) -> Result<(tempfile::TempDir, String), Box<dyn std::error::Error>> {
    let (dir, _) = fixture(&format!("{BUN}{extra}"), false)?;
    let root = dir.path();
    put(root, "mise.toml", MISE)?;
    put(root, "frontend/package.json", ROOT_PACKAGE)?;
    put(root, "frontend/bun.lock", LOCK)?;
    put(
        root,
        "frontend/docs/package.json",
        r#"{"name":"chainargos-docs","scripts":{"test":"vitest run"}}"#,
    )?;
    put(
        root,
        "frontend/packages/ui/package.json",
        r#"{"name":"@chainargos/ui","scripts":{"test":"vitest run"}}"#,
    )?;
    put(root, "frontend/docs/src/a.ts", "export const a = 1;\n")?;
    put(
        root,
        "frontend/packages/ui/src/b.ts",
        "export const b = 1;\n",
    )?;
    put(root, "shared/data.json", "{}\n")?;
    let base = commit(root, "bun")?;
    Ok((dir, base))
}

fn runs(plan: &Plan) -> BTreeSet<&str> {
    plan.jobs
        .iter()
        .filter(|job| job.job_id.starts_with("bun-") && job.disposition == JobDisposition::Run)
        .map(|job| job.job_id.as_str())
        .collect()
}

fn pr(edit: impl FnOnce(&Path) -> TestResult) -> Result<Plan, Box<dyn std::error::Error>> {
    let (dir, base) = bun_fixture()?;
    edit(dir.path())?;
    let head = commit(dir.path(), "change")?;
    Ok(plan_pr(dir.path(), Some(&base), &head)?.0)
}

#[test]
fn a_rust_only_change_skips_every_member() -> TestResult {
    let plan = pr(|root| put(root, "apps/a2/src/lib.rs", "pub fn g() {}\n"))?;
    assert!(runs(&plan).is_empty(), "{:?}", plan.jobs);
    assert_eq!(
        plan.jobs
            .iter()
            .filter(|job| job.job_id.starts_with("bun-"))
            .count(),
        2
    );
    Ok(())
}

#[test]
fn a_member_change_selects_it_and_its_workspace_dependents() -> TestResult {
    let plan = pr(|root| {
        put(
            root,
            "frontend/packages/ui/src/b.ts",
            "export const b = 2;\n",
        )
    })?;
    assert_eq!(
        runs(&plan),
        BTreeSet::from(["bun-chainargos-docs", "bun-chainargos-ui"])
    );
    let plan = pr(|root| put(root, "frontend/docs/src/a.ts", "export const a = 2;\n"))?;
    assert_eq!(runs(&plan), BTreeSet::from(["bun-chainargos-docs"]));
    Ok(())
}

#[test]
fn shared_files_and_declared_inputs_select_members() -> TestResult {
    let plan = pr(|root| put(root, "frontend/bun.lock", &format!("{LOCK}\n")))?;
    assert_eq!(
        runs(&plan),
        BTreeSet::from(["bun-chainargos-docs", "bun-chainargos-ui"])
    );
    let plan = pr(|root| put(root, "shared/data.json", "{\"x\":1}\n"))?;
    assert_eq!(
        runs(&plan),
        BTreeSet::from(["bun-chainargos-docs", "bun-chainargos-ui"])
    );
    Ok(())
}

#[test]
fn a_push_runs_every_member() -> TestResult {
    let (dir, base) = bun_fixture()?;
    put(
        dir.path(),
        "frontend/docs/src/a.ts",
        "export const a = 3;\n",
    )?;
    let head = commit(dir.path(), "change")?;
    let (plan, _) = plan_push(dir.path(), Some(&base), &head)?;
    assert_eq!(
        runs(&plan),
        BTreeSet::from(["bun-chainargos-docs", "bun-chainargos-ui"])
    );
    Ok(())
}

const SUITES: &str = "
[stacks.bun.members.\"chainargos-docs\".suites.unit]
scripts = [\"test\"]

[stacks.bun.members.\"@chainargos/ui\".suites.smoke]
scripts = [\"test\"]
required = false
";

fn bun_jobs(plan: &Plan) -> Vec<(&str, JobDisposition)> {
    plan.jobs
        .iter()
        .filter(|job| job.job_id.starts_with("bun-"))
        .map(|job| (job.job_id.as_str(), job.disposition))
        .collect()
}

fn pr_with(
    extra: &str,
    edit: impl FnOnce(&Path) -> TestResult,
) -> Result<Plan, Box<dyn std::error::Error>> {
    let (dir, base) = bun_fixture_with(extra)?;
    edit(dir.path())?;
    let head = commit(dir.path(), "change")?;
    Ok(plan_pr(dir.path(), Some(&base), &head)?.0)
}

#[test]
fn suite_jobs_carry_their_member_disposition_and_omitted_member_jobs_have_none() -> TestResult {
    use JobDisposition::{NotAffected, Run};
    let plan = pr_with(SUITES, |root| {
        put(root, "apps/a2/src/lib.rs", "pub fn g() {}\n")
    })?;
    assert_eq!(
        bun_jobs(&plan),
        [
            ("bun-chainargos-docs--unit", NotAffected),
            ("bun-chainargos-ui--smoke", NotAffected),
        ]
    );
    let plan = pr_with(SUITES, |root| {
        put(root, "frontend/docs/src/a.ts", "export const a = 2;\n")
    })?;
    assert_eq!(
        bun_jobs(&plan),
        [
            ("bun-chainargos-docs--unit", Run),
            ("bun-chainargos-ui--smoke", NotAffected),
        ]
    );
    let plan = pr_with(SUITES, |root| {
        put(
            root,
            "frontend/packages/ui/src/b.ts",
            "export const b = 2;\n",
        )
    })?;
    assert_eq!(
        bun_jobs(&plan),
        [
            ("bun-chainargos-docs--unit", Run),
            ("bun-chainargos-ui--smoke", Run),
        ]
    );
    Ok(())
}

#[test]
fn a_push_runs_every_suite() -> TestResult {
    let (dir, base) = bun_fixture_with(SUITES)?;
    put(
        dir.path(),
        "frontend/docs/src/a.ts",
        "export const a = 3;\n",
    )?;
    let head = commit(dir.path(), "change")?;
    let (plan, _) = plan_push(dir.path(), Some(&base), &head)?;
    assert_eq!(
        runs(&plan),
        BTreeSet::from(["bun-chainargos-docs--unit", "bun-chainargos-ui--smoke"])
    );
    Ok(())
}
