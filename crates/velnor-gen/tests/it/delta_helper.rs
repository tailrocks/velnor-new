// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{pinned, step};
use crate::delta_cache::OLDEST_HELPER;
use crate::facts::TestResult;
use crate::pipeline::workflow;
use velnor_gen::expected_files;
use velnor_model::{RepoPath, RepoView};

#[test]
fn a_workspace_with_many_crates_stages_its_delta_through_one_package_variable() -> TestResult {
    let repo = crate::golden::load(&crate::golden::samples().join("lab-150"))?;
    let repo = with_helper(repo, "0.1.93", "0.1.95")?;
    let files = expected_files(&repo)?;
    let text = files
        .get(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or("no rust-ci workflow")?;
    let stage = step(text, "velnor-ci delta stage").ok_or("no delta stage")?;
    assert!(!stage.contains("--package"), "{stage}");
    let packages: Vec<&str> = stage
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("VELNOR_DELTA_PACKAGES:"))
        .skip(1)
        .take_while(|line| line.starts_with("            "))
        .map(str::trim)
        .collect();
    assert!(packages.contains(&"app"), "{stage}");
    assert!(packages.contains(&"grow-149"), "{stage}");
    Ok(())
}

/// Re-pins a sample's helper lock to a release the sample does not lock itself.
fn with_helper(
    mut repo: RepoView,
    from: &str,
    to: &str,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    let text = String::from_utf8(repo.files.remove(&lock).ok_or("no helper lock")?)?;
    if !text.contains(from) {
        return Err(format!("the sample lock does not pin {from}").into());
    }
    repo.files.insert(lock, text.replace(from, to).into_bytes());
    Ok(repo)
}

#[test]
fn a_workspace_locked_at_0_1_93_renders_no_delta_steps() -> TestResult {
    let repo = crate::golden::load(&crate::golden::samples().join("lab-150"))?;
    let files = expected_files(&repo)?;
    let text = files
        .get(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or("no rust-ci workflow")?;
    assert!(!text.contains("velnor-ci delta"), "{text}");
    assert!(!text.contains("the delta cache"), "{text}");
    Ok(())
}

#[test]
fn delta_steps_need_a_helper_that_has_the_delta_command() -> TestResult {
    let old = workflow(&pinned(OLDEST_HELPER, "")?)?;
    assert!(!old.contains("velnor-ci delta"), "{old}");
    assert!(!old.contains("velnor-dlt-"), "{old}");
    let new = workflow(&pinned("0.1.95", "")?)?;
    assert!(new.contains("velnor-ci delta prepare"), "{new}");
    assert!(new.contains("Restore the delta cache"), "{new}");
    Ok(())
}

#[test]
fn helper_0_1_94_gets_no_delta_steps_and_0_1_95_gets_them() -> TestResult {
    let refused = workflow(&pinned("0.1.94", "")?)?;
    assert!(!refused.contains("velnor-ci delta"), "{refused}");
    assert!(!refused.contains("the delta cache"), "{refused}");
    let accepted = workflow(&pinned("0.1.95", "")?)?;
    assert!(accepted.contains("velnor-ci delta prepare"), "{accepted}");
    Ok(())
}
