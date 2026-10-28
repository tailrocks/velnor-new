// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error, fs, path::Path};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    IMAGE_CANDIDATE_SCHEMA, IMAGE_CANDIDATE_SOURCE_INPUTS, ImageCandidateArchive,
    ImageCandidateBuild, ImageCandidateOci, ImageCandidateRecord, ImageCandidateSource,
    ImageCandidateWorkflow, RUNNER_IMAGE_PUBLICATION_SCHEMA, RUNNER_PUBLICATION_SOURCE_INPUTS,
    RunnerImagePublicationLock, RunnerImagePublicationQualification, RunnerImagePublicationRecord,
    RunnerImagePublicationReferencedWorkflow, RunnerImagePublicationSource,
    serialize_image_candidate_record,
};

use super::{FixtureProfile, oci};

const REPOSITORY: &str = "tailrocks/velnor-new";
const RUN_ATTEMPT: u32 = 1;
const CALLER_WORKFLOW: &str = ".github/workflows/product-release.yml";
const CALLEE_WORKFLOW: &str = ".github/workflows/product-release-images.yml";

struct RunnerRecordInput<'a> {
    profile: FixtureProfile,
    source: &'a str,
    authority_sha: &'a str,
    inputs: &'a BTreeMap<String, String>,
    archive: ImageCandidateArchive,
    oci: ImageCandidateOci,
    workflow: ImageCandidateWorkflow,
    source_files: &'a BTreeMap<String, Vec<u8>>,
}

pub(super) fn source_files(
    profile: FixtureProfile,
) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let paths: &[&str] = match profile {
        FixtureProfile::Job => &IMAGE_CANDIDATE_SOURCE_INPUTS,
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            &RUNNER_PUBLICATION_SOURCE_INPUTS
        }
    };
    paths
        .iter()
        .map(|path| Ok(((*path).to_owned(), fs::read(root.join(path))?)))
        .collect()
}

pub(super) fn make_record(
    profile: FixtureProfile,
    source: &str,
    authority_sha: &str,
    inputs: &BTreeMap<String, String>,
    archive_sha: &str,
    image: &oci::Layout,
    source_files: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let archive = ImageCandidateArchive {
        sha256: archive_sha.to_owned(),
        size_bytes: image.archive.len() as u64,
    };
    let workflow = ImageCandidateWorkflow {
        repository: REPOSITORY.to_owned(),
        repository_id: 1_390_620_900,
        event: "workflow_dispatch".to_owned(),
        workflow_ref: format!("{REPOSITORY}/{}@refs/heads/main", profile.workflow_path()),
        authority_commit: authority_sha.to_owned(),
        run_id: profile.run_id(),
        run_attempt: u64::from(RUN_ATTEMPT),
    };
    let oci = image.identity.clone();
    match profile {
        FixtureProfile::Job => {
            let record = ImageCandidateRecord {
                schema: IMAGE_CANDIDATE_SCHEMA.to_owned(),
                source: ImageCandidateSource {
                    commit: source.to_owned(),
                    source_ref: "refs/heads/main".to_owned(),
                    inputs: inputs.clone(),
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
                authority_sha,
                inputs,
                archive,
                oci,
                workflow,
                source_files,
            })
        }
    }
}

fn runner_record(input: RunnerRecordInput<'_>) -> Result<Vec<u8>, Box<dyn Error>> {
    let RunnerRecordInput {
        profile,
        source,
        authority_sha,
        inputs,
        archive,
        oci,
        workflow,
        source_files,
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
        referenced_workflow: RunnerImagePublicationReferencedWorkflow {
            path: format!("{REPOSITORY}/{CALLEE_WORKFLOW}@{authority_sha}"),
            reference: "refs/heads/main".to_owned(),
            sha: authority_sha.to_owned(),
        },
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
        qualification: RunnerImagePublicationQualification {
            job_id: "qualify-images".to_owned(),
            run_id: profile.run_id(),
            run_attempt: RUN_ATTEMPT,
            conclusion: "success".to_owned(),
        },
    };
    Ok(serde_json::to_vec(&record)?)
}

pub(super) fn state_value(
    profile: FixtureProfile,
    source: &str,
    authority_sha: &str,
    source_files: BTreeMap<String, Vec<u8>>,
    attestation: &Value,
) -> Value {
    let run_id = profile.run_id();
    let main_sha = authority_sha;
    let producer_path = match profile {
        FixtureProfile::Job => profile.workflow_path(),
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            CALLER_WORKFLOW
        }
    };
    let referenced_workflows = match profile {
        FixtureProfile::Job => Vec::new(),
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            vec![
                json!({"path":format!("{REPOSITORY}/{CALLEE_WORKFLOW}@{authority_sha}"),"ref":"refs/heads/main","sha":authority_sha}),
            ]
        }
    };
    let producer = json!({"id":run_id,"run_attempt":RUN_ATTEMPT,"event":"workflow_dispatch","path":producer_path,"head_branch":"main","head_sha":authority_sha,
        "head_repository":{"id":1_390_620_900,"full_name":REPOSITORY},"status":"completed","conclusion":"success","referenced_workflows":referenced_workflows});
    let artifact_name = match profile {
        FixtureProfile::Job => "velnor-job-image-candidate-1",
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            "image-assets"
        }
    };
    let artifact = json!({"id":profile.artifact_id(),"name":artifact_name,"expired":false,"size_in_bytes":4096,
        "workflow_run":{"id":run_id,"head_sha":authority_sha,"head_branch":"main"}});
    let ci_run = json!({"id":profile.ci_run_id(),"run_number":23,"run_attempt":1,"path":".github/workflows/ci.yml","event":"push","head_branch":"main","head_sha":source,
        "head_repository":{"id":1_390_620_900,"full_name":REPOSITORY},"status":"completed","conclusion":"success"});
    let jobs = match profile {
        FixtureProfile::Job => json!([
            {"name":"Build Linux job image candidate","status":"completed","conclusion":"success"},
            {"name":"Attest Linux job image candidate","status":"completed","conclusion":"success"}
        ]),
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => json!([
            {"name":"Build runner images","status":"completed","conclusion":"success"},
            {"name":"Attest runner images","status":"completed","conclusion":"success"},
            {"name":"qualify-images","status":"completed","conclusion":"success"}
        ]),
    };
    let tag_reference = format!("{}:sha-{source}", profile.repository());
    let source_files = source_files
        .into_iter()
        .map(|(path, bytes)| (path, encode_base64(&bytes)))
        .collect::<BTreeMap<_, _>>();
    json!({"repository":{"id":1_390_620_900,"full_name":REPOSITORY,"default_branch":"main"},
        "source_commit":source,"main_sha":main_sha,"source_files":source_files,"producer":producer,"artifact":artifact,
        "jobs":jobs,"ci_run":ci_run,"attestation":attestation,
        "crane":{"tag_reference":tag_reference,"tag_digest":null,"manifest_reference":null,"remote":null,"fail_blob_readback":false,"corrupt_remote_blob":null,"remote_manifest_response_corruption":null}})
}

pub(super) fn attestation(
    profile: FixtureProfile,
    source: &str,
    authority_sha: &str,
    archive_sha: &str,
    record_sha: &str,
) -> Value {
    let caller_path = profile.workflow_path();
    let signer_path = match profile {
        FixtureProfile::Job => caller_path,
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            CALLEE_WORKFLOW
        }
    };
    let caller_uri = workflow_uri(caller_path);
    let signer_uri = workflow_uri(signer_path);
    let invocation = format!(
        "https://github.com/{REPOSITORY}/actions/runs/{}/attempts/{RUN_ATTEMPT}",
        profile.run_id()
    );
    let mut subjects = BTreeMap::from([
        (profile.archive_name().to_owned(), archive_sha.to_owned()),
        (profile.record_name().to_owned(), record_sha.to_owned()),
    ]);
    let extra_digest = hex(&Sha256::digest(b"fixture source closure"));
    let extra_names: &[&str] = match profile {
        FixtureProfile::Job => &["job-image-native-source-closure.json"],
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => &[
            "velnor-runner-linux-amd64.tar",
            "velnor-dind-linux-amd64.tar",
            "velnor-resource-probe-linux-amd64.tar",
            "RESOURCE_PROBE_MANIFEST.json",
            "LICENSE",
            "NOTICE",
            "SHA256SUMS",
            "runner-image-publication.json",
            "dind-image-publication.json",
            "resource-probe-image-publication.json",
        ],
    };
    for name in extra_names {
        subjects
            .entry((*name).to_owned())
            .or_insert_with(|| extra_digest.clone());
    }
    let subject_values = subjects
        .iter()
        .map(|(name, digest)| json!({"name":name,"digest":{"sha256":digest}}))
        .collect::<Vec<_>>();
    let source_digest = match profile {
        FixtureProfile::Job => source,
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            authority_sha
        }
    };
    json!([{"verificationResult":{"statement":{"predicateType":"https://slsa.dev/provenance/v1","subject":subject_values,
        "predicate":{"buildDefinition":{"internalParameters":{"github":{"event_name":"workflow_dispatch"}}}}},
        "signature":{"certificate":{"issuer":"https://token.actions.githubusercontent.com","subjectAlternativeName":signer_uri,
            "sourceRepositoryURI":format!("https://github.com/{REPOSITORY}"),"sourceRepositoryOwnerURI":"https://github.com/tailrocks","sourceRepositoryIdentifier":"1390620900",
            "sourceRepositoryDigest":source_digest,"sourceRepositoryRef":"refs/heads/main","buildSignerURI":signer_uri,"buildSignerDigest":authority_sha,
            "buildConfigURI":caller_uri,"buildConfigDigest":authority_sha,"buildTrigger":"workflow_dispatch","runInvocationURI":invocation,"runnerEnvironment":"github-hosted"}}}}])
}

fn workflow_uri(path: &str) -> String {
    format!("https://github.com/{REPOSITORY}/{path}@refs/heads/main")
}

fn hex(bytes: &[u8]) -> String {
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

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        output.push(char::from(TABLE[usize::from(first >> 2)]));
        output.push(char::from(
            TABLE[usize::from(((first & 3) << 4) | (second >> 4))],
        ));
        output.push(if chunk.len() > 1 {
            char::from(TABLE[usize::from(((second & 15) << 2) | (third >> 6))])
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            char::from(TABLE[usize::from(third & 63)])
        } else {
            '='
        });
    }
    output
}
