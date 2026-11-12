// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{Fixture, FixtureProfile, hex};
mod apply;
mod attestation;
mod authenticated_source;
pub(crate) use attestation::{
    current_attestation, invalid_invocation_uri, with_invocation, with_uri,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum AttemptArtifactCase {
    OlderAndCurrent,
    OlderOnly,
    PreviousArtifactId,
    UnqualifiedCurrentName,
    DuplicateCurrentName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RejectionCase {
    WrongRepository,
    WrongSelectedSource,
    WrongCallerWorkflow,
    WrongReusableWorkflow,
    WrongRunAttempt,
    WrongArtifact,
    FailedLatestCi,
    StaleLatestCi,
    WrongAttestationDigest,
    MissingAttestationSubject,
    WrongProofEnvelopeDigest,
    WrongProofContextDigest,
    WrongRunnerLock,
    FailedQualification,
    BareRunnerJobName,
    WrongRunnerJobCallerPrefix,
    DuplicateRunnerJobName,
    FailedRunnerJob,
    ArchiveSubstitution,
    WrongProducerEvent,
    WrongProducerRef,
    WrongProducerHead,
    WrongQualifierSource,
    WrongQualifierRef,
    WrongQualifierWorkflow,
    WrongQualifierSignerPath,
    WrongQualifierSignerRef,
    WrongQualifierSignerSha,
    WrongQualifierAttempt,
    WrongQualifierJobId,
    FailedQualifierJob,
    WrongProofSigner,
    WrongProofSubjectDigest,
    WrongProofSource,
    WrongProofRef,
    WrongProofLock,
    WrongProofArchiveDigest,
    WrongProofArchiveSize,
    WrongProofOciIndex,
    WrongProofPlatform,
    WrongProofConfiguration,
    WrongProofLayer,
}

impl RejectionCase {
    pub(crate) const fn profile(self) -> FixtureProfile {
        match self {
            Self::WrongRepository
            | Self::WrongRunAttempt
            | Self::WrongAttestationDigest
            | Self::MissingAttestationSubject => FixtureProfile::Job,
            Self::WrongSelectedSource
            | Self::WrongCallerWorkflow
            | Self::WrongReusableWorkflow
            | Self::WrongArtifact
            | Self::FailedLatestCi
            | Self::StaleLatestCi
            | Self::WrongRunnerLock
            | Self::FailedQualification
            | Self::BareRunnerJobName
            | Self::WrongRunnerJobCallerPrefix
            | Self::DuplicateRunnerJobName
            | Self::FailedRunnerJob
            | Self::ArchiveSubstitution
            | Self::WrongProducerEvent
            | Self::WrongProducerRef
            | Self::WrongProducerHead
            | Self::WrongQualifierSource
            | Self::WrongQualifierRef
            | Self::WrongQualifierWorkflow
            | Self::WrongQualifierSignerPath
            | Self::WrongQualifierSignerRef
            | Self::WrongQualifierSignerSha
            | Self::WrongQualifierAttempt
            | Self::WrongQualifierJobId
            | Self::FailedQualifierJob
            | Self::WrongProofSigner
            | Self::WrongProofSubjectDigest
            | Self::WrongProofSource
            | Self::WrongProofRef
            | Self::WrongProofLock
            | Self::WrongProofArchiveDigest
            | Self::WrongProofArchiveSize
            | Self::WrongProofOciIndex
            | Self::WrongProofPlatform
            | Self::WrongProofConfiguration
            | Self::WrongProofLayer => FixtureProfile::Runner,
            Self::WrongProofEnvelopeDigest | Self::WrongProofContextDigest => {
                FixtureProfile::Runner
            }
        }
    }
}

impl Fixture {
    pub(crate) fn corrupt_job_closure_attestation(&self) -> Result<(), Box<dyn Error>> {
        self.update_state(|state| {
            alter_subject_digest(state, "job-image-native-source-closure.json");
        })
    }

    pub(crate) fn attestation_output(&self) -> Result<Value, Box<dyn Error>> {
        let state = self.read_state()?;
        state
            .get("attestation")
            .cloned()
            .ok_or_else(|| "fixture has no attestation output".into())
    }

    pub(crate) fn qualification_attestation_output(&self) -> Result<Value, Box<dyn Error>> {
        let state = self.read_state()?;
        state
            .get("qualification_attestation")
            .cloned()
            .filter(Value::is_array)
            .ok_or_else(|| "fixture has no qualification attestation output".into())
    }

    pub(crate) fn set_attestation_output(&self, output: Value) -> Result<(), Box<dyn Error>> {
        let mut state = self.read_state()?;
        state["attestation_response"] = output;
        self.write_state(&state)
    }

    pub(crate) fn set_attestation_pages(&self, pages: Value) -> Result<(), Box<dyn Error>> {
        let mut state = self.read_state()?;
        state["attestation_pages"] = pages;
        state
            .as_object_mut()
            .ok_or("fixture state is not an object")?
            .remove("attestation_response");
        self.write_state(&state)
    }

    pub(crate) fn configure_attempt_two_artifacts(
        &mut self,
        case: AttemptArtifactCase,
    ) -> Result<(), Box<dyn Error>> {
        if self.profile != FixtureProfile::Runner || self.producer_run_attempt != 2 {
            return Err("attempt-two artifact fixture requires the Runner profile".into());
        }
        let mut state = self.read_state()?;
        let current = state["artifacts"]
            .as_array()
            .and_then(|artifacts| artifacts.first())
            .cloned()
            .ok_or("fixture has no producer artifact")?;
        let current_id = current["id"].as_u64().ok_or("fixture artifact has no id")?;
        let previous_id = current_id
            .checked_sub(1)
            .ok_or("fixture current artifact id cannot identify its predecessor")?;
        let mut previous = current.clone();
        previous["id"] = json!(previous_id);
        previous["name"] = json!("image-assets-1");
        let mut current = current;
        current["name"] = json!("image-assets-2");
        let artifacts = match case {
            AttemptArtifactCase::OlderAndCurrent => vec![previous, current],
            AttemptArtifactCase::OlderOnly => vec![previous],
            AttemptArtifactCase::PreviousArtifactId => {
                self.artifact_id = previous_id;
                vec![previous, current]
            }
            AttemptArtifactCase::UnqualifiedCurrentName => {
                let mut unqualified = current;
                unqualified["name"] = json!("image-assets");
                vec![previous, unqualified]
            }
            AttemptArtifactCase::DuplicateCurrentName => {
                let mut duplicate = current.clone();
                duplicate["id"] = json!(
                    current_id
                        .checked_add(1)
                        .ok_or("fixture artifact id overflow")?
                );
                vec![previous, current, duplicate]
            }
        };
        state["artifacts"] = json!(artifacts);
        self.write_state(&state)
    }

    pub(crate) fn artifact_ids(&self) -> Result<Vec<u64>, Box<dyn Error>> {
        let state = self.read_state()?;
        state["artifacts"]
            .as_array()
            .ok_or("fixture artifact list is missing")?
            .iter()
            .map(|artifact| {
                artifact["id"]
                    .as_u64()
                    .ok_or_else(|| "fixture artifact ID is invalid".into())
            })
            .collect()
    }

    pub(crate) fn set_source_response(
        &self,
        path: &str,
        bytes: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        let mut state = self.read_state()?;
        state["source_files"][path] = json!(super::records::encode_base64(bytes));
        self.write_state(&state)
    }

    pub(crate) fn preseed_registry_state(&self) -> Result<Value, Box<dyn Error>> {
        let bytes: &[u8] = b"preexisting unrelated registry blob";
        let digest = format!("sha256:{}", hex(&Sha256::digest(bytes)));
        let path = format!("blobs/sha256/{}", digest.trim_start_matches("sha256:"));
        let mut state = self.read_state()?;
        state["crane"]["foreign_tags"] = json!({
            "ghcr.io/tailrocks/velnor-preserved:keep": digest.clone()
        });
        state["crane"]["foreign_inventory"] = json!({
            (path): {"digest":digest,"size":bytes.len(),"bytes":bytes}
        });
        self.write_state(&state)?;
        self.registry_state()
    }

    pub(crate) fn registry_state(&self) -> Result<Value, Box<dyn Error>> {
        let state = self.read_state()?;
        let crane = &state["crane"];
        Ok(json!({
            "tag_reference":crane["tag_reference"],
            "tag_digest":crane["tag_digest"],
            "manifest_reference":crane["manifest_reference"],
            "remote":crane["remote"],
            "foreign_tags":crane["foreign_tags"],
            "foreign_inventory":crane["foreign_inventory"]
        }))
    }

    fn update_state(&self, mutate: impl FnOnce(&mut Value)) -> Result<(), Box<dyn Error>> {
        let mut state = self.read_state()?;
        mutate(&mut state);
        self.write_state(&state)
    }

    fn update_record(&self, mutate: impl FnOnce(&mut Value)) -> Result<(), Box<dyn Error>> {
        let mut record: Value = serde_json::from_slice(&fs::read(&self.record)?)?;
        mutate(&mut record);
        fs::write(&self.record, serde_json::to_vec(&record)?)?;
        Ok(())
    }

    fn update_qualification_proof(
        &self,
        mutate: impl FnOnce(&mut Value),
    ) -> Result<(), Box<dyn Error>> {
        let path = self
            .qualification_proof
            .as_ref()
            .ok_or("fixture has no qualification proof")?;
        let mut proof: Value = serde_json::from_slice(&fs::read(path)?)?;
        mutate(&mut proof);
        let bytes = serde_json::to_vec(&proof)?;
        fs::write(path, &bytes)?;
        let digest = hex(&Sha256::digest(bytes));
        self.update_state(|state| {
            state["qualification_attestation"][0]["verificationResult"]["statement"]["subject"]
                [0]["digest"]["sha256"] = json!(digest);
        })
    }

    pub(super) fn read_state(&self) -> Result<Value, Box<dyn Error>> {
        Ok(serde_json::from_slice(&fs::read(&self.state)?)?)
    }

    pub(super) fn write_state(&self, state: &Value) -> Result<(), Box<dyn Error>> {
        fs::write(&self.state, serde_json::to_vec(state)?)?;
        Ok(())
    }
}

fn ci_runs(fixture: &Fixture, latest_status: &str, conclusion: Option<&str>) -> Value {
    let source = &fixture.source;
    let repository = json!({"id":1_390_620_900,"full_name":"tailrocks/velnor-new"});
    let earlier = json!({"id":40_613_400_010i64,"run_number":22,"run_attempt":1,"path":".github/workflows/ci.yml","event":"push","head_branch":"main","head_sha":source,
        "head_repository":repository,"status":"completed","conclusion":"success"});
    let latest = json!({"id":40_613_400_011i64,"run_number":23,"run_attempt":1,"path":".github/workflows/ci.yml","event":"push","head_branch":"main","head_sha":source,
        "head_repository":repository,"status":latest_status,"conclusion":conclusion});
    json!([earlier, latest])
}

fn alter_subject_digest(state: &mut Value, name: &str) {
    if let Some(subject) = state["attestation"][0]["verificationResult"]["statement"]["subject"]
        .as_array_mut()
        .and_then(|subjects| subjects.iter_mut().find(|subject| subject["name"] == name))
    {
        subject["digest"]["sha256"] = json!("f".repeat(64));
    }
}

fn remove_subject(state: &mut Value, name: &str) {
    if let Some(subjects) =
        state["attestation"][0]["verificationResult"]["statement"]["subject"].as_array_mut()
    {
        subjects.retain(|subject| subject["name"] != name);
    }
}
