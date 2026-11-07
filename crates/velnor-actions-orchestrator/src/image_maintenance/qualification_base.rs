// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::validate;
use crate::{OrchestratorError, internal::internal};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Cursor, Read},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};
use velnor_actions_contract::QUALIFICATION_BASE_DIGEST;
use velnor_actions_mise::{CheckDeadline, ImageCandidateCommand};

const MAX_BASE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 1024 * 1024;

pub(super) fn prepare(temp: &Path, deadline: CheckDeadline) -> Result<PathBuf, OrchestratorError> {
    let root = temp.join("velnor-qualification-base");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .map_err(|_| internal("native_base_directory"))?;
    let config = root.join("docker-config");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&config)
        .map_err(|_| internal("native_base_config"))?;
    let host = Path::new("/var/run/docker.sock");
    let pull = ImageCandidateCommand::qualification_pull_base(&config)
        .and_then(|command| command.with_private_docker_socket(host))
        .map_err(|_| internal("native_base_pull_command"))?;
    ImageCandidateCommand::run(&pull, deadline).map_err(|_| internal("native_base_pull"))?;
    let saved = root.join("saved.tar");
    let save = ImageCandidateCommand::qualification_save_base(&saved, &config)
        .and_then(|command| command.with_private_docker_socket(host))
        .map_err(|_| internal("native_base_save_command"))?;
    ImageCandidateCommand::run(&save, deadline).map_err(|_| internal("native_base_save"))?;
    let normalized = root.join("normalized.tar");
    normalize(&saved, &normalized, deadline)?;
    let evidence = deadline
        .scope(|| {
            velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_oci_candidate(
                &normalized,
            )
        })
        .map_err(|_| internal("native_base_deadline"))?
        .map_err(|_| internal("native_base_oci"))?;
    if evidence.platform_manifest_digest != QUALIFICATION_BASE_DIGEST {
        return Err(internal("native_base_digest"));
    }
    let layout = root.join("layout");
    deadline
        .scope(|| {
            velnor_archive_guard::owned_archive_guard::oci_candidate::materialize_oci_candidate(
                &normalized,
                &layout,
                &evidence,
            )
        })
        .map_err(|_| internal("native_base_deadline"))?
        .map_err(|_| internal("native_base_materialize"))?;
    Ok(layout)
}

fn normalize(
    source: &Path,
    destination: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let bytes = deadline
        .scope(|| validate::read_limited(source, MAX_BASE_BYTES))
        .map_err(|_| internal("native_base_deadline"))??;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| internal("native_base_archive_create"))?;
    let mut output = tar::Builder::new(file);
    let mut archive = tar::Archive::new(Cursor::new(bytes));
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    let mut index = None;
    for entry in archive
        .entries()
        .map_err(|_| internal("native_base_archive"))?
    {
        deadline
            .remaining()
            .map_err(|_| internal("native_base_deadline"))?;
        let mut entry = entry.map_err(|_| internal("native_base_archive"))?;
        let name = entry
            .path()
            .map_err(|_| internal("native_base_path"))?
            .to_str()
            .ok_or_else(|| internal("native_base_path"))?
            .trim_end_matches('/')
            .to_owned();
        if names.len() >= 8192 {
            return Err(internal("native_base_entries"));
        }
        if !names.insert(name.clone()) {
            return Err(internal("native_base_duplicate"));
        }
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            if name != "blobs" && name != "blobs/sha256" {
                return Err(internal("native_base_path"));
            }
            continue;
        }
        if !kind.is_file() {
            return Err(internal("native_base_entry"));
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .filter(|size| *size <= MAX_BASE_BYTES)
            .ok_or_else(|| internal("native_base_size"))?;
        if name == "index.json" {
            index = Some(read_index(&mut entry, size)?);
        } else if name == "manifest.json" {
            if size > MAX_METADATA_BYTES {
                return Err(internal("native_base_metadata"));
            }
        } else if name == "oci-layout" || valid_blob_name(&name) {
            append(&mut output, &name, size, &mut entry)?;
        } else {
            return Err(internal("native_base_path"));
        }
    }
    let index = index.ok_or_else(|| internal("native_base_index"))?;
    if !names.contains("oci-layout") {
        return Err(internal("native_base_layout"));
    }
    append(
        &mut output,
        "index.json",
        index.len() as u64,
        &mut Cursor::new(index),
    )?;
    output
        .finish()
        .map_err(|_| internal("native_base_archive_write"))?;
    Ok(())
}

fn valid_blob_name(name: &str) -> bool {
    name.strip_prefix("blobs/sha256/").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn read_index(entry: &mut impl Read, size: u64) -> Result<Vec<u8>, OrchestratorError> {
    if size > MAX_METADATA_BYTES {
        return Err(internal("native_base_index"));
    }
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|_| internal("native_base_index"))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| internal("native_base_index"))?;
    let value = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| internal("native_base_index"))?;
    let entries = value["manifests"]
        .as_array()
        .filter(|entries| entries.len() == 1)
        .ok_or_else(|| internal("native_base_index"))?;
    let descriptor = &entries[0];
    if descriptor["digest"].as_str() != Some(QUALIFICATION_BASE_DIGEST)
        || descriptor["mediaType"].as_str() != Some("application/vnd.oci.image.manifest.v1+json")
    {
        return Err(internal("native_base_index"));
    }
    let size = descriptor["size"]
        .as_u64()
        .filter(|size| *size > 0 && *size <= MAX_METADATA_BYTES)
        .ok_or_else(|| internal("native_base_index"))?;
    serde_json::to_vec(&serde_json::json!({"schemaVersion": 2, "mediaType": "application/vnd.oci.image.index.v1+json", "manifests": [{"mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": QUALIFICATION_BASE_DIGEST, "size": size, "platform": {"os": "linux", "architecture": "amd64"}}]}))
        .map_err(|_| internal("native_base_index"))
}

fn append(
    output: &mut tar::Builder<File>,
    name: &str,
    size: u64,
    source: &mut impl Read,
) -> Result<(), OrchestratorError> {
    let mut header = tar::Header::new_gnu();
    header.set_size(size);
    header.set_mode(0o444);
    header.set_cksum();
    output
        .append_data(&mut header, name, source)
        .map_err(|_| internal("native_base_archive_write"))
}
