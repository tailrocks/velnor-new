// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::release_fixture::{jobs, release_view, steps, uses};
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

const FILE: &str = ".github/workflows/defaults.yml";

fn config(checkout_with: &str, mise_with: &str) -> String {
    format!(
        r#"
[[workflow]]
file = "defaults"
name = "Defaults"

[[workflow.job]]
id = "build"
permissions = {{ contents = "read" }}

[[workflow.job.step]]
uses = "checkout"
{checkout_with}

[[workflow.job.step]]
uses = "mise"
{mise_with}

[[workflow.job.step]]
name = "Build"
run = "bash scripts/build.sh"
"#
    )
}

fn files(config: &str) -> Result<std::collections::BTreeMap<RepoPath, String>, Box<dyn Error>> {
    let mut view = release_view(&[], None)?;
    view.files.insert(
        RepoPath::new(".velnor/workflows.toml")?,
        config.as_bytes().to_vec(),
    );
    Ok(expected_files(&view)?)
}

fn rendered(config: &str) -> Result<Node, Box<dyn Error>> {
    let text = files(config)?
        .remove(&RepoPath::new(FILE)?)
        .ok_or("not rendered")?;
    Ok(parse(&text)?)
}

fn refusal(config: &str) -> Result<String, Box<dyn Error>> {
    match files(config) {
        Err(error) => Ok(error.to_string()),
        Ok(_) => Err("expected a refusal".into()),
    }
}

fn input_of(tree: &Node, action: &str, key: &str) -> Result<Option<String>, Box<dyn Error>> {
    let all = jobs(tree)?;
    let job = all.first().ok_or("no job")?.1;
    let step = steps(job)
        .iter()
        .find(|step| uses(step).is_some_and(|u| u.starts_with(action)))
        .ok_or("no such step")?;
    Ok(step
        .at(&["with", key])
        .and_then(Node::text)
        .map(str::to_owned))
}

#[test]
fn every_checkout_step_drops_the_credentials() -> TestResult {
    for with in [
        "",
        "with = { ref = \"main\" }",
        "with = { persist-credentials = \"false\" }",
    ] {
        let tree = rendered(&config(with, ""))?;
        let mut seen = 0;
        for (_, job) in jobs(&tree)? {
            for step in steps(job) {
                if uses(step).is_some_and(|u| u.starts_with("actions/checkout@")) {
                    seen += 1;
                    assert_eq!(
                        step.at(&["with", "persist-credentials"])
                            .and_then(Node::text),
                        Some("false"),
                        "{with}"
                    );
                }
            }
        }
        assert_eq!(seen, 1, "{with}");
    }
    Ok(())
}

#[test]
fn a_checkout_that_keeps_the_credentials_is_refused() -> TestResult {
    for value in ["true", "${{ inputs.keep }}", "yes", ""] {
        let with = format!("with = {{ persist-credentials = \"{value}\" }}");
        let message = refusal(&config(&with, ""))?;
        assert!(
            message.contains("persist-credentials"),
            "{value}: {message}"
        );
    }
    Ok(())
}

#[test]
fn the_mise_step_never_uses_the_implicit_cache() -> TestResult {
    for with in [
        "",
        "with = { install_args = \"rust\" }",
        "with = { cache = \"false\" }",
    ] {
        let tree = rendered(&config("", with))?;
        assert_eq!(
            input_of(&tree, "jdx/mise-action@", "cache")?.as_deref(),
            Some("false"),
            "{with}"
        );
    }
    Ok(())
}

#[test]
fn a_mise_cache_override_is_refused() -> TestResult {
    for with in [
        "with = { cache = \"true\" }",
        "with = { cache = \"${{ inputs.keep }}\" }",
        "with = { cache_save = \"true\" }",
        "with = { Cache = \"true\" }",
        "with = { CACHE_SAVE = \"true\" }",
        "with = { cache = \"true\", cacHe = \"false\" }",
    ] {
        let message = refusal(&config("", with))?;
        assert!(
            message.to_lowercase().contains("cache"),
            "{with}: {message}"
        );
    }
    Ok(())
}
