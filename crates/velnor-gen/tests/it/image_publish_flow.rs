// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView};

use crate::facts::TestResult;
use crate::image_workflow::{CONFIG, image, jobs, rendered, sample, step_named, steps};

#[test]
fn the_staged_binaries_get_their_executable_bits_back_before_the_image_is_built() -> TestResult {
    let workflow = rendered("image-workspace")?;
    let publish = &workflow["jobs"]["image-multi-bin-svc-publish"];
    let names: Vec<&str> = steps(publish)
        .iter()
        .filter_map(|step| step["name"].as_str())
        .collect();
    let restore = names
        .iter()
        .position(|name| name.starts_with("Restore the executable bits"))
        .ok_or("no restore step")?;
    let push = names
        .iter()
        .position(|name| name.starts_with("Build and push"))
        .ok_or("no push step")?;
    assert!(restore < push);
    assert_eq!(
        step_named(publish, "Restore the executable bits")?["run"],
        "chmod 0755 dist/legacy-server dist/legacy-migration"
    );
    Ok(())
}

fn two_images(view: RepoView) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = view;
    let path = RepoPath::new(CONFIG)?;
    let text = String::from_utf8(view.files.remove(&path).ok_or("no config")?)?;
    let second = text
        .split("[[image]]")
        .nth(1)
        .ok_or("no image table")?
        .replace("name = \"plain-svc\"", "name = \"other-svc\"")
        .replace("acme/plain-svc", "acme/other-svc");
    view.files
        .insert(path, format!("{text}\n[[image]]{second}").into_bytes());
    Ok(view)
}

#[test]
fn several_images_share_one_gate_and_every_publish_job_waits_for_it() -> TestResult {
    let workflow = image(&two_images(sample("image-plain")?)?)?;
    let gates: Vec<&String> = jobs(&workflow)?
        .keys()
        .filter(|id| id.ends_with("gate"))
        .collect();
    assert_eq!(gates, ["image-gate"]);
    for name in ["plain-svc", "other-svc"] {
        let publish = &workflow["jobs"][format!("image-{name}-publish")];
        assert!(
            publish["needs"]
                .as_array()
                .is_some_and(|needs| needs.iter().any(|need| need == "image-gate"))
        );
    }
    Ok(())
}

#[test]
fn a_second_registry_is_never_skipped_because_the_first_already_holds_the_tag() -> TestResult {
    let both = rendered("image-ghcr")?;
    let publish = &both["jobs"]["image-cluster-svc-publish"];
    assert!(step_named(publish, "Build and push")?.get("if").is_none());
    assert!(
        step_named(publish, "Log in to ghcr.io")?
            .get("if")
            .is_none()
    );
    let single = rendered("image-plain")?;
    let publish = &single["jobs"]["image-plain-svc-publish"];
    assert!(step_named(publish, "Build and push")?.get("if").is_some());
    Ok(())
}

#[test]
fn the_pull_request_filter_matches_a_declared_input_file_and_the_dockerfile_specific_ignore_file()
-> TestResult {
    let workflow = rendered("image-ghcr")?;
    let paths = workflow["on"]["pull_request"]["paths"]
        .as_array()
        .ok_or("no paths")?;
    for wanted in ["migrations", "migrations/**", "Dockerfile.dockerignore"] {
        assert!(paths.iter().any(|path| path == wanted), "{wanted}");
    }
    Ok(())
}
