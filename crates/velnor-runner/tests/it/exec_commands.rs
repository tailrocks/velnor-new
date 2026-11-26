// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use serde_json::json;
use velnor_runner::exec::{AdapterRegistry, AnnotationLevel, JobCancel, JobResult};

use crate::exec_builder::{execute, job, run, run_message};
use crate::exec_support::Scratch;

#[test]
fn every_encoded_form_of_a_secret_is_masked() -> Result<()> {
    let scratch = Scratch::new()?;
    let mut message = job(vec![run(
        "a",
        r#"cat <<'FORMS'
raw:ab"cd'e<f>&g h/i
json:ab\"cd'e<f>&g h/i
uri:ab%22cd%27e%3Cf%3E%26g%20h%2Fi
xml:ab&quot;cd&apos;e&lt;f&gt;&amp;g h/i
expr:ab"cd''e<f>&g h/i
FORMS"#,
    )]);
    message["Variables"] = json!({"S": {"Value": "ab\"cd'e<f>&g h/i", "IsSecret": true}});
    let ran = run_message(&scratch, &message)?;
    for label in ["raw", "json", "uri", "xml", "expr"] {
        assert!(ran.has_line(&format!("{label}:***")), "{}", ran.stdout);
    }
    Ok(())
}

#[test]
fn a_legacy_add_mask_command_registers_the_secret() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![run(
        "a",
        "echo '##[add-mask]legacy-sec-1'\necho 'legacy-sec-1 printed'",
    )]);
    let ran = run_message(&scratch, &message)?;
    assert!(ran.has_line("*** printed"), "{}", ran.stdout);
    assert!(!ran.stdout.contains("legacy-sec-1"), "{}", ran.stdout);
    Ok(())
}

#[tokio::test]
async fn stop_commands_suspend_workflow_commands_until_the_token() -> Result<()> {
    let script = [
        "echo '::stop-commands::resume-token-77'",
        "echo '::warning::inside-stop'",
        "echo '::add-mask::inside-secret-9'",
        "echo 'inside-secret-9 visible'",
        "echo '::resume-token-77::'",
        "echo '::warning::after-resume'",
        "echo '::add-mask::after-secret-9'",
        "echo 'after-secret-9 hidden'",
    ]
    .join("\n");
    let (run, lines) = execute(
        &job(vec![run("a", &script)]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    let warnings: Vec<&str> = run
        .annotations
        .iter()
        .filter(|annotation| annotation.level == AnnotationLevel::Warning)
        .map(|annotation| annotation.message.as_str())
        .collect();
    assert_eq!(warnings, ["after-resume"]);
    assert!(lines.iter().any(|line| line == "inside-secret-9 visible"));
    assert!(lines.iter().any(|line| line == "*** hidden"), "{lines:?}");
    Ok(())
}

#[tokio::test]
async fn a_stop_token_that_names_a_command_fails_the_step() -> Result<()> {
    let (run, _) = execute(
        &job(vec![run(
            "a",
            "echo '::stop-commands::add-mask'\necho '::warning::still-live'",
        )]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(run.result, JobResult::Failed);
    let messages: Vec<&str> = run
        .annotations
        .iter()
        .map(|annotation| annotation.message.as_str())
        .collect();
    assert!(
        messages.contains(&"Unable to process command '::stop-commands::add-mask' successfully."),
        "{messages:?}"
    );
    assert!(messages.contains(&"still-live"), "{messages:?}");
    Ok(())
}

#[tokio::test]
async fn annotations_are_bounded_per_step_and_per_message() -> Result<()> {
    let first = "for i in $(seq 1 25); do echo \"::error::e$i\"; done\nfor i in $(seq 1 12); do echo \"::warning::w$i\"; done\nprintf '::notice::%s\\n' \"$(head -c 5000 /dev/zero | tr '\\0' n)\"";
    let second = "for i in 1 2 3; do echo \"::error::f$i\"; done";
    let (run, _) = execute(
        &job(vec![run("a", first), run("b", second)]),
        &Scratch::new()?,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    let of = |level| -> Vec<&str> {
        run.annotations
            .iter()
            .filter(|annotation| annotation.level == level)
            .map(|annotation| annotation.message.as_str())
            .collect()
    };
    let errors = of(AnnotationLevel::Error);
    assert_eq!(errors.len(), 13, "{errors:?}");
    assert_eq!(errors[0], "e1");
    assert_eq!(errors[9], "e10");
    assert_eq!(errors[10], "f1");
    assert_eq!(of(AnnotationLevel::Warning).len(), 10);
    let notices = of(AnnotationLevel::Notice);
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].chars().count(), 4096);
    Ok(())
}
