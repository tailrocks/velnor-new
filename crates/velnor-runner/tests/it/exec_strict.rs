// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use serde_json::{Value, json};
use velnor_runner::exec::{
    AdapterRegistry, AnnotationLevel, JobCancel, JobResult, JobRun, StepResult,
};

use crate::exec_builder::{execute, expression, job, literal, mapping, run, with};
use crate::exec_support::Scratch;

const NOT_A_MAPPING: &str = "Unexpected template token: a mapping was expected";
const NO_REFERENCE: &str = "unsupported: step without a definition reference";

async fn finish(message: &Value) -> Result<(JobRun, Vec<String>)> {
    execute(
        message,
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await
}

fn errors(finished: &JobRun) -> Vec<&str> {
    finished
        .annotations
        .iter()
        .filter(|annotation| annotation.level == AnnotationLevel::Error)
        .map(|annotation| annotation.message.as_str())
        .collect()
}

fn has_line(lines: &[String], expected: &str) -> bool {
    lines.iter().any(|line| line == expected)
}

#[tokio::test]
async fn secrets_that_differ_only_by_case_resolve_to_the_last_value() -> Result<()> {
    let script = "case \"${{ secrets.MY_SECRET }}\" in\n  beta-value) echo last-wins;;\n  alpha-value) echo first-wins;;\nesac";
    let mut message = job(vec![run("a", script)]);
    message["Variables"] = json!({
        "MY_SECRET": {"Value": "alpha-value", "IsSecret": true},
        "my_secret": {"Value": "beta-value", "IsSecret": true}
    });
    let (finished, lines) = finish(&message).await?;
    assert_eq!(finished.result, JobResult::Succeeded, "{lines:?}");
    assert!(has_line(&lines, "last-wins"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn step_ids_that_differ_only_by_non_ascii_case_keep_the_last_step() -> Result<()> {
    let steps = vec![
        run("\u{e9}", "echo x=1 >> \"$GITHUB_OUTPUT\""),
        run("\u{c9}", "echo x=2 >> \"$GITHUB_OUTPUT\""),
        run("c", "echo \"seen-${{ steps['\u{c9}'].outputs.x }}\""),
    ];
    let (finished, lines) = finish(&job(steps)).await?;
    assert_eq!(finished.result, JobResult::Succeeded, "{lines:?}");
    assert!(has_line(&lines, "seen-2"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn the_tool_cache_directory_exists_before_any_step_runs() -> Result<()> {
    let script = "[ -d \"${{ runner.tool_cache }}\" ] && echo cache-exists\n[ \"$RUNNER_TOOL_CACHE\" = \"${{ runner.tool_cache }}\" ] && echo same-path";
    let (finished, lines) = finish(&job(vec![run("a", script)])).await?;
    assert_eq!(finished.result, JobResult::Succeeded, "{lines:?}");
    assert!(has_line(&lines, "cache-exists"), "{lines:?}");
    assert!(has_line(&lines, "same-path"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_job_outputs_token_that_is_not_a_well_formed_mapping_fails_the_job() -> Result<()> {
    let malformed = [
        json!("oops"),
        literal("oops"),
        json!({"type": 2, "map": [{"key": literal("a")}]}),
        json!({"type": 2, "map": [{"value": literal("a")}]}),
        json!({"type": 2, "map": ["a"]}),
        json!({"type": 2, "map": {}}),
        json!({"type": 1}),
        json!({"type": "2"}),
    ];
    for token in malformed {
        let mut message = job(vec![run("a", "echo ran")]);
        message["JobOutputs"] = token.clone();
        let (finished, lines) = finish(&message).await?;
        assert_eq!(finished.result, JobResult::Failed, "{token}: {lines:?}");
        assert_eq!(finished.steps[0].conclusion, StepResult::Succeeded);
        assert!(finished.outputs.is_empty(), "{token}");
        assert!(
            errors(&finished)
                .iter()
                .any(|error| error.starts_with("Unexpected template token")),
            "{token}: {:?}",
            finished.annotations
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_slot_that_is_not_a_mapping_fails_where_it_is_evaluated() -> Result<()> {
    let mut environment = job(vec![run("a", "echo never")]);
    environment["EnvironmentVariables"] = json!([literal("oops")]);
    let mut defaults = job(vec![run("a", "echo never")]);
    defaults["Defaults"] = json!([literal("oops")]);
    for message in [environment, defaults] {
        let (finished, lines) = finish(&message).await?;
        assert_eq!(finished.result, JobResult::Failed, "{lines:?}");
        assert!(finished.steps.is_empty(), "{:?}", finished.steps);
        assert!(errors(&finished).contains(&NOT_A_MAPPING), "{lines:?}");
    }
    for slot in ["Environment", "Inputs"] {
        let message = job(vec![with(run("a", "echo never"), slot, literal("oops"))]);
        let (finished, lines) = finish(&message).await?;
        assert_eq!(finished.result, JobResult::Failed, "{slot}: {lines:?}");
        assert_eq!(finished.steps[0].conclusion, StepResult::Failed, "{slot}");
        assert!(!has_line(&lines, "never"), "{slot}: {lines:?}");
        assert!(
            errors(&finished).contains(&NOT_A_MAPPING),
            "{slot}: {lines:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn null_tokens_are_empty_mappings() -> Result<()> {
    let mut message = job(vec![with(
        run("a", "echo ran"),
        "Environment",
        json!({"type": 7}),
    )]);
    message["JobOutputs"] = json!({"type": 7});
    message["EnvironmentVariables"] = json!([null, {"type": 7}]);
    let (finished, lines) = finish(&message).await?;
    assert_eq!(finished.result, JobResult::Succeeded, "{lines:?}");
    assert!(finished.outputs.is_empty());
    Ok(())
}

#[tokio::test]
async fn an_action_step_without_a_reference_fails_the_job_before_any_step() -> Result<()> {
    for reference in [None, Some(Value::Null)] {
        let mut broken = run("a", "echo never");
        match reference {
            Some(value) => broken["Reference"] = value,
            None => {
                broken
                    .as_object_mut()
                    .map(|fields| fields.remove("Reference"));
            }
        }
        let (finished, lines) = finish(&job(vec![run("b", "echo also-never"), broken])).await?;
        assert_eq!(finished.result, JobResult::Failed, "{lines:?}");
        assert!(finished.steps.is_empty(), "{:?}", finished.steps);
        assert!(errors(&finished).contains(&NO_REFERENCE), "{lines:?}");
        assert!(!has_line(&lines, "also-never"), "{lines:?}");
    }
    Ok(())
}

const SHOW_A: &str = "echo \"A=[$A]\"";
const FROM_JSON: &str = "fromJSON('{\"A\":\"from-json\"}')";

#[tokio::test]
async fn a_mapping_token_without_entries_is_an_empty_mapping() -> Result<()> {
    let empty = json!({"type": 2});
    let mut message = job(vec![with(
        run("a", "echo ran"),
        "Environment",
        empty.clone(),
    )]);
    message["JobOutputs"] = empty.clone();
    message["EnvironmentVariables"] = json!([empty]);
    let (finished, lines) = finish(&message).await?;
    assert_eq!(finished.result, JobResult::Succeeded, "{lines:?}");
    assert!(has_line(&lines, "ran"), "{lines:?}");
    assert!(finished.outputs.is_empty());
    Ok(())
}

#[tokio::test]
async fn an_expression_that_yields_a_mapping_supplies_the_whole_mapping() -> Result<()> {
    let step_env = job(vec![with(
        run("a", SHOW_A),
        "Environment",
        expression(FROM_JSON),
    )]);
    let mut job_env = job(vec![run("a", SHOW_A)]);
    job_env["EnvironmentVariables"] = json!([expression(FROM_JSON)]);
    let mut outputs = job(vec![run("a", "echo ran")]);
    outputs["JobOutputs"] = expression(FROM_JSON);
    let inputs = job(vec![with(
        run("a", "echo never"),
        "Inputs",
        expression("fromJSON('{\"script\":\"echo from-inputs\"}')"),
    )]);
    for (message, expected) in [
        (step_env, "A=[from-json]"),
        (job_env, "A=[from-json]"),
        (outputs, "ran"),
        (inputs, "from-inputs"),
    ] {
        let (finished, lines) = finish(&message).await?;
        assert_eq!(finished.result, JobResult::Succeeded, "{lines:?}");
        assert!(has_line(&lines, expected), "{expected}: {lines:?}");
        assert!(!has_line(&lines, "never"), "{lines:?}");
    }
    Ok(())
}

#[tokio::test]
async fn an_expression_mapping_with_a_non_string_value_or_no_mapping_fails() -> Result<()> {
    for expected in [
        "fromJSON('{\"A\":[1]}')",
        "fromJSON('{\"A\":{}}')",
        "'text'",
    ] {
        let message = job(vec![with(
            run("a", "echo never"),
            "Environment",
            expression(expected),
        )]);
        let (finished, lines) = finish(&message).await?;
        assert_eq!(finished.result, JobResult::Failed, "{expected}: {lines:?}");
        assert!(!has_line(&lines, "never"), "{expected}: {lines:?}");
        assert!(
            errors(&finished)
                .iter()
                .any(|error| error.starts_with("Unexpected template token")),
            "{expected}: {:?}",
            finished.annotations
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_scalar_whose_type_is_not_an_integer_fails_instead_of_reading_as_a_literal() -> Result<()>
{
    let token = json!({"type": "0", "lit": "x"});
    let message = job(vec![with(
        run("a", "echo never"),
        "Environment",
        mapping(&[("A", token)]),
    )]);
    let (finished, lines) = finish(&message).await?;
    assert_eq!(finished.result, JobResult::Failed, "{lines:?}");
    assert!(!has_line(&lines, "never"), "{lines:?}");
    assert!(
        errors(&finished)
            .iter()
            .any(|error| error.starts_with("Unexpected template token")),
        "{:?}",
        finished.annotations
    );
    Ok(())
}
