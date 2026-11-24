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

const MIRROR: &str = "\n[crates_mirror]\nrunner = 'mirror-box'\nregistry = 'sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/'\ncainfo = '/opt/velnor/crates-mirror/ca-bundle.pem'\n";

fn step_index(job: &serde_json::Value, needle: &str) -> Option<usize> {
    job["steps"].as_array()?.iter().position(|step| {
        step["name"]
            .as_str()
            .is_some_and(|name| name.contains(needle))
            || step["run"].as_str().is_some_and(|run| run.contains(needle))
    })
}

#[test]
fn the_crates_mirror_of_the_image_runner_is_set_up_before_the_tools_and_cargo() -> TestResult {
    let mirrored = crate::image_workflow::image(&edited("image-plain", |text| {
        text.replace("ubuntu-26.04-scale-set", "mirror-box") + MIRROR
    })?)?;
    let build = &mirrored["jobs"]["image-plain-svc-build"];
    let setup = step_index(build, "crates-mirror --registry").ok_or("no mirror step")?;
    let install = step_index(build, "Install tools").ok_or("no tools step")?;
    let compile = step_index(build, "cargo zigbuild").ok_or("no compile")?;
    let after = build["steps"]
        .as_array()
        .ok_or("no steps")?
        .iter()
        .rposition(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("crates-mirror --registry"))
        })
        .ok_or("no second mirror step")?;
    assert!(setup < install && install < after && after < compile);
    let other = crate::image_workflow::image(&edited("image-plain", |text| {
        text.replace("ubuntu-26.04-scale-set", "mirror-box")
            + &MIRROR.replace("mirror-box", "another-box")
    })?)?;
    assert!(
        step_index(
            &other["jobs"]["image-plain-svc-build"],
            "crates-mirror --registry"
        )
        .is_none()
    );
    Ok(())
}

#[test]
fn rustflags_that_set_a_cpu_are_refused_wherever_cargo_would_read_them() -> TestResult {
    for (path, content) in [
        (
            ".cargo/config.toml",
            "[build]\nrustflags = [\"-C\", \"target-cpu=native\"]\n",
        ),
        (
            ".cargo/config",
            "[target.x86_64-unknown-linux-gnu]\nrustflags = \"-Ctarget-feature=+avx2\"\n",
        ),
        (
            ".cargo/config.toml",
            "[build]\nrustflags = [\"-Ctarget_cpu=native\"]\n",
        ),
        (
            ".cargo/config",
            "[build]\nrustflags = \"-C target_feature=+avx2\"\n",
        ),
        (
            ".cargo/config.toml",
            "[target.'cfg(unix)']\nrustflags = [\"-C\", \"target-cpu=native\"]\n",
        ),
    ] {
        let mut view = sample("image-plain")?;
        view.files
            .insert(RepoPath::new(path)?, content.as_bytes().to_vec());
        assert!(
            matches!(
                expected_files(&view),
                Err(GenerateError::Image(ImageError::CpuBaseline { .. }))
            ),
            "{path}: {content}"
        );
    }
    let mut harmless = sample("image-plain")?;
    harmless.files.insert(
        RepoPath::new(".cargo/config.toml")?,
        b"[build]\nrustflags = [\"--cfg\", \"tokio_unstable\"]\n[target.aarch64-apple-darwin]\nrustflags = [\"-C\", \"target-cpu=native\"]\n".to_vec(),
    );
    assert!(expected_files(&harmless).is_ok());
    Ok(())
}
