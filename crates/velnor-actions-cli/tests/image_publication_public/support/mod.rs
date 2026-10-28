// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod manifest_outcomes;
mod oci;
mod profile;
mod real_crane;
mod records;
mod registry_state;
mod rejections;

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

pub(crate) use manifest_outcomes::{CapturedOutcome, ReplayMutation};
pub(crate) use profile::FixtureProfile;
pub(crate) use registry_state::{BlobFault, ExistingTag, MovedTag};
pub(crate) use rejections::RejectionCase;

const RUN_ATTEMPT: u32 = 1;

pub(crate) struct Fixture {
    _temp: TempDir,
    pub(crate) profile: FixtureProfile,
    pub(crate) source: String,
    pub(crate) authority_sha: String,
    pub(crate) producer_run_id: u64,
    pub(crate) artifact_id: u64,
    pub(crate) archive: PathBuf,
    pub(crate) record: PathBuf,
    pub(crate) receipt: PathBuf,
    pub(crate) scratch: PathBuf,
    pub(crate) archive_sha256: String,
    pub(crate) platform_digest: String,
    pub(crate) config_digest: String,
    pub(crate) lock_sha256: String,
    pub(crate) expected_digest: String,
    pub(crate) expected_inventory: BTreeMap<String, Vec<u8>>,
    pub(crate) archive_index: Vec<u8>,
    pub(crate) layer_digests: Vec<String>,
    state: PathBuf,
    docker_config: PathBuf,
    mise_bin: PathBuf,
    error_path: PathBuf,
}

struct FixtureSubjects {
    archive: PathBuf,
    record: PathBuf,
    archive_sha256: String,
    record_sha256: String,
    lock_sha256: String,
    source_files: BTreeMap<String, Vec<u8>>,
}

struct FixtureRuntime {
    state: PathBuf,
    scratch: PathBuf,
    docker_config: PathBuf,
    mise_bin: PathBuf,
    error_path: PathBuf,
}

impl Fixture {
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        Self::new_for(FixtureProfile::Job)
    }

    pub(crate) fn new_for(profile: FixtureProfile) -> Result<Self, Box<dyn Error>> {
        Self::new_with_profile(profile, None)
    }

    pub(crate) fn new_with_image_config(
        image_config: Option<Value>,
    ) -> Result<Self, Box<dyn Error>> {
        Self::new_with_profile(FixtureProfile::Job, image_config)
    }

    pub(crate) fn new_with_raw_tar() -> Result<Self, Box<dyn Error>> {
        let source = "a".repeat(40);
        Self::new_with_image(
            FixtureProfile::Job,
            oci::create_with_raw_tar(&serde_json::json!({
                "Labels":{"org.opencontainers.image.revision":source},
                "User":"fixture"
            }))?,
        )
    }

    pub(crate) fn new_with_large_raw_tar() -> Result<Self, Box<dyn Error>> {
        let source = "a".repeat(40);
        Self::new_with_image(
            FixtureProfile::Job,
            oci::create_with_large_raw_tar(&serde_json::json!({
                "Labels":{"org.opencontainers.image.revision":source},
                "User":"fixture"
            }))?,
        )
    }

    fn new_with_profile(
        profile: FixtureProfile,
        image_config: Option<Value>,
    ) -> Result<Self, Box<dyn Error>> {
        let image = match image_config {
            Some(config) => oci::create_with_image_config(&config)?,
            None => oci::create_for(profile, &"a".repeat(40))?,
        };
        Self::new_with_image(profile, image)
    }

    fn new_with_image(profile: FixtureProfile, image: oci::Layout) -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let source = "a".repeat(40);
        let authority_sha = profile.authority_sha(&source);
        let subjects = write_subjects(&base, profile, &source, &authority_sha, &image)?;
        let attestation = records::attestation(
            profile,
            &source,
            &authority_sha,
            &subjects.archive_sha256,
            &subjects.record_sha256,
        );
        let state = records::state_value(
            profile,
            &source,
            &authority_sha,
            subjects.source_files,
            &attestation,
        );
        let runtime = create_runtime(&base, state, profile, &source)?;
        let platform_digest = image.identity.platform_manifest_digest.clone();
        Ok(Self {
            _temp: temp,
            profile,
            source,
            authority_sha,
            producer_run_id: profile.run_id(),
            artifact_id: profile.artifact_id(),
            archive: subjects.archive,
            record: subjects.record,
            receipt: base.join("receipt.json"),
            scratch: runtime.scratch,
            archive_sha256: subjects.archive_sha256,
            platform_digest: platform_digest.clone(),
            config_digest: image.identity.image_configuration_digest.clone(),
            lock_sha256: subjects.lock_sha256,
            expected_digest: platform_digest,
            expected_inventory: image.inventory,
            archive_index: image.index,
            layer_digests: image.identity.layer_digests,
            state: runtime.state,
            docker_config: runtime.docker_config,
            mise_bin: runtime.mise_bin,
            error_path: runtime.error_path,
        })
    }

    pub(crate) fn invoke(&self) -> Result<Output, std::io::Error> {
        match fs::remove_file(&self.error_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let path = self
            .mise_bin
            .parent()
            .ok_or_else(|| std::io::Error::other("fixture mise binary has no parent directory"))?;
        let home = self
            .state
            .parent()
            .ok_or_else(|| std::io::Error::other("fixture state has no parent directory"))?;
        Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
            .env_clear()
            .env("PATH", path)
            .env("HOME", home)
            .env("GH_TOKEN", "fixture-source-token")
            .env("DOCKER_CONFIG", &self.docker_config)
            .args([
                "image-maintenance",
                "publish-image",
                "--profile",
                self.profile.cli_name(),
                "--source-commit",
            ])
            .arg(&self.source)
            .args(["--producer-run-id", &self.producer_run_id.to_string()])
            .args(["--producer-run-attempt", &RUN_ATTEMPT.to_string()])
            .args([
                "--artifact-id",
                &self.artifact_id.to_string(),
                "--archive-path",
            ])
            .arg(&self.archive)
            .arg("--record-path")
            .arg(&self.record)
            .arg("--private-scratch")
            .arg(&self.scratch)
            .arg("--receipt-output")
            .arg(&self.receipt)
            .output()
    }

    pub(crate) fn registry_digest(&self) -> Result<Option<String>, Box<dyn Error>> {
        let state: Value = serde_json::from_slice(&fs::read(&self.state)?)?;
        Ok(state["crane"]["tag_digest"].as_str().map(str::to_owned))
    }

    pub(crate) fn stored_inventory(&self) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
        let state: Value = serde_json::from_slice(&fs::read(&self.state)?)?;
        state["crane"]["remote"]["inventory"]
            .as_object()
            .ok_or("remote OCI inventory is missing")?
            .iter()
            .map(|(path, entry)| {
                let bytes = entry["bytes"]
                    .as_array()
                    .ok_or("remote OCI blob bytes are missing")?
                    .iter()
                    .map(|byte| {
                        u8::try_from(byte.as_u64().ok_or("remote OCI blob byte is invalid")?)
                            .map_err(|_| "remote OCI blob byte is out of range")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((path.clone(), bytes))
            })
            .collect()
    }

    pub(crate) fn fail_remote_readback(&self) -> Result<(), Box<dyn Error>> {
        let mut state: Value = serde_json::from_slice(&fs::read(&self.state)?)?;
        state["crane"]["fail_blob_readback"] = json!(true);
        fs::write(&self.state, serde_json::to_vec(&state)?)?;
        Ok(())
    }

    pub(crate) fn set_initial_manifest_outcome(
        &self,
        outcome: CapturedOutcome,
        mutation: ReplayMutation,
    ) -> Result<(), Box<dyn Error>> {
        let mut state: Value = serde_json::from_slice(&fs::read(&self.state)?)?;
        state["crane"]["tag_lookup_output"] =
            manifest_outcomes::replay(outcome, mutation, self.profile, &self.source)?;
        state["crane"]["tag_lookup_output_consumed"] = json!(false);
        fs::write(&self.state, serde_json::to_vec(&state)?)?;
        Ok(())
    }

    pub(crate) fn set_remote_blob_fault(
        &self,
        kind: &str,
        fault: BlobFault,
    ) -> Result<(), Box<dyn Error>> {
        registry_state::set_blob_fault(self, kind, fault)
    }

    pub(crate) fn move_tag_after_readback(&self) -> Result<MovedTag, Box<dyn Error>> {
        registry_state::move_tag_after_readback(self)
    }

    pub(crate) fn corrupt_remote_manifest_response_at(
        &self,
        offset: u64,
    ) -> Result<(), Box<dyn Error>> {
        registry_state::corrupt_remote_manifest_response_at(self, offset)
    }

    pub(crate) fn seed_existing_tag(&self, tag: ExistingTag) -> Result<(), Box<dyn Error>> {
        registry_state::seed_existing_tag(self, tag)
    }

    pub(crate) fn last_fixture_error(&self) -> Result<Option<String>, Box<dyn Error>> {
        if self.error_path.is_file() {
            return Ok(Some(fs::read_to_string(&self.error_path)?));
        }
        let state: Value = serde_json::from_slice(&fs::read(&self.state)?)?;
        Ok(state["last_fixture_error"].as_str().map(str::to_owned))
    }
}

fn write_subjects(
    base: &std::path::Path,
    profile: FixtureProfile,
    source: &str,
    authority_sha: &str,
    image: &oci::Layout,
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
    let record_bytes = records::make_record(
        profile,
        source,
        authority_sha,
        &source_inputs,
        &archive_sha256,
        image,
        &source_files,
    )?;
    fs::write(&record, &record_bytes)?;
    Ok(FixtureSubjects {
        archive,
        record,
        archive_sha256,
        record_sha256: hex(&Sha256::digest(&record_bytes)),
        lock_sha256,
        source_files,
    })
}

fn create_runtime(
    base: &std::path::Path,
    mut state_value: Value,
    profile: FixtureProfile,
    source: &str,
) -> Result<FixtureRuntime, Box<dyn Error>> {
    let home = base.join("home");
    fs::create_dir(&home)?;
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
    let state = home.join("fixture-state.json");
    state_value["crane"]["tag_lookup_output"] = manifest_outcomes::replay(
        CapturedOutcome::Missing,
        ReplayMutation::Exact,
        profile,
        source,
    )?;
    state_value["crane"]["tag_lookup_output_consumed"] = json!(false);
    fs::write(&state, serde_json::to_vec(&state_value)?)?;
    let scratch = base.join("scratch");
    fs::create_dir(&scratch)?;
    fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))?;
    let docker_config = base.join("docker-config");
    fs::create_dir(&docker_config)?;
    fs::set_permissions(&docker_config, fs::Permissions::from_mode(0o700))?;
    let config = docker_config.join("config.json");
    fs::write(&config, b"{}")?;
    fs::set_permissions(config, fs::Permissions::from_mode(0o600))?;
    let bin = base.join("bin");
    fs::create_dir(&bin)?;
    let mise_bin = bin.join("mise");
    fs::copy(fixture_executable()?, &mise_bin)?;
    fs::set_permissions(&mise_bin, fs::Permissions::from_mode(0o755))?;
    Ok(FixtureRuntime {
        state,
        scratch,
        docker_config,
        mise_bin,
        error_path: home.join("image-publication-fixture-error"),
    })
}

fn fixture_executable() -> Result<PathBuf, Box<dyn Error>> {
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"));
    let examples = cli
        .parent()
        .ok_or("CLI executable has no parent directory")?
        .join("examples");
    let executable = examples.join(format!(
        "image-publication-fixture{}",
        std::env::consts::EXE_SUFFIX
    ));
    if !executable.is_file() {
        return Err(format!("registered image-publication-fixture is missing at {}; run package nextest so Cargo builds examples", executable.display()).into());
    }
    Ok(executable)
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
