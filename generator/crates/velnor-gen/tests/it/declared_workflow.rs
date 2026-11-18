// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::RepoPath;
use velnor_render::{DeclaredRule, Rule};
use velnor_rust::{DetectError, Reason};

use crate::pipeline::view;

type TestResult = Result<(), Box<dyn Error>>;

const DECLARED: &str = ".github/workflows/preview-authority.yml";

const TWO_JOBS: &str = r#"
[[workflow]]
name = "Preview authority"
file = "preview-authority.yml"
on = "workflow_call"
permissions = { contents = "read" }
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
environment = "generator-source-preview"
permissions = { actions = "read", contents = "write" }
outputs = { sign = "steps.draft.outputs.sign" }

[[workflow.job.step]]
uses = "download-artifact"
with = { artifact-ids = "${{ needs.source.outputs.artifact_id }}" }

[[workflow.job.step]]
id = "draft"
helper = "validate-artifact-id"
if = "needs.source.result == 'success'"
env = { ARTIFACT_ID = "${{ needs.source.outputs.artifact_id }}" }
"#;

type Files = BTreeMap<RepoPath, String>;

fn generate(config: &str) -> Result<Result<Files, GenerateError>, Box<dyn Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    crate::facts::lock_pinned_tools(&mut repo)?;
    Ok(expected_with_notes(&repo).map(|(files, _)| files))
}

fn declared_rules(config: &str) -> Result<Vec<DeclaredRule>, Box<dyn Error>> {
    match generate(config)? {
        Err(GenerateError::Rules(violations)) => Ok(violations
            .into_iter()
            .filter_map(|violation| match violation.rule {
                Rule::Declared(rule) => Some(rule),
                _ => None,
            })
            .collect()),
        _ => Err("expected a rule violation".into()),
    }
}

#[test]
fn a_declared_workflow_renders_jobs_with_needs_outputs_environment_permissions_and_step_conditions()
-> TestResult {
    let files = generate(TWO_JOBS)??;
    let text = files.get(&RepoPath::new(DECLARED)?).ok_or("not rendered")?;
    let tree = crate::yaml_tree::document(text)?;
    let at = |pointer: &str| tree.pointer(pointer).cloned();
    assert_eq!(at("/on/workflow_call"), Some(serde_json::json!({})));
    assert_eq!(at("/permissions/contents"), Some("read".into()));
    assert_eq!(at("/jobs/draft/needs"), Some(serde_json::json!(["source"])));
    assert_eq!(
        at("/jobs/draft/environment"),
        Some("generator-source-preview".into())
    );
    assert_eq!(at("/jobs/draft/permissions/contents"), Some("write".into()));
    assert_eq!(
        at("/jobs/draft/outputs/sign"),
        Some("${{ steps.draft.outputs.sign }}".into())
    );
    assert_eq!(
        at("/jobs/source/outputs/artifact_id"),
        Some("${{ steps.upload.outputs.artifact-id }}".into())
    );
    assert_eq!(
        at("/jobs/draft/steps/1/if"),
        Some("${{ needs.source.result == 'success' }}".into())
    );
    assert_eq!(
        at("/jobs/draft/steps/1/run"),
        Some("velnor-ci validate-artifact-id".into())
    );
    let uses = at("/jobs/source/steps/0/uses").ok_or("no uses")?;
    assert!(
        uses.as_str()
            .is_some_and(|uses| uses.starts_with("actions/upload-artifact@") && uses.len() == 64)
    );
    assert!(
        at("/jobs/source/if")
            .and_then(|value| value.as_str().map(str::to_owned))
            .is_some_and(|value| value.contains("sdk-preview-"))
    );
    Ok(())
}

#[test]
fn without_a_workflow_table_no_declared_file_is_written() -> TestResult {
    let files = generate("")??;
    assert!(!files.contains_key(&RepoPath::new(DECLARED)?));
    Ok(())
}

#[test]
fn a_declared_workflow_refuses_a_free_run_string() -> TestResult {
    let config = TWO_JOBS.replace(
        "helper = \"validate-artifact-id\"",
        "run = \"echo $ARTIFACT_ID\"",
    );
    assert!(matches!(
        generate(&config)?,
        Err(GenerateError::Detect(DetectError::Invalid {
            reason: Reason::InvalidValue { key, value: step },
            ..
        })) if step == "draft" && key.contains("run")
    ));
    Ok(())
}

#[test]
fn a_declared_workflow_refuses_unknown_needs_unknown_output_steps_and_unlisted_actions()
-> TestResult {
    let cases = [
        (
            TWO_JOBS.replace("needs = [\"source\"]", "needs = [\"missing\"]"),
            DeclaredRule::NeedNotDeclaredEarlier("missing".to_owned()),
        ),
        (
            TWO_JOBS.replace("steps.draft.outputs.sign", "steps.nowhere.outputs.sign"),
            DeclaredRule::UndeclaredStep("nowhere".to_owned()),
        ),
        (
            TWO_JOBS.replace("uses = \"upload-artifact\"", "uses = \"evil/action\""),
            DeclaredRule::UnknownAction("evil/action".to_owned()),
        ),
        (
            TWO_JOBS.replace("needs.source.outputs.artifact_id", "secrets.TOKEN"),
            DeclaredRule::ContextNotAllowed("secrets.TOKEN".to_owned()),
        ),
    ];
    for (config, expected) in cases {
        assert!(declared_rules(&config)?.contains(&expected), "{expected:?}");
    }
    Ok(())
}
