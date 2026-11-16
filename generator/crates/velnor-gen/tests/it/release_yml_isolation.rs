// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::{APP_TOKEN, app, steps, two_roots, uses};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const RELEASE_PLZ: &str = "\"$RUNNER_TEMP/release-plz\"";
const RELEASE_PLZ_SHA256: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
const RELEASE_PLZ_URL: &str = "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz-x86_64-unknown-linux-gnu.tar.gz";

type TokenJob<'a> = (&'static str, &'a [Node]);

fn token_jobs(tree: &Node) -> Result<Vec<TokenJob<'_>>, Box<dyn Error>> {
    ["release-pr", "tag-0"]
        .into_iter()
        .map(|id| Ok((id, steps(tree.at(&["jobs", id]).ok_or("no job")?))))
        .collect()
}

fn run(step: &Node) -> &str {
    step.get("run").and_then(Node::text).unwrap_or_default()
}

fn position(all: &[Node], matches: impl Fn(&Node) -> bool) -> Result<usize, Box<dyn Error>> {
    all.iter()
        .position(matches)
        .ok_or_else(|| "step not found".into())
}

#[test]
fn release_plz_is_downloaded_from_the_locked_url_and_verified_against_the_locked_digest_before_the_token()
-> TestResult {
    let (_, tree) = two_roots()?;
    for (id, all) in token_jobs(&tree)? {
        let at = |needle: &str| position(all, |step| run(step).contains(needle));
        let download = at(RELEASE_PLZ_URL)?;
        let digest = at(RELEASE_PLZ_SHA256)?;
        let verify = at("sha256sum --check")?;
        let unpack = at("tar --extract")?;
        let checkout = position(all, |step| {
            uses(step).is_some_and(|u| u.starts_with("actions/checkout@"))
        })?;
        let token = position(all, |step| {
            uses(step).is_some_and(|u| u.starts_with(APP_TOKEN))
        })?;
        assert!(
            download < digest && digest < verify && verify < unpack && unpack < checkout,
            "{id}"
        );
        assert!(checkout < token, "{id}");
        assert!(run(&all[download]).starts_with("curl "), "{id}");
        for step in all {
            let text = run(step);
            assert!(
                !uses(step).is_some_and(|u| u.starts_with("jdx/mise-action@")),
                "{id}"
            );
            assert!(!text.contains("mise"), "{id}: {text}");
        }
        for step in all.iter().filter(|step| run(step).contains("release-plz ")) {
            assert!(
                run(step).starts_with(&format!("{RELEASE_PLZ} release")),
                "{id}: {}",
                run(step)
            );
        }
    }
    Ok(())
}

#[test]
fn every_release_plz_step_pins_the_rust_toolchain_that_the_job_installed_before_the_checkout()
-> TestResult {
    let (_, tree) = two_roots()?;
    for (id, all) in token_jobs(&tree)? {
        let installed = position(all, |step| {
            run(step).starts_with("rustup toolchain install stable ")
        })?;
        let checkout = position(all, |step| {
            uses(step).is_some_and(|u| u.starts_with("actions/checkout@"))
        })?;
        assert!(installed < checkout, "{id}");
        let steps: Vec<&Node> = all
            .iter()
            .filter(|step| run(step).starts_with("\"$RUNNER_TEMP/release-plz\" release"))
            .collect();
        assert!(!steps.is_empty(), "{id}");
        for step in steps {
            assert_eq!(
                step.at(&["env", "RUSTUP_TOOLCHAIN"]).and_then(Node::text),
                Some("stable"),
                "{id}"
            );
        }
    }
    Ok(())
}

#[test]
fn the_release_plz_steps_cancel_every_program_hook_cargo_reads_from_the_environment() -> TestResult
{
    let (_, tree) = app()?;
    for (id, all) in token_jobs(&tree)? {
        let steps: Vec<&Node> = all
            .iter()
            .filter(|step| run(step).starts_with("\"$RUNNER_TEMP/release-plz\" release"))
            .collect();
        assert!(!steps.is_empty(), "{id}");
        for step in steps {
            for (key, value) in [
                ("RUSTC_WRAPPER", ""),
                ("RUSTC_WORKSPACE_WRAPPER", ""),
                ("CARGO_BUILD_RUSTC_WRAPPER", ""),
                ("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER", ""),
                ("CARGO_BUILD_RUSTC", "rustc"),
                ("RUSTC", "rustc"),
            ] {
                assert_eq!(
                    step.at(&["env", key]).and_then(Node::text),
                    Some(value),
                    "{id} {key}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn a_build_checks_that_the_tag_it_built_is_the_commit_the_gate_verified() -> TestResult {
    let (_, tree) = app()?;
    let build = tree
        .at(&["jobs", "build-app-linux-x64"])
        .ok_or("no build")?;
    let all = steps(build);
    assert_eq!(all[0].get("id").and_then(Node::text), Some("source"));
    assert!(uses(&all[0]).is_some_and(|u| u.starts_with("actions/checkout@")));
    let check = &all[1];
    assert_eq!(run(check), "test \"$CHECKED_OUT\" = \"$COMMIT\"");
    assert_eq!(
        check.at(&["env", "COMMIT"]).and_then(Node::text),
        Some("${{ github.event.workflow_run.head_sha }}")
    );
    assert_eq!(
        check.at(&["env", "CHECKED_OUT"]).and_then(Node::text),
        Some("${{ steps.source.outputs.commit }}")
    );
    Ok(())
}
