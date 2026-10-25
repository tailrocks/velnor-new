use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};

use crate::{
    RenderedFile,
    lane_share::{LaneShare, job_workflow_extract},
    render::test_render_capture::{with_full_tree_capture, with_source_job_workflow_capture},
    workflow_routing::{parse_authority_document, validate_relocated_job_workflows},
    yaml::Yaml,
};

use super::super::super::super::render_job_workflow_relocations_for_capture;
use super::super::super::validate_routed_child_projection;

struct RoutedPair {
    parent: RenderedFile,
    relocations: Vec<job_workflow_extract::JobWorkflowRelocation>,
    runner_labels: BTreeMap<String, Vec<String>>,
}

#[test]
fn routed_children_preserve_scale_set_projection_and_hosted_container_absence() {
    let pair = routed_pair();
    let parent_document = parse_authority_document(&pair.parent).expect("parent parses");
    let hosted = relocation(&pair, job_workflow_extract::PROTOTYPE_JOB_ID);
    let hosted_document =
        parse_authority_document(&hosted.expected_callee_file).expect("hosted child parses");
    assert!(
        parsed_field(
            parsed_job(&hosted_document, &hosted.prepared.source_id),
            "container"
        )
        .is_none()
    );

    let local = relocation(&pair, job_workflow_extract::PROTOTYPE_LOCAL_JOB_ID);
    validate_routed_child_projection(
        &local.expected_callee_file,
        &local.prepared.source_id,
        &parent_document,
        &pair.runner_labels[&local.prepared.source_id],
    )
    .expect("local child matches the routed parent execution fields");
    let local_document =
        parse_authority_document(&local.expected_callee_file).expect("local child parses");
    assert_scale_set_container(parsed_job(&local_document, &local.prepared.source_id));
}

#[test]
fn routed_child_rejects_container_image_drift() {
    assert_local_child_drift_rejected(|bytes| {
        bytes.replacen(&test_image('a'), &test_image('b'), 1)
    });
}

#[test]
fn routed_child_rejects_ordered_volume_drift() {
    assert_local_child_drift_rejected(|bytes| {
        bytes.replacen(
            "/.velnor-runtime-identity:/.velnor-runtime-identity:ro",
            "/.velnor-runtime-identity:/tmp/velnor-runtime-identity:ro",
            1,
        )
    });
}

#[test]
fn routed_child_rejects_shell_default_drift() {
    assert_local_child_drift_rejected(|bytes| bytes.replacen("bash -e {0}", "sh -e {0}", 1));
}

fn assert_local_child_drift_rejected(edit: impl FnOnce(&str) -> String) {
    let pair = routed_pair();
    let parent_document = parse_authority_document(&pair.parent).expect("parent parses");
    let local = relocation(&pair, job_workflow_extract::PROTOTYPE_LOCAL_JOB_ID);
    let changed = edit(&local.expected_callee_file.bytes);
    assert_ne!(changed, local.expected_callee_file.bytes);
    let changed = RenderedFile {
        path: local.expected_callee_file.path.clone(),
        bytes: changed,
    };

    assert!(
        validate_routed_child_projection(
            &changed,
            &local.prepared.source_id,
            &parent_document,
            &pair.runner_labels[&local.prepared.source_id],
        )
        .is_err()
    );
}

fn routed_pair() -> RoutedPair {
    let fixture = super::super::fixture::rendered_fixture();
    let context = capture_context();
    let source_ids = BTreeSet::from([
        job_workflow_extract::PROTOTYPE_JOB_ID.to_owned(),
        job_workflow_extract::PROTOTYPE_LOCAL_JOB_ID.to_owned(),
    ]);
    let captured = with_source_job_workflow_capture(source_ids.clone(), || {
        with_full_tree_capture(|| {
            let mut prepared = BTreeMap::new();
            let mut runner_labels = BTreeMap::new();
            for source_id in &source_ids {
                let source = &fixture.workflow.jobs[source_id];
                let runner = job_workflow_extract::capture_runner_metadata(source_id, source)
                    .expect("source has a supported typed runner");
                let child =
                    job_workflow_extract::prepare_job_for_capture(source_id, source, &source_ids)
                        .expect("source job lowers under the exact paired allowlist");
                runner_labels.insert(source_id.clone(), runner.final_runner_labels);
                prepared.insert(source_id.clone(), child);
            }
            let shared = empty_lane_share(fixture.workflow.jobs.clone(), prepared);
            let relocations = render_job_workflow_relocations_for_capture(
                &fixture.workflow,
                &shared,
                &context,
                &BTreeSet::new(),
                &fixture.policy,
            )
            .expect("children route from source jobs under the same policy");
            let parent = fixture
                .files
                .iter()
                .find(|file| file.path == crate::render::WORKFLOW_PATH)
                .expect("parent workflow is rendered")
                .clone();
            let parent_document = parse_authority_document(&parent).expect("parent parses");
            for relocation in &relocations {
                let runner = job_workflow_extract::capture_runner_metadata(
                    &relocation.prepared.source_id,
                    &relocation.prepared.source_job,
                )
                .expect("source runner metadata is available");
                job_workflow_extract::validate_finalized_runner(
                    &relocation.prepared.source_id,
                    &relocation.prepared.source_job,
                    &fixture.parent,
                    &relocation.expected_caller,
                )
                .expect("parent caller and runner metadata stay source-bound");
                validate_routed_child_projection(
                    &relocation.expected_callee_file,
                    &relocation.prepared.source_id,
                    &parent_document,
                    &runner.final_runner_labels,
                )
                .expect("routed child execution fields match the parent");
            }
            let mut diagnostic_parent = fixture.parent.clone();
            replace_callers(&mut diagnostic_parent, &relocations);
            let child_files = relocations
                .iter()
                .map(|relocation| relocation.expected_callee_file.clone())
                .collect::<Vec<_>>();
            validate_relocated_job_workflows(&diagnostic_parent, &child_files, &relocations)
                .expect("relocations pass the normal source-bound validator");
            drop(parent_document);
            (parent, relocations, runner_labels)
        })
    });
    let (parent, relocations, runner_labels) = captured;
    RoutedPair {
        parent,
        relocations,
        runner_labels,
    }
}

fn capture_context() -> crate::render::RenderContext {
    let selector = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .expect("test Scale Set selector is valid");
    let mut context = super::super::context();
    context.scale_set_selector = Some(selector);
    context
}

fn empty_lane_share(
    jobs: BTreeMap<String, velnor_actions_contract::Job>,
    job_workflows: BTreeMap<String, job_workflow_extract::PreparedJob>,
) -> LaneShare {
    LaneShare {
        jobs,
        job_workflows,
        calls: BTreeMap::new(),
        checkouts: BTreeMap::new(),
        env_steps: BTreeMap::new(),
        runtime_preludes: BTreeMap::new(),
        prefixes: BTreeMap::new(),
        preludes: BTreeMap::new(),
        postludes: BTreeMap::new(),
        files: Vec::new(),
    }
}

fn replace_callers(parent: &mut Yaml, relocations: &[job_workflow_extract::JobWorkflowRelocation]) {
    let Yaml::Map(fields) = parent else {
        panic!("parent workflow is a map");
    };
    let Some((_, Yaml::Map(jobs))) = fields.iter_mut().find(|(name, _)| name == "jobs") else {
        panic!("parent jobs is a map");
    };
    for relocation in relocations {
        let Some((_, job)) = jobs
            .iter_mut()
            .find(|(job_id, _)| job_id == &relocation.prepared.source_id)
        else {
            panic!("source job exists in parent");
        };
        *job = relocation.expected_caller.clone();
    }
}

fn relocation<'a>(
    pair: &'a RoutedPair,
    source_id: &str,
) -> &'a job_workflow_extract::JobWorkflowRelocation {
    pair.relocations
        .iter()
        .find(|relocation| relocation.prepared.source_id == source_id)
        .expect("requested source relocation exists")
}

fn assert_scale_set_container(job: &saphyr::Yaml<'_>) {
    let container = parsed_field(job, "container").expect("Scale Set job has a container");
    assert_eq!(
        parsed_field(container, "image").and_then(saphyr::Yaml::as_str),
        Some(test_image('a').as_str())
    );
    let volumes = parsed_field(container, "volumes")
        .and_then(saphyr::Yaml::as_vec)
        .expect("container mounts are an ordered sequence");
    let actual = volumes
        .iter()
        .map(|volume| volume.as_str().expect("mount is scalar"))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            "/opt/velnor/job-facts:/opt/velnor/job-facts:ro",
            "/.velnor-runtime-identity:/.velnor-runtime-identity:ro",
        ]
    );
    let run = parsed_field(job, "defaults")
        .and_then(|defaults| parsed_field(defaults, "run"))
        .expect("routed container has job run defaults");
    assert_eq!(
        parsed_field(run, "shell").and_then(saphyr::Yaml::as_str),
        Some("bash -e {0}")
    );
}

fn parsed_job<'node, 'input>(
    document: &'node saphyr::Yaml<'input>,
    source_id: &str,
) -> &'node saphyr::Yaml<'input> {
    document
        .as_mapping()
        .and_then(|fields| fields.iter().find(|(key, _)| key.as_str() == Some("jobs")))
        .and_then(|(_, jobs)| jobs.as_mapping())
        .and_then(|jobs| {
            jobs.iter()
                .find(|(job_id, _)| job_id.as_str() == Some(source_id))
        })
        .map(|(_, job)| job)
        .expect("workflow contains the requested job")
}

fn parsed_field<'node, 'input>(
    value: &'node saphyr::Yaml<'input>,
    name: &str,
) -> Option<&'node saphyr::Yaml<'input>> {
    value
        .as_mapping()?
        .iter()
        .find(|(key, _)| key.as_str() == Some(name))
        .map(|(_, value)| value)
}

fn test_image(digest: char) -> String {
    format!(
        "ghcr.io/tailrocks/velnor-job@sha256:{}",
        digest.to_string().repeat(64)
    )
}
