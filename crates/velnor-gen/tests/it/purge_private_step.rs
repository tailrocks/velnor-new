// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, names, step};
use crate::facts::TestResult;

const PURGE: &str = "Delete the private-base caches unless the repository is private";

fn trim(version: &str) -> Result<String, Box<dyn std::error::Error>> {
    jobs(version, "shape = \"lint-test\"\n")?
        .into_iter()
        .find(|(id, _)| id == "rust-trim")
        .map(|(_, block)| block)
        .ok_or_else(|| "no trim job".into())
}

#[test]
fn from_helper_0_1_112_the_purge_is_the_first_trim_step_and_runs_after_an_earlier_failure()
-> TestResult {
    let block = trim("0.1.112")?;
    let order = names(&block);
    let install = order
        .iter()
        .position(|name| name == "Install the released helper launcher")
        .ok_or("no install step")?;
    assert_eq!(order.get(install + 1).map(String::as_str), Some(PURGE));
    let purge = step(&block, PURGE).ok_or("no purge step")?;
    assert!(
        purge.contains("run: velnor-ci cache purge-private --if-public\n"),
        "{purge}"
    );
    assert!(
        purge.contains("GITHUB_TOKEN: '${{ github.token }}'"),
        "{purge}"
    );
    assert!(
        purge.contains("if: '${{ !cancelled() }}'"),
        "a step without a status check is skipped after an earlier failure: {purge}"
    );
    assert!(!purge.contains("success()"), "{purge}");
    assert!(!purge.contains("--yes"), "{purge}");
    let budget = order
        .iter()
        .position(|name| name == "Enforce the repository cache budget")
        .ok_or("no budget step")?;
    assert!(install + 1 < budget, "{order:?}");
    Ok(())
}

#[test]
fn pinned_helpers_below_0_1_112_render_no_purge_step() -> TestResult {
    for version in ["0.1.81", "0.1.111"] {
        let block = trim(version)?;
        assert!(step(&block, PURGE).is_none(), "{version}: {block}");
        assert!(!block.contains("purge-private"), "{version}: {block}");
    }
    Ok(())
}
