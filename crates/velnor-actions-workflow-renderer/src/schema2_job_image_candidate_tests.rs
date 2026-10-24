use std::collections::BTreeSet;

use velnor_actions_contract::RoutingWorkflow;

use crate::yaml::Yaml;

use super::super::{Schema2WorkflowRequest, product_release_test_pins};
use super::{ARTIFACT_NAME, PUBLICATION_EVIDENCE, WORKFLOW_PATH};

#[path = "schema2_job_image_native_tests.rs"]
mod native_source;

#[test]
fn image_selection_emits_repeatable_candidate_build_attestation_and_publish() {
    let request = request();
    let first = crate::schema2::render_schema2_workflows(&request)
        .expect("Render the job image candidate workflow.");
    let second = crate::schema2::render_schema2_workflows(&request)
        .expect("Render the job image candidate workflow again.");
    let first = first
        .iter()
        .find(|file| file.path == WORKFLOW_PATH)
        .expect("The image selection contains the job image candidate workflow.");
    let second = second
        .iter()
        .find(|file| file.path == WORKFLOW_PATH)
        .expect("The second image selection contains the job image candidate workflow.");
    assert_eq!(first.bytes, second.bytes);
    let rendered = first.bytes.clone();

    for expected in [
        "\"on\":",
        "push:",
        "workflow_dispatch:",
        "build-image-candidate:",
        "attest-image-candidate:",
        "publish-job-image:",
        "needs:",
        "--source-commit",
        "GITHUB_SHA",
        "--archive",
        "RUNNER_TEMP/job-image.oci.tar",
        "--record",
        "RUNNER_TEMP/job-image-candidate.json",
        "image-maintenance check-job-source",
        "image-maintenance install-buildx",
        "image-maintenance build-job-candidate",
        "image-maintenance validate-job-candidate",
        "job-image-native-source-closure.json",
        "env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "-u ACTIONS_ID_TOKEN_REQUEST_URL",
        "-u ACTIONS_RUNTIME_TOKEN",
        "-u GITHUB_TOKEN",
        "-u MISE_GITHUB_TOKEN",
        "-u GH_TOKEN",
        "-u GH_HOST",
        "-u GH_CONFIG_DIR",
        "target/release/velnor-actions",
        ".velnor/runner.lock",
        "--runner-temp \\\"$RUNNER_TEMP\\\"",
        "${{ github.sha }}",
        "${{ runner.temp }}/job-image.oci.tar",
        "${{ runner.temp }}/job-image-candidate.json",
        "actions/attest-build-provenance@",
        "actions/upload-artifact@",
        "actions/download-artifact@",
        "subject-path:",
        "permissions:",
        "attestations: write",
        "id-token: write",
        "artifact-metadata: write",
        "VELNOR_CANDIDATE_ARTIFACT_ID",
        "GH_TOKEN",
        "packages: write",
        "github.event_name == 'workflow_dispatch'",
        "refs/heads/main",
        "job-image-publish",
        "cancel-in-progress: false",
        PUBLICATION_EVIDENCE,
    ] {
        assert!(
            rendered.contains(expected),
            "The workflow does not contain {expected}: {rendered}"
        );
    }
    assert!(rendered.contains(ARTIFACT_NAME));
    assert!(ARTIFACT_NAME.ends_with("-${{ github.run_attempt }}"));
    assert!(rendered.contains(crate::steps::UPLOAD_ARTIFACT_USES));
    assert!(rendered.contains(crate::steps::DOWNLOAD_ARTIFACT_USES));
    assert!(rendered.contains(crate::steps::ATTEST_BUILD_PROVENANCE_USES));
    assert!(!rendered.contains("contents: write"));
    assert!(!rendered.contains("docker push"));
}

#[test]
fn build_job_has_no_attestation_or_repository_writer_permissions() {
    let workflow = super::render(&request()).expect("Render the job image candidate workflow.");
    let jobs = field(&workflow, "jobs");
    let build = field(jobs, "build-image-candidate");
    assert_eq!(field(build, "timeout-minutes"), &Yaml::Int(10));
    let permissions = field(build, "permissions");
    assert!(optional_field(permissions, "actions").is_none());
    assert_eq!(field(permissions, "contents"), &Yaml::str("read"));
    assert!(optional_field(permissions, "packages").is_none());
    assert!(optional_field(permissions, "id-token").is_none());
    assert!(optional_field(permissions, "attestations").is_none());

    let attest = field(jobs, "attest-image-candidate");
    assert_eq!(field(attest, "timeout-minutes"), &Yaml::Int(10));
    let permissions = field(attest, "permissions");
    assert_eq!(field(permissions, "attestations"), &Yaml::str("write"));
    assert_eq!(field(permissions, "id-token"), &Yaml::str("write"));
    assert_eq!(field(permissions, "contents"), &Yaml::str("read"));
    assert!(optional_field(permissions, "packages").is_none());
    assert!(optional_field(permissions, "contents").is_some());

    let publish = field(jobs, "publish-job-image");
    assert_eq!(field(publish, "timeout-minutes"), &Yaml::Int(5));
    assert_eq!(
        field(publish, "if"),
        &Yaml::str(
            "${{ github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' }}"
        )
    );
    let permissions = field(publish, "permissions");
    assert_eq!(field(permissions, "actions"), &Yaml::str("read"));
    assert_eq!(field(permissions, "attestations"), &Yaml::str("read"));
    assert_eq!(field(permissions, "contents"), &Yaml::str("read"));
    assert_eq!(field(permissions, "packages"), &Yaml::str("write"));
    assert!(optional_field(permissions, "id-token").is_none());
    assert_eq!(
        field(field(publish, "concurrency"), "group"),
        &Yaml::str("${{ github.repository }}-job-image-publish")
    );
    assert_eq!(
        field(field(publish, "concurrency"), "cancel-in-progress"),
        &Yaml::Bool(false)
    );
}

#[test]
fn build_job_uses_closed_native_image_maintenance_operations() {
    let workflow = super::render(&request()).expect("Render the job image candidate workflow.");
    let build = field(field(&workflow, "jobs"), "build-image-candidate");
    let Yaml::Seq(steps) = field(build, "steps") else {
        panic!("The build steps are necessary.");
    };

    let source_build = step_named(steps, "Build exact-source native image maintenance");
    let source_command = string_field(source_build, "run");
    for expected in [
        "trusted_git rev-parse --verify HEAD",
        "/usr/bin/env -i PATH=/usr/bin:/bin",
        "--git-dir=\"$GITHUB_WORKSPACE/.git\" --work-tree=\"$GITHUB_WORKSPACE\" -c core.fsmonitor=false",
        "test \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"",
        "GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_OPTIONAL_LOCKS=0",
        "diff --cached --quiet --no-ext-diff --no-textconv",
        "status --porcelain=v1 --untracked-files=all",
        "The source checkout is not clean.",
        "env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "-u ACTIONS_ID_TOKEN_REQUEST_URL",
        "-u ACTIONS_RUNTIME_TOKEN",
        "-u GITHUB_TOKEN",
        "-u MISE_GITHUB_TOKEN",
        "-u GH_TOKEN",
        "-u GH_HOST",
        "-u GH_CONFIG_DIR",
        "target/release/velnor-actions",
        "set -o noclobber",
        "mktemp -d",
        "VELNOR_NATIVE_IMAGE_ACTIONS=%s",
        "chmod 0500",
    ] {
        assert!(
            source_command.contains(expected),
            "Missing {expected}: {source_command}"
        );
    }
    assert!(!source_command.contains("python"));
    let pinned_build = crate::commands::join_argv_for_run(&test_pins().build_argv)
        .expect("The pinned native build command is valid.");
    assert!(source_command.contains(&pinned_build));
    let pinned_tools = crate::commands::join_argv_for_run(&test_pins().install_build_tools_argv)
        .expect("The pinned tool installation command is valid.");
    assert_eq!(
        string_field(step_named(steps, "Install pinned Rust and MBX"), "run"),
        format!("set -eu\n: \"${{GITHUB_TOKEN:?missing workflow token}}\"\n{pinned_tools}")
    );

    for (name, operation) in [
        (
            "Verify native implementation source closure",
            "check-job-source",
        ),
        (
            "Install the Buildx plugin from .velnor/runner.lock",
            "install-buildx",
        ),
        ("Build the job image candidate", "build-job-candidate"),
    ] {
        let command = string_field(step_named(steps, name), "run");
        assert!(command.contains("$VELNOR_NATIVE_IMAGE_ACTIONS"));
        assert!(command.contains(&format!("image-maintenance {operation}")));
        if operation == "install-buildx" {
            assert!(command.contains("--runner-temp \"$RUNNER_TEMP\""));
        }
        assert!(!command.contains("python"));
    }
}
#[test]
fn publisher_consumes_same_run_artifact_and_keeps_token_out_of_evidence_upload() {
    let workflow = super::render(&request()).expect("Render the job image candidate workflow.");
    let publish = field(field(&workflow, "jobs"), "publish-job-image");
    let Yaml::Seq(steps) = field(publish, "steps") else {
        panic!("The publisher steps are necessary.");
    };
    let download = steps
        .iter()
        .find(|step| {
            optional_field(step, "uses") == Some(&Yaml::str(crate::steps::DOWNLOAD_ARTIFACT_USES))
        })
        .expect("Find the same-run candidate download step.");
    assert_eq!(
        field(field(download, "with"), "path"),
        &Yaml::str("candidate")
    );
    assert_eq!(
        field(field(download, "with"), "artifact-ids"),
        &Yaml::str("${{ needs.build-image-candidate.outputs.artifact_id }}")
    );

    let publisher = steps
        .iter()
        .find(|step| {
            optional_field(step, "name")
                == Some(&Yaml::str(
                    "Publish the job image with its signed build evidence.",
                ))
        })
        .expect("Find the job image publisher step.");
    let env = field(publisher, "env");
    assert_eq!(field(env, "GH_TOKEN"), &Yaml::str("${{ github.token }}"));
    assert_eq!(
        field(env, "VELNOR_CANDIDATE_ARTIFACT_ID"),
        &Yaml::str("${{ needs.build-image-candidate.outputs.artifact_id }}")
    );
    let Yaml::Str(command) = field(publisher, "run") else {
        panic!("The publisher command is necessary.");
    };
    assert!(command.contains("candidate/publish.py --root ."));
    assert!(command.contains("--archive \"candidate/job-image.oci.tar\""));
    assert!(command.contains("--record \"candidate/job-image-candidate.json\""));
    assert!(command.contains("--result \"$RUNNER_TEMP/job-image-publication.json\""));

    let evidence = steps
        .iter()
        .find(|step| {
            optional_field(step, "name")
                == Some(&Yaml::str("Upload job image publication evidence"))
        })
        .expect("Find the publication evidence upload step.");
    assert!(optional_field(evidence, "env").is_none());
    assert_eq!(
        field(field(evidence, "with"), "name"),
        &Yaml::str(PUBLICATION_EVIDENCE)
    );
    assert!(PUBLICATION_EVIDENCE.ends_with("-${{ github.run_attempt }}"));
    assert_eq!(
        field(field(evidence, "with"), "retention-days"),
        &Yaml::Int(7)
    );
}

#[test]
fn no_image_selection_emits_no_candidate_workflow() {
    let mut request = request();
    request.workflows.clear();
    let files = crate::schema2::render_schema2_workflows(&request)
        .expect("Render workflows with no selected workflows.");
    assert!(!files.iter().any(|file| file.path == WORKFLOW_PATH));
}

fn request() -> Schema2WorkflowRequest {
    let scale_set =
        Schema2WorkflowRequest::canonical_scale_set().expect("Read the Scale Set selector.");
    Schema2WorkflowRequest {
        version: "0.1.19".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set,
        workflows: BTreeSet::from([RoutingWorkflow::ImageRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(test_pins()),
    }
}

fn test_pins() -> super::super::ProductReleasePins {
    product_release_test_pins::test_pins()
}

fn step_named<'a>(steps: &'a [Yaml], name: &str) -> &'a Yaml {
    steps
        .iter()
        .find(|step| optional_field(step, "name") == Some(&Yaml::str(name)))
        .unwrap_or_else(|| panic!("The workflow step is necessary: {name}."))
}

fn string_field<'a>(value: &'a Yaml, key: &str) -> &'a str {
    let Yaml::Str(text) = field(value, key) else {
        panic!("The YAML field {key} must be a string.");
    };
    text
}

fn field<'a>(value: &'a Yaml, key: &str) -> &'a Yaml {
    let Yaml::Map(fields) = value else {
        panic!("A YAML map is necessary for {key}.");
    };
    fields
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .unwrap_or_else(|| panic!("The YAML field is necessary: {key}."))
}

fn optional_field<'a>(value: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    let Yaml::Map(fields) = value else {
        panic!("A YAML map is necessary for {key}.");
    };
    fields
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
}

#[test]
fn pushes_cancel_superseded_runs_but_dispatches_never_share_their_group() {
    let workflow = super::render(&request()).expect("Render the job image candidate workflow.");
    let concurrency = field(&workflow, "concurrency");
    let group = field(concurrency, "group");
    let Yaml::Str(group) = group else {
        panic!("group is a plain string: {group:?}");
    };
    assert!(group.contains("${{ github.event_name }}"), "{group}");
    assert!(group.contains("${{ github.ref }}"), "{group}");
    assert_eq!(
        field(concurrency, "cancel-in-progress"),
        &Yaml::str("${{ github.event_name == 'push' }}")
    );
    let build = field(field(&workflow, "jobs"), "build-image-candidate");
    let outputs = field(build, "outputs");
    assert!(optional_field(outputs, "artifact_id").is_some());
    assert!(
        optional_field(outputs, "artifact_name").is_none(),
        "no job reads the artifact name"
    );
}
