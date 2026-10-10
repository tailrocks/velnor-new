//! Canonical-first workflow size fallback and fail-closed behavior.
use velnor_actions_contract_workflow::Step;
use velnor_actions_workflow_steps::{RenderError, checkout_step, plan_step, shell_step};
use velnor_actions_workflow_tree::yaml::AnchorName;
use velnor_actions_workflow_tree::{MAX_WORKFLOW_BYTES, check_first_line, with_marker};

use super::impl_renderer_fixtures::*;

fn oversized_command() -> String {
    let filler = "0".repeat(300_000);
    format!("true; printf '%s' '{filler}'")
}

fn repeated_shell_step(name: &str, command: &str) -> Result<Step, RenderError> {
    shell_step(
        name,
        vec!["sh".to_owned(), "-c".to_owned(), command.to_owned()],
        std::collections::BTreeMap::new(),
    )
}

fn repeated_jobs(
    command: &str,
) -> Result<Vec<(String, velnor_actions_contract_workflow::Job)>, RenderError> {
    Ok(vec![
        job(
            "plan",
            "Plan",
            Vec::new(),
            vec![
                checkout_step(&checkout_pin())?,
                acquire_fixture()?,
                plan_step(),
            ],
        ),
        job(
            "shared-one",
            "Shared one",
            vec!["plan".to_owned()],
            vec![repeated_shell_step("Shared command", command)?],
        ),
        job(
            "shared-two",
            "Shared two",
            vec!["plan".to_owned()],
            vec![repeated_shell_step("Shared command", command)?],
        ),
    ])
}

#[test]
fn oversized_canonical_render_falls_back_to_exact_command_aliases() -> Result<(), RenderError> {
    let command = oversized_command();
    let ir = fixture_ir(repeated_jobs(&command)?);
    let text = strict(&ir, &fixture_ctx())?;
    assert!(text.len() < MAX_WORKFLOW_BYTES);
    assert_eq!(text.matches("run: &velnor_run_1").count(), 1);
    assert_eq!(text.matches("run: *velnor_run_1").count(), 1);
    Ok(())
}

#[test]
fn command_aliasing_is_deterministic_and_semantically_exact() {
    let command = "printf '%s\\n' one\nprintf '%s\\n' two";
    let shared = velnor_actions_workflow_tree::Yaml::Map(vec![
        (
            "run".to_owned(),
            velnor_actions_workflow_tree::Yaml::str(command),
        ),
        (
            "run".to_owned(),
            velnor_actions_workflow_tree::Yaml::str(command),
        ),
    ]);
    let shared = velnor_actions_workflow_tree::yaml::share_repeated_run_scalars(shared);
    let velnor_actions_workflow_tree::Yaml::Map(entries) = &shared else {
        panic!("shared run fields remain a mapping");
    };
    let Some((_, velnor_actions_workflow_tree::Yaml::AnchoredScalar { name, value })) =
        entries.first()
    else {
        panic!("first repeated run owns a scalar anchor");
    };
    assert_eq!(AnchorName::new("velnor_run_1").as_ref(), Some(name));
    assert_eq!(value, command);
    let Some((_, second)) = entries.get(1) else {
        panic!("second repeated run remains present");
    };
    assert_eq!(
        second,
        &velnor_actions_workflow_tree::Yaml::Alias(name.clone())
    );
    let rendered = velnor_actions_workflow_tree::render_yaml(&shared);
    assert!(rendered.contains("run: &velnor_run_1 "));
    assert!(rendered.contains("run: *velnor_run_1\n"));
    assert_eq!(
        velnor_actions_workflow_tree::yaml::share_repeated_run_scalars(shared.clone()),
        shared
    );
}

#[test]
fn oversized_output_after_aliasing_fails_closed() -> Result<(), RenderError> {
    let unique = "9".repeat(MAX_WORKFLOW_BYTES + 64);
    let command = format!("true; printf '%s' '{unique}'");
    let mut jobs = repeated_jobs(&command)?;
    jobs.truncate(2);
    let ir = fixture_ir(jobs);
    let error = strict(&ir, &fixture_ctx()).expect_err("still-oversized output must fail");
    assert!(
        error
            .to_string()
            .contains("workflow_too_large:.github/workflows/ci.yml:")
            && error.to_string().ends_with(":500000"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn hand_edited_workflow_never_bypasses_marker_validation() -> Result<(), RenderError> {
    let generated = with_marker(VERSION, "run: kept\n")?;
    let hand_edited = format!("run: hand-edited\n{generated}");
    assert!(check_first_line(&generated, VERSION).is_ok());
    assert!(check_first_line(&hand_edited, VERSION).is_err());
    Ok(())
}
