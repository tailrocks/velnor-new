// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{FixtureProfile, hex, oci, records};

pub(super) struct FixtureSubjects {
    pub(super) archive: PathBuf,
    pub(super) record: PathBuf,
    pub(super) qualification_proof: Option<PathBuf>,
    pub(super) qualification_attestation: Option<Value>,
    pub(super) archive_sha256: String,
    pub(super) record_sha256: String,
    pub(super) lock_sha256: String,
    pub(super) source_files: BTreeMap<String, Vec<u8>>,
}

pub(super) fn write_subjects(
    base: &Path,
    profile: FixtureProfile,
    source: &str,
    authority_sha: &str,
    image: &oci::Layout,
    run_attempt: u32,
) -> Result<FixtureSubjects, Box<dyn Error>> {
    let source_files = records::source_files(profile)?;
    let source_inputs = source_files
        .iter()
        .map(|(path, bytes)| (path.clone(), hex(&Sha256::digest(bytes))))
        .collect::<BTreeMap<_, _>>();
    let lock_sha256 = source_inputs
        .get(".velnor/runner.lock")
        .ok_or("source fixture has no Runner lock")?
        .clone();
    let archive = base.join(profile.archive_name());
    fs::write(&archive, &image.archive)?;
    let record = base.join(profile.record_name());
    let archive_sha256 = hex(&Sha256::digest(&image.archive));
    let record_bytes = records::make_record(records::RecordInput {
        profile,
        source,
        authority_sha,
        inputs: &source_inputs,
        archive_sha: &archive_sha256,
        image,
        source_files: &source_files,
        run_attempt,
    })?;
    fs::write(&record, &record_bytes)?;
    if let Some(envelope) = &image.envelope {
        fs::write(base.join(provenance_name(profile)), envelope)?;
        for (name, (_, bytes)) in &image.contexts {
            fs::write(base.join(name), bytes)?;
        }
    }
    let (qualification_proof, qualification_attestation) = match profile {
        FixtureProfile::Job => (None, None),
        FixtureProfile::Runner | FixtureProfile::Dind | FixtureProfile::ResourceProbe => {
            let proof = records::qualification_proof(
                profile,
                source,
                &lock_sha256,
                run_attempt,
                image,
                &hex(&Sha256::digest(&record_bytes)),
            )?;
            let path = base.join("RUNNER_IMAGE_QUALIFICATION.json");
            fs::write(&path, &proof)?;
            let attestation =
                records::qualification_attestation(profile, source, &hex(&Sha256::digest(&proof)));
            (Some(path), Some(attestation))
        }
    };
    Ok(FixtureSubjects {
        archive,
        record,
        qualification_proof,
        qualification_attestation,
        archive_sha256,
        record_sha256: hex(&Sha256::digest(&record_bytes)),
        lock_sha256,
        source_files,
    })
}

fn provenance_name(profile: FixtureProfile) -> &'static str {
    match profile {
        FixtureProfile::Runner => "runner-image-provenance.json",
        FixtureProfile::Dind => "dind-image-provenance.json",
        FixtureProfile::ResourceProbe => "resource-probe-image-provenance.json",
        FixtureProfile::Job => "",
    }
}
