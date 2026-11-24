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

#[test]
fn the_tip_is_checked_right_before_the_push_and_main_moves_only_while_it_holds() -> TestResult {
    for (name, id) in [
        ("image-plain", "image-plain-svc-publish"),
        ("image-ghcr", "image-cluster-svc-publish"),
    ] {
        let workflow = rendered(name)?;
        let publish = &workflow["jobs"][id];
        let names: Vec<&str> = steps(publish)
            .iter()
            .filter_map(|step| step["name"].as_str())
            .collect();
        let tip = names
            .iter()
            .position(|text| text.starts_with("Check that the commit is still the tip"))
            .ok_or("no tip step")?;
        let push = names
            .iter()
            .position(|text| text.starts_with("Build and push"))
            .ok_or("no push step")?;
        assert_eq!(
            tip + 1,
            push,
            "{name}: nothing may sit between the check and the push"
        );
        assert!(
            step_named(publish, "still the tip")?["run"]
                .as_str()
                .is_some_and(|run| run
                    .ends_with("image gate --repo=\"$REPOSITORY\" --sha=\"$COMMIT\" --tip-only"))
        );
        let tags = step_named(publish, "Build and push")?["with"]["tags"]
            .as_str()
            .ok_or("no tags")?
            .to_owned();
        let moving = tags.lines().filter(|line| line.contains(":main")).count();
        let guarded = tags
            .lines()
            .filter(|line| line.contains("steps.tip.outputs.ready == 'true'"))
            .count();
        assert!(moving > 0 && moving == guarded, "{name}: {tags}");
    }
    Ok(())
}

#[test]
fn the_pull_request_image_build_is_its_own_job_with_the_publish_jobs_sparse_checkout() -> TestResult
{
    let workflow = rendered("image-ghcr")?;
    let build = &workflow["jobs"]["image-cluster-svc-build"];
    assert!(
        steps(build)
            .iter()
            .all(|step| !step["uses"].as_str().unwrap_or("").starts_with("docker/")),
        "the compile job holds no docker step"
    );
    let upload = step_named(build, "Upload image-cluster-svc")?;
    assert!(upload.get("if").is_none());
    let check = &workflow["jobs"]["image-cluster-svc-check"];
    assert_eq!(
        check["needs"],
        serde_json::json!(["image-cluster-svc-build"])
    );
    assert!(
        check["if"]
            .as_str()
            .is_some_and(|text| text.contains("pull_request"))
    );
    assert!(check.get("environment").is_none());
    let publish = &workflow["jobs"]["image-cluster-svc-publish"];
    let sparse = |job: &serde_json::Value| steps(job)[0]["with"]["sparse-checkout"].clone();
    assert_eq!(sparse(check), sparse(publish));
    assert_eq!(step_named(check, "without pushing")?["with"]["push"], false);
    assert!(!check.to_string().contains("secrets."));
    let off = image(&crate::image_workflow::edited("image-plain", |text| {
        text.replace("pr_build = \"always\"", "pr_build = \"off\"")
    })?)?;
    assert!(off["jobs"].get("image-plain-svc-check").is_none());
    Ok(())
}

#[test]
fn pr_only_checks_reach_the_gate_and_the_authority_and_widen_their_token_to_checks_read()
-> TestResult {
    let workflow = rendered("image-ghcr")?;
    for id in ["image-gate", "image-cluster-svc-authority"] {
        let job = &workflow["jobs"][id];
        assert_eq!(job["permissions"]["checks"], "read", "{id}");
        let gate = steps(job)
            .into_iter()
            .find(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("image gate"))
            })
            .ok_or("no gate step")?;
        assert!(
            gate["run"]
                .as_str()
                .is_some_and(|run| run.ends_with("--pr-only-check=DCO")),
            "{id}"
        );
    }
    let plain = rendered("image-plain")?;
    assert!(
        plain["jobs"]["image-gate"]["permissions"]
            .get("checks")
            .is_none()
    );
    Ok(())
}

#[test]
fn pr_only_checks_get_an_administration_capable_app_token_that_is_revoked_and_nothing_else_does()
-> TestResult {
    let workflow = rendered("image-ghcr")?;
    for id in ["image-gate", "image-cluster-svc-authority"] {
        let job = &workflow["jobs"][id];
        assert_eq!(job["environment"], "release", "{id}");
        let app = step_named(job, "Create the release App token")?;
        let run = app["run"].as_str().ok_or("no app step")?;
        assert!(run.contains("--permission=administration:write"), "{id}");
        let gate = step_named(job, "Wait for every required check")?;
        assert_eq!(
            gate["env"]["GITHUB_TOKEN"], "${{ steps.app.outputs.token }}",
            "{id}"
        );
        assert!(
            step_named(job, "Revoke the release App token").is_ok(),
            "{id}"
        );
    }
    let plain = rendered("image-plain")?;
    for id in ["image-gate", "image-plain-svc-authority"] {
        let job = &plain["jobs"][id];
        assert!(job.get("environment").is_none(), "{id}");
        assert!(!job.to_string().contains("RELEASE_APP"), "{id}");
    }
    Ok(())
}

#[test]
fn a_release_version_is_bounded_so_that_it_is_a_valid_image_tag() -> TestResult {
    let workflow = rendered("image-plain")?;
    let authority = &workflow["jobs"]["image-plain-svc-authority"];
    let check = step_named(authority, "strict version")?["run"]
        .as_str()
        .ok_or("no check")?
        .to_owned();
    assert!(
        check.contains("[0-9]{1,40}[.][0-9]{1,40}[.][0-9]{1,40}$"),
        "{check}"
    );
    Ok(())
}
