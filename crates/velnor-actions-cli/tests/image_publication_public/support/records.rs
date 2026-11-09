// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::Path,
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    IMAGE_CANDIDATE_SCHEMA, IMAGE_CANDIDATE_SOURCE_INPUTS, ImageCandidateArchive,
    ImageCandidateBuild, ImageCandidateOci, ImageCandidateRecord, ImageCandidateSource,
    ImageCandidateWorkflow, RUNNER_IMAGE_PUBLICATION_SCHEMA, RunnerImagePublicationLock,
    RunnerImagePublicationRecord, RunnerImagePublicationSource, native_image_candidate_inputs,
    runner_publication_source_inputs, serialize_image_candidate_record,
};

pub(super) use super::encoding::encode_base64;
use super::{FixtureProfile, oci};
#[path = "records/qualification.rs"]
mod qualification;
pub(super) use qualification::{qualification_attestation, qualification_proof};
#[path = "records/attestation.rs"]
mod attestation;
pub(super) use attestation::{AttestationInput, attestation, workflow_uri};

pub(super) const REPOSITORY: &str = "tailrocks/velnor-new";
pub(super) const QUALIFICATION_ATTEST_WORKFLOW: &str =
    ".github/workflows/runner-image-qualification-attest.yml";
const CI_WORKFLOW: &str = ".github/workflows/ci.yml";
const JOB_ATTESTER_WORKFLOW: &str = ".github/workflows/job-image-candidate.yml";

#[derive(Clone, Copy)]
pub(super) struct FixtureHeads<'a> {
    pub(super) source: &'a str,
    pub(super) authority_sha: &'a str,
    pub(super) current_main_sha: &'a str,
}

#[derive(Clone, Copy)]
pub(super) struct RecordInput<'a> {
    pub(super) profile: FixtureProfile,
    pub(super) source: &'a str,
    pub(super) authority_sha: &'a str,
    pub(super) inputs: &'a BTreeMap<String, String>,
    pub(super) archive_sha: &'a str,
    pub(super) image: &'a oci::Layout,
    pub(super) source_files: &'a BTreeMap<String, Vec<u8>>,
    pub(super) run_attempt: u32,
}

struct RunnerRecordInput<'a> {
    profile: FixtureProfile,
    source: &'a str,
    inputs: &'a BTreeMap<String, String>,
    archive: ImageCandidateArchive,
    oci: ImageCandidateOci,
    workflow: ImageCandidateWorkflow,
    source_files: &'a BTreeMap<String, Vec<u8>>,
    provenance: &'a velnor_actions_contract::RunnerImagePublicationProvenance,
}

pub(super) fn source_files(
    profile: FixtureProfile,
) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let semantic = IMAGE_CANDIDATE_SOURCE_INPUTS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let paths: Vec<&str> = match profile {
        FixtureProfile::Job => native_image_candidate_inputs().to_vec(),
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            runner_publication_source_inputs()?.to_vec()
        }
    };
    paths
        .iter()
        .map(|path| {
            let bytes = if profile == FixtureProfile::Job && !semantic.contains(path) {
                format!("fixture source: {path}\n").into_bytes()
            } else {
                fs::read(root.join(path))
                    .map_err(|error| format!("fixture input {path}: {error}"))?
            };
            Ok(((*path).to_owned(), bytes))
        })
        .collect()
}

pub(super) fn make_record(input: RecordInput<'_>) -> Result<Vec<u8>, Box<dyn Error>> {
    let RecordInput {
        profile,
        source,
        authority_sha,
        inputs,
        archive_sha,
        image,
        source_files,
        run_attempt,
    } = input;
    let archive = ImageCandidateArchive {
        sha256: archive_sha.to_owned(),
        size_bytes: image.archive.len() as u64,
    };
    let workflow = ImageCandidateWorkflow {
        repository: REPOSITORY.to_owned(),
        repository_id: 1_390_620_900,
        event: "push".to_owned(),
        workflow_ref: format!("{REPOSITORY}/{CI_WORKFLOW}@refs/heads/main"),
        authority_commit: authority_sha.to_owned(),
        run_id: profile.ci_run_id(),
        run_attempt: u64::from(run_attempt),
    };
    let oci = image.identity.clone();
    match profile {
        FixtureProfile::Job => {
            let record = ImageCandidateRecord {
                schema: IMAGE_CANDIDATE_SCHEMA.to_owned(),
                source: ImageCandidateSource {
                    commit: source.to_owned(),
                    source_ref: "refs/heads/main".to_owned(),
                    inputs: inputs
                        .iter()
                        .filter(|(path, _)| IMAGE_CANDIDATE_SOURCE_INPUTS.contains(&path.as_str()))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                },
                workflow,
                platform: "linux/amd64".to_owned(),
                build: ImageCandidateBuild {
                    builder: "github.com/docker/buildx v0.38.0".to_owned(),
                    platform: "linux/amd64".to_owned(),
                    arguments: Vec::new(),
                    exporter: "oci".to_owned(),
                    provenance: "disabled".to_owned(),
                    compression: "gzip".to_owned(),
                    force_compression: true,
                },
                archive,
                oci,
            };
            Ok(serialize_image_candidate_record(&record)?)
        }
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            runner_record(RunnerRecordInput {
                profile,
                source,
                inputs,
                archive,
                oci,
                workflow,
                source_files,
                provenance: image
                    .provenance
                    .as_ref()
                    .ok_or("runner provenance missing")?,
            })
        }
    }
}

fn runner_record(input: RunnerRecordInput<'_>) -> Result<Vec<u8>, Box<dyn Error>> {
    let RunnerRecordInput {
        profile,
        source,
        inputs,
        archive,
        oci,
        workflow,
        source_files,
        provenance,
    } = input;
    let lock_bytes = source_files
        .get(".velnor/runner.lock")
        .ok_or("runner lock fixture source missing")?;
    let lock_text = std::str::from_utf8(lock_bytes.as_slice())?;
    let lock_values: toml::Value = toml::from_str(lock_text)?;
    let record = RunnerImagePublicationRecord {
        schema: RUNNER_IMAGE_PUBLICATION_SCHEMA.to_owned(),
        image: profile.name().to_owned(),
        source: RunnerImagePublicationSource {
            commit: source.to_owned(),
            source_ref: "refs/heads/main".to_owned(),
            inputs: inputs.clone(),
        },
        workflow,
        platform: "linux/amd64".to_owned(),
        archive,
        oci,
        lock: RunnerImagePublicationLock {
            sha256: hex(&Sha256::digest(lock_bytes.as_slice())),
            runner_version: lock_values["runner_version"]
                .as_str()
                .ok_or("runner lock version")?
                .to_owned(),
            docker_version: lock_values["docker_version"]
                .as_str()
                .ok_or("docker lock version")?
                .to_owned(),
            buildx_version: lock_values["buildx_version"]
                .as_str()
                .ok_or("buildx lock version")?
                .to_owned(),
            compose_version: lock_values["compose_version"]
                .as_str()
                .ok_or("compose lock version")?
                .to_owned(),
            node_version: lock_values["node_version"]
                .as_str()
                .ok_or("node lock version")?
                .to_owned(),
        },
        provenance: provenance.clone(),
    };
    Ok(serde_json::to_vec(&record)?)
}

pub(super) fn state_value(
    profile: FixtureProfile,
    heads: FixtureHeads<'_>,
    source_files: BTreeMap<String, Vec<u8>>,
    attestation: &Value,
    qualification_attestation: Option<&Value>,
    run_attempt: u32,
    artifact_id: u64,
) -> Value {
    let FixtureHeads {
        source,
        authority_sha,
        current_main_sha: main_sha,
    } = heads;
    let run_id = profile.ci_run_id();
    let producer = json!({"id":run_id,"run_attempt":run_attempt,"event":"push","path":CI_WORKFLOW,"head_branch":"main","head_sha":authority_sha,
        "head_repository":{"id":1_390_620_900,"full_name":REPOSITORY},"status":"completed","conclusion":"success"});
    let artifact_name = match profile {
        FixtureProfile::Job => format!("velnor-job-image-candidate-{run_attempt}"),
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            format!("image-assets-{run_attempt}")
        }
    };
    let artifact = json!({"id":artifact_id,"name":artifact_name,"expired":false,"size_in_bytes":4096,
        "workflow_run":{"id":run_id,"head_sha":authority_sha,"head_branch":"main"}});
    let ci_run = json!({"id":profile.ci_run_id(),"run_number":23,"run_attempt":run_attempt,"path":".github/workflows/ci.yml","event":"push","head_branch":"main","head_sha":source,
        "head_repository":{"id":1_390_620_900,"full_name":REPOSITORY},"status":"completed","conclusion":"success"});
    let publisher = json!({"id":profile.publisher_run_id(),"run_attempt":1,"event":"push","path":profile.publication_workflow_path(),"head_branch":"main","head_sha":source,
        "head_repository":{"id":1_390_620_900,"full_name":REPOSITORY},"status":"in_progress","conclusion":null});
    let qualifier = json!({"id":profile.qualification_run_id(),"run_attempt":1,"event":"push","path":".github/workflows/runner-image-qualification.yml","head_branch":"main","head_sha":source,
        "head_repository":{"id":1_390_620_900,"full_name":REPOSITORY},"status":"completed","conclusion":"success",
        "referenced_workflows":[{"path":format!("{REPOSITORY}/{QUALIFICATION_ATTEST_WORKFLOW}@{source}"),"ref":"refs/heads/main","sha":source}]});
    let attestor = json!({"id":profile.job_attestor_run_id(),"run_attempt":run_attempt,"event":"push","path":JOB_ATTESTER_WORKFLOW,"head_branch":"main","head_sha":source,
        "head_repository":{"id":1_390_620_900,"full_name":REPOSITORY},"status":"completed","conclusion":"success"});
    let qualifier_jobs = json!([{"id":profile.qualification_job_id(),"name":"qualify-images","status":"completed","conclusion":"success","head_sha":source}]);
    let ci_jobs = json!([
        {"id":profile.ci_run_id() + 1,"run_id":profile.ci_run_id(),"name":"Build","head_sha":source,"status":"completed","conclusion":"success"},
        {"id":profile.ci_run_id() + 2,"run_id":profile.ci_run_id(),"name":"Required","head_sha":source,"status":"completed","conclusion":"success"}
    ]);
    let jobs = match profile {
        FixtureProfile::Job => json!([
            {"id":profile.job_attestor_run_id() + 1,"name":"Build Linux job image candidate","status":"completed","conclusion":"success"},
            {"id":profile.job_attestor_run_id() + 2,"name":"Attest Linux job image candidate","status":"completed","conclusion":"success"}
        ]),
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => json!([
            {"name":"Run runner images release graph / Build runner images","status":"completed","conclusion":"success"},
            {"name":"Run runner images release graph / Attest runner images","status":"completed","conclusion":"success"},
            {"name":"Run runner images release graph / qualify-images","status":"completed","conclusion":"success"}
        ]),
    };
    let tag_reference = format!("{}:sha-{source}", profile.repository());
    let source_files = source_files
        .into_iter()
        .map(|(path, bytes)| (path, encode_base64(&bytes)))
        .collect::<BTreeMap<_, _>>();
    json!({"repository":{"id":1_390_620_900,"full_name":REPOSITORY,"default_branch":"main"},
        "source_commit":source,"main_sha":main_sha,"source_files":source_files,"producer":producer,"requested_attempt":run_attempt,"artifacts":[artifact],
        "jobs":jobs,"attestor":attestor,"ci_run":ci_run,"ci_jobs":ci_jobs,"ci_job_pages":null,"publisher":publisher,"qualifier":qualifier,"qualifier_jobs":qualifier_jobs,"attestation":attestation,"qualification_attestation":qualification_attestation,
        "crane":{"tag_reference":tag_reference,"tag_digest":null,"manifest_reference":null,"remote":null,"promotion_tags":{},"promotion_operations":[],"fail_blob_readback":false,"corrupt_remote_blob":null,"remote_manifest_response_corruption":null}})
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}
