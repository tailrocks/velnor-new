// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::{CONFIG_PATH, LOCK_PATH};

use crate::private_sources::SECTION;
use crate::release_fixture::{APP, LOCK, release_view};
use crate::yaml_tree::document;

type TestResult = Result<(), Box<dyn Error>>;

const BINARY: &str = "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n";

fn release(private: bool) -> Result<serde_json::Value, Box<dyn Error>> {
    let mut view = release_view(&[APP], Some(BINARY))?;
    let section = if private { SECTION } else { "" };
    view.files.insert(
        CONFIG_PATH,
        format!("full_run = \"off\"\n{section}").into_bytes(),
    );
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.81", "0.1.86").into_bytes());
    let mut files = expected_files(&view)?;
    let text = files
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    Ok(document(&text)?)
}

fn steps(tree: &serde_json::Value, job: &str) -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
    Ok(tree["jobs"][job]["steps"]
        .as_array()
        .ok_or("no steps")?
        .clone())
}

fn build_index(steps: &[serde_json::Value]) -> Result<usize, Box<dyn Error>> {
    steps
        .iter()
        .position(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.starts_with("cargo build"))
        })
        .ok_or_else(|| "no cargo build".into())
}

#[test]
fn a_release_build_fetches_the_private_sources_before_an_offline_build() -> TestResult {
    let tree = release(true)?;
    let steps = steps(&tree, "build-app-linux-x64")?;
    let build = build_index(&steps)?;
    let token = steps
        .iter()
        .position(|step| step["id"] == "private-token")
        .ok_or("no token step")?;
    let fetch = steps
        .iter()
        .position(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.starts_with("velnor-ci fetch "))
        })
        .ok_or("no fetch step")?;
    let check = steps
        .iter()
        .position(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("\"$CHECKED_OUT\" = \"$COMMIT\""))
        })
        .ok_or("no same-commit check")?;
    assert!(
        check < token && token + 1 == fetch && fetch < build,
        "{steps:?}"
    );
    assert_eq!(
        steps[fetch]["run"],
        "velnor-ci fetch --manifest-path Cargo.toml"
    );
    let command = steps[build]["run"].as_str().ok_or("no command")?;
    assert!(command.contains(" --locked --offline "), "{command}");
    Ok(())
}

#[test]
fn the_release_token_reaches_only_the_fetch_step_and_is_not_gated_on_a_pull_request() -> TestResult
{
    let tree = release(true)?;
    for (id, job) in tree["jobs"].as_object().ok_or("no jobs")? {
        let all = job["steps"].as_array().cloned().unwrap_or_default();
        let builds = id.starts_with("build-");
        for step in &all {
            let rendered = step.to_string();
            let token = step["id"] == "private-token";
            let fetch = step["run"]
                .as_str()
                .is_some_and(|run| run.starts_with("velnor-ci fetch "));
            assert_eq!(
                rendered.contains("private-token"),
                builds && (token || fetch),
                "{id}: {rendered}"
            );
            assert_eq!(
                rendered.contains("DEPS_APP_PRIVATE_KEY"),
                builds && token,
                "{id}: {rendered}"
            );
            if token {
                assert!(step.get("if").is_none(), "{rendered}");
            }
        }
    }
    Ok(())
}

#[test]
fn a_release_without_private_sources_builds_as_before() -> TestResult {
    let tree = release(false)?;
    let steps = steps(&tree, "build-app-linux-x64")?;
    let command = steps[build_index(&steps)?]["run"]
        .as_str()
        .ok_or("no command")?
        .to_owned();
    assert!(!command.contains("--offline"), "{command}");
    assert!(steps.iter().all(|step| step["id"] != "private-token"));
    Ok(())
}
