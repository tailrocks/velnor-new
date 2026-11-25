// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView};

use crate::facts::TestResult;
use crate::image_workflow::{CONFIG, edited, image, jobs, rendered, sample, step_named, steps};
use serde_json::Value;

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
fn a_rerun_pushes_again_so_that_a_missing_branch_tag_or_registry_is_repaired() -> TestResult {
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
    assert!(step_named(publish, "Build and push")?.get("if").is_none());
    assert!(step_named(publish, "Refuse to push")?.get("id").is_some());
    Ok(())
}

#[test]
fn the_image_changes_job_selects_an_image_by_a_declared_input_file_and_the_dockerfile_specific_ignore_file()
-> TestResult {
    let workflow = rendered("image-ghcr")?;
    let select = step_named(&workflow["jobs"]["image-changes"], "Select the images")?;
    let matches = select["env"]["VELNOR_IMAGE_MATCHES"]
        .as_str()
        .ok_or("no matches")?;
    for wanted in [
        "cluster-svc=migrations",
        "cluster-svc=migrations/**",
        "cluster-svc=Dockerfile.dockerignore",
    ] {
        assert!(matches.lines().any(|line| line == wanted), "{wanted}");
    }
    Ok(())
}

#[test]
fn a_commit_that_is_no_longer_the_tip_says_so_and_never_moves_the_branch_tag() -> TestResult {
    for (name, publish, advance) in [
        (
            "image-plain",
            "image-plain-svc-publish",
            "image-plain-svc-advance",
        ),
        (
            "image-ghcr",
            "image-cluster-svc-publish",
            "image-cluster-svc-advance",
        ),
    ] {
        let workflow = rendered(name)?;
        let tags = step_named(&workflow["jobs"][publish], "Build and push")?["with"]["tags"]
            .as_str()
            .ok_or("no tags")?
            .to_owned();
        assert!(!tags.contains(":main"), "{name}: {tags}");
        let advance = &workflow["jobs"][advance];
        assert!(
            step_named(advance, "Say that the default-branch tag was withheld")?["if"]
                .as_str()
                .is_some_and(|text| text.contains("steps.advance.outputs.moved != 'true'")),
            "{name}"
        );
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
    assert!(
        upload["if"]
            .as_str()
            .is_some_and(|text| text.contains("steps.tip.outputs.ready"))
    );
    let check = &workflow["jobs"]["image-cluster-svc-check"];
    assert_eq!(
        check["needs"],
        serde_json::json!(["image-cluster-svc-build", "image-changes"])
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
                    .is_some_and(|run| run.contains("image gate") && run.contains("--wait-seconds"))
            })
            .ok_or("no gate step")?;
        assert!(
            gate["run"]
                .as_str()
                .is_some_and(|run| run.contains(" --pr-only-check=DCO")),
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
fn pr_only_checks_are_verified_by_hosted_jobs_with_the_app_key_and_waited_for_with_a_read_only_token()
-> TestResult {
    let workflow = rendered("image-ghcr")?;
    for id in ["image-exemptions", "image-cluster-svc-authority"] {
        let job = &workflow["jobs"][id];
        assert_eq!(job["runs-on"], "ubuntu-latest", "{id}");
        assert_eq!(job["environment"], "release", "{id}");
        let app = step_named(job, "Create the release App token")?;
        let run = app["run"].as_str().ok_or("no app step")?;
        assert!(run.contains("--permission=administration:write"), "{id}");
        assert!(
            step_named(job, "Revoke the release App token").is_ok(),
            "{id}"
        );
        let names: Vec<&str> = steps(job)
            .iter()
            .filter_map(|step| step["name"].as_str())
            .collect();
        let position = |prefix: &str| names.iter().position(|text| text.starts_with(prefix));
        let launcher = position("Install the released helper launcher").ok_or("no launcher")?;
        let minted = position("Create the release App token").ok_or("no app step")?;
        assert!(
            launcher < minted,
            "{id}: the token exists only after the helper is pinned"
        );
        for step in steps(job) {
            let run = step["run"].as_str().unwrap_or("");
            assert!(!run.starts_with("velnor-ci "), "{id}: {run}");
        }
    }
    let exemptions = &workflow["jobs"]["image-exemptions"];
    let verify = step_named(exemptions, "Verify that the PR-only checks")?;
    assert_eq!(
        verify["env"]["GITHUB_TOKEN"],
        "${{ steps.app.outputs.token }}"
    );
    assert!(
        verify["run"]
            .as_str()
            .is_some_and(|run| run.contains("--only=exemptions"))
    );
    let authority = &workflow["jobs"]["image-cluster-svc-authority"];
    let names: Vec<&str> = steps(authority)
        .iter()
        .filter_map(|step| step["name"].as_str())
        .collect();
    let strict = names
        .iter()
        .position(|text| text.contains("strict version"))
        .ok_or("no strict step")?;
    let minted = names
        .iter()
        .position(|text| text.contains("App token") && text.starts_with("Create"))
        .ok_or("no app step")?;
    assert!(
        strict < minted,
        "the tag is validated before the key is used"
    );
    let gate = step_named(authority, "Wait for every required check")?;
    assert!(gate["run"].as_str().is_some_and(
        |run| run.contains("--verify-exemptions") && run.contains("--wait-seconds=1200")
    ));
    let waiting = &workflow["jobs"]["image-gate"];
    assert_eq!(waiting["runs-on"], "ubuntu-24.04");
    assert_eq!(waiting["needs"], serde_json::json!(["image-exemptions"]));
    assert!(waiting.get("environment").is_none());
    assert!(!waiting.to_string().contains("RELEASE_APP"));
    assert!(!waiting.to_string().contains("--verify-exemptions"));
    let plain = rendered("image-plain")?;
    assert!(plain["jobs"].get("image-exemptions").is_none());
    for id in ["image-gate", "image-plain-svc-authority"] {
        let job = &plain["jobs"][id];
        assert!(job.get("environment").is_none(), "{id}");
        assert!(!job.to_string().contains("RELEASE_APP"), "{id}");
        assert_eq!(job["runs-on"], "ubuntu-latest", "{id}");
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

#[test]
fn the_staged_artifact_outlives_a_weekend_of_environment_approval() -> TestResult {
    let workflow = rendered("image-plain")?;
    let build = &workflow["jobs"]["image-plain-svc-build"];
    assert_eq!(
        step_named(build, "Upload image-plain-svc")?["with"]["retention-days"],
        7
    );
    Ok(())
}

#[test]
fn drafts_are_skipped_only_when_the_repository_skips_them() -> TestResult {
    let plain = rendered("image-plain")?;
    let condition = |workflow: &serde_json::Value, id: &str| workflow["jobs"][id]["if"].to_string();
    assert!(!condition(&plain, "image-plain-svc-build").contains("draft"));
    let skipping = image(&crate::image_workflow::edited("image-plain", |text| {
        format!("skip_draft_pull_requests = true\n{text}")
    })?)?;
    for id in ["image-plain-svc-build", "image-plain-svc-check"] {
        assert!(
            condition(&skipping, id).contains("github.event.pull_request.draft != true"),
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn a_superseded_commit_still_publishes_its_sha_image_so_that_its_release_tag_can_be_promoted()
-> TestResult {
    let workflow = rendered("image-plain")?;
    let gate = step_named(
        &workflow["jobs"]["image-gate"],
        "Wait for every required check",
    )?;
    let run = gate["run"].as_str().ok_or("no gate")?;
    assert!(!run.contains("--require-tip"), "{run}");
    Ok(())
}

#[test]
fn the_pull_request_trigger_follows_pr_build_and_never_filters_by_path_on_the_server() -> TestResult
{
    let triggers = |view: &RepoView| -> Result<Value, Box<dyn std::error::Error>> {
        Ok(image(view)?["on"].clone())
    };
    let inputs = triggers(&sample("image-ghcr")?)?;
    assert!(inputs["pull_request"].get("paths").is_none());
    let always = triggers(&sample("image-plain")?)?;
    assert!(always["pull_request"].get("paths").is_none());
    let off = triggers(&edited("image-plain", |text| {
        text.replace("pr_build = \"always\"", "pr_build = \"off\"")
    })?)?;
    assert!(off.get("pull_request").is_none());
    Ok(())
}
