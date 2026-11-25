// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::image_workflow::{SAMPLES, edited, image, name_of, rendered, steps};

fn position(job: &serde_json::Value, needle: &str) -> Result<usize, String> {
    steps(job)
        .iter()
        .position(|step| {
            step["name"]
                .as_str()
                .is_some_and(|name| name.contains(needle))
                || step["run"].as_str().is_some_and(|run| run.contains(needle))
        })
        .ok_or_else(|| format!("no step with {needle}"))
}

#[test]
fn a_release_tag_is_authorized_by_a_trusted_actor_or_a_ruleset_before_anything_else_runs()
-> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        let authority = &workflow["jobs"][format!("image-{}-authority", name_of(name))];
        let authorize = position(authority, "image authorize")?;
        assert!(authorize > position(authority, "strict version")?, "{name}");
        assert!(authorize < position(authority, "image gate")?, "{name}");
        if let Ok(minted) = position(authority, "App token") {
            assert!(
                authorize < minted,
                "{name}: the key is minted after the check"
            );
        }
        let step = &steps(authority)[authorize];
        assert_eq!(step["env"]["GITHUB_TOKEN"], "${{ github.token }}", "{name}");
        assert_eq!(step["env"]["ACTOR"], "${{ github.actor }}", "{name}");
        assert_eq!(step["env"]["REF_NAME"], "${{ github.ref_name }}", "{name}");
    }
    Ok(())
}

#[test]
fn the_configured_release_actors_are_the_only_ones_named_in_the_check() -> TestResult {
    let plain = rendered("image-plain")?;
    let authority = &plain["jobs"]["image-plain-svc-authority"];
    let run = steps(authority)[position(authority, "image authorize")?]["run"]
        .as_str()
        .ok_or("no run")?
        .to_owned();
    assert!(!run.contains("--release-actor"), "{run}");
    let view = edited("image-plain", |text| {
        text.replace(
            "bins = [\"plain-svc\"]",
            "bins = [\"plain-svc\"]\nrelease_actors = [\"Release-Manager\", \"ship-it[bot]\"]",
        )
    })?;
    let workflow = image(&view)?;
    let authority = &workflow["jobs"]["image-plain-svc-authority"];
    let run = steps(authority)[position(authority, "image authorize")?]["run"]
        .as_str()
        .ok_or("no run")?
        .to_owned();
    assert!(run.contains("--release-actor=Release-Manager"), "{run}");
    assert!(run.contains("--release-actor='ship-it[bot]'"), "{run}");
    Ok(())
}

#[test]
fn the_promote_job_installs_the_helper_before_it_logs_in_and_compares_digests_first() -> TestResult
{
    for name in SAMPLES {
        let workflow = rendered(name)?;
        let promote = &workflow["jobs"][format!("image-{}-promote", name_of(name))];
        let install = position(promote, "Install velnor-ci")?;
        assert!(install < position(promote, "Log in")?, "{name}");
        assert!(
            position(promote, "Log in")? < position(promote, "image promote")?,
            "{name}"
        );
    }
    Ok(())
}
