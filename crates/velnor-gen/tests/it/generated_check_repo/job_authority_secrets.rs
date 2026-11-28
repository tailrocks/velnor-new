// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::job_authority_summary;
use velnor_model::RepoPath;
use velnor_workflows::{reads_secret_in_condition, reads_secret_in_text};

use crate::facts::TestResult;

const READS: [&str; 5] = [
    "${{ secrets.A }}",
    "${{ secrets['A'] }}",
    "${{ SECRETS.A }}",
    "${{ toJSON(secrets) }}",
    "${{ format('{0}', secrets.A) }}",
];

fn workflow(top: &str, step: &str) -> String {
    format!(
        "on:\n  push:\n    branches: [main]\npermissions:\n  contents: read\n{top}jobs:\n  build:\n    runs-on: x\n    steps:\n      - run: echo ok\n      - {step}\n"
    )
}

fn left_out(text: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let path = RepoPath::new(".github/workflows/ci.yml")?;
    let (kept, notes) = job_authority_summary(&path, text, "main")?;
    let omitted = notes
        .iter()
        .filter(|note| note.contains("left out"))
        .count();
    assert_eq!(kept + omitted, 1, "{notes:?}");
    Ok(kept == 0)
}

#[test]
fn every_form_of_a_secret_read_in_a_step_leaves_the_job_out() -> TestResult {
    for read in READS {
        let text = workflow("", &format!("env: {{ TOKEN: \"{read}\" }}"));
        assert!(left_out(&text)?, "{read}");
    }
    assert!(left_out(&workflow(
        "",
        "if: secrets.A != ''\n        run: echo"
    ))?);
    Ok(())
}

#[test]
fn a_secret_read_in_the_workflow_env_leaves_every_job_out() -> TestResult {
    for read in READS {
        let text = workflow(&format!("env:\n  TOKEN: \"{read}\"\n"), "run: echo");
        assert!(left_out(&text)?, "{read}");
    }
    Ok(())
}

#[test]
fn text_that_only_mentions_secrets_keeps_the_job() -> TestResult {
    for step in [
        "run: echo secrets.A",
        "name: rotate secrets[0]",
        "run: echo \"${{ 'secrets.A' }}\"",
    ] {
        assert!(!left_out(&workflow("", step))?, "{step}");
    }
    assert!(!left_out(&workflow(
        "env:\n  NOTE: secrets.A\n",
        "run: echo"
    ))?);
    Ok(())
}

#[test]
fn every_form_of_the_secrets_context_in_text_is_a_read() {
    for text in [
        "${{ secrets.TOKEN }}",
        "${{ secrets['TOKEN'] }}",
        "${{ SECRETS.TOKEN }}",
        "${{ toJSON(secrets) }}",
        "a ${{ 'x' }} b ${{ secrets.T }}",
        "${{ '}}' && secrets.T }}",
    ] {
        assert!(reads_secret_in_text(text), "{text}");
    }
    for text in [
        "echo secrets.TOKEN",
        "${{ 'secrets.TOKEN' }}",
        "${{ github.ref }} secrets.A",
        "${{ vars.secrets_name }}",
    ] {
        assert!(!reads_secret_in_text(text), "{text}");
    }
}

#[test]
fn a_condition_is_read_with_or_without_the_delimiters() {
    assert!(reads_secret_in_condition("secrets.A != ''"));
    assert!(reads_secret_in_condition("${{ secrets.A != '' }}"));
    assert!(!reads_secret_in_condition("github.ref == 'secrets.A'"));
}
