use velnor_actions_contract::{ExecutionMode, ScaleSetSelector};

use super::super::{WorkflowRoutingPolicy, validate_generated_workflow_calls};
use crate::{RenderError, RenderedFile};

fn selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("valid selector")
}

fn policy(mode: ExecutionMode) -> WorkflowRoutingPolicy {
    WorkflowRoutingPolicy::from_mode(mode, selector())
}

fn workflow(path: &str, bytes: &str) -> RenderedFile {
    RenderedFile {
        path: format!(".github/workflows/{path}.yml"),
        bytes: bytes.to_owned(),
    }
}

#[test]
fn final_inventory_parses_yaml_structure_and_preserves_hosted_and_both() {
    let hosted = workflow(
        "ci",
        "on: [push]\njobs: {check: {runs-on: 'ubuntu-26.04', steps: []}}\n",
    );
    for mode in [ExecutionMode::Hosted, ExecutionMode::Both] {
        assert_eq!(
            validate_generated_workflow_calls(std::slice::from_ref(&hosted), &policy(mode)),
            Ok(())
        );
    }
    let scale_set = workflow(
        "ci",
        "on: [push]\njobs: {check: {runs-on: [velnor, linux-amd64], steps: []}}\n",
    );
    assert_eq!(
        validate_generated_workflow_calls(&[scale_set], &policy(ExecutionMode::ScaleSet)),
        Ok(())
    );
}

#[test]
fn block_scalar_text_does_not_create_workflow_jobs() {
    let file = workflow(
        "ci",
        "on: push\njobs:\n  check:\n    runs-on: [velnor, linux-amd64]\n    steps:\n      - run: |\n          jobs:\n            un-routed:\n              runs-on: ubuntu-24.04\n",
    );
    assert_eq!(
        validate_generated_workflow_calls(&[file], &policy(ExecutionMode::ScaleSet)),
        Ok(())
    );
}

#[test]
fn final_inventory_reads_explicit_core_tags_as_yaml_values() {
    let file = workflow(
        "tagged-core",
        "!!map\n!!str on: !!map {workflow_call: !!map {}}\n!!str jobs: !!map\n  check: !!map\n    runs-on: [velnor, linux-amd64]\n",
    );
    assert_eq!(
        validate_generated_workflow_calls(&[file], &policy(ExecutionMode::ScaleSet)),
        Ok(())
    );
}

#[test]
fn every_final_workflow_job_is_checked_even_when_condition_is_false() {
    let file = workflow(
        "late-added",
        "on: push\njobs:\n  skipped:\n    if: false\n    runs-on: ubuntu-24.04\n",
    );
    let error = validate_generated_workflow_calls(&[file], &policy(ExecutionMode::ScaleSet))
        .expect_err("a newly added job must be inspected");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "workflow_job_not_routed_to_scale_set:.github/workflows/late-added.yml:skipped"
                .to_owned()
        )
    );
}

#[test]
fn final_inventory_rejects_unresolved_runner_expressions() {
    let file = workflow(
        "dynamic-runner",
        "on: push\njobs:\n  check:\n    runs-on: '${{ inputs.runner }}'\n",
    );
    let error = validate_generated_workflow_calls(&[file], &policy(ExecutionMode::ScaleSet))
        .expect_err("unbound inputs cannot choose a Scale Set runner");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "workflow_job_not_routed_to_scale_set:.github/workflows/dynamic-runner.yml:check"
                .to_owned()
        )
    );
}

#[test]
fn external_reusable_workflows_remain_compatible_outside_scale_set_mode() {
    let file = workflow(
        "external-call",
        "on: push\njobs:\n  check:\n    uses: owner/project/.github/workflows/check.yml@v1\n",
    );
    for mode in [ExecutionMode::Hosted, ExecutionMode::Both] {
        assert_eq!(
            validate_generated_workflow_calls(std::slice::from_ref(&file), &policy(mode)),
            Ok(())
        );
    }
    let error = validate_generated_workflow_calls(&[file], &policy(ExecutionMode::ScaleSet))
        .expect_err("external callee placement has no local source evidence");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "unsupported_external_reusable_workflow:.github/workflows/external-call.yml:check:owner/project/.github/workflows/check.yml@v1".to_owned()
        )
    );
}

#[test]
fn reusable_workflow_target_must_accept_calls_and_cycles_fail() {
    let calling_file = workflow(
        "caller",
        "on: push\njobs:\n  nested:\n    uses: ./.github/workflows/callee.yml\n",
    );
    let called_file = workflow(
        "callee",
        "on: {workflow_call: {}}\njobs:\n  check:\n    runs-on: [velnor, linux-amd64]\n",
    );
    assert_eq!(
        validate_generated_workflow_calls(
            &[calling_file.clone(), called_file.clone()],
            &policy(ExecutionMode::ScaleSet),
        ),
        Ok(())
    );
    let non_callable = workflow("callee", "on: push\njobs: {}\n");
    assert!(
        validate_generated_workflow_calls(
            &[calling_file.clone(), non_callable],
            &policy(ExecutionMode::ScaleSet)
        )
        .is_err()
    );
    let cycle = workflow(
        "callee",
        "on: {workflow_call: {}}\njobs:\n  back:\n    uses: ./.github/workflows/caller.yml\n",
    );
    assert!(
        validate_generated_workflow_calls(&[calling_file, cycle], &policy(ExecutionMode::ScaleSet))
            .is_err()
    );
}
