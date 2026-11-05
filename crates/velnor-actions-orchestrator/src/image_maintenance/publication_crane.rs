// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use velnor_actions_contract::ImageCandidateOci;
use velnor_actions_mise::{CheckDeadline, PinnedTool, PinnedToolExec, ToolCatalog};

use crate::{OrchestratorError, internal::internal};

#[path = "publication_crane/blobs.rs"]
mod blobs;
#[path = "publication_crane/role.rs"]
mod role;
pub(in crate::image_maintenance) use role::verify_native_role;

const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PublishedImage {
    pub digest: String,
    pub idempotent: bool,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    config: Descriptor,
    layers: Vec<Descriptor>,
}

#[derive(Debug, Deserialize)]
struct Descriptor {
    digest: String,
    size: u64,
}

pub(super) fn publish(
    layout: &Path,
    repository: &str,
    tag: &str,
    expected: &ImageCandidateOci,
    deadline: CheckDeadline,
    mut revalidate: impl FnMut() -> Result<(), OrchestratorError>,
) -> Result<PublishedImage, OrchestratorError> {
    let docker_config = private_docker_config()?;
    let reference = format!("{repository}:{tag}");
    let expected_digest = expected.platform_manifest_digest.clone();
    let current = inspect_tag(&docker_config, &reference, deadline)?;
    let idempotent = match current {
        TagState::Missing => {
            revalidate()?;
            crane_ok(
                &docker_config,
                vec![
                    OsString::from("push"),
                    layout.as_os_str().to_owned(),
                    OsString::from(&reference),
                ],
                deadline,
                "image_publication_crane_push",
            )?;
            false
        }
        TagState::Digest(digest) if digest == expected_digest => {
            revalidate()?;
            true
        }
        TagState::Digest(_) => return Err(internal("image_publication_tag_conflict")),
    };
    let pushed = read_tag_digest(&docker_config, &reference, deadline)?;
    if pushed != expected_digest {
        return Err(internal("image_publication_pushed_digest"));
    }
    verify_remote_bytes(
        layout,
        &docker_config,
        repository,
        &expected_digest,
        expected,
        deadline,
    )?;
    let final_tag = read_tag_digest(&docker_config, &reference, deadline)?;
    if final_tag != expected_digest {
        return Err(internal("image_publication_tag_changed"));
    }
    Ok(PublishedImage {
        digest: expected_digest,
        idempotent,
    })
}

pub(in crate::image_maintenance) fn promote(
    repository: &str,
    source_tag: &str,
    version: &str,
    deadline: CheckDeadline,
    mut authorize: impl FnMut(&str) -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    let docker_config = private_docker_config()?;
    let source = format!("{repository}:{source_tag}");
    let target = format!("{repository}:{version}");
    let expected = read_tag_digest(&docker_config, &source, deadline)?;
    authorize(&expected)?;
    match inspect_tag(&docker_config, &target, deadline)? {
        TagState::Missing => {
            authorize(&expected)?;
            if read_tag_digest(&docker_config, &source, deadline)? != expected {
                return Err(internal("image_publication_source_tag_changed"));
            }
            match inspect_tag(&docker_config, &target, deadline)? {
                TagState::Missing => crane_ok(
                    &docker_config,
                    vec![
                        OsString::from("tag"),
                        OsString::from(format!("{repository}@{expected}")),
                        OsString::from(version),
                    ],
                    deadline,
                    "image_publication_crane_tag",
                )?,
                TagState::Digest(digest) if digest == expected => (),
                TagState::Digest(_) => return Err(internal("image_publication_tag_conflict")),
            }
        }
        TagState::Digest(digest) if digest == expected => (),
        TagState::Digest(_) => return Err(internal("image_publication_tag_conflict")),
    }
    if read_tag_digest(&docker_config, &target, deadline)? != expected {
        return Err(internal("image_publication_tag_changed"));
    }
    if read_tag_digest(&docker_config, &source, deadline)? != expected {
        return Err(internal("image_publication_source_tag_changed"));
    }
    Ok(())
}

enum TagState {
    Missing,
    Digest(String),
}

fn inspect_tag(
    docker_config: &Path,
    reference: &str,
    deadline: CheckDeadline,
) -> Result<TagState, OrchestratorError> {
    let args = vec![OsString::from("manifest"), OsString::from(reference)];
    let output = crane_run(docker_config, args, deadline)?;
    if output.success {
        return Ok(TagState::Digest(digest(&output.stdout)));
    }
    if is_missing_tag(&output, reference) {
        return Ok(TagState::Missing);
    }
    Err(internal("image_publication_tag_lookup"))
}

fn read_tag_digest(
    docker_config: &Path,
    reference: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let output = crane_run(
        docker_config,
        vec![OsString::from("manifest"), OsString::from(reference)],
        deadline,
    )?;
    if !output.success {
        return Err(internal("image_publication_tag_digest"));
    }
    if output.stdout.is_empty() {
        return Err(internal("image_publication_tag_digest"));
    }
    Ok(digest(&output.stdout))
}

fn crane_ok(
    docker_config: &Path,
    args: Vec<OsString>,
    deadline: CheckDeadline,
    failure: &'static str,
) -> Result<(), OrchestratorError> {
    let output = crane_run(docker_config, args, deadline)?;
    if !output.success || output.code != Some(0) || output.signal.is_some() {
        return Err(internal(failure));
    }
    Ok(())
}

fn verify_remote_bytes(
    layout: &Path,
    docker_config: &Path,
    repository: &str,
    expected_digest: &str,
    expected: &ImageCandidateOci,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    blobs::verify_remote_bytes(
        layout,
        docker_config,
        repository,
        expected_digest,
        expected,
        deadline,
    )
}

fn crane_run(
    docker_config: &Path,
    args: Vec<OsString>,
    deadline: CheckDeadline,
) -> Result<velnor_actions_mise::ProcessOutput, OrchestratorError> {
    deadline
        .remaining()
        .map_err(|_| internal("image_publication_deadline"))?;
    let command = PinnedToolExec::new(vec![PinnedTool::Crane], OsStr::new("crane"), args)
        .map_err(|_| internal("image_publication_crane_unavailable"))?
        .command(&ToolCatalog::pinned())
        .and_then(|command| {
            command.with_env(&[(
                OsString::from("DOCKER_CONFIG"),
                docker_config.as_os_str().to_owned(),
            )])
        })
        .map_err(|_| internal("image_publication_crane_environment"))?;
    let (output, _) = command
        .run_quiescent_until(OUTPUT_LIMIT, deadline)
        .map_err(|_| internal("image_publication_crane_process"))?;
    Ok(output)
}

pub(super) fn crane_file(
    docker_config: &Path,
    args: Vec<OsString>,
    output: &mut fs::File,
    stdout_cap: u64,
    deadline: CheckDeadline,
) -> Result<
    (
        velnor_actions_mise::ProcessFileOutput,
        velnor_actions_mise::ProcessGroupDrainReceipt,
    ),
    OrchestratorError,
> {
    deadline
        .remaining()
        .map_err(|_| internal("image_publication_deadline"))?;
    let command = crane_command(docker_config, args)?;
    let result = command
        .run_quiescent_stdout_to_file_until(output, stdout_cap, OUTPUT_LIMIT, deadline)
        .map_err(|_| internal("image_publication_crane_process"))?;
    Ok(result)
}

fn crane_command(
    docker_config: &Path,
    args: Vec<OsString>,
) -> Result<velnor_actions_mise::IsolatedCommand, OrchestratorError> {
    PinnedToolExec::new(vec![PinnedTool::Crane], OsStr::new("crane"), args)
        .map_err(|_| internal("image_publication_crane_unavailable"))?
        .command(&ToolCatalog::pinned())
        .and_then(|command| {
            command.with_env(&[(
                OsString::from("DOCKER_CONFIG"),
                docker_config.as_os_str().to_owned(),
            )])
        })
        .map_err(|_| internal("image_publication_crane_environment"))
}

fn private_docker_config() -> Result<std::path::PathBuf, OrchestratorError> {
    let value = env::var_os("DOCKER_CONFIG")
        .ok_or_else(|| internal("image_publication_docker_config_missing"))?;
    let path = std::path::PathBuf::from(value);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| internal("image_publication_docker_config_missing"))?;
    if !path.is_absolute()
        || !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o777 != 0o700
        || path.canonicalize().ok().as_deref() != Some(path.as_path())
    {
        return Err(internal("image_publication_docker_config_private"));
    }
    let config = path.join("config.json");
    let config_metadata = fs::symlink_metadata(&config)
        .map_err(|_| internal("image_publication_docker_config_missing"))?;
    if !config_metadata.is_file()
        || config_metadata.file_type().is_symlink()
        || config_metadata.permissions().mode() & 0o077 != 0
    {
        return Err(internal("image_publication_docker_config_private"));
    }
    Ok(path)
}

fn is_missing_tag(output: &velnor_actions_mise::ProcessOutput, reference: &str) -> bool {
    if output.success
        || output.code != Some(1)
        || output.signal.is_some()
        || !output.stdout.is_empty()
    {
        return false;
    }
    let Some(image) = reference.strip_prefix("ghcr.io/") else {
        return false;
    };
    let Some((repository, tag)) = image.split_once(':') else {
        return false;
    };
    if repository.is_empty() || tag.is_empty() || tag.contains(':') {
        return false;
    }
    let prefix = format!(
        "Error: fetching manifest {reference}: GET https://ghcr.io/v2/{repository}/manifests/{tag}: "
    );
    let Some(detail) = output
        .stderr
        .strip_prefix(prefix.as_bytes())
        .and_then(|value| value.strip_suffix(b"\n"))
    else {
        return false;
    };
    let Some(message) = detail
        .strip_prefix(b"MANIFEST_UNKNOWN: ")
        .or_else(|| detail.strip_prefix(b"NAME_UNKNOWN: "))
    else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(message) else {
        return false;
    };
    !message.is_empty()
        && message.len() <= 512
        && !text
            .chars()
            .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
        && !message.iter().enumerate().any(|(index, byte)| {
            if *byte != b':' {
                return false;
            }
            let code = message[..index]
                .rsplit(|byte| !byte.is_ascii_uppercase() && *byte != b'_')
                .next()
                .unwrap_or_default();
            !code.is_empty()
        })
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex(&Sha256::digest(bytes)))
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
