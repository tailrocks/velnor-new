// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, ImageError, expected_files};
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::image_workflow::{edited, sample};

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
    let mut submodule = sample("image-plain")?;
    submodule.tracked_paths.insert(RepoPath::new("Dockerfile")?);
    submodule.gitlinks.insert(RepoPath::new("Dockerfile")?);
    assert!(matches!(
        expected_files(&submodule),
        Err(GenerateError::Image(ImageError::DockerfileSymlink { .. }))
    ));
    let mut context_inside = edited("image-plain", |text| {
        text.replace(
            "name = \"plain-svc\"\n",
            "name = \"plain-svc\"\ncontext = \"vendor/app\"\ndockerfile = \"Dockerfile\"\n",
        )
    })?;
    context_inside.gitlinks.insert(RepoPath::new("vendor")?);
    assert!(matches!(
        expected_files(&context_inside),
        Err(GenerateError::Image(ImageError::DockerfileSymlink { .. }))
    ));
    let mut inside = sample("image-plain")?;
    inside.gitlinks.insert(RepoPath::new("vendor")?);
    inside
        .tracked_paths
        .insert(RepoPath::new("vendor/Dockerfile")?);
    let nested = edited("image-plain", |text| {
        text.replace(
            "name = \"plain-svc\"\n",
            "name = \"plain-svc\"\ndockerfile = \"vendor/Dockerfile\"\n",
        )
    })?;
    inside.files = nested.files;
    assert!(matches!(
        expected_files(&inside),
        Err(GenerateError::Image(ImageError::DockerfileSymlink { .. }))
    ));
    Ok(())
}
