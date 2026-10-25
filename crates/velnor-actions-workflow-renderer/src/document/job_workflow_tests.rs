use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CacheMode, Concurrency, Job, JobTimeout, Permissions, Step, StepKind, Trigger, WorkflowIr,
};

use super::super::{render_job_workflow_relocations, workflow_to_yaml};
use crate::{
    RenderedFile,
    lane_share::LaneShare,
    lane_share::job_workflow_extract::{JobWorkflowRelocation, PROTOTYPE_JOB_ID, prepare_job},
    render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext},
    workflow_routing::{
        WorkflowRoutingPolicy, route_workflow_yaml_for_jobs,
        validate_generated_workflow_calls_with_ir,
    },
    yaml::Yaml,
};

#[path = "job_workflow_tests/call_schema.rs"]
mod call_schema;
#[path = "job_workflow_tests/caller_status_guard.rs"]
mod caller_status_guard;
#[cfg(feature = "test-render-capture")]
#[path = "job_workflow_tests/capture_runner.rs"]
mod capture_runner;
#[path = "job_workflow_tests/fixture.rs"]
mod fixture;
#[path = "job_workflow_tests/identity_condition.rs"]
mod identity_condition;
#[path = "job_workflow_tests/unbound_needs.rs"]
mod unbound_needs;

fn source_job() -> Job {
    Job {
        display_name: "Rust CLI hosted".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: vec!["plan".to_owned()],
        condition: Some("${{ needs.plan.result == 'success' }}".to_owned()),
        permissions: Some(Permissions::default()),
        cache_mode: Some(CacheMode::Read),
        environment: None,
        services: Vec::new(),
        steps: vec![Step {
            name: "Read source key".to_owned(),
            id: None,
            role: None,
            condition: Some("${{ needs.plan.result == 'success' }}".to_owned()),
            parallel: false,
            kind: StepKind::Shell {
                run: vec!["printf '%s' '${{ needs.plan.outputs.sources_key }}'".to_owned()],
                env: BTreeMap::from([(
                    "VELNOR_RUN_KEY_ATTEMPT".to_owned(),
                    "${{ needs.plan.outputs.plan_attempt || github.run_attempt }}".to_owned(),
                )]),
            },
        }],
    }
}
fn context() -> RenderContext {
    RenderContext {
        tool_pins: Vec::new(),
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        scale_set_image_source_sha: None,
        verified_cache_helpers: BTreeMap::new(),
        require_verified_cache_helper: false,
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        mise_quiet: true,
        plan_consumer_env: BTreeMap::new(),
    }
}

fn workflow(job: Job) -> WorkflowIr {
    workflow_with_jobs(BTreeMap::from([(PROTOTYPE_JOB_ID.to_owned(), job)]))
}

fn workflow_with_jobs(jobs: BTreeMap<String, Job>) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: vec!["main".to_owned()],
            merge_group: false,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
        cache_mode: None,
    }
}

fn lane_share(job: Job) -> LaneShare {
    let prepared = prepare_job(PROTOTYPE_JOB_ID, &job).expect("source job lowers");
    LaneShare {
        jobs: BTreeMap::from([(PROTOTYPE_JOB_ID.to_owned(), job)]),
        job_workflows: BTreeMap::from([(PROTOTYPE_JOB_ID.to_owned(), prepared)]),
        calls: BTreeMap::new(),
        checkouts: BTreeMap::new(),
        env_steps: BTreeMap::new(),
        runtime_preludes: BTreeMap::new(),
        prefixes: BTreeMap::new(),
        preludes: BTreeMap::new(),
        postludes: BTreeMap::new(),
        files: Vec::<RenderedFile>::new(),
    }
}

#[test]
fn rendered_parent_and_child_pass_the_source_bound_relocation_gate() {
    let job = source_job();
    let workflow = workflow(job.clone());
    let shared = lane_share(job);
    let context = context();
    let parent = workflow_to_yaml(&workflow, &shared, &context, &BTreeSet::new())
        .expect("parent caller serializes");
    let parent = route_workflow_yaml_for_jobs(
        ".github/workflows/ci.yml",
        parent,
        &WorkflowRoutingPolicy::hosted_only(),
        &shared.jobs,
    )
    .expect("local reusable caller keeps hosted placement");
    let relocations =
        render_job_workflow_relocations(&workflow, &shared, &context, &BTreeSet::new())
            .expect("child serializes from the source job");

    assert_eq!(relocations.len(), 1);
    let relocation = &relocations[0];
    let Yaml::Map(parent_fields) = &parent else {
        panic!("parent workflow is a map");
    };
    let Some((_, Yaml::Map(parent_jobs))) = parent_fields.iter().find(|(key, _)| key == "jobs")
    else {
        panic!("parent workflow has jobs");
    };
    let caller = &parent_jobs[0].1;
    assert_eq!(caller, &relocation.expected_caller);
    assert_caller_and_child_snapshots(caller, relocation);

    crate::workflow_routing::validate_relocated_job_workflows(
        &parent,
        std::slice::from_ref(&relocation.expected_callee_file),
        &relocations,
    )
    .expect("parent and child match the typed source descriptor");
}

fn assert_caller_and_child_snapshots(caller: &Yaml, relocation: &JobWorkflowRelocation) {
    let Yaml::Map(caller_fields) = caller else {
        panic!("reusable caller is a map");
    };
    assert_eq!(
        caller_fields
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "name",
            "uses",
            "with",
            "permissions",
            "cache-mode",
            "needs",
            "if"
        ]
    );
    assert!(caller_fields.iter().any(|(name, value)| {
        name == "uses"
            && value
                == &Yaml::str("./.github/workflows/ci-rust-rust-velnor-actions-cli__hosted.yml")
    }));
    assert!(
        caller_fields.iter().any(|(name, value)| {
            name == "needs" && value == &Yaml::Seq(vec![Yaml::str("plan")])
        })
    );
    assert!(caller_fields.iter().any(|(name, value)| {
        name == "if" && value == &Yaml::str("${{ needs.plan.result == 'success' }}")
    }));
    let Some((_, Yaml::Map(input_fields))) = caller_fields.iter().find(|(name, _)| name == "with")
    else {
        panic!("caller has five workflow_call inputs");
    };
    assert_eq!(input_fields.len(), 5);
    assert!(
        input_fields
            .iter()
            .any(|(name, _)| name == "run_key_attempt")
    );

    let child = &relocation.expected_callee_file.bytes;
    assert_eq!(
        relocation.expected_callee_file.path,
        ".github/workflows/ci-rust-rust-velnor-actions-cli__hosted.yml"
    );
    assert!(child.contains("workflow_call:"));
    assert!(child.contains("runs-on: ubuntu-26.04"));
    assert!(child.contains("inputs.run_key_attempt"));
    assert!(child.contains("inputs.sources_key"));
    assert!(child.contains("Read source key"));
    assert!(!child.contains("needs: plan"));
}

#[test]
fn emitted_child_preserves_typed_cache_parallel_and_required_graph() {
    let fixture = fixture::rendered_fixture();
    assert_eq!(fixture.relocations.len(), 1);
    crate::workflow_routing::validate_relocated_job_workflows(
        &fixture.parent,
        &fixture.files,
        &fixture.relocations,
    )
    .expect("the emitted parent and child are bound to the typed source job");
    validate_generated_workflow_calls_with_ir(&fixture.files, &fixture.policy, &fixture.workflow)
        .expect("the complete workflow and local-call inventory remains valid");
    assert!(fixture.files.iter().any(|file| {
        file.path == velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH
            && file.bytes.contains(&fixture.source_task_digest)
    }));

    assert_parent_and_required_dependencies(&fixture);
    assert_child_keeps_cache_parallel_and_call_schema(&fixture.relocations[0]);
}

fn assert_parent_and_required_dependencies(fixture: &fixture::RenderedFixture) {
    let parent_caller = workflow_job(&fixture.parent, PROTOTYPE_JOB_ID);
    assert!(job_needs(parent_caller).contains(&"plan"));
    let source_condition = fixture.workflow.jobs[PROTOTYPE_JOB_ID]
        .condition
        .as_deref()
        .expect("the typed source job carries its affected-job predicate");
    let caller_condition = crate::lane_share::job_workflow_extract::caller_condition_for_selection(
        PROTOTYPE_JOB_ID,
        Some(source_condition),
    )
    .expect("the explicit caller condition preserves the default success gate");
    let Yaml::Map(caller_fields) = parent_caller else {
        panic!("the hosted job is a reusable-workflow caller");
    };
    assert!(
        caller_fields
            .iter()
            .any(|(name, value)| { name == "if" && value == &Yaml::str(caller_condition.clone()) })
    );
    let required = workflow_job(&fixture.parent, crate::render::FINAL_JOB_ID);
    assert!(job_needs(required).contains(&PROTOTYPE_JOB_ID));
    assert!(job_needs(required).contains(&fixture::LOCAL_JOB_ID));
}

fn assert_child_keeps_cache_parallel_and_call_schema(relocation: &JobWorkflowRelocation) {
    let child = &relocation.expected_callee_file;
    assert_eq!(
        relocation.prepared.source_job.steps[3], relocation.prepared.child_job.steps[3],
        "the real declared-task wrapper call and its digest remain unchanged"
    );
    assert_eq!(
        action_with(&relocation.prepared.source_job.steps[2])["key"],
        "${{needs.plan.outputs.sources_key}}"
    );
    assert_eq!(
        action_with(&relocation.prepared.source_job.steps[2])["restore-keys"],
        "${{needs.plan.outputs.sources_restore_keys}}"
    );
    assert_eq!(
        action_with(&relocation.prepared.child_job.steps[2])["key"],
        "${{inputs.sources_key}}"
    );
    assert_eq!(
        action_with(&relocation.prepared.child_job.steps[2])["restore-keys"],
        "${{inputs.sources_restore_keys}}"
    );
    assert!(relocation.prepared.source_job.steps[4].parallel);
    assert!(relocation.prepared.child_job.steps[5].parallel);
    assert!(child.bytes.contains("inputs.sources_key"));
    assert!(child.bytes.contains("inputs.sources_restore_keys"));
    assert!(child.bytes.contains("declared-task-0"));
    let join = child
        .bytes
        .find("wait-all:")
        .expect("parallel group is joined");
    let save = child
        .bytes
        .find("name: Save cache")
        .expect("the cache save remains after the parallel group");
    assert!(join < save);

    let child_yaml = crate::workflow_routing::parse_authority_document(child)
        .expect("the emitted child is valid workflow YAML");
    assert_no_unbound_needs(&child_yaml);
    call_schema::assert_workflow_call_schema(&child_yaml);
    let child_job = parsed_workflow_job(&child_yaml, PROTOTYPE_JOB_ID);
    assert!(parsed_job_field(child_job, "needs").is_none());
    assert!(parsed_job_field(child_job, "if").is_none());
}

fn workflow_job<'a>(workflow: &'a Yaml, id: &str) -> &'a Yaml {
    let Yaml::Map(fields) = workflow else {
        panic!("workflow is a map");
    };
    let Some((_, Yaml::Map(jobs))) = fields.iter().find(|(name, _)| name == "jobs") else {
        panic!("workflow has jobs");
    };
    &jobs
        .iter()
        .find(|(job_id, _)| job_id == id)
        .expect("workflow job exists")
        .1
}

fn job_needs(job: &Yaml) -> Vec<&str> {
    let Yaml::Map(fields) = job else {
        panic!("workflow job is a map");
    };
    let Some((_, Yaml::Seq(needs))) = fields.iter().find(|(name, _)| name == "needs") else {
        return Vec::new();
    };
    needs
        .iter()
        .map(|dependency| match dependency {
            Yaml::Str(dependency) => dependency.as_str(),
            _ => panic!("job dependency is a string"),
        })
        .collect()
}

fn action_with(step: &Step) -> &BTreeMap<String, String> {
    let StepKind::Action { with, .. } = &step.kind else {
        panic!("source cache step is an action");
    };
    with
}

fn parsed_workflow_job<'a, 'source>(
    workflow: &'a saphyr::Yaml<'source>,
    id: &str,
) -> &'a saphyr::Yaml<'source> {
    parsed_job_field(workflow, "jobs")
        .and_then(saphyr::Yaml::as_mapping)
        .and_then(|jobs| {
            jobs.iter()
                .find_map(|(job_id, job)| (job_id.as_str() == Some(id)).then_some(job))
        })
        .expect("parsed workflow contains the requested job")
}

fn parsed_job_field<'a, 'source>(
    job: &'a saphyr::Yaml<'source>,
    name: &str,
) -> Option<&'a saphyr::Yaml<'source>> {
    job.as_mapping()?
        .iter()
        .find_map(|(field, value)| (field.as_str() == Some(name)).then_some(value))
}

fn assert_no_unbound_needs(workflow: &saphyr::Yaml<'_>) {
    assert!(
        !yaml_has_unbound_needs(workflow),
        "serialized child contains an unbound needs expression"
    );
}

fn yaml_has_unbound_needs(value: &saphyr::Yaml<'_>) -> bool {
    if let Some(mapping) = value.as_mapping() {
        return mapping.iter().any(|(key, child)| {
            let condition = key.as_str() == Some("if");
            unbound_needs::value_has_unbound_needs(child, condition)
        });
    }
    if let Some(sequence) = value.as_sequence() {
        return sequence.iter().any(|child| yaml_has_unbound_needs(child));
    }
    value.as_str().is_some_and(|text| {
        crate::lane_share::job_workflow_extract::test_expression_has_unbound_needs(text)
            .expect("serialized expression is well formed")
    })
}
