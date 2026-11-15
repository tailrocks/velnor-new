// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::{APP_TOKEN, app, steps, two_roots, uses};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const ISOLATED: &str = "mise --no-config --no-env --no-hooks";
const MISE_SHA256: &str = "7129dbab31d698f9028db9374f7ba7987232b0426f837dcbce437130a61415f9";

fn token_jobs(tree: &Node) -> Result<Vec<(&'static str, &[Node])>, Box<dyn Error>> {
    ["release-pr", "tag"]
        .into_iter()
        .map(|id| Ok((id, steps(tree.at(&["jobs", id]).ok_or("no job")?))))
        .collect()
}

fn run(step: &Node) -> &str {
    step.get("run").and_then(Node::text).unwrap_or_default()
}

#[test]
fn release_plz_is_installed_by_name_and_version_without_the_repository_mise_config() -> TestResult {
    let (_, tree) = two_roots()?;
    for (id, all) in token_jobs(&tree)? {
        let mise = all
            .iter()
            .find(|step| uses(step).is_some_and(|u| u.starts_with("jdx/mise-action@")))
            .ok_or("no mise step")?;
        assert_eq!(
            mise.at(&["with", "install"]).and_then(Node::text),
            Some("false"),
            "{id}"
        );
        assert_eq!(
            mise.at(&["with", "env"]).and_then(Node::text),
            Some("false"),
            "{id}"
        );
        assert_eq!(
            mise.at(&["with", "sha256"]).and_then(Node::text),
            Some(MISE_SHA256),
            "{id}"
        );
        assert!(mise.at(&["with", "install_args"]).is_none(), "{id}");
        let install = format!("{ISOLATED} install aqua:release-plz/release-plz@0.4.3");
        assert!(all.iter().any(|step| run(step) == install), "{id}");
        for step in all.iter().filter(|step| run(step).contains("release-plz ")) {
            assert!(
                run(step).starts_with(&format!(
                    "{ISOLATED} exec aqua:release-plz/release-plz@0.4.3 -- release-plz "
                )),
                "{id}: {}",
                run(step)
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
fn no_step_before_the_release_plz_commands_exports_repository_mise_env() -> TestResult {
    let (_, tree) = app()?;
    for (id, all) in token_jobs(&tree)? {
        let token = all
            .iter()
            .position(|step| uses(step).is_some_and(|u| u.starts_with(APP_TOKEN)))
            .ok_or("no token")?;
        for step in &all[..token] {
            if uses(step).is_some_and(|u| u.starts_with("jdx/mise-action@")) {
                assert_eq!(
                    step.at(&["with", "env"]).and_then(Node::text),
                    Some("false"),
                    "{id}"
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
