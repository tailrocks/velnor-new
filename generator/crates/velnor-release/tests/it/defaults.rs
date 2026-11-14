// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleasePlan;

use crate::fixture::{TestResult, files};

#[test]
fn valid_files_without_velnor_config_use_the_defaults() -> TestResult {
    let plan = ReleasePlan::read(&files())?;
    assert_eq!(plan.after, ["Rust CI"]);
    assert_eq!(plan.roots.len(), 1);
    assert_eq!(plan.roots[0].root, ".");
    assert_eq!(plan.roots[0].pr_branch_prefix, "release-plz-");
    assert_eq!(plan.roots[0].packages[0].0, "app");
    assert_eq!(plan.roots[0].packages[0].1.as_str(), "app-v{{ version }}");
    assert_eq!(plan.binaries.len(), 0);
    Ok(())
}

#[test]
fn binaries_get_default_platforms_and_the_tag_pattern_of_their_package() -> TestResult {
    let mut files = files();
    files.release_toml = Some(
        "after = [\"CI\"]\n[[binaries]]\npackage = \"app\"\n[[binaries]]\npackage = \"tool\"\nplatforms = [\"linux-x64\"]\n"
            .to_owned(),
    );
    let plan = ReleasePlan::read(&files)?;
    assert_eq!(plan.after, ["CI"]);
    assert_eq!(plan.binaries.len(), 2);
    assert_eq!(
        plan.binaries[0].platforms,
        ["linux-x64", "linux-arm64", "macos-arm64"]
    );
    assert_eq!(plan.binaries[1].platforms, ["linux-x64"]);
    assert_eq!(plan.binaries[1].tag_pattern.as_str(), "tool-v{{ version }}");
    Ok(())
}
