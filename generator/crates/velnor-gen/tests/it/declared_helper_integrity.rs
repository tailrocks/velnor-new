// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::release_fixture::steps;
use crate::yaml_tree::Node;

const PUSH: &str = "[workflow.on.push]\nbranches = [\"main\"]";

const HELPER_JOB: &str = r#"
[[workflow.job]]
id = "check"
helper = true

[[workflow.job.step]]
name = "Build"
run = "echo build"

[[workflow.job.step]]
id = "validate"
helper = "validate-artifact-id"

[[workflow.job.step]]
name = "Plan"
run = "velnor-ci plan"
"#;

fn config() -> String {
    workflow("", PUSH, HELPER_JOB)
}

fn names(tree: &Node) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(steps(job(tree, "check")?)
        .iter()
        .filter_map(|step| step.get("name").and_then(Node::text).map(str::to_owned))
        .collect())
}

#[test]
fn a_helper_job_installs_the_integrity_launcher_right_after_the_setup_action() -> TestResult {
    let names = names(&rendered(&config())?)?;
    let setup = names
        .iter()
        .position(|name| name.starts_with("Install velnor-ci"))
        .ok_or("no setup step")?;
    assert_eq!(names[setup - 1], "Resolve released helper tools");
    assert_eq!(names[setup + 1], "Install the released helper launcher");
    Ok(())
}

#[test]
fn helper_calls_go_through_the_per_invocation_launcher() -> TestResult {
    let tree = rendered(&config())?;
    let runs: Vec<_> = steps(job(&tree, "check")?)
        .iter()
        .filter_map(|step| step.get("run").and_then(Node::text).map(str::to_owned))
        .collect();
    assert!(
        runs.iter().all(|run| !run.starts_with("velnor-ci ")),
        "{runs:?}"
    );
    assert!(runs.contains(&". \"$VELNOR_EXEC\" validate-artifact-id".to_owned()));
    assert!(runs.contains(&". \"$VELNOR_EXEC\" plan".to_owned()));
    assert_eq!(
        tree.at(&["defaults", "run", "shell"]).and_then(Node::text),
        Some("bash -p {0}")
    );
    Ok(())
}

#[test]
fn a_helper_call_that_is_not_the_plain_command_word_is_refused() -> TestResult {
    for run in [
        "env A=b velnor-ci plan",
        "./velnor-ci plan",
        "A=b velnor-ci plan",
    ] {
        let job = HELPER_JOB.replace("velnor-ci plan", run);
        let message = refusal(&workflow("", PUSH, &job))?;
        assert!(
            message.contains("must start with velnor-ci"),
            "{run}: {message}"
        );
    }
    Ok(())
}
