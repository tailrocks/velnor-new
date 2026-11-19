// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::release_fixture::{jobs, release_view, steps};
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

const FILE: &str = ".github/workflows/preview-authority.yml";

const TWO_JOBS: &str = r#"
[[workflow]]
file = "preview-authority"
name = "Preview authority"
if = "github.event_name == 'push' && startsWith(github.ref, 'refs/tags/sdk-preview-')"

[[workflow.job]]
id = "source"
timeout_minutes = 5
permissions = { actions = "read", contents = "read" }
outputs = { artifact_id = "steps.upload.outputs.artifact-id" }

[[workflow.job.step]]
id = "upload"
uses = "upload-artifact"
with = { name = "source", path = "dist" }

[[workflow.job]]
id = "draft"
needs = ["source"]
helper = true
environment = "generator-source-preview"
permissions = { actions = "read", contents = "write", checks = "none" }
outputs = { sign = "steps.draft.outputs.sign" }

[[workflow.job.step]]
uses = "download-artifact"
with = { artifact-ids = "${{ needs.source.outputs.artifact_id }}" }

[[workflow.job.step]]
id = "draft"
helper = "validate-artifact-id"
if = "needs.source.result == 'success'"
env = { ARTIFACT_ID = "${{ needs.source.outputs.artifact_id }}" }

[[workflow.job.step]]
name = "Show the id"
run = "velnor-ci show-id"
"#;

fn rendered(config: &str) -> Result<Node, Box<dyn Error>> {
    let mut view = release_view(&[], None)?;
    view.files.insert(
        RepoPath::new(".velnor/workflows.toml")?,
        config.as_bytes().to_vec(),
    );
    let mut files = expected_files(&view)?;
    let text = files.remove(&RepoPath::new(FILE)?).ok_or("not rendered")?;
    Ok(parse(&text)?)
}

fn refusal(config: &str) -> Result<String, Box<dyn Error>> {
    let mut view = release_view(&[], None)?;
    view.files.insert(
        RepoPath::new(".velnor/workflows.toml")?,
        config.as_bytes().to_vec(),
    );
    match expected_files(&view) {
        Err(error) => Ok(error.to_string()),
        Ok(_) => Err("expected a refusal".into()),
    }
}

fn job<'a>(tree: &'a Node, id: &str) -> Result<&'a Node, Box<dyn Error>> {
    Ok(jobs(tree)?
        .into_iter()
        .find(|(name, _)| *name == id)
        .ok_or("no such job")?
        .1)
}

#[test]
fn a_declared_workflow_renders_environment_permissions_conditions_and_steps() -> TestResult {
    let tree = rendered(TWO_JOBS)?;
    let draft = job(&tree, "draft")?;
    assert_eq!(
        draft.get("environment").and_then(Node::text),
        Some("generator-source-preview")
    );
    assert_eq!(
        draft.at(&["permissions", "contents"]).and_then(Node::text),
        Some("write")
    );
    assert_eq!(
        draft.at(&["permissions", "checks"]).and_then(Node::text),
        Some("none")
    );
    assert_eq!(draft.get("needs").map(Node::texts), Some(vec!["source"]));
    let all = steps(draft);
    let validate = all
        .iter()
        .find(|step| step.get("id").and_then(Node::text) == Some("draft"))
        .ok_or("no helper step")?;
    assert_eq!(
        validate.get("if").and_then(Node::text),
        Some("${{ needs.source.result == 'success' }}")
    );
    assert_eq!(
        validate.get("run").and_then(Node::text),
        Some(". \"$VELNOR_EXEC\" validate-artifact-id")
    );
    assert!(
        all.iter()
            .any(|step| step.get("run").and_then(Node::text) == Some(". \"$VELNOR_EXEC\" show-id"))
    );
    let source = job(&tree, "source")?;
    let upload = steps(source);
    let uses = upload
        .first()
        .and_then(|step| step.get("uses"))
        .and_then(Node::text)
        .ok_or("no uses")?;
    assert!(uses.starts_with("actions/upload-artifact@") && uses.len() == 64);
    assert!(
        source
            .get("if")
            .and_then(Node::text)
            .is_some_and(|value| value.contains("sdk-preview-"))
    );
    assert!(
        draft
            .get("if")
            .and_then(Node::text)
            .is_some_and(|value| value.contains("sdk-preview-"))
    );
    Ok(())
}

#[test]
fn a_run_step_still_obeys_the_run_rules() -> TestResult {
    for (run, needle) in [
        ("echo a; echo b", ";"),
        ("echo a | cat", "|"),
        ("echo a & echo b", "&"),
        ("echo ${{ github.sha }}", "expression"),
        ("echo a\\necho b", "more than one line"),
    ] {
        let config = TWO_JOBS.replace("velnor-ci show-id", run);
        assert!(refusal(&config)?.contains(needle), "{run}");
    }
    Ok(())
}

#[test]
fn a_step_must_be_exactly_one_of_run_uses_and_helper() -> TestResult {
    let both = TWO_JOBS.replace(
        "helper = \"validate-artifact-id\"",
        "helper = \"validate-artifact-id\"\nrun = \"velnor-ci x\"",
    );
    assert!(refusal(&both)?.contains("exactly one"));
    let none = TWO_JOBS.replace("run = \"velnor-ci show-id\"", "");
    assert!(refusal(&none)?.contains("exactly one"));
    Ok(())
}

#[test]
fn a_helper_step_needs_the_helper_installed_in_its_job() -> TestResult {
    let config = TWO_JOBS.replace("helper = true\n", "");
    assert!(refusal(&config)?.contains("helper = true"));
    Ok(())
}

#[test]
fn a_declared_workflow_refuses_unknown_needs_steps_actions_contexts_and_scopes() -> TestResult {
    let cases = [
        (
            TWO_JOBS.replace("needs = [\"source\"]", "needs = [\"missing\"]"),
            "missing",
        ),
        (
            TWO_JOBS.replace("steps.draft.outputs.sign", "steps.nowhere.outputs.sign"),
            "nowhere",
        ),
        (
            TWO_JOBS.replace("uses = \"upload-artifact\"", "uses = \"evil/action\""),
            "evil/action",
        ),
        (
            TWO_JOBS.replace("needs.source.outputs.artifact_id", "secrets.TOKEN"),
            "secrets.TOKEN",
        ),
        (
            TWO_JOBS.replace("contents = \"write\"", "bogus = \"write\""),
            "bogus",
        ),
        (
            TWO_JOBS.replace(
                "needs.source.result == 'success'",
                "needs.other.result == 'x'",
            ),
            "needs.other.result",
        ),
        (
            TWO_JOBS.replace("needs.source.result", "inputs.nope"),
            "inputs.nope",
        ),
    ];
    for (config, needle) in cases {
        assert!(refusal(&config)?.contains(needle), "{needle}");
    }
    Ok(())
}

#[test]
fn a_condition_may_read_results_but_a_value_may_not() -> TestResult {
    let config = TWO_JOBS.replace(
        "env = { ARTIFACT_ID = \"${{ needs.source.outputs.artifact_id }}\" }",
        "env = { ARTIFACT_ID = \"${{ needs.source.result }}\" }",
    );
    assert!(refusal(&config)?.contains("needs.source.result"));
    Ok(())
}
