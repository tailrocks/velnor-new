// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::RELEASE_WORKFLOW_PATH;

use crate::release_fixture::{APP, APP_TOKEN, LOCK, TOOLS, release_view, steps, uses};
use crate::release_unguarded::expected_release;
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;
type Step = (String, Vec<String>);

fn refuse_steps(version: &str) -> Result<Vec<Step>, Box<dyn Error>> {
    let mut view = release_view(
        &[APP, TOOLS],
        Some("roots = [\".\", \"tools\"]\nafter = [\"Rust CI\", \"Lint\"]\n"),
    )?;
    view.files.insert(
        velnor_rust::LOCK_PATH,
        LOCK.replace("0.1.79", version).into_bytes(),
    );
    let text = expected_release(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    let tree = parse(&text)?;
    let mut found = Vec::new();
    for id in ["release-pr", "tag"] {
        let job = tree.at(&["jobs", id]).ok_or("no job")?;
        for step in steps(job) {
            let run = step.get("run").and_then(Node::text).unwrap_or_default();
            if run.starts_with("velnor-ci release check-config") {
                let mut env: Vec<String> = step
                    .get("env")
                    .map(|env| {
                        env.keys()
                            .into_iter()
                            .map(|key| {
                                format!(
                                    "{key}={}",
                                    env.get(key).and_then(Node::text).unwrap_or_default()
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                env.sort();
                env.insert(0, run.to_owned());
                found.push((id.to_owned(), env));
            }
        }
    }
    Ok(found)
}

#[test]
fn a_helper_that_knows_the_flag_gets_every_pattern_the_live_repository_and_the_token() -> TestResult
{
    let found = refuse_steps("0.1.81")?;
    assert_eq!(found.len(), 2);
    for (id, step) in found {
        let (prefixes, released) = if id == "tag" {
            (
                " --branch-prefix release-plz.toml release-plz-app- --branch-prefix tools/release-plz.toml release-plz-tools-",
                " --released-versions",
            )
        } else {
            ("", "")
        };
        assert_eq!(
            step,
            [
                &format!(
                    "velnor-ci release check-config release-plz.toml tools/release-plz.toml --tag-pattern release-plz.toml 'app-v{{{{ version }}}}' --tag-pattern release-plz.toml '{{{{ package }}}}-v{{{{ version }}}}' --tag-pattern tools/release-plz.toml 'tools-v{{{{ version }}}}' --tag-pattern tools/release-plz.toml '{{{{ package }}}}-v{{{{ version }}}}'{prefixes} --repo=\"$REPOSITORY\" --manifests{released}"
                ),
                "CARGO_BUILD_RUSTC=rustc",
                "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER=",
                "CARGO_BUILD_RUSTC_WRAPPER=",
                "GITHUB_TOKEN=${{ github.token }}",
                "REPOSITORY=${{ github.repository }}",
                "RUSTC=rustc",
                "RUSTC_WORKSPACE_WRAPPER=",
                "RUSTC_WRAPPER=",
                "RUSTUP_TOOLCHAIN=stable",
            ]
        );
    }
    Ok(())
}

#[test]
fn helper_0_1_80_gets_the_tag_patterns_but_not_released_versions() -> TestResult {
    let found = refuse_steps("0.1.80")?;
    assert_eq!(found.len(), 2);
    for (_, step) in found {
        assert!(
            step[0].ends_with("--repo=\"$REPOSITORY\" --manifests"),
            "{}",
            step[0]
        );
        assert!(!step[0].contains("--released-versions"));
    }
    Ok(())
}

#[test]
fn a_helper_that_predates_the_release_checks_is_refused_with_the_version_to_pin() -> TestResult {
    for version in ["0.1.79", "0.1.0"] {
        let error = refuse_steps(version)
            .err()
            .ok_or("accepted an old helper")?;
        assert_eq!(
            error.to_string(),
            format!(
                "velnor-ci {version} cannot check the release on the released commit, because it does not recheck the live tag ruleset or the manifests; pin velnor-ci 0.1.80 or later with `velnor-gen pin-helper 0.1.80`"
            )
        );
    }
    Ok(())
}

#[test]
fn the_check_runs_before_the_app_token_with_only_the_job_token() -> TestResult {
    let mut view = release_view(
        &[APP, TOOLS],
        Some("roots = [\".\", \"tools\"]\nafter = [\"Rust CI\", \"Lint\"]\n"),
    )?;
    view.files.insert(
        velnor_rust::LOCK_PATH,
        LOCK.replace("0.1.79", "0.1.80").into_bytes(),
    );
    let text = expected_release(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    let tree = parse(&text)?;
    for id in ["release-pr", "tag"] {
        let job = tree.at(&["jobs", id]).ok_or("no job")?;
        let all = steps(job);
        let check = all
            .iter()
            .position(|step| {
                step.get("run")
                    .and_then(Node::text)
                    .is_some_and(|run| run.starts_with("velnor-ci release check-config"))
            })
            .ok_or("no check-config step")?;
        let token = all
            .iter()
            .position(|step| uses(step).is_some_and(|action| action.starts_with(APP_TOKEN)))
            .ok_or("no app token step")?;
        assert!(
            check < token,
            "{id}: check-config at {check}, token at {token}"
        );
        let env = all[check].get("env").ok_or("no env")?;
        let mut keys = env.keys();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "CARGO_BUILD_RUSTC",
                "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
                "CARGO_BUILD_RUSTC_WRAPPER",
                "GITHUB_TOKEN",
                "REPOSITORY",
                "RUSTC",
                "RUSTC_WORKSPACE_WRAPPER",
                "RUSTC_WRAPPER",
                "RUSTUP_TOOLCHAIN",
            ],
            "{id}"
        );
        assert_eq!(
            env.get("GITHUB_TOKEN").and_then(Node::text),
            Some("${{ github.token }}")
        );
    }
    Ok(())
}
