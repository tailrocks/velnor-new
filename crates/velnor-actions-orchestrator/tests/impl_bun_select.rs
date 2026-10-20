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
    let (dir, _) = fixture(BUN, false)?;
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
