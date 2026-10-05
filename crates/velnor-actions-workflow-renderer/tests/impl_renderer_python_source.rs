use velnor_actions_contract::{GeneratorValidation, ValidatorKind, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    RenderError, ValidatorSourceInput, ValidatorSourceInputKind, render_workflow_ir,
};

use super::impl_renderer_tree_policy::{fixture_ctx, fixture_ir, validator_commands};

#[test]
fn python_source_suites_are_required_and_run_after_install_with_scrubbing()
-> Result<(), RenderError> {
    let mut context = fixture_ctx();
    context.validator_commands = validator_commands();
    context
        .validator_commands
        .iter_mut()
        .find(|command| command.validator == ValidatorKind::PythonSourceTests)
        .expect("Python source command")
        .source_units[0]
        .inputs
        .push(ValidatorSourceInput {
            path: "crates/fixture/source.rs".to_owned(),
            kind: ValidatorSourceInputKind::Fixture,
        });
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let text = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &context,
    )?;
    let start = text
        .find("  python-source-tests:")
        .expect("python source job");
    let end = text[start..]
        .find("  plan:")
        .map_or(text.len(), |offset| start + offset);
    let job = &text[start..end];
    let install = job
        .find("name: Prepare Python source tests")
        .expect("install step");
    let run = job
        .find("name: Run Python source tests")
        .expect("source test step");
    assert!(install < run, "install precedes test execution:\n{job}");
    assert!(job.contains("python@3.14.8"), "pinned Python:\n{job}");
    assert!(
        job.contains("test -f crates/fixture/source.rs"),
        "external fixture is source-bound:\n{job}"
    );
    assert!(
        job.contains("test -f .mise-version")
            && job.contains("test -f crates/velnor-actions-mise/src/catalog.rs"),
        "toolchain selection is source-bound:\n{job}"
    );
    assert!(
        job.contains("GITHUB_TOKEN: \"\""),
        "test step scrubs credentials:\n{job}"
    );
    let required = text
        .split_once("  required:")
        .expect("required gate")
        .1
        .split("  publish-baseline:")
        .next()
        .expect("required job section");
    assert!(
        required.contains("- python-source-tests"),
        "required waits for Python suite:\n{required}"
    );
    Ok(())
}

#[test]
fn python_source_validator_without_typed_closure_is_rejected() {
    let mut context = fixture_ctx();
    context.validator_commands = validator_commands();
    context
        .validator_commands
        .iter_mut()
        .find(|command| command.validator == ValidatorKind::PythonSourceTests)
        .expect("Python source command")
        .source_units
        .clear();
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let result = render_workflow_ir(
        &fixture_ir().expect("fixture IR"),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &context,
    );
    assert!(matches!(
        result,
        Err(RenderError::BadCommand(problem)) if problem == "python_source_closure_empty"
    ));
}
