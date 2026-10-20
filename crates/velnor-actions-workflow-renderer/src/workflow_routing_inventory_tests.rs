use std::collections::BTreeMap;

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
    ValidatedImageReference,
};
use velnor_actions_contract::{
    Concurrency, ExecutionMode, Job, JobTimeout, Permissions, ScaleSetSelector, Step, StepKind,
    Trigger, WorkflowIr,
};

use super::super::{
    WorkflowRoutingPolicy, validate_generated_workflow_calls,
    validate_generated_workflow_calls_with_ir,
};
use crate::{RenderError, RenderedFile};

fn selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("valid selector")
}

fn policy(mode: ExecutionMode) -> WorkflowRoutingPolicy {
    crate::workflow_routing::test_policy(mode, selector())
}

fn configured(mode: ExecutionMode) -> WorkflowRoutingPolicy {
    let image = ValidatedImageReference::parse(
        "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect("valid immutable test image");
    WorkflowRoutingPolicy::configured(mode, selector(), Some(image))
}

fn configured_without_image(mode: ExecutionMode) -> WorkflowRoutingPolicy {
    WorkflowRoutingPolicy::configured(mode, selector(), None)
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
    assert_eq!(
        validate_generated_workflow_calls(
            std::slice::from_ref(&hosted),
            &WorkflowRoutingPolicy::hosted_only(),
        ),
        Ok(())
    );
    assert_eq!(
        validate_generated_workflow_calls(&[hosted], &configured(ExecutionMode::Both)),
        Ok(())
    );
    let scale_set = workflow(
        "ci",
        "on: [push]\njobs: {check: {runs-on: [velnor, linux-amd64], container: {image: ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}, steps: []}}\n",
    );
    assert_eq!(
        validate_generated_workflow_calls(&[scale_set], &policy(ExecutionMode::ScaleSet)),
        Ok(())
    );
}

#[test]
fn hosted_only_rejects_scale_set_jobs() {
    let file = workflow(
        "hosted-only",
        "on: push\njobs:\n  check:\n    runs-on: [velnor, linux-amd64]\n",
    );
    assert!(
        validate_generated_workflow_calls(&[file], &WorkflowRoutingPolicy::hosted_only()).is_err()
    );
}

#[test]
fn block_scalar_text_does_not_create_workflow_jobs() {
    let file = workflow(
        "ci",
        "on: push\njobs:\n  check:\n    runs-on: [velnor, linux-amd64]\n    container: {image: ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}\n    steps:\n      - run: |\n          jobs:\n            un-routed:\n              runs-on: ubuntu-24.04\n",
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
        "!!map\n!!str on: !!map {workflow_call: !!map {}}\n!!str jobs: !!map\n  check: !!map\n    runs-on: [velnor, linux-amd64]\n    container: {image: ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}\n",
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
fn hosted_only_allows_external_reusable_workflows() {
    let file = workflow(
        "external-call",
        "on: push\njobs:\n  check:\n    uses: owner/project/.github/workflows/check.yml@v1\n",
    );
    assert_eq!(
        validate_generated_workflow_calls(
            std::slice::from_ref(&file),
            &WorkflowRoutingPolicy::hosted_only(),
        ),
        Ok(())
    );
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
        "on: {workflow_call: {}}\njobs:\n  check:\n    runs-on: [velnor, linux-amd64]\n    container: {image: ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}\n",
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

#[test]
fn configured_modes_require_an_immutable_container_on_every_selector_leaf() {
    let file = workflow(
        "selector-leaf",
        "on: push\njobs:\n  check:\n    if: false\n    runs-on: [velnor, linux-amd64]\n",
    );
    for mode in [
        ExecutionMode::Hosted,
        ExecutionMode::ScaleSet,
        ExecutionMode::Both,
    ] {
        let error =
            validate_generated_workflow_calls(std::slice::from_ref(&file), &configured(mode))
                .expect_err("false conditions do not bypass container checks");
        assert_eq!(
            error,
            RenderError::InvalidWorkflow(
                "job_container_missing:.github/workflows/selector-leaf.yml:check".to_owned()
            )
        );
    }
}

#[test]
fn selector_leaf_requires_default_image_even_when_it_has_an_explicit_container() {
    let file = workflow(
        "explicit-container",
        "on: push\njobs:\n  check:\n    runs-on: [velnor, linux-amd64]\n    container:\n      image: ghcr.io/team/job@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
    );
    let error = validate_generated_workflow_calls(
        &[file],
        &configured_without_image(ExecutionMode::Hosted),
    )
    .expect_err("the configured default image remains mandatory");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "job_container_image_missing:.github/workflows/explicit-container.yml:check".to_owned()
        )
    );
}

#[test]
fn hosted_container_image_is_preserved_without_scale_set_validation() {
    let file = workflow(
        "hosted-container",
        "on: push\njobs:\n  check:\n    runs-on: ubuntu-26.04\n    container: ubuntu:24.04\n",
    );
    assert_eq!(
        validate_generated_workflow_calls(&[file], &policy(ExecutionMode::Hosted)),
        Ok(())
    );
}

#[test]
fn typed_native_check_needs_the_matching_ir_and_rendered_fields() {
    let selector = selector();
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::Hosted, selector, None);
    let workflow_ir = native_check_ir();
    let file = workflow(
        "ci",
        &format!(
            "on: push\njobs:\n  check-native:\n    runs-on: native-scale-set\n    if: \"{EPHEMERAL_CHECK_ADMISSION_CONDITION}\"\n    steps: []\n"
        ),
    );
    assert!(validate_generated_workflow_calls(std::slice::from_ref(&file), &policy).is_err());
    assert_eq!(
        validate_generated_workflow_calls_with_ir(
            std::slice::from_ref(&file),
            &policy,
            &workflow_ir,
        ),
        Ok(())
    );

    let altered = workflow(
        "ci",
        &format!(
            "on: push\njobs:\n  check-native:\n    runs-on: another-runner\n    if: \"{EPHEMERAL_CHECK_ADMISSION_CONDITION}\"\n    steps: []\n"
        ),
    );
    assert!(validate_generated_workflow_calls_with_ir(&[altered], &policy, &workflow_ir).is_err());
}

fn native_check_ir() -> WorkflowIr {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "check-native".to_owned(),
        Job {
            services: Vec::new(),
            display_name: "Check / native".to_owned(),
            runs_on: "native-scale-set".to_owned(),
            check_runner: Some(CheckRunner {
                label: "native-scale-set".to_owned(),
                platform: CheckPlatform::MacosArm64,
                executor: CheckExecutor::EphemeralSelfHosted,
                container: None,
            }),
            timeout_minutes: JobTimeout::VALIDATOR,
            needs: Vec::new(),
            condition: Some(EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned()),
            permissions: None,
            cache_mode: None,
            environment: None,
            steps: vec![Step {
                name: "Run native check".to_owned(),
                id: None,
                role: None,
                condition: None,
                parallel: false,
                kind: StepKind::Shell {
                    run: vec!["true".to_owned()],
                    env: BTreeMap::new(),
                },
            }],
        },
    );
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "ci".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        jobs,
    }
}

#[test]
fn configured_hosted_and_both_modes_accept_hosted_matrix_rows() {
    let file = workflow(
        "hosted-matrix",
        "on: push\njobs:\n  check:\n    runs-on: '${{ matrix.runner }}'\n    strategy:\n      matrix:\n        runner: [ubuntu-24.04, ubuntu-26.04]\n    steps: []\n",
    );
    for mode in [ExecutionMode::Hosted, ExecutionMode::Both] {
        assert_eq!(
            validate_generated_workflow_calls(std::slice::from_ref(&file), &configured(mode)),
            Ok(())
        );
    }
}

#[test]
fn reusable_callers_cannot_set_container_in_any_mode() {
    let file = workflow(
        "caller-container",
        "on: push\njobs:\n  call:\n    uses: ./.github/workflows/reuse.yml\n    container: ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    );
    for mode in [
        ExecutionMode::Hosted,
        ExecutionMode::Both,
        ExecutionMode::ScaleSet,
    ] {
        let error = validate_generated_workflow_calls(std::slice::from_ref(&file), &policy(mode))
            .expect_err("reusable callers cannot have a container");
        assert_eq!(
            error,
            RenderError::InvalidWorkflow(
                "reusable_workflow_job_has_container:.github/workflows/caller-container.yml:call"
                    .to_owned()
            )
        );
    }
}

#[test]
fn configured_modes_reject_unresolved_external_calls_and_mutable_images() {
    let external = workflow(
        "external-call",
        "on: push\njobs:\n  call:\n    uses: owner/project/.github/workflows/check.yml@v1\n",
    );
    for mode in [ExecutionMode::Hosted, ExecutionMode::Both] {
        assert!(
            validate_generated_workflow_calls(std::slice::from_ref(&external), &configured(mode))
                .is_err()
        );
    }
    let mutable = workflow(
        "mutable-image",
        "on: push\njobs:\n  check:\n    runs-on: ubuntu-26.04\n    container: ubuntu:latest\n",
    );
    assert!(
        validate_generated_workflow_calls(&[mutable], &configured(ExecutionMode::ScaleSet))
            .is_err()
    );
}
