// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, File, OpenOptions},
    io::Read,
};

use serde_json::json;
use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    ImageCandidateArchive, ImageCandidateOci, RunnerImageNamedContext,
    RunnerImagePublicationProvenance,
};
use velnor_actions_mise::CheckDeadline;
use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    encode_provenance_envelope, project_attested_oci_file,
};

use super::super::FixtureProfile;
use super::{Layout, captured_archive};

pub(crate) fn create_runner_for(
    profile: FixtureProfile,
    source: &str,
) -> Result<Layout, Box<dyn Error>> {
    let role = role(profile)?;
    project_runner_archive(profile, captured_archive::make(role, source, source)?)
}

pub(crate) fn create_runner_with_config(
    profile: FixtureProfile,
    source: &str,
    config: &serde_json::Value,
) -> Result<Layout, Box<dyn Error>> {
    let role = role(profile)?;
    let archive = captured_archive::make(role, source, source)?;
    project_runner_archive(profile, rewrite_configuration(&archive, config)?)
}

pub(crate) fn runner_config(
    profile: FixtureProfile,
    revision: &str,
    image_kind: &str,
) -> serde_json::Value {
    match profile {
        FixtureProfile::Runner | FixtureProfile::Dind => json!({
            "Labels": {
                "org.opencontainers.image.revision": revision,
                "io.velnor.image.kind": image_kind,
            },
            "User": "fixture",
        }),
        FixtureProfile::ResourceProbe => json!({
            "Labels":{"org.opencontainers.image.revision":revision},
            "User":"65532:65532",
            "Env":["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
            "Entrypoint":["/velnor/resource-probe"],
            "WorkingDir":"/"
        }),
        FixtureProfile::Job => json!({}),
    }
}

fn role(profile: FixtureProfile) -> Result<&'static str, Box<dyn Error>> {
    match profile {
        FixtureProfile::Runner => Ok("runner"),
        FixtureProfile::Dind => Ok("dind"),
        FixtureProfile::ResourceProbe => Ok("resource_probe"),
        FixtureProfile::Job => Err("job image has no runner provenance".into()),
    }
}

fn project_runner_archive(
    profile: FixtureProfile,
    original: Vec<u8>,
) -> Result<Layout, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let original_path = temp.path().join("original.oci.tar");
    fs::write(&original_path, original)?;
    let source_file = File::open(&original_path)?;
    let projected_path = temp.path().join("projected.oci.tar");
    let mut projected_file = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(&projected_path)?;
    let instant = std::time::Instant::now();
    let (projection, envelope) = CheckDeadline::with_clock(
        move || instant,
        || -> Result<_, Box<dyn Error>> {
            let deadline = CheckDeadline::after(std::time::Duration::from_secs(1))?;
            let projection = deadline
                .scope(|| project_attested_oci_file(&source_file, &mut projected_file))??;
            let envelope =
                deadline.scope(|| encode_provenance_envelope(&projection.provenance))??;
            Ok((projection, envelope))
        },
    )?;
    let archive = fs::read(&projected_path)?;
    let evidence = projection.candidate.evidence;
    let (index, inventory) = indexed_files(&archive)?;
    let (contexts, named_contexts) = contexts(profile)?;
    let provenance = RunnerImagePublicationProvenance {
        envelope: ImageCandidateArchive {
            sha256: super::hex(&Sha256::digest(&envelope)),
            size_bytes: envelope.len() as u64,
        },
        original_archive: ImageCandidateArchive {
            sha256: projection.provenance.original_archive_sha256,
            size_bytes: projection.provenance.original_archive_size_bytes,
        },
        original_index: ImageCandidateArchive {
            sha256: super::hex(&Sha256::digest(&projection.provenance.original_index)),
            size_bytes: projection.provenance.original_index.len() as u64,
        },
        named_contexts,
    };
    Ok(Layout {
        archive,
        index,
        identity: evidence_identity(&evidence),
        inventory,
        provenance: Some(provenance),
        envelope: Some(envelope),
        contexts,
    })
}

type BlobInventory = BTreeMap<String, Vec<u8>>;
type ContextFiles = BTreeMap<String, (RunnerImageNamedContext, Vec<u8>)>;
type NamedContexts = BTreeMap<String, RunnerImageNamedContext>;

fn indexed_files(archive: &[u8]) -> Result<(Vec<u8>, BlobInventory), Box<dyn Error>> {
    let mut inventory = BTreeMap::new();
    let mut index = Vec::new();
    for entry in tar::Archive::new(archive).entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry
            .path()?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        if name == "index.json" {
            index = bytes;
        } else if name.starts_with("blobs/") {
            inventory.insert(name, bytes);
        }
    }
    Ok((index, inventory))
}

fn evidence_identity(
    evidence: &velnor_archive_guard::owned_archive_guard::oci_candidate::OciCandidateEvidence,
) -> ImageCandidateOci {
    ImageCandidateOci {
        archive_index_digest: evidence.archive_index_digest.clone(),
        archive_index_size_bytes: evidence.archive_index_size_bytes,
        platform_manifest_digest: evidence.platform_manifest_digest.clone(),
        platform_manifest_size_bytes: evidence.platform_manifest_size_bytes,
        image_configuration_digest: evidence.image_configuration_digest.clone(),
        image_configuration_size_bytes: evidence.image_configuration_size_bytes,
        layer_digests: evidence.layer_digests.clone(),
    }
}

fn contexts(profile: FixtureProfile) -> Result<(ContextFiles, NamedContexts), Box<dyn Error>> {
    let names: &[&str] = match profile {
        FixtureProfile::Runner => &[
            "resource_probe",
            "storage_init",
            "job_start_hook",
            "job_started_hook",
        ],
        FixtureProfile::Dind | FixtureProfile::ResourceProbe => &["resource_probe"],
        FixtureProfile::Job => return Err("job image has no runner provenance".into()),
    };
    let mut contexts = BTreeMap::new();
    let mut named_contexts = BTreeMap::new();
    for name in names {
        let bytes = format!("fixture context {name}\n").into_bytes();
        let context = RunnerImageNamedContext {
            path: format!("target/velnor-runner-family/{name}"),
            sha256: super::hex(&Sha256::digest(&bytes)),
            size_bytes: bytes.len() as u64,
        };
        named_contexts.insert((*name).to_owned(), context.clone());
        contexts.insert((*name).to_owned(), (context, bytes));
    }
    Ok((contexts, named_contexts))
}

fn rewrite_configuration(
    archive: &[u8],
    config_value: &serde_json::Value,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut files = BTreeMap::new();
    for entry in tar::Archive::new(archive).entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry
            .path()?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        files.insert(name, bytes);
    }
    let mut outer: serde_json::Value =
        serde_json::from_slice(files.get("index.json").ok_or("outer index")?)?;
    let mut nested = read_json(&files, &outer["manifests"][0])?;
    let platform = nested["manifests"]
        .as_array_mut()
        .ok_or("nested manifests")?
        .iter_mut()
        .find(|item| item["platform"]["os"] == "linux")
        .ok_or("platform manifest")?;
    let mut manifest = read_json(&files, platform)?;
    let mut configuration = read_json(&files, &manifest["config"])?;
    configuration["config"] = config_value.clone();
    replace_blob(
        &mut files,
        &mut manifest["config"],
        serde_json::to_vec(&configuration)?,
    )?;
    replace_blob(&mut files, platform, serde_json::to_vec(&manifest)?)?;
    let platform_value = platform.clone();
    let attestation_descriptor = nested["manifests"]
        .as_array_mut()
        .ok_or("nested manifests")?
        .iter_mut()
        .find(|item| item["platform"]["os"] == "unknown")
        .ok_or("attestation descriptor")?;
    let mut attestation = read_json(&files, attestation_descriptor)?;
    attestation["subject"]["digest"] = platform_value["digest"].clone();
    attestation["subject"]["size"] = platform_value["size"].clone();
    attestation_descriptor["annotations"]["vnd.docker.reference.digest"] =
        platform_value["digest"].clone();
    let statement_descriptor = &mut attestation["layers"][0];
    let mut statement = read_json(&files, statement_descriptor)?;
    statement["subject"][0]["digest"]["sha256"] = json!(
        platform_value["digest"]
            .as_str()
            .ok_or("platform digest")?
            .strip_prefix("sha256:")
            .ok_or("platform sha")?
    );
    replace_blob(
        &mut files,
        statement_descriptor,
        serde_json::to_vec(&statement)?,
    )?;
    replace_blob(
        &mut files,
        attestation_descriptor,
        serde_json::to_vec(&attestation)?,
    )?;
    replace_blob(
        &mut files,
        &mut outer["manifests"][0],
        serde_json::to_vec(&nested)?,
    )?;
    files.insert("index.json".to_owned(), serde_json::to_vec(&outer)?);
    let mut output = tar::Builder::new(Vec::new());
    for (name, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len().try_into()?);
        header.set_mode(0o644);
        header.set_cksum();
        output.append_data(&mut header, name, bytes.as_slice())?;
    }
    Ok(output.into_inner()?)
}

fn read_json(
    files: &BTreeMap<String, Vec<u8>>,
    descriptor: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let key = descriptor["digest"]
        .as_str()
        .ok_or("descriptor digest")?
        .replace(':', "/");
    Ok(serde_json::from_slice(
        files
            .get(&format!("blobs/{key}"))
            .ok_or("descriptor blob")?,
    )?)
}

fn replace_blob(
    files: &mut BTreeMap<String, Vec<u8>>,
    descriptor: &mut serde_json::Value,
    bytes: Vec<u8>,
) -> Result<(), Box<dyn Error>> {
    let old = descriptor["digest"]
        .as_str()
        .ok_or("descriptor digest")?
        .replace(':', "/");
    files.remove(&format!("blobs/{old}"));
    let digest = super::hex(&Sha256::digest(&bytes));
    descriptor["digest"] = json!(format!("sha256:{digest}"));
    descriptor["size"] = json!(bytes.len());
    files.insert(format!("blobs/sha256/{digest}"), bytes);
    Ok(())
}
