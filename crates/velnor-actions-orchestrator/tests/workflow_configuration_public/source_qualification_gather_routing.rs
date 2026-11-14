// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::source_qualification::WORKFLOW_PATH;
use super::source_qualification_selection::{fixture, install_selector};
use serde_json::Value;
use std::{error::Error, fs, path::Path, process::Command};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

fn run_gather(
    root: &Path,
    command: &str,
    event: &str,
    plan: &str,
    selection: &str,
    decision: &str,
    results: [&str; 3],
) -> Result<bool, Box<dyn Error>> {
    Ok(Command::new("sh")
        .args(["-e", "-c", command])
        .current_dir(root)
        .env("GITHUB_EVENT_NAME", event)
        .env("PLAN_RESULT", plan)
        .env("SELECTION_RESULT", selection)
        .env("DECISION", decision)
        .env("ORDINARY_RESULT", results[0])
        .env("KEEP_RESULT", results[1])
        .env("GENERATION_RESULT", results[2])
        .status()?
        .success())
}

type GatherCase = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    [&'static str; 3],
    bool,
);

const GATHER_CASES: &[GatherCase] = &[
    (
        "pull_request",
        "success",
        "success",
        "skip",
        ["skipped", "skipped", "skipped"],
        true,
    ),
    (
        "pull_request",
        "success",
        "success",
        "run",
        ["success", "success", "success"],
        true,
    ),
    (
        "workflow_dispatch",
        "skipped",
        "success",
        "run",
        ["success", "success", "success"],
        true,
    ),
    (
        "pull_request",
        "failure",
        "failure",
        "",
        ["skipped", "skipped", "skipped"],
        false,
    ),
    (
        "pull_request",
        "success",
        "success",
        "run",
        ["success", "failure", "success"],
        false,
    ),
    (
        "pull_request",
        "success",
        "success",
        "run",
        ["success", "cancelled", "success"],
        false,
    ),
    (
        "pull_request",
        "success",
        "success",
        "run",
        ["success", "skipped", "success"],
        false,
    ),
    (
        "workflow_dispatch",
        "skipped",
        "success",
        "skip",
        ["skipped", "skipped", "skipped"],
        false,
    ),
];

#[test]
fn source_qualification_gather_preserves_dispatch_and_fails_closed() -> Result<(), Box<dyn Error>> {
    let (repo, _) = fixture()?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow: Value = serde_saphyr::from_str(tree.get(WORKFLOW_PATH).ok_or("workflow")?)?;
    let steps = workflow["jobs"]["source-qualification"]["steps"]
        .as_array()
        .ok_or("gather")?;
    let verdict = steps
        .iter()
        .find(|step| step["name"] == "Require planned producer results")
        .and_then(|step| step["run"].as_str())
        .ok_or("verdict")?;
    install_selector(repo.path())?;
    let selection = workflow["jobs"]["selection"]["steps"]
        .as_array()
        .ok_or("selection")?;
    let selector = selection
        .iter()
        .find(|step| step["name"] == "Admit source qualification selection")
        .and_then(|step| step["run"].as_str())
        .ok_or("selector")?;
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("output");
    fs::write(&output, "")?;
    let status = Command::new("sh")
        .args(["-e", "-c", selector])
        .current_dir(repo.path())
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("PLAN_RESULT", "skipped")
        .env("GITHUB_OUTPUT", &output)
        .status()?;
    assert!(status.success());
    assert_eq!(fs::read_to_string(&output)?.trim(), "decision=run");
    let uploads: Vec<_> = steps
        .iter()
        .filter(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/upload-artifact@"))
        })
        .collect();
    let [upload] = uploads.as_slice() else {
        return Err("expected one gather artifact".into());
    };
    assert!(
        upload["if"]
            .as_str()
            .is_some_and(|condition| condition.contains("decision == 'run'"))
    );
    assert_eq!(
        upload["with"]["name"],
        "source-qualification-${{ github.run_id }}-${{ github.run_attempt }}"
    );
    for &(event, plan_result, selection, decision, results, expected) in GATHER_CASES {
        assert_eq!(
            run_gather(
                repo.path(),
                verdict,
                event,
                plan_result,
                selection,
                decision,
                results
            )?,
            expected,
            "{event} {decision} {results:?}"
        );
    }
    Ok(())
}
