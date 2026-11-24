// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, ImageError, expected_files};
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::image_workflow::{edited, rendered, sample};

#[test]
fn a_title_edit_of_a_pull_request_neither_cancels_nor_joins_the_image_run_of_the_pull_request()
-> TestResult {
    for name in ["image-plain", "image-ghcr", "image-workspace"] {
        let workflow = rendered(name)?;
        let group = workflow["concurrency"]["group"].to_string();
        let cancel = workflow["concurrency"]["cancel-in-progress"].to_string();
        for text in [&group, &cancel] {
            assert!(
                text.contains(
                    "github.event.action == 'edited' && github.event.changes.base == null"
                ),
                "{name}: {text}"
            );
        }
        assert!(group.contains("github.run_id"), "{name}: {group}");
    }
    Ok(())
}

#[test]
fn a_default_branch_that_looks_like_a_release_version_is_refused() -> TestResult {
    let view = edited("image-plain", |text| {
        text.replace("default_branch = \"main\"", "default_branch = \"1.2.3\"")
    })?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(
            ImageError::BranchReleaseVersion { .. }
        ))
    ));
    let view = edited("image-plain", |text| {
        text.replace("default_branch = \"main\"", "default_branch = \"v1.2\"")
    })?;
    assert!(expected_files(&view).is_ok());
    Ok(())
}

#[test]
fn a_symlinked_dockerfile_or_ignore_file_is_refused() -> TestResult {
    for path in ["Dockerfile", ".dockerignore", "Dockerfile.dockerignore"] {
        let mut view = sample("image-plain")?;
        view.tracked_paths.insert(RepoPath::new(path)?);
        view.symlinks.insert(RepoPath::new(path)?);
        assert!(
            matches!(
                expected_files(&view),
                Err(GenerateError::Image(ImageError::DockerfileSymlink { .. }))
            ),
            "{path}"
        );
    }
    Ok(())
}
