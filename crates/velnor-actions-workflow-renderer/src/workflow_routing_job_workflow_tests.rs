use std::collections::BTreeMap;
use std::collections::BTreeSet;

use velnor_actions_contract::{CacheMode, Job, JobTimeout, PermissionLevel, Permissions};
use velnor_actions_contract::{Step, StepKind};

use super::{validate_relocated_job_workflows, validate_relocated_job_workflows_with_selection};
use crate::{
    RenderedFile,
    lane_share::job_workflow_extract::{
        JobWorkflowRelocation, JobWorkflowSelection, PROTOTYPE_JOB_ID, PROTOTYPE_LOCAL_JOB_ID,
        prepare_job, source_bound_caller_inputs,
    },
    yaml::Yaml,
};

fn relocation() -> JobWorkflowRelocation {
    let source_job = Job {
        display_name: "Rust CLI hosted".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: vec!["plan".to_owned()],
        condition: Some("${{ needs.plan.result == 'success' }}".to_owned()),
        permissions: Some(Permissions {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: PermissionLevel::Read,
            packages: PermissionLevel::None,
        }),
        cache_mode: Some(CacheMode::Read),
        environment: None,
        services: Vec::new(),
        steps: vec![Step {
            name: "task argv --digest source-digest cache-owner source-owner".to_owned(),
            id: None,
            role: None,
            condition: None,
            parallel: false,
            kind: StepKind::Shell {
                run: vec!["printf ok".to_owned()],
                env: BTreeMap::new(),
            },
        }],
    };
    let prepared = prepare_job(PROTOTYPE_JOB_ID, &source_job).expect("valid source job");
    JobWorkflowRelocation {
        expected_caller: caller_yaml(&source_job),
        expected_callee_file: RenderedFile {
            path: prepared.callee_path.clone(),
            bytes: callee_bytes(PROTOTYPE_JOB_ID),
        },
        prepared,
    }
}

fn caller_yaml(source_job: &Job) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Rust CLI hosted")),
        (
            "uses".to_owned(),
            Yaml::str(format!(
                "./.github/workflows/ci-rust-{PROTOTYPE_JOB_ID}.yml"
            )),
        ),
        (
            "with".to_owned(),
            Yaml::Map(
                source_bound_caller_inputs(source_job)
                    .into_iter()
                    .map(|input| (input.name.to_owned(), Yaml::str(input.expression)))
                    .collect(),
            ),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("contents".to_owned(), Yaml::str("read")),
                ("actions".to_owned(), Yaml::str("read")),
            ]),
        ),
        Yaml::cache_mode_field(&CacheMode::Read),
        ("needs".to_owned(), Yaml::Seq(vec![Yaml::str("plan")])),
        (
            "if".to_owned(),
            Yaml::str("${{ needs.plan.result == 'success' }}"),
        ),
    ])
}

fn callee_bytes(inner_job_id: &str) -> String {
    format!(
        "name: Rust CLI hosted\non:\n  workflow_call:\n    inputs:\n      plan_result:\n        type: string\n        required: true\n      qualification_campaign:\n        type: string\n        required: true\n      run_key_attempt:\n        type: string\n        required: true\n      sources_key:\n        type: string\n        required: true\n      sources_restore_keys:\n        type: string\n        required: true\npermissions:\n  contents: read\njobs:\n  {inner_job_id}:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: task argv --digest source-digest cache-owner source-owner\n        run: printf ok\n"
    )
}

fn parent(relocation: &JobWorkflowRelocation) -> Yaml {
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            relocation.prepared.source_id.clone(),
            relocation.expected_caller.clone(),
        )]),
    )])
}

fn validate(relocation: &JobWorkflowRelocation, parent: &Yaml, files: &[RenderedFile]) -> bool {
    validate_relocated_job_workflows(parent, files, std::slice::from_ref(relocation)).is_ok()
}

#[test]
fn accepts_exact_source_bound_caller_and_single_job_callee() {
    let relocation = relocation();
    assert!(validate(
        &relocation,
        &parent(&relocation),
        std::slice::from_ref(&relocation.expected_callee_file),
    ));
}

#[test]
fn rejects_parent_binding_or_source_metadata_changes() {
    let relocation = relocation();
    let mut wrong_target = parent(&relocation);
    replace_job_field(
        &mut wrong_target,
        "uses",
        Yaml::str("./.github/workflows/foreign.yml"),
    );
    assert!(!validate(
        &relocation,
        &wrong_target,
        std::slice::from_ref(&relocation.expected_callee_file),
    ));

    let mut wrong_gate = parent(&relocation);
    replace_job_field(&mut wrong_gate, "if", Yaml::str("always()"));
    assert!(!validate(
        &relocation,
        &wrong_gate,
        std::slice::from_ref(&relocation.expected_callee_file),
    ));

    let mut wrong_input = parent(&relocation);
    replace_input_binding(
        &mut wrong_input,
        "sources_key",
        Yaml::str("${{ github.sha }}"),
    );
    assert!(!validate(
        &relocation,
        &wrong_input,
        std::slice::from_ref(&relocation.expected_callee_file),
    ));
}

#[test]
fn rejects_wrong_inner_id_and_missing_or_duplicate_callee_files() {
    let mut relocation = relocation();
    let wrong_inner = RenderedFile {
        path: relocation.expected_callee_file.path.clone(),
        bytes: callee_bytes("foreign-inner-job"),
    };
    assert!(!validate(&relocation, &parent(&relocation), &[wrong_inner],));
    relocation.expected_callee_file.bytes = callee_bytes("foreign-inner-job");
    assert!(!validate(
        &relocation,
        &parent(&relocation),
        std::slice::from_ref(&relocation.expected_callee_file),
    ));
    assert!(!validate(&relocation, &parent(&relocation), &[]));
    assert!(!validate(
        &relocation,
        &parent(&relocation),
        &[
            relocation.expected_callee_file.clone(),
            relocation.expected_callee_file.clone(),
        ],
    ));
}

#[test]
fn rejects_source_step_body_drift_for_argv_digest_and_cache_owner_values() {
    let relocation = relocation();
    for (before, after) in [
        ("source-digest", "changed-digest"),
        ("task argv", "different argv"),
        ("source-owner", "foreign-owner"),
    ] {
        let changed = RenderedFile {
            path: relocation.expected_callee_file.path.clone(),
            bytes: relocation.expected_callee_file.bytes.replace(before, after),
        };
        assert!(!validate(&relocation, &parent(&relocation), &[changed],));
    }
}

#[test]
fn rejects_extra_workflow_call_inputs_and_outputs() {
    for (needle, insertion) in [
        (
            "      sources_restore_keys:\n",
            "      unexpected:\n        type: string\n        required: true\n      sources_restore_keys:\n",
        ),
        (
            "permissions:\n",
            "    outputs:\n      answer:\n        value: ok\npermissions:\n",
        ),
    ] {
        let mut relocation = relocation();
        relocation.expected_callee_file.bytes = relocation
            .expected_callee_file
            .bytes
            .replace(needle, insertion);
        let candidate = relocation.expected_callee_file.clone();
        assert!(!validate(&relocation, &parent(&relocation), &[candidate],));
    }
}

#[test]
fn rejects_a_child_snapshot_that_does_not_match_the_typed_source_job() {
    let mut relocation = relocation();
    relocation.prepared.child_job.runs_on = "ubuntu-24.04".to_owned();
    assert!(!validate(
        &relocation,
        &parent(&relocation),
        std::slice::from_ref(&relocation.expected_callee_file),
    ));
}

#[test]
fn rejects_duplicate_relocation_ids_and_duplicate_parent_callers() {
    let relocation = relocation();
    let duplicate = vec![relocation.clone(), relocation.clone()];
    assert!(
        validate_relocated_job_workflows(
            &parent(&relocation),
            std::slice::from_ref(&relocation.expected_callee_file),
            &duplicate,
        )
        .is_err()
    );

    let duplicate_parent = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![
            (
                relocation.prepared.source_id.clone(),
                relocation.expected_caller.clone(),
            ),
            (
                relocation.prepared.source_id.clone(),
                relocation.expected_caller.clone(),
            ),
        ]),
    )]);
    assert!(!validate(
        &relocation,
        &duplicate_parent,
        std::slice::from_ref(&relocation.expected_callee_file),
    ));
}

#[test]
fn validates_explicit_relocation_inventory_including_empty_selection() {
    let empty_selection = BTreeSet::new();
    assert!(
        validate_relocated_job_workflows_with_selection(
            &Yaml::Map(Vec::new()),
            &[],
            &empty_selection,
            &[],
        )
        .is_ok()
    );

    let selected = BTreeSet::from([
        PROTOTYPE_JOB_ID.to_owned(),
        PROTOTYPE_LOCAL_JOB_ID.to_owned(),
    ]);
    let mut hosted = relocation();
    hosted.prepared.selection = JobWorkflowSelection::explicit(&selected);
    assert!(
        validate_relocated_job_workflows_with_selection(
            &parent(&hosted),
            std::slice::from_ref(&hosted.expected_callee_file),
            &selected,
            std::slice::from_ref(&hosted),
        )
        .is_err()
    );
}

#[test]
fn rejects_mixed_legacy_and_explicit_relocation_selections() {
    let hosted = relocation();
    let selected = BTreeSet::from([
        PROTOTYPE_JOB_ID.to_owned(),
        PROTOTYPE_LOCAL_JOB_ID.to_owned(),
    ]);
    let mut local = hosted.clone();
    local.prepared.source_id = PROTOTYPE_LOCAL_JOB_ID.to_owned();
    local.prepared.selection = JobWorkflowSelection::explicit(&selected);

    let result = validate_relocated_job_workflows_with_selection(
        &Yaml::Map(Vec::new()),
        &[],
        &selected,
        &[hosted, local],
    );
    assert!(matches!(
        result,
        Err(crate::RenderError::InvalidWorkflow(problem))
            if problem == "job_workflow_selection_state_mismatch"
    ));
}

fn replace_job_field(parent: &mut Yaml, name: &str, replacement: Yaml) {
    let Yaml::Map(document) = parent else {
        panic!("parent workflow is a map");
    };
    let Some((_, Yaml::Map(jobs))) = document.iter_mut().find(|(key, _)| key == "jobs") else {
        panic!("parent workflow has jobs");
    };
    let Some((_, Yaml::Map(fields))) = jobs.first_mut() else {
        panic!("parent workflow has one caller");
    };
    let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == name) else {
        panic!("caller has {name}");
    };
    *value = replacement;
}

fn replace_input_binding(parent: &mut Yaml, name: &str, replacement: Yaml) {
    let Yaml::Map(document) = parent else {
        panic!("parent workflow is a map");
    };
    let Some((_, Yaml::Map(jobs))) = document.iter_mut().find(|(key, _)| key == "jobs") else {
        panic!("parent workflow has jobs");
    };
    let Some((_, Yaml::Map(fields))) = jobs.first_mut() else {
        panic!("parent workflow has one caller");
    };
    let Some((_, Yaml::Map(inputs))) = fields.iter_mut().find(|(key, _)| key == "with") else {
        panic!("caller has with inputs");
    };
    let Some((_, value)) = inputs.iter_mut().find(|(key, _)| key == name) else {
        panic!("caller has input {name}");
    };
    *value = replacement;
}
