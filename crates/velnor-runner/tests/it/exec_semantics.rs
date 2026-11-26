// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;

use crate::exec_builder::{expression, job, literal, mapping, run, run_message, step, with};
use crate::exec_support::Scratch;

#[test]
fn file_commands_apply_what_they_can_and_fail_the_step() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![
        run(
            "s",
            "printf 'A=1\\nnot-a-pair\\nB=2\\n' >> \"$GITHUB_ENV\"\necho 'out=kept' >> \"$GITHUB_OUTPUT\"\necho '/opt/kept' >> \"$GITHUB_PATH\"",
        ),
        with(
            step(
                "t",
                expression(
                    "format('echo \"after A={0} B={1} out={2}\"', env.A, env.B, steps.s.outputs.out)",
                ),
            ),
            "Condition",
            json!("always()"),
        ),
    ]);
    let ran = run_message(&scratch, &message)?;
    assert!(
        ran.has_line("Unable to process file command 'env' successfully."),
        "{}",
        ran.stdout
    );
    assert!(
        ran.has_line("Invalid format 'not-a-pair'"),
        "{}",
        ran.stdout
    );
    assert!(ran.has_line("step 1 failed"), "{}", ran.stdout);
    assert!(ran.has_line("after A=1 B= out=kept"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn heredoc_values_flow_through_output_and_env_files() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![
        run(
            "s",
            "printf 'multi<<EOF\\nline1\\n\\nline3\\nEOF\\n' >> \"$GITHUB_OUTPUT\"\nprintf 'BLOB<<DELIM\\na=b\\nc\\nDELIM\\nTAIL=x\\n' >> \"$GITHUB_ENV\"",
        ),
        run("t", "printf 'blob=[%s] tail=%s\\n' \"$BLOB\" \"$TAIL\""),
        with(
            run("u", "printf 'multi=[%s]\\n' \"$MULTI\""),
            "Environment",
            mapping(&[("MULTI", expression("steps.s.outputs.multi"))]),
        ),
    ]);
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("blob=[a=b"), "{}", ran.stdout);
    assert!(ran.has_line("c] tail=x"), "{}", ran.stdout);
    assert!(ran.has_line("multi=[line1"), "{}", ran.stdout);
    assert!(ran.has_line("line3]"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(0));
    Ok(())
}

#[test]
fn job_outputs_drop_empty_values_and_warn_about_secrets() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut message = job(vec![run("s", "true")]);
    message["Variables"] = json!({"MY_SECRET": {"Value": "s3cr3t-value", "IsSecret": true}});
    message["JobOutputs"] = mapping(&[
        ("leak", expression("format('pre-{0}', secrets.MY_SECRET)")),
        ("fine", literal("ok")),
        ("empty", literal("")),
    ]);
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("output fine=ok"), "{}", ran.stdout);
    assert!(
        ran.has_line("Skip output 'leak' since it may contain secret."),
        "{}",
        ran.stdout
    );
    assert!(
        !ran.stdout
            .lines()
            .any(|line| line.starts_with("output leak") || line.starts_with("output empty")),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[test]
fn preflight_names_job_container_and_services() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut message = job(vec![run("s", "echo ran")]);
    message["JobContainer"] = json!({"type": 0, "lit": "ubuntu:24.04"});
    message["JobServiceContainers"] =
        json!({"type": 2, "map": [{"key": literal("db"), "value": literal("postgres")}]});
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("unsupported: job container"), "{}", ran.stdout);
    assert!(ran.has_line("unsupported: job services"), "{}", ran.stdout);
    assert!(ran.has_line("job failed"), "{}", ran.stdout);
    assert!(!ran.stdout.lines().any(|line| line.starts_with("step ")));
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn default_shell_stops_at_the_first_failing_line() -> Result<()> {
    let scratch = Scratch::new()?;
    let ran = run_message(&scratch, &job(vec![run("s", "false\necho not-reached")]))?;
    assert!(ran.has_line("step 1 failed"), "{}", ran.stdout);
    assert!(!ran.has_line("not-reached"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn base64_shifted_forms_of_a_secret_are_masked() -> Result<()> {
    const SECRET: &str = "s3cr3t-Value-12345";
    let scratch = Scratch::new()?;
    let mut message = job(vec![step(
        "s",
        expression(
            "format('for prefix in \"\" a ab; do printf \"%s%s\" \"$prefix\" \"{0}\" | base64; done', secrets.MY_SECRET)",
        ),
    )]);
    message["Variables"] = json!({"MY_SECRET": {"Value": SECRET, "IsSecret": true}});
    let ran = run_message(&scratch, &message)?;
    for shift in 0..3 {
        let form = STANDARD.encode(&SECRET.as_bytes()[shift..]);
        assert!(!ran.stdout.contains(&form), "shift {shift}: {}", ran.stdout);
    }
    assert!(ran.stdout.contains("***"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn every_encoder_form_of_a_secret_is_masked() -> Result<()> {
    let secrets = [
        ("BACKSLASH", "a\\b\"c-Value-9z"),
        ("QUOTED", "\"quoted-secret-9\""),
        ("AMPERSAND", "part1&+part2&part3"),
    ];
    let forms = [
        "a\\\\b\\\"c-Value-9z",
        "a\\b\\\"c-Value-9z",
        "quoted-secret-9",
        "part1&+",
        "art2&part3",
    ];
    let script = forms
        .iter()
        .map(|form| format!("printf '%s\\n' '{form}'"))
        .collect::<Vec<_>>()
        .join("\n");
    let scratch = Scratch::new()?;
    let mut message = job(vec![run("s", &script)]);
    let variables: serde_json::Map<String, serde_json::Value> = secrets
        .iter()
        .map(|(name, value)| {
            (
                (*name).to_owned(),
                json!({"Value": value, "IsSecret": true}),
            )
        })
        .collect();
    message["Variables"] = serde_json::Value::Object(variables);
    let ran = run_message(&scratch, &message)?;
    for form in forms {
        assert!(!ran.stdout.contains(form), "{form} leaked: {}", ran.stdout);
    }
    assert_eq!(
        ran.stdout.lines().filter(|line| *line == "***").count(),
        forms.len(),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[test]
fn regex_mask_hints_apply_as_regexes_and_other_hints_as_values() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut message = job(vec![run(
        "s",
        "echo tok_abc123 and tok_zzz999\necho plain-value-1\necho tok_short",
    )]);
    message["Mask"] = json!([
        {"Type": "regex", "Value": "tok_[a-z0-9]{6}"},
        {"Type": "variable", "Value": "plain-value-1"}
    ]);
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("*** and ***"), "{}", ran.stdout);
    assert!(ran.has_line("***"), "{}", ran.stdout);
    assert!(ran.has_line("tok_short"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn github_context_data_wins_over_system_variables() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut message = job(vec![step(
        "s",
        expression("format('echo \"job={0} token={1}\"', github.job, github.token)"),
    )]);
    message["Variables"] = json!({
        "system.github.token": {"Value": "variable-token", "IsSecret": true},
        "system.github.job": {"Value": "variable-job", "IsSecret": false}
    });
    message["ContextData"] = json!({
        "github": {"t": 2, "d": [
            {"k": "job", "v": {"t": 0, "s": "context-job"}},
            {"k": "token", "v": {"t": 0, "s": "context-token"}}
        ]}
    });
    let ran = run_message(&scratch, &message)?;
    assert!(
        ran.has_line("job=context-job token=context-token"),
        "{}",
        ran.stdout
    );
    Ok(())
}
