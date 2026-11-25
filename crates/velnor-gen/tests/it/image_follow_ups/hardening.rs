// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod inputs;
mod queue;
mod release_trust;

use serde_json::Value;
use velnor_gen::{GenerateError, ImageError, expected_files};
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::image_workflow::{edited, image, rendered, sample, step_named, steps};

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
        text.replace(
            "default_branch = \"main\"",
            "default_branch = \"sha-0123abc\"",
        )
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

#[test]
fn only_the_cargo_config_that_cargo_reads_is_checked_in_each_directory() -> TestResult {
    let bad = b"[build]\nrustflags = [\"-C\", \"target-cpu=native\"]\n".to_vec();
    let fine = b"[build]\nrustflags = [\"--cfg\", \"tokio_unstable\"]\n".to_vec();
    let mut shadowed = sample("image-plain")?;
    shadowed
        .files
        .insert(RepoPath::new(".cargo/config")?, fine.clone());
    shadowed
        .files
        .insert(RepoPath::new(".cargo/config.toml")?, bad.clone());
    assert!(expected_files(&shadowed).is_ok());
    let mut effective = sample("image-plain")?;
    effective.files.insert(RepoPath::new(".cargo/config")?, bad);
    effective
        .files
        .insert(RepoPath::new(".cargo/config.toml")?, fine);
    assert!(matches!(
        expected_files(&effective),
        Err(GenerateError::Image(ImageError::CpuBaseline { .. }))
    ));
    Ok(())
}

fn second_image(off: bool) -> String {
    let build = if off { "pr_build = \"off\"\n" } else { "" };
    format!(
        "\n[[image]]\nname = \"other-svc\"\nbins = [\"plain-svc\"]\nstage = \"dist-other\"\npublish_runner = \"ubuntu-26.04-scale-set-publish\"\n{build}\n[[image.registry]]\nhost = \"docker.io\"\nrepository = \"acme/other\"\nusername = \"secrets.DOCKERHUB_USERNAME\"\ntoken = \"secrets.DOCKERHUB_TOKEN\"\n"
    )
}

fn two_images(off: bool) -> Result<Value, Box<dyn std::error::Error>> {
    image(&edited("image-plain", |text| {
        format!("{text}{}", second_image(off))
    })?)
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

#[test]
fn several_images_are_selected_by_their_own_paths_and_one_image_needs_no_selection() -> TestResult {
    let single = rendered("image-plain")?;
    assert!(single["jobs"]["image-changes"].is_null());
    assert!(single["jobs"]["image-plain-svc-build"]["needs"].is_null());
    let both = two_images(false)?;
    let changes = &both["jobs"]["image-changes"];
    let select = step_named(changes, "Select the images")?;
    let run = text(&select["run"]);
    assert!(run.contains("image changes"), "{run}");
    let matches = text(&select["env"]["VELNOR_IMAGE_MATCHES"]);
    assert!(
        matches.lines().any(|line| line == "plain-svc=**"),
        "{matches}"
    );
    assert!(
        matches
            .lines()
            .any(|line| line == "other-svc=**/Cargo.toml"),
        "{matches}"
    );
    for name in ["plain-svc", "other-svc"] {
        let build = &both["jobs"][format!("image-{name}-build").as_str()];
        assert!(
            text(&build["if"]).contains(&format!("needs.image-changes.outputs.{name} == 'true'"))
        );
        assert_eq!(build["needs"], serde_json::json!(["image-changes"]));
        assert!(text(&changes["outputs"][name]).contains(&format!("outputs.{name}")));
    }
    let one_off = two_images(true)?;
    assert!(one_off["jobs"]["image-changes"].is_null());
    Ok(())
}

#[test]
fn the_gate_ignores_its_own_jobs_and_the_authority_waits_for_the_publish_job() -> TestResult {
    let workflow = rendered("image-plain")?;
    let gate =
        text(&step_named(&workflow["jobs"]["image-gate"], "Wait for every required")?["run"]);
    assert!(gate.contains("--ignore-job=image-gate"), "{gate}");
    assert!(
        gate.contains("--ignore-job=image-plain-svc-publish"),
        "{gate}"
    );
    assert!(!gate.contains("--require-published"), "{gate}");
    let authority = text(
        &step_named(
            &workflow["jobs"]["image-plain-svc-authority"],
            "Wait for every required",
        )?["run"],
    );
    assert!(
        authority.contains("--require-published=image-plain-svc-publish"),
        "{authority}"
    );
    assert!(authority.contains("--ignore-job=image-gate"), "{authority}");
    assert!(
        !authority.contains("--ignore-job=image-plain-svc-publish"),
        "{authority}"
    );
    assert!(!authority.contains("--wait-seconds=0"), "{authority}");
    Ok(())
}

#[test]
fn the_image_build_restores_the_shared_sources_cache_before_the_tools_and_cargo() -> TestResult {
    let workflow = rendered("image-plain")?;
    let build = &workflow["jobs"]["image-plain-svc-build"];
    let all = steps(build);
    let position = |needle: &str| {
        all.iter().position(|step| {
            step["name"]
                .as_str()
                .is_some_and(|name| name.contains(needle))
                || step["uses"]
                    .as_str()
                    .is_some_and(|uses| uses.contains(needle))
        })
    };
    let locks = position("Hash the sources lockfiles").ok_or("no lock hash")?;
    let restore = position("cache/restore").ok_or("no restore")?;
    let install = position("Install tools").ok_or("no tools")?;
    let compile = position("Compile").ok_or("no compile")?;
    assert!(locks < restore && restore < install && install < compile);
    let key = text(&all[restore]["with"]["key"]);
    assert!(key.contains("velnor-src-"), "{key}");
    Ok(())
}

#[test]
fn the_staged_artifact_is_uploaded_with_hidden_files_and_a_rust_source_change_builds_the_image()
-> TestResult {
    let workflow = rendered("image-plain")?;
    let upload = step_named(
        &workflow["jobs"]["image-plain-svc-build"],
        "Upload image-plain-svc",
    )?;
    assert_eq!(upload["with"]["include-hidden-files"], Value::Bool(true));
    let filtered = image(&edited("image-plain", |text| {
        text.replace("pr_build = \"always\"", "pr_build = \"inputs\"")
    })?)?;
    let select = step_named(&filtered["jobs"]["image-changes"], "Select the images")?;
    let matches = text(&select["env"]["VELNOR_IMAGE_MATCHES"]);
    assert!(
        matches.lines().any(|line| line == "plain-svc=**"),
        "{matches}"
    );
    let nested = rendered("image-workspace")?;
    let select = step_named(&nested["jobs"]["image-changes"], "Select the images")?;
    let matches = text(&select["env"]["VELNOR_IMAGE_MATCHES"]);
    assert!(
        matches
            .lines()
            .any(|line| line.ends_with("=backend-rust/**")),
        "{matches}"
    );
    Ok(())
}

#[test]
fn an_image_of_a_shared_compile_is_staged_and_checked_only_when_it_was_selected() -> TestResult {
    let view = edited("image-plain", |text| {
        text.replace("pr_build = \"always\"", "pr_build = \"inputs\"") + &second_image(false)
    })?;
    let workflow = image(&view)?;
    let jobs_map = workflow["jobs"].as_object().ok_or("no jobs")?;
    let shared: Vec<&String> = jobs_map
        .keys()
        .filter(|id| id.ends_with("-build"))
        .collect();
    assert_eq!(shared.len(), 1, "{shared:?}");
    let build = &workflow["jobs"][shared[0].as_str()];
    for image in ["plain-svc", "other-svc"] {
        let wanted = format!("needs.image-changes.outputs.{image} == 'true'");
        let staged: Vec<&Value> = steps(build)
            .into_iter()
            .filter(|step| {
                step["name"]
                    .as_str()
                    .is_some_and(|name| name.contains(image) && !name.contains("target"))
                    && step["if"]
                        .as_str()
                        .is_some_and(|text| text.contains(&wanted))
            })
            .collect();
        assert!(
            staged.len() >= 2,
            "{image}: stage and upload are both conditional"
        );
        let check = &workflow["jobs"][format!("image-{image}-check").as_str()];
        assert!(text(&check["if"]).contains(&wanted), "{image}");
        assert!(
            check["needs"]
                .as_array()
                .is_some_and(|needs| needs.iter().any(|need| need == "image-changes")),
            "{image}"
        );
    }
    Ok(())
}
