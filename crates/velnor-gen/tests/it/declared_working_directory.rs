// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::release_fixture::steps;
use crate::yaml_tree::Node;

fn config(step_keys: &str) -> String {
    workflow(
        "",
        "",
        &format!(
            "\n[[workflow.job]]\nid = \"check\"\n\n[[workflow.job.step]]\nname = \"Sum\"\nrun = \"sha256sum a.txt\"\n{step_keys}\n"
        ),
    )
}

#[test]
fn a_run_step_can_run_in_a_directory_of_the_workspace() -> TestResult {
    for directory in ["dist", "target/release", "a-b_c.d/e"] {
        let tree = rendered(&config(&format!("working_directory = \"{directory}\"")))?;
        let step = &steps(job(&tree, "check")?)[0];
        assert_eq!(
            step.get("working-directory").and_then(Node::text),
            Some(directory)
        );
    }
    let tree = rendered(&config(""))?;
    assert!(
        steps(job(&tree, "check")?)[0]
            .get("working-directory")
            .is_none()
    );
    Ok(())
}

#[test]
fn a_working_directory_outside_the_workspace_or_computed_is_refused() -> TestResult {
    for directory in [
        "..",
        "../x",
        "a/../b",
        "a/..",
        "/etc",
        "~/x",
        "~",
        ".",
        "./a",
        "a/./b",
        "a//b",
        "a/",
        "",
        "${{ github.workspace }}",
        "$HOME/x",
        "a\\\\b",
        "C:x",
        "a`b",
    ] {
        let message = refusal(&config(&format!("working_directory = '{directory}'")))
            .map_err(|error| format!("{directory:?}: {error}"))?;
        assert!(
            message.contains("working_directory"),
            "{directory:?}: {message}"
        );
    }
    Ok(())
}

#[test]
fn only_a_run_step_takes_a_working_directory() -> TestResult {
    let uses = workflow(
        "",
        "",
        "\n[[workflow.job]]\nid = \"check\"\n\n[[workflow.job.step]]\nname = \"Out\"\nuses = \"checkout\"\nworking_directory = \"dist\"\n",
    );
    let message = refusal(&uses)?;
    assert!(message.contains("not a run step"), "{message}");
    Ok(())
}
