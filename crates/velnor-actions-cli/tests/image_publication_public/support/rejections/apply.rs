// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use serde_json::json;

use super::{Fixture, RejectionCase, alter_subject_digest, ci_runs, remove_subject};

impl Fixture {
    pub(crate) fn apply_rejection(&self, case: RejectionCase) -> Result<(), Box<dyn Error>> {
        if self.apply_legacy_rejection(case)?
            || self.apply_run_identity_rejection(case)?
            || self.apply_proof_rejection(case)?
        {
            Ok(())
        } else {
            Err("unknown public rejection case".into())
        }
    }

    fn apply_legacy_rejection(&self, case: RejectionCase) -> Result<bool, Box<dyn Error>> {
        match case {
            RejectionCase::WrongRepository => self.update_state(|state| {
                state["repository"]["id"] = json!(1);
            })?,
            RejectionCase::WrongSelectedSource => self.update_record(|record| {
                record["source"]["commit"] = json!("c".repeat(40));
                record["workflow"]["authority_commit"] = json!("c".repeat(40));
            })?,
            RejectionCase::WrongCallerWorkflow => self.update_state(|state| {
                state["producer"]["path"] = json!(".github/workflows/untrusted.yml");
            })?,
            RejectionCase::WrongReusableWorkflow => self.update_state(|state| {
                state["qualifier"]["referenced_workflows"][0]["path"] =
                    json!("tailrocks/velnor-new/.github/workflows/untrusted.yml");
            })?,
            RejectionCase::WrongRunAttempt => self.update_state(|state| {
                state["producer"]["run_attempt"] = json!(2);
            })?,
            RejectionCase::WrongArtifact => self.update_state(|state| {
                state["artifacts"][0]["id"] = json!(987_654_329_u64);
            })?,
            RejectionCase::FailedLatestCi => self.update_state(|state| {
                state["ci_runs"] = ci_runs(self, "completed", Some("failure"));
            })?,
            RejectionCase::StaleLatestCi => self.update_state(|state| {
                state["ci_runs"] = ci_runs(self, "queued", None);
            })?,
            RejectionCase::WrongAttestationDigest => self.update_state(|state| {
                alter_subject_digest(state, self.profile.archive_name());
            })?,
            RejectionCase::MissingAttestationSubject => self.update_state(|state| {
                remove_subject(state, self.profile.record_name());
            })?,
            RejectionCase::WrongRunnerLock => self.update_record(|record| {
                record["lock"]["sha256"] = json!("e".repeat(64));
            })?,
            RejectionCase::FailedQualification => self.update_state(|state| {
                state["qualifier"]["conclusion"] = json!("failure");
            })?,
            RejectionCase::BareRunnerJobName => self.update_state(|state| {
                state["qualifier_jobs"][0]["name"] = json!("Run runner images / qualify-images");
            })?,
            RejectionCase::WrongRunnerJobCallerPrefix => self.update_state(|state| {
                state["qualifier"]["path"] = json!(".github/workflows/untrusted.yml");
            })?,
            RejectionCase::DuplicateRunnerJobName => self.update_state(|state| {
                let mut jobs = state["qualifier_jobs"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if let Some(first) = jobs.first().cloned() {
                    jobs.push(first);
                }
                state["qualifier_jobs"] = json!(jobs);
            })?,
            RejectionCase::FailedRunnerJob => self.update_state(|state| {
                state["qualifier_jobs"][0]["conclusion"] = json!("failure");
            })?,
            RejectionCase::ArchiveSubstitution => {
                let mut archive = fs::read(&self.archive)?;
                archive.push(0);
                fs::write(&self.archive, archive)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn apply_run_identity_rejection(&self, case: RejectionCase) -> Result<bool, Box<dyn Error>> {
        match case {
            RejectionCase::WrongProducerEvent => self.update_state(|state| {
                state["producer"]["event"] = json!("workflow_dispatch");
            })?,
            RejectionCase::WrongProducerRef => self.update_state(|state| {
                state["producer"]["head_branch"] = json!("release");
            })?,
            RejectionCase::WrongProducerHead => self.update_state(|state| {
                state["producer"]["head_sha"] = json!("c".repeat(40));
            })?,
            RejectionCase::WrongQualifierSource => self.update_state(|state| {
                state["qualifier"]["head_sha"] = json!("c".repeat(40));
            })?,
            RejectionCase::WrongQualifierRef => self.update_state(|state| {
                state["qualifier"]["head_branch"] = json!("release");
            })?,
            RejectionCase::WrongQualifierWorkflow => self.update_state(|state| {
                state["qualifier"]["path"] = json!(".github/workflows/untrusted.yml");
            })?,
            RejectionCase::WrongQualifierSignerPath => self.update_state(|state| {
                state["qualifier"]["referenced_workflows"][0]["path"] = json!(format!(
                    "{}/.github/workflows/untrusted.yml@{}",
                    "tailrocks/velnor-new", self.source
                ));
            })?,
            RejectionCase::WrongQualifierSignerRef => self.update_state(|state| {
                state["qualifier"]["referenced_workflows"][0]["ref"] = json!("refs/tags/v1.0.0");
            })?,
            RejectionCase::WrongQualifierSignerSha => self.update_state(|state| {
                state["qualifier"]["referenced_workflows"][0]["sha"] = json!("c".repeat(40));
            })?,
            RejectionCase::WrongQualifierAttempt => self.update_state(|state| {
                state["qualifier_response_run_attempt"] = json!(2);
            })?,
            RejectionCase::WrongQualifierJobId => {
                self.update_qualification_proof(|proof| {
                    proof["qualifier_job_id"] = json!(761_999_999_u64);
                })?;
            }
            RejectionCase::FailedQualifierJob => self.update_state(|state| {
                state["qualifier_jobs"][0]["conclusion"] = json!("failure");
            })?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn apply_proof_rejection(&self, case: RejectionCase) -> Result<bool, Box<dyn Error>> {
        match case {
            RejectionCase::WrongProofSigner => self.update_state(|state| {
                state["qualification_attestation"][0]["verificationResult"]["signature"]["certificate"]["buildSignerURI"] =
                    json!("https://github.com/tailrocks/velnor-new/.github/workflows/untrusted.yml@refs/heads/main");
                state["qualification_attestation"][0]["verificationResult"]["signature"]["certificate"]["subjectAlternativeName"] =
                    json!("https://github.com/tailrocks/velnor-new/.github/workflows/untrusted.yml@refs/heads/main");
            })?,
            RejectionCase::WrongProofSubjectDigest => self.update_state(|state| {
                state["qualification_attestation"][0]["verificationResult"]["statement"]["subject"][0]["digest"]["sha256"] =
                    json!("e".repeat(64));
            })?,
            RejectionCase::WrongProofSource => self.update_qualification_proof(|proof| {
                proof["source_commit"] = json!("c".repeat(40));
            })?,
            RejectionCase::WrongProofRef => self.update_qualification_proof(|proof| {
                proof["source_ref"] = json!("refs/heads/other");
            })?,
            RejectionCase::WrongProofLock => self.update_qualification_proof(|proof| {
                proof["runner_lock_sha256"] = json!("e".repeat(64));
            })?,
            RejectionCase::WrongProofArchiveDigest => self.update_qualification_proof(|proof| {
                proof["images"][self.profile.name()]["archive"]["sha256"] = json!("e".repeat(64));
            })?,
            RejectionCase::WrongProofArchiveSize => self.update_qualification_proof(|proof| {
                let size = proof["images"][self.profile.name()]["archive"]["size_bytes"]
                    .as_u64()
                    .unwrap_or_default();
                proof["images"][self.profile.name()]["archive"]["size_bytes"] = json!(size + 1);
            })?,
            RejectionCase::WrongProofOciIndex => self.update_qualification_proof(|proof| {
                proof["images"][self.profile.name()]["oci"]["archive_index_digest"] =
                    json!("e".repeat(64));
            })?,
            RejectionCase::WrongProofPlatform => self.update_qualification_proof(|proof| {
                proof["images"][self.profile.name()]["oci"]["platform_manifest_digest"] =
                    json!(format!("sha256:{}", "e".repeat(64)));
            })?,
            RejectionCase::WrongProofConfiguration => self.update_qualification_proof(|proof| {
                proof["images"][self.profile.name()]["oci"]["image_configuration_digest"] =
                    json!(format!("sha256:{}", "e".repeat(64)));
            })?,
            RejectionCase::WrongProofLayer => self.update_qualification_proof(|proof| {
                proof["images"][self.profile.name()]["oci"]["layer_digests"][0] =
                    json!(format!("sha256:{}", "e".repeat(64)));
            })?,
            _ => return Ok(false),
        }
        Ok(true)
    }
}
