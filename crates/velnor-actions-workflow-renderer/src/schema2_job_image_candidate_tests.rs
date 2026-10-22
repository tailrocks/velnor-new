use std::collections::BTreeSet;

use crate::workflow_routing::WorkflowRoutingPolicy;
use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_contract::{ExecutionMode, RoutingWorkflow};

use crate::yaml::Yaml;

use super::super::{Schema2WorkflowRequest, product_release_test_pins};
use super::{ARTIFACT_NAME, PUBLICATION_EVIDENCE, WORKFLOW_PATH};

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
        "images/job/ubuntu-26.04/candidate/candidate.py build",
        "--source-commit",
        "GITHUB_SHA",
        "--archive",
        "RUNNER_TEMP/job-image.oci.tar",
        "--record",
        "RUNNER_TEMP/job-image-candidate.json",
        ".velnor/runner.lock",
        "buildx_version",
        "buildx_asset_url",
        "buildx_sha256",
        "docker-buildx",
        "hashlib.file_digest",
        "actual_sha256 != expected_sha256",
        "plugin.parent.chmod(0o700)",
        "os.chmod(temporary, 0o755)",
        "DOCKER_CONFIG",
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
    assert_eq!(field(build, "timeout-minutes"), &Yaml::Int(5));
    let permissions = field(build, "permissions");
    assert!(optional_field(permissions, "actions").is_none());
    assert_eq!(field(permissions, "contents"), &Yaml::str("read"));
    assert!(optional_field(permissions, "packages").is_none());
    assert!(optional_field(permissions, "id-token").is_none());
    assert!(optional_field(permissions, "attestations").is_none());

    let attest = field(jobs, "attest-image-candidate");
    assert_eq!(field(attest, "timeout-minutes"), &Yaml::Int(5));
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

#[path = "schema2_job_image_candidate_tests/buildx_installer_tests.rs"]
mod buildx_installer_tests;

#[test]
fn attestation_download_uses_only_the_uploaded_artifact_id() {
    let workflow = super::render(&request()).expect("Render the job image candidate workflow.");
    let attest = field(field(&workflow, "jobs"), "attest-image-candidate");
    let Yaml::Seq(steps) = field(attest, "steps") else {
        panic!("The attestation steps are necessary.");
    };
    let download = steps
        .iter()
        .find(|step| {
            optional_field(step, "uses") == Some(&Yaml::str(crate::steps::DOWNLOAD_ARTIFACT_USES))
        })
        .expect("The artifact download step is not available.");
    let inputs = field(download, "with");
    assert_eq!(
        field(inputs, "artifact-ids"),
        &Yaml::str("${{ needs.build-image-candidate.outputs.artifact_id }}")
    );
    assert!(optional_field(inputs, "name").is_none());
    assert!(steps.iter().any(|step| {
        optional_field(step, "name") == Some(&Yaml::str("Examine the uploaded artifact ID"))
    }));
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
    let image =
        ValidatedImageReference::parse(&format!("ghcr.io/team/job@sha256:{}", "a".repeat(64)))
            .expect("Read the image reference.");
    Schema2WorkflowRequest {
        version: "0.1.19".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: scale_set.clone(),
        routing: WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, scale_set, Some(image)),
        workflows: BTreeSet::from([RoutingWorkflow::ImageRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(test_pins()),
    }
}

fn lock_text(sha256: &str) -> String {
    format!(
        "buildx_version = \"0.38.0\"\nbuildx_asset_url = \"https://github.com/docker/buildx/releases/download/v0.38.0/buildx-v0.38.0.linux-amd64\"\nbuildx_sha256 = \"{sha256}\"\n"
    )
}

fn run_installer(
    workspace: &std::path::Path,
    runner_temp: &std::path::Path,
    fake_bin: &std::path::Path,
) -> std::process::Output {
    use std::process::Command;

    let path = std::env::join_paths(std::iter::once(fake_bin.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH is available.")),
    ))
    .expect("The test path cannot be joined.");
    Command::new("bash")
        .args(["-e", "-o", "pipefail", "-c", super::BUILDX_INSTALL])
        .env("GITHUB_WORKSPACE", workspace)
        .env("RUNNER_TEMP", runner_temp)
        .env("PATH", path)
        .output()
        .expect("Run the test installer.")
}

fn temporary_directory(label: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};

    std::env::temp_dir().join(format!(
        "velnor-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time is later than the epoch.")
            .as_nanos()
    ))
}

fn test_pins() -> super::super::ProductReleasePins {
    product_release_test_pins::test_pins()
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
