use std::collections::BTreeSet;

use super::{context, lane_share, source_job, workflow};
use crate::render::test_render_capture::{
    with_full_tree_capture, with_source_job_workflow_capture,
};
use crate::{
    lane_share::job_workflow_extract::{
        PROTOTYPE_JOB_ID, validate_finalized_caller, validate_finalized_runner,
        validate_rendered_child_runner,
    },
    workflow_routing::{WorkflowRoutingPolicy, route_workflow_yaml_for_jobs},
    yaml::Yaml,
};

#[path = "capture_runner_routed_container.rs"]
mod routed_container;

#[test]
fn existing_prototype_caller_is_accepted_only_as_its_exact_source_snapshot() {
    let job = source_job();
    let workflow = workflow(job.clone());
    let shared = lane_share(job);
    let context = context();
    let parent = super::workflow_to_yaml(&workflow, &shared, &context, &BTreeSet::new())
        .expect("typed source lowers to its existing reusable caller");
    let parent = route_workflow_yaml_for_jobs(
        ".github/workflows/ci.yml",
        parent,
        &WorkflowRoutingPolicy::hosted_only(),
        &shared.jobs,
    )
    .expect("the prototype remains hosted");
    let relocations =
        super::render_job_workflow_relocations(&workflow, &shared, &context, &BTreeSet::new())
            .expect("the expected caller and child derive from the typed source");
    let relocation = relocations.first().expect("one prototype relocation");

    let runner = validate_finalized_runner(
        PROTOTYPE_JOB_ID,
        &relocation.prepared.source_job,
        &parent,
        &relocation.expected_caller,
    )
    .expect("the existing caller exactly matches its source-derived snapshot");
    validate_rendered_child_runner(
        &relocation.expected_callee_file,
        PROTOTYPE_JOB_ID,
        &runner.final_runner_labels,
    )
    .expect("the child preserves the finalized hosted runner");

    let mut unexpected_parent = parent;
    replace_job_field(
        &mut unexpected_parent,
        PROTOTYPE_JOB_ID,
        "uses",
        Yaml::str("./.github/workflows/unexpected.yml"),
    );
    assert!(
        validate_finalized_runner(
            PROTOTYPE_JOB_ID,
            &relocation.prepared.source_job,
            &unexpected_parent,
            &relocation.expected_caller,
        )
        .is_err()
    );
}

#[test]
fn selected_source_caller_is_accepted_only_as_its_exact_source_snapshot() {
    let source_id = "rust-velnor-actions-actionlint__hosted";
    let source = super::source_job();
    let allowlist = BTreeSet::from([
        source_id.to_owned(),
        "rust-velnor-actions-actionlint__local".to_owned(),
    ]);
    let prepared = crate::lane_share::job_workflow_extract::prepare_job_for_capture(
        source_id, &source, &allowlist,
    )
    .expect("selected source lowers from its exact owner ID");
    let expected_caller = super::super::caller_yaml(&prepared)
        .expect("the expected caller derives from the source snapshot");
    let parent = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(source_id.to_owned(), expected_caller.clone())]),
    )]);

    validate_finalized_caller(source_id, &source, &parent, &expected_caller)
        .expect("a selected source's exact reusable caller is accepted");

    let mut unexpected_parent = parent;
    replace_job_field(
        &mut unexpected_parent,
        source_id,
        "uses",
        Yaml::str("./.github/workflows/unexpected.yml"),
    );
    assert!(
        validate_finalized_caller(source_id, &source, &unexpected_parent, &expected_caller)
            .is_err()
    );
}

#[test]
fn malformed_capture_scope_does_not_change_prototype_render_result() {
    let job = super::source_job();
    let workflow = super::workflow(job.clone());
    let shared = super::lane_share(job);
    let context = super::context();
    let expected = super::workflow_to_yaml(&workflow, &shared, &context, &BTreeSet::new())
        .expect("the existing prototype renders without capture state");

    let actual = with_full_tree_capture(|| {
        with_source_job_workflow_capture(BTreeSet::new(), || {
            super::workflow_to_yaml(&workflow, &shared, &context, &BTreeSet::new())
        })
    })
    .expect("invalid diagnostic owner IDs must not alter production rendering");

    assert_eq!(actual, expected);
}

fn replace_job_field(parent: &mut Yaml, job_id: &str, name: &str, value: Yaml) {
    let Yaml::Map(fields) = parent else {
        panic!("workflow is a map");
    };
    let Some((_, Yaml::Map(jobs))) = fields.iter_mut().find(|(key, _)| key == "jobs") else {
        panic!("workflow jobs are a map");
    };
    let Some((_, Yaml::Map(job_fields))) = jobs.iter_mut().find(|(key, _)| key == job_id) else {
        panic!("the caller is a map");
    };
    let Some((_, field)) = job_fields.iter_mut().find(|(key, _)| key == name) else {
        panic!("caller field exists");
    };
    *field = value;
}
