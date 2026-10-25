use std::collections::BTreeMap;

use velnor_actions_contract::{
    Concurrency, ExecutionMode, Job, JobTimeout, Permissions, ScaleSetSelector, Step, StepKind,
    Trigger, WorkflowIr,
};

use super::{
    WorkflowRoutingOverrides, WorkflowRoutingPolicy, validate_generated_workflow_calls,
    validate_generated_workflow_calls_with_ir_and_overrides,
    validate_generated_workflow_calls_with_overrides,
};
use crate::RenderedFile;

const PRODUCT_PATH: &str = ".github/workflows/product-release-images.yml";

fn scale_set_policy() -> WorkflowRoutingPolicy {
    let selector = ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("valid selector");
    WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector, None)
}

fn workflow(path: &str, jobs: &str) -> RenderedFile {
    RenderedFile {
        path: path.to_owned(),
        bytes: format!("on: push\njobs:\n{jobs}"),
    }
}

fn ci_ir() -> WorkflowIr {
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
        jobs: BTreeMap::from([(
            "check".to_owned(),
            Job {
                display_name: "Check".to_owned(),
                runs_on: "ubuntu-26.04".to_owned(),
                check_runner: None,
                condition: None,
                timeout_minutes: JobTimeout::VALIDATOR,
                needs: Vec::new(),
                permissions: None,
                cache_mode: None,
                environment: None,
                services: Vec::new(),
                steps: vec![Step {
                    name: "Run check".to_owned(),
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
        )]),
    }
}

#[test]
fn explicit_hosted_override_accepts_bytes_without_changing_legacy_policy() {
    let file = workflow(
        PRODUCT_PATH,
        "  publish:\n    runs-on: ubuntu-26.04\n    steps: []\n",
    );
    let original_bytes = file.bytes.clone();
    let policy = scale_set_policy();
    assert!(validate_generated_workflow_calls(std::slice::from_ref(&file), &policy).is_err());

    let mut overrides = WorkflowRoutingOverrides::default();
    overrides.mark_hosted_only(PRODUCT_PATH);
    assert_eq!(
        validate_generated_workflow_calls_with_overrides(
            std::slice::from_ref(&file),
            &policy,
            &overrides,
        ),
        Ok(())
    );
    assert_eq!(file.bytes, original_bytes);
}

#[test]
fn hosted_override_does_not_change_policy_for_other_files() {
    let marked = workflow(
        PRODUCT_PATH,
        "  publish:\n    runs-on: ubuntu-26.04\n    steps: []\n",
    );
    let unmarked_path = ".github/workflows/other.yml";
    let unmarked = workflow(
        unmarked_path,
        "  check:\n    runs-on: ubuntu-26.04\n    steps: []\n",
    );
    let mut overrides = WorkflowRoutingOverrides::default();
    overrides.mark_hosted_only(PRODUCT_PATH);

    let error = validate_generated_workflow_calls_with_overrides(
        &[marked, unmarked],
        &scale_set_policy(),
        &overrides,
    )
    .expect_err("a path classification must not change unrelated workflow policy");
    assert!(error.to_string().contains(unmarked_path), "{error}");
}

#[test]
fn hosted_override_still_rejects_a_scale_set_leaf() {
    let file = workflow(
        PRODUCT_PATH,
        "  publish:\n    runs-on: [velnor, linux-amd64]\n    steps: []\n",
    );
    let mut overrides = WorkflowRoutingOverrides::default();
    overrides.mark_hosted_only(PRODUCT_PATH);

    assert!(
        validate_generated_workflow_calls_with_overrides(&[file], &scale_set_policy(), &overrides,)
            .is_err()
    );
}

#[test]
fn missing_workflow_override_fails() {
    let path = ".github/workflows/not-emitted.yml";
    let mut overrides = WorkflowRoutingOverrides::default();
    overrides.mark_hosted_only(path);
    let error =
        validate_generated_workflow_calls_with_overrides(&[], &scale_set_policy(), &overrides)
            .expect_err("unmatched classifications fail closed");
    assert!(
        error.to_string().contains(&format!(
            "workflow_routing_override_workflow_missing:{path}"
        )),
        "{error}"
    );
}

#[test]
fn nonworkflow_override_fails_even_when_the_path_is_emitted() {
    let path = ".github/actions/not-a-workflow/action.yml";
    let file = RenderedFile {
        path: path.to_owned(),
        bytes: "name: action\nruns: {using: composite, steps: []}\n".to_owned(),
    };
    let mut overrides = WorkflowRoutingOverrides::default();
    overrides.mark_hosted_only(path);
    let error =
        validate_generated_workflow_calls_with_overrides(&[file], &scale_set_policy(), &overrides)
            .expect_err("only generated workflow files can receive a placement class");
    assert!(
        error.to_string().contains(&format!(
            "workflow_routing_override_workflow_missing:{path}"
        )),
        "{error}"
    );
}

#[test]
fn override_cannot_reclassify_the_typed_ci_workflow() {
    let file = workflow(
        ".github/workflows/ci.yml",
        "  build:\n    runs-on: ubuntu-26.04\n    steps: []\n",
    );
    let mut overrides = WorkflowRoutingOverrides::default();
    overrides.mark_hosted_only(file.path.clone());
    let error = validate_generated_workflow_calls_with_ir_and_overrides(
        &[file],
        &scale_set_policy(),
        &ci_ir(),
        &overrides,
    )
    .expect_err("producer placement cannot bypass the typed CI source binding");
    assert!(
        error
            .to_string()
            .contains("workflow_routing_override_source_bound_workflow:.github/workflows/ci.yml"),
        "{error}"
    );
}

#[test]
fn hosted_override_does_not_skip_reusable_call_inventory() {
    let file = workflow(
        PRODUCT_PATH,
        "  nested:\n    uses: ./.github/workflows/missing.yml\n",
    );
    let mut overrides = WorkflowRoutingOverrides::default();
    overrides.mark_hosted_only(PRODUCT_PATH);
    let error =
        validate_generated_workflow_calls_with_overrides(&[file], &scale_set_policy(), &overrides)
            .expect_err("a marked file still participates in the cross-file call inventory");
    assert!(
        error
            .to_string()
            .contains("unresolved_local_reusable_workflow"),
        "{error}"
    );
}
