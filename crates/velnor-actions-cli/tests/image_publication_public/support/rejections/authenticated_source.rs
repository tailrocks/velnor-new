// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Fixture, hex};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, fs};

impl Fixture {
    pub(crate) fn replace_authenticated_runner_lock(
        &self,
        bytes: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        let path = ".velnor/runner.lock";
        self.set_source_response(path, bytes)?;
        let lock: toml::Value = toml::from_str(std::str::from_utf8(bytes)?)?;
        let digest = hex(&Sha256::digest(bytes));
        let mut record: Value = serde_json::from_slice(&fs::read(&self.record)?)?;
        record["source"]["inputs"][path] = json!(digest);
        record["lock"]["sha256"] = json!(digest);
        for field in [
            "runner_version",
            "docker_version",
            "buildx_version",
            "compose_version",
            "node_version",
        ] {
            record["lock"][field] = json!(lock[field].as_str().ok_or("missing lock version")?);
        }
        let bytes = serde_json::to_vec(&record)?;
        fs::write(&self.record, &bytes)?;
        let mut proof: Value = serde_json::from_slice(&fs::read(
            self.qualification_proof
                .as_ref()
                .ok_or("Runner qualification proof is missing")?,
        )?)?;
        proof["runner_lock_sha256"] = json!(digest);
        proof["images"][self.profile.name()]["record_sha256"] = json!(hex(&Sha256::digest(&bytes)));
        let proof_bytes = serde_json::to_vec(&proof)?;
        fs::write(
            self.qualification_proof
                .as_ref()
                .ok_or("Runner qualification proof is missing")?,
            &proof_bytes,
        )?;
        let mut state = self.read_state()?;
        let subjects = state["attestation"][0]["verificationResult"]["statement"]["subject"]
            .as_array_mut()
            .ok_or("subjects missing")?;
        let subject = subjects
            .iter_mut()
            .find(|subject| subject["name"] == self.profile.record_name())
            .ok_or("record subject missing")?;
        subject["digest"]["sha256"] = json!(hex(&Sha256::digest(&bytes)));
        state["qualification_attestation"][0]["verificationResult"]["statement"]["subject"][0]["digest"]
            ["sha256"] = json!(hex(&Sha256::digest(&proof_bytes)));
        self.write_state(&state)
    }

    pub(crate) fn replace_authenticated_source(
        &self,
        path: &str,
        bytes: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        self.set_source_response(path, bytes)?;
        let mut record: Value = serde_json::from_slice(&fs::read(&self.record)?)?;
        record["source"]["inputs"][path] = json!(hex(&Sha256::digest(bytes)));
        let record_bytes = serde_json::to_vec(&record)?;
        fs::write(&self.record, &record_bytes)?;
        let record_digest = hex(&Sha256::digest(&record_bytes));
        let mut state = self.read_state()?;
        let subjects = state["attestation"][0]["verificationResult"]["statement"]["subject"]
            .as_array_mut()
            .ok_or("fixture attestation has no subjects")?;
        let subject = subjects
            .iter_mut()
            .find(|subject| subject["name"] == self.profile.record_name())
            .ok_or("fixture attestation has no publication record subject")?;
        subject["digest"]["sha256"] = json!(record_digest);
        self.write_state(&state)?;
        self.update_qualification_proof(|proof| {
            proof["images"][self.profile.name()]["record_sha256"] = json!(record_digest);
        })
    }
}
