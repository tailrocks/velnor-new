// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, ImageError, expected_files};
use velnor_model::{RepoPath, RepoView};

use crate::facts::TestResult;
use crate::image_workflow::{edited, sample};

fn plain_with(
    change: impl FnOnce(String) -> String,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    edited("image-plain", change)
}

#[test]
fn two_images_cannot_share_a_registry_repository_or_a_release_tag_family() -> TestResult {
    let second = |name: &str, repository: &str| {
        format!(
            "\n[[image]]\nname = \"{name}\"\nbins = [\"plain-svc\"]\n\n[[image.registry]]\nhost = \"docker.io\"\nrepository = \"{repository}\"\nusername = \"secrets.DOCKERHUB_USERNAME\"\ntoken = \"secrets.DOCKERHUB_TOKEN\"\n"
        )
    };
    let shared = plain_with(|text| format!("{text}{}", second("other-svc", "acme/plain-svc")))?;
    assert!(matches!(
        expected_files(&shared),
        Err(GenerateError::Image(ImageError::RegistryShared { .. }))
    ));
    let overlapping = plain_with(|text| format!("{text}{}", second("plain-svc-v2", "acme/other")))?;
    assert!(matches!(
        expected_files(&overlapping),
        Err(GenerateError::Image(ImageError::TagFamilies { .. }))
    ));
    let distinct = plain_with(|text| format!("{text}{}", second("other-svc", "acme/other")))?;
    assert!(expected_files(&distinct).is_ok());
    Ok(())
}

#[test]
fn a_default_branch_that_is_not_an_image_tag_is_refused() -> TestResult {
    let view = plain_with(|text| {
        text.replace(
            "default_branch = \"main\"",
            "default_branch = \"release/stable\"",
        )
    })?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::BranchTag { .. }))
    ));
    Ok(())
}

#[test]
fn the_sysroot_lock_and_a_custom_profile_must_exist() -> TestResult {
    let mut view = sample("image-workspace")?;
    let lock = RepoPath::new(".velnor/image-sysroot.lock")?;
    view.files.remove(&lock);
    view.tracked_paths.remove(&lock);
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::SysrootLockMissing { .. }))
    ));
    let view = plain_with(|text| {
        text.replace(
            "pr_build = \"always\"",
            "pr_build = \"always\"\nprofile = \"fast\"",
        )
    })?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::ProfileUndefined { .. }))
    ));
    let mut view = view;
    let manifest = RepoPath::new("Cargo.toml")?;
    let text = String::from_utf8(view.files.remove(&manifest).ok_or("no manifest")?)?;
    view.files.insert(
        manifest,
        format!("{text}\n[profile.fast]\ninherits = \"release\"\n").into_bytes(),
    );
    assert!(expected_files(&view).is_ok());
    Ok(())
}

#[test]
fn a_tool_pinned_only_for_another_operating_system_does_not_count() -> TestResult {
    let mut view = sample("image-plain")?;
    let path = RepoPath::new("mise.toml")?;
    let text = String::from_utf8(view.files.remove(&path).ok_or("no mise.toml")?)?;
    view.files.insert(
        path,
        text.replace(
            "zig = \"0.17.1\"",
            "zig = { version = \"0.17.1\", os = [\"macos\"] }",
        )
        .into_bytes(),
    );
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::ToolMissing {
            tool: "zig",
            ..
        }))
    ));
    Ok(())
}

#[test]
fn the_dockerfile_must_be_tracked_and_the_runner_must_be_linux() -> TestResult {
    let mut view = sample("image-plain")?;
    let dockerfile = RepoPath::new("Dockerfile")?;
    view.files.remove(&dockerfile);
    view.tracked_paths.remove(&dockerfile);
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::DockerfileMissing { .. }))
    ));
    let view = plain_with(|text| {
        text.replace(
            "runner = \"ubuntu-26.04-scale-set\"\nhosted = false\nfork_runner = \"ubuntu-latest\"",
            "runner = \"macos-15\"\nhosted = true",
        )
    })?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::NotLinuxRunner { .. }))
    ));
    Ok(())
}

#[test]
fn a_tool_pin_must_apply_on_the_runner_platform_the_way_mise_decides() -> TestResult {
    let pinned = |filter: &str| -> Result<RepoView, Box<dyn std::error::Error>> {
        let mut view = sample("image-plain")?;
        let path = RepoPath::new("mise.toml")?;
        let text = String::from_utf8(view.files.remove(&path).ok_or("no mise.toml")?)?;
        view.files.insert(
            path,
            text.replace(
                "zig = \"0.17.1\"",
                &format!("zig = {{ version = \"0.17.1\", os = [\"{filter}\"] }}"),
            )
            .into_bytes(),
        );
        Ok(view)
    };
    assert!(matches!(
        expected_files(&pinned("linux/arm64")?),
        Err(GenerateError::Image(ImageError::ToolMissing {
            tool: "zig",
            ..
        }))
    ));
    assert!(expected_files(&pinned("unix")?).is_ok());
    assert!(expected_files(&pinned("linux/x64")?).is_ok());
    Ok(())
}
