use std::{collections::BTreeSet, fmt::Write as _};

use velnor_actions_contract::{ExecutionMode, RoutingWorkflow};

use super::{
    MbxQualificationPins, MisePinQualificationPins, Schema2WorkflowRequest,
    render_schema2_workflows, render_schema2_workflows_with_verified_routing,
};
use crate::workflow_routing::{self, WorkflowRoutingPolicy};

fn request() -> Schema2WorkflowRequest {
    let selector = Schema2WorkflowRequest::canonical_scale_set().expect("valid selector");
    let product = super::product_release_test_pins::test_pins();
    Schema2WorkflowRequest {
        version: "0.1.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: selector.clone(),
        routing: WorkflowRoutingPolicy::configured(ExecutionMode::Both, selector, None),
        workflows: BTreeSet::from([RoutingWorkflow::Qualification]),
        mbx_qualification: Some(MbxQualificationPins {
            mise_setup: product.linux_x86_64_setup.clone(),
            candidate_action_uses: format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
            mbx_version: "1.28.0".to_owned(),
            rust_version: product.rust_version.clone(),
            verify_rust_argv: product.verify_rust_argv.clone(),
        }),
        mise_pin_qualification: Some(MisePinQualificationPins {
            linux_x86_64_setup: product.linux_x86_64_setup.clone(),
            macos_x86_64_setup: product.macos_x86_64_setup.clone(),
        }),
        product_release: Some(product),
    }
}

#[test]
fn paired_sdk_jobs_are_bound_to_the_rendered_source_and_use_same_run_artifact() {
    let request = request();
    let (files, routing) = render_schema2_workflows_with_verified_routing(&request)
        .expect("paired qualification renders with bound routing");
    let qualification = files
        .iter()
        .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
        .expect("qualification file is present");
    let yaml = qualification.bytes.as_str();

    assert!(yaml.contains("paired-sdk-build:"), "{yaml}");
    assert!(yaml.contains("paired-sdk-tls:"), "{yaml}");
    assert!(yaml.contains("inputs.mode == 'paired-sdk-build' || inputs.mode == 'paired-sdk-tls'"));
    assert!(yaml.contains("inputs.mode == 'paired-sdk-tls'"));
    assert!(yaml.contains("timeout-minutes: 30"));
    assert!(yaml.contains("velnor-paired-sdk-${{ github.run_id }}-${{ github.run_attempt }}"));
    assert!(yaml.contains("artifact-ids: ${{ needs.paired-sdk-build.outputs.artifact_id }}"));
    assert!(yaml.contains(
        "VELNOR_PAIRED_SDK_BINARY_SHA256: ${{ needs.paired-sdk-build.outputs.binary_sha256 }}"
    ));
    assert!(yaml.contains("paired-sdk-build.outputs.artifact_id"));
    assert!(yaml.contains("paired-sdk-build.outputs.binary_sha256"));

    let consumer = job_section(yaml, "paired-sdk-tls");
    assert!(
        consumer.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"),
        "{consumer}"
    );
    assert!(
        consumer.contains("needs:\n      - paired-sdk-build"),
        "{consumer}"
    );
    assert!(!consumer.contains("container:"), "{consumer}");
    let builder = job_section(yaml, "paired-sdk-build");
    assert!(builder.contains("runs-on: ubuntu-26.04"), "{builder}");
    assert!(!builder.contains("container:"), "{builder}");
    assert!(builder.contains("actions: write"), "{builder}");
    assert!(builder.contains("contents: read"), "{builder}");
    assert!(consumer.contains("actions: read"), "{consumer}");

    workflow_routing::validate_generated_workflow_calls(&files, &routing)
        .expect("final inventory accepts the renderer-bound qualification");
}

#[test]
fn configured_image_policy_is_preserved_for_ordinary_jobs_and_scoped_for_pair() {
    let mut request = request();
    request.routing = workflow_routing::test_policy(ExecutionMode::Both, request.scale_set.clone());
    let (files, routing) = render_schema2_workflows_with_verified_routing(&request)
        .expect("configured image policy renders with paired qualification");
    let qualification = files
        .iter()
        .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
        .expect("qualification file is present");

    let builder = job_section(&qualification.bytes, "paired-sdk-build");
    assert!(builder.contains("runs-on: ubuntu-26.04"), "{builder}");
    assert!(!builder.contains("container:"), "{builder}");

    let consumer = job_section(&qualification.bytes, "paired-sdk-tls");
    assert!(
        consumer.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"),
        "{consumer}"
    );
    assert!(!consumer.contains("container:"), "{consumer}");

    let ordinary = job_section(&qualification.bytes, "verify-scale-set");
    assert!(ordinary.contains("container:"), "{ordinary}");
    assert!(
        ordinary.contains(
            "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ),
        "ordinary Scale Set jobs retain the configured immutable image: {ordinary}"
    );
    assert!(ordinary.contains("volumes:"), "{ordinary}");
    assert!(
        ordinary.contains("/opt/velnor/job-facts:/opt/velnor/job-facts:ro"),
        "{ordinary}"
    );
    assert!(
        ordinary.contains("/.velnor-runtime-identity:/.velnor-runtime-identity:ro"),
        "{ordinary}"
    );
    assert!(ordinary.contains("shell: bash -e {0}"), "{ordinary}");
    workflow_routing::validate_generated_workflow_calls(&files, &routing)
        .expect("bound pair exception leaves ordinary placement policy intact");
}

#[test]
fn final_inventory_rejects_bytes_changed_after_pair_binding() {
    let request = request();
    let (mut files, routing) = render_schema2_workflows_with_verified_routing(&request)
        .expect("paired qualification renders");
    let qualification = files
        .iter_mut()
        .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
        .expect("qualification file is present");
    qualification
        .bytes
        .push_str("\n# changed after the producer proof\n");

    assert!(workflow_routing::validate_generated_workflow_calls(&files, &routing).is_err());
}

#[test]
fn final_inventory_rejects_changed_pair_fields() {
    let cases = [
        (
            "paired-sdk-tls",
            "if: inputs.mode == 'paired-sdk-tls'",
            "if: inputs.mode == 'both'",
        ),
        (
            "paired-sdk-tls",
            "runs-on: [velnor, ubuntu-26.04-scale-set]",
            "runs-on: ubuntu-26.04",
        ),
        (
            "paired-sdk-tls",
            "needs:\n      - paired-sdk-build",
            "needs:\n      - other-job",
        ),
        ("paired-sdk-tls", "actions: read", "actions: write"),
        (
            "paired-sdk-tls",
            "Run Node and Rust HTTPS checks",
            "Run an unrelated step",
        ),
        (
            "paired-sdk-build",
            "if: inputs.mode == 'paired-sdk-build' || inputs.mode == 'paired-sdk-tls'",
            "if: inputs.mode == 'both'",
        ),
        (
            "paired-sdk-build",
            "runs-on: ubuntu-26.04",
            "runs-on: ubuntu-24.04",
        ),
        ("paired-sdk-build", "actions: write", "actions: read"),
        (
            "paired-sdk-build",
            "Build and record the fixture digest",
            "Build an unrelated artifact",
        ),
    ];

    for (job, before, after) in cases {
        let (mut files, routing) = render_schema2_workflows_with_verified_routing(&request())
            .expect("paired qualification renders");
        let qualification = files
            .iter_mut()
            .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
            .expect("qualification file is present");
        replace_in_job(&mut qualification.bytes, job, before, after);
        assert!(
            workflow_routing::validate_generated_workflow_calls(&files, &routing).is_err(),
            "final inventory accepted changed {job} field {before:?}"
        );
    }
}

#[test]
fn final_inventory_rejects_renamed_container_and_duplicate_pair_jobs() {
    let (mut files, routing) = render_schema2_workflows_with_verified_routing(&request())
        .expect("paired qualification renders");
    let qualification = files
        .iter_mut()
        .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
        .expect("qualification file is present");
    assert!(qualification.bytes.contains("  paired-sdk-tls:\n"));
    let marker_start = qualification
        .bytes
        .find("  paired-sdk-tls:\n")
        .expect("consumer marker exists");
    qualification.bytes.replace_range(
        marker_start..marker_start + "  paired-sdk-tls".len(),
        "renamed-paired-sdk-tls",
    );
    assert!(workflow_routing::validate_generated_workflow_calls(&files, &routing).is_err());

    let (mut files, routing) = render_schema2_workflows_with_verified_routing(&request())
        .expect("paired qualification renders");
    let qualification = files
        .iter_mut()
        .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
        .expect("qualification file is present");
    replace_in_job(
        &mut qualification.bytes,
        "paired-sdk-tls",
        "runs-on: [velnor, ubuntu-26.04-scale-set]",
        "runs-on: [velnor, ubuntu-26.04-scale-set]\n    container:\n      image: ghcr.io/team/job",
    );
    assert!(workflow_routing::validate_generated_workflow_calls(&files, &routing).is_err());

    let (mut files, routing) = render_schema2_workflows_with_verified_routing(&request())
        .expect("paired qualification renders");
    let qualification = files
        .iter_mut()
        .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
        .expect("qualification file is present");
    let duplicate = job_section(&qualification.bytes, "paired-sdk-tls").to_owned();
    write!(qualification.bytes, "\n  paired-sdk-tls:\n{duplicate}")
        .expect("write duplicate fixture job");
    assert!(workflow_routing::validate_generated_workflow_calls(&files, &routing).is_err());
}

#[test]
fn bound_policy_clones_are_immutable_and_repeated_renders_match() {
    let request = request();
    let (first_files, first_routing) = render_schema2_workflows_with_verified_routing(&request)
        .expect("first paired qualification render");
    let (second_files, second_routing) = render_schema2_workflows_with_verified_routing(&request)
        .expect("repeated paired qualification render");
    assert_eq!(first_files, second_files);
    assert_eq!(first_routing, second_routing);

    let copied_policy = first_routing.clone();
    workflow_routing::validate_generated_workflow_calls(&first_files, &copied_policy)
        .expect("cloned bound policy retains its exact immutable snapshot");
    assert_eq!(first_routing, copied_policy);
}

#[test]
fn bound_policy_rejects_reserved_pair_ids_outside_qualification() {
    let (mut files, routing) = render_schema2_workflows_with_verified_routing(&request())
        .expect("paired qualification renders");
    files.push(crate::RenderedFile {
        path: ".github/workflows/other.yml".to_owned(),
        bytes: "name: Other\non: push\njobs:\n  paired-sdk-tls:\n    runs-on: ubuntu-26.04\n    steps: []\n".to_owned(),
    });

    assert!(workflow_routing::validate_generated_workflow_calls(&files, &routing).is_err());
}

#[test]
fn unavailable_product_pins_emit_a_finite_failing_mode_job() {
    let mut request = request();
    request.product_release = None;
    let qualification =
        render_schema2_workflows(&request).expect("missing optional pins render explicit failure");
    let text = &qualification
        .iter()
        .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
        .expect("qualification file is present")
        .bytes;

    assert!(text.contains("paired-sdk-unavailable:"));
    assert!(text.contains("timeout-minutes: 5"));
    assert!(text.contains("exit 1"));
    assert!(!text.contains("paired-sdk-build:"));
    assert!(!text.contains("paired-sdk-tls:"));
}

#[test]
fn scale_set_verification_wrapper_remains_unmodified_by_pair_modes() {
    let mut request = request();
    request.routing =
        WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, request.scale_set.clone(), None);
    let (files, routing) = render_schema2_workflows_with_verified_routing(&request)
        .expect("scale-set verification wrapper renders");
    let text = &files
        .iter()
        .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
        .expect("qualification wrapper exists")
        .bytes;

    assert_eq!(routing, request.routing);
    assert!(text.contains("verification-only"));
    assert!(!text.contains("paired-sdk-"));
}

fn job_section<'a>(yaml: &'a str, job_id: &str) -> &'a str {
    let marker = format!("\n  {job_id}:\n");
    let (_, section) = yaml.split_once(&marker).expect("job marker is present");
    let mut end = 0;
    for line in section.split_inclusive('\n') {
        if line.starts_with("  ") && !line.starts_with("    ") {
            break;
        }
        end += line.len();
    }
    &section[..end]
}

fn replace_in_job(yaml: &mut String, job_id: &str, before: &str, after: &str) {
    let marker = format!("\n  {job_id}:\n");
    let start = yaml.find(&marker).expect("job marker is present") + marker.len();
    let end = yaml[start..]
        .split_inclusive('\n')
        .take_while(|line| !line.starts_with("  ") || line.starts_with("    "))
        .map(str::len)
        .sum::<usize>()
        + start;
    let section = &yaml[start..end];
    assert_eq!(
        section.matches(before).count(),
        1,
        "{before:?} is unique in {job_id}"
    );
    let replacement = section.replacen(before, after, 1);
    yaml.replace_range(start..end, &replacement);
}
