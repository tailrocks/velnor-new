// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, names, step};
use crate::facts::TestResult;

const PURGE: &str = "Delete the private-base caches unless the repository is private";
const BUDGET: &str = "Enforce the repository cache budget";

fn trim(version: &str) -> Result<String, Box<dyn std::error::Error>> {
    jobs(version, "shape = \"lint-test\"\n")?
        .into_iter()
        .find(|(id, _)| id == "rust-trim")
        .map(|(_, block)| block)
        .ok_or_else(|| "no trim job".into())
}

#[test]
fn from_helper_0_1_112_the_purge_is_the_last_trim_step_so_a_failed_purge_skips_no_maintenance()
-> TestResult {
    let block = trim("0.1.112")?;
    let order = names(&block);
    assert_eq!(order.last().map(String::as_str), Some(PURGE), "{order:?}");
    let budget = order
        .iter()
        .position(|name| name == BUDGET)
        .ok_or("no budget step")?;
    assert_eq!(budget + 2, order.len(), "{order:?}");
    assert_eq!(
        order.iter().filter(|name| *name == PURGE).count(),
        1,
        "{order:?}"
    );
    Ok(())
}

#[test]
fn the_purge_runs_after_an_earlier_step_fails() -> TestResult {
    let block = trim("0.1.112")?;
    let purge = step(&block, PURGE).ok_or("no purge step")?;
    assert!(
        purge.contains("if: '${{ !cancelled() }}'"),
        "a step without a status check is skipped after an earlier failure: {purge}"
    );
    assert!(!purge.contains("success()"), "{purge}");
    assert!(
        purge.contains("run: velnor-ci cache purge-private --if-public\n"),
        "{purge}"
    );
    assert!(
        purge.contains("GITHUB_TOKEN: '${{ github.token }}'"),
        "{purge}"
    );
    assert!(!purge.contains("--yes"), "{purge}");
    let budget = step(&block, BUDGET).ok_or("no budget step")?;
    assert!(!budget.contains("    if:"), "{budget}");
    Ok(())
}

#[test]
fn pinned_helpers_below_0_1_112_render_no_purge_step_and_keep_the_budget_last() -> TestResult {
    for version in ["0.1.81", "0.1.111"] {
        let block = trim(version)?;
        assert!(step(&block, PURGE).is_none(), "{version}: {block}");
        assert!(!block.contains("purge-private"), "{version}: {block}");
        assert_eq!(
            names(&block).last().map(String::as_str),
            Some(BUDGET),
            "{version}"
        );
    }
    Ok(())
}
