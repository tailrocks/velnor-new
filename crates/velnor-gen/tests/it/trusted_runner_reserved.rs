// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::declared_fixture::{JOB, TestResult, refusal, rendered_file, workflow};
use crate::facts::lock_pinned_tools;
use crate::pipeline::{pin_helper, view};

const POOL: &str = "velnor-box-trusted";
const BOX: &str = "velnor-box";
const ON: &str = "[workflow.on.workflow_dispatch]\n";

const CASES: &str = r#"
[workflow.on.workflow_dispatch]

[[workflow.on.workflow_dispatch.input]]
name = "mode"
type = "choice"
options = ["both", "js"]
default = "both"

[[workflow.case]]
id = "js"

[[workflow.case.step]]
name = "Run"
run = "echo js"

[[workflow.job]]
id = "plain"

[[workflow.job.step]]
name = "Plain"
run = "echo plain"
"#;

fn second(file: &str, header: &str, jobs: &str) -> String {
    format!("\n[[workflow]]\nfile = \"{file}\"\nname = \"{file}\"\n{header}\n{ON}\n{jobs}\n")
}

#[test]
fn a_workflow_runner_equal_to_the_trusted_label_is_refused_when_cases_replace_the_scale_set()
-> TestResult {
    for runner in [POOL.to_owned(), POOL.to_uppercase()] {
        let header = format!(
            "runner = \"{runner}\"\ntrusted_runner = \"{POOL}\"\nhosted_runner = \"ubuntu-24.04\"\nscale_set_runner = \"{BOX}\"\nfork_runner = \"ubuntu-24.04\""
        );
        let message = refusal(&workflow(&header, "", CASES))?;
        assert!(message.contains("trusted_runner"), "{runner}: {message}");
    }
    Ok(())
}

#[test]
fn a_workflow_runner_on_the_trusted_label_of_another_workflow_is_refused_in_either_order()
-> TestResult {
    let trusted = format!("runner = \"{BOX}\"\ntrusted_runner = \"{POOL}\"");
    for runner in [POOL.to_owned(), POOL.to_uppercase()] {
        let other = second("deploy", &format!("runner = \"{runner}\""), JOB);
        let first = workflow(&trusted, ON, JOB);
        let sequences = [format!("{first}{other}"), format!("{other}{first}")];
        for config in sequences {
            let message = refusal(&config)?;
            assert!(message.contains("trusted_runner"), "{config}: {message}");
        }
    }
    Ok(())
}

#[test]
fn a_job_on_the_trusted_label_of_another_workflow_is_refused() -> TestResult {
    let trusted = format!("runner = \"{BOX}\"\ntrusted_runner = \"{POOL}\"");
    let job = format!(
        "\n[[workflow.job]]\nid = \"deploy\"\nrunner = \"{POOL}\"\n\n[[workflow.job.step]]\nname = \"Deploy\"\nrun = \"echo ok\"\n"
    );
    let config = format!(
        "{}{}",
        workflow(&trusted, ON, JOB),
        second("deploy", &format!("runner = \"{BOX}\""), &job)
    );
    let message = refusal(&config)?;
    assert!(message.contains("trusted_runner"), "{message}");
    Ok(())
}

#[test]
fn two_workflows_may_share_one_trusted_label() -> TestResult {
    let trusted = format!("runner = \"{BOX}\"\ntrusted_runner = \"{POOL}\"");
    let config = format!(
        "{}{}",
        workflow(&trusted, ON, JOB),
        second("deploy", &trusted, JOB)
    );
    rendered_file(&config, "deploy")?;
    Ok(())
}

fn combined(rust_ci: &str, workflows: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], Some(rust_ci.as_bytes()))?;
    pin_helper(&mut repo, "0.1.110")?;
    repo.files.insert(
        RepoPath::new(".velnor/workflows.toml")?,
        workflows.as_bytes().to_vec(),
    );
    lock_pinned_tools(&mut repo)?;
    match expected_files(&repo) {
        Err(error) => Ok(error.to_string()),
        Ok(_) => Err(format!("accepted: {rust_ci} {workflows}").into()),
    }
}

#[test]
fn a_workflow_on_the_trusted_label_of_rust_ci_is_refused() -> TestResult {
    let rust_ci = format!("runner = '{BOX}'\ntrusted_runner = '{POOL}'\n");
    let workflows = workflow(&format!("runner = \"{POOL}\""), ON, JOB);
    let message = combined(&rust_ci, &workflows)?;
    assert!(message.contains("trusted_runner"), "{message}");
    Ok(())
}

#[test]
fn a_rust_ci_runner_on_the_trusted_label_of_a_workflow_is_refused() -> TestResult {
    let rust_ci = format!("runner = '{POOL}'\n");
    let workflows = workflow(
        &format!("runner = \"{BOX}\"\ntrusted_runner = \"{POOL}\""),
        ON,
        JOB,
    );
    let message = combined(&rust_ci, &workflows)?;
    assert!(message.contains("trusted_runner"), "{message}");
    Ok(())
}
