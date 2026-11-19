// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::Platform;
use velnor_release::{ReleaseError, ReleasePlan};

use crate::fixture::{TestResult, edit, files, plz_edited, refusal};

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
    let mut files = plz_edited(
        "name = \"app\"",
        "name = \"app\"\n\n[[package]]\nname = \"tool\"",
    )?;
    files.release_toml = Some(
        "after = [\"CI\"]\n[[binaries]]\npackage = \"app\"\n[[binaries]]\npackage = \"tool\"\nplatforms = [\"linux-x64\"]\n"
            .to_owned(),
    );
    let plan = ReleasePlan::read(&files)?;
    assert_eq!(plan.after, ["CI"]);
    assert_eq!(plan.binaries.len(), 2);
    assert_eq!(
        plan.binaries[0].platforms,
        [
            Platform::LinuxX64,
            Platform::LinuxArm64,
            Platform::MacosArm64
        ]
    );
    assert_eq!(plan.binaries[1].platforms, [Platform::LinuxX64]);
    assert_eq!(plan.binaries[0].bin, "app");
    assert_eq!(plan.binaries[1].tag_pattern.as_str(), "tool-v{{ version }}");
    Ok(())
}

#[test]
fn a_workspace_that_releases_next_to_listed_packages_must_name_a_tag() -> TestResult {
    let mut files = plz_edited(
        "name = \"app\"\n",
        "name = \"app\"\ngit_tag_name = \"app-v{{ version }}\"\n",
    )?;
    let listed_only = edit(
        &files.release_plz_toml["."],
        "git_tag_name = \"{{ package }}-v{{ version }}\"\n",
        "",
    )?;
    files.release_plz_toml.insert(".".to_owned(), listed_only);
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::MissingGitTagName { package: None, .. })
    ));
    Ok(())
}

#[test]
fn a_root_publishes_to_crates_io_unless_every_released_package_opts_out() -> TestResult {
    assert!(ReleasePlan::read(&files())?.roots[0].publishes_to_crates_io);
    for opt_out in [
        ("publish_no_verify = true", "git_only = true"),
        ("publish_no_verify = true", "publish = false"),
    ] {
        let plan = ReleasePlan::read(&plz_edited(opt_out.0, opt_out.1)?)?;
        assert!(!plan.roots[0].publishes_to_crates_io, "{}", opt_out.1);
    }
    let one_of_two = plz_edited(
        "name = \"app\"",
        "name = \"app\"\ngit_only = true\n\n[[package]]\nname = \"tool\"",
    )?;
    assert!(ReleasePlan::read(&one_of_two)?.roots[0].publishes_to_crates_io);
    Ok(())
}

#[test]
fn a_binary_may_name_its_bin_and_a_repeated_platform_counts_once() -> TestResult {
    let mut files = files();
    files.release_toml = Some(
        "[[binaries]]\npackage = \"app\"\nbin = \"rg\"\nplatforms = [\"linux-x64\", \"linux-x64\"]\n"
            .to_owned(),
    );
    let plan = ReleasePlan::read(&files)?;
    assert_eq!(plan.binaries[0].bin, "rg");
    assert_eq!(plan.binaries[0].platforms, [Platform::LinuxX64]);
    Ok(())
}
