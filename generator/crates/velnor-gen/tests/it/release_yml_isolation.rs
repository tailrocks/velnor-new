// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::{APP_TOKEN, app, steps, two_roots, uses};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const MISE: &str = "\"$RUNNER_TEMP/mise\"";
const ISOLATED: &str = "--no-config --no-env --no-hooks";
const MISE_SHA256: &str = "7129dbab31d698f9028db9374f7ba7987232b0426f837dcbce437130a61415f9";

type TokenJob<'a> = (&'static str, &'a [Node]);

fn token_jobs(tree: &Node) -> Result<Vec<TokenJob<'_>>, Box<dyn Error>> {
    ["release-pr", "tag"]
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
fn mise_is_downloaded_verified_and_used_to_install_release_plz_before_the_repository_is_checked_out()
-> TestResult {
    let (_, tree) = two_roots()?;
    for (id, all) in token_jobs(&tree)? {
        let at = |needle: &str| position(all, |step| run(step).contains(needle));
        let download = at("curl ")?;
        let digest = at(MISE_SHA256)?;
        let verify = at("sha256sum --check")?;
        let install = at(&format!(
            "{MISE} {ISOLATED} install aqua:release-plz/release-plz@0.4.3"
        ))?;
        let checkout = position(all, |step| {
            uses(step).is_some_and(|u| u.starts_with("actions/checkout@"))
        })?;
        let token = position(all, |step| {
            uses(step).is_some_and(|u| u.starts_with(APP_TOKEN))
        })?;
        assert!(
            download < digest && digest < verify && verify < install && install < checkout,
            "{id}"
        );
        assert!(checkout < token, "{id}");
        assert!(
            run(&all[download]).contains("/releases/download/v2026.11.1/mise-v2026.11.1-linux-x64")
        );
        for step in all {
            assert!(
                !uses(step).is_some_and(|u| u.starts_with("jdx/mise-action@")),
                "{id}"
            );
        }
        for step in all.iter().filter(|step| run(step).contains("release-plz ")) {
            assert!(
                run(step).starts_with(&format!(
                    "{MISE} {ISOLATED} exec aqua:release-plz/release-plz@0.4.3 -- release-plz "
                )),
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
            .filter(|step| run(step).contains(" -- release-plz "))
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
            .filter(|step| run(step).contains(" -- release-plz "))
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
