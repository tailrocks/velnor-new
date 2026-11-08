// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::image_candidate_record::{
    ImageCandidateArchive, ImageCandidateBuild, ImageCandidateOci, ImageCandidateWorkflow,
    validate_archive, validate_build, validate_oci, validate_workflow,
};
use crate::ContractError;

pub const IMAGE_QUALIFICATION_SCHEMA: &str = "velnor.native-image-qualification.v1";
pub const QUALIFICATION_DAEMON_IMAGE: &str =
    "ubuntu:26.04@sha256:42527649dbc4f9197a6073f426f488cbff0e9cc9f2afce32d91c383e4875634a";
pub const QUALIFICATION_BASE_DIGEST: &str =
    "sha256:42527649dbc4f9197a6073f426f488cbff0e9cc9f2afce32d91c383e4875634a";
pub const QUALIFICATION_DAEMON_SCRIPT: &str = r#"set -eu
umask 077
cp /docker.tgz /tmp/docker-private.tgz
printf '%s  %s\n' "$1" /tmp/docker-private.tgz | sha256sum -c -
tar -tzf /tmp/docker-private.tgz | LC_ALL=C sort > /tmp/members
printf '%s\n' docker/ docker/containerd docker/containerd-shim-runc-v2 docker/ctr docker/docker docker/docker-init docker/docker-proxy docker/dockerd docker/runc | LC_ALL=C sort > /tmp/expected
cmp /tmp/members /tmp/expected
tar -tvzf /tmp/docker-private.tgz | awk 'substr($1,1,1) == "-" { n++; next } substr($1,1,1) == "d" { d++; next } { exit 1 } END { if (n != 8 || d != 1) exit 1 }'
mkdir -m 700 /native /run/velnor-q
chmod 0755 /native
chgrp "$2" /run/velnor-q
chmod 0710 /run/velnor-q
for member in containerd containerd-shim-runc-v2 ctr docker docker-init docker-proxy dockerd runc; do
  tar -xOzf /tmp/docker-private.tgz "docker/$member" > "/native/$member"
  chmod 0555 "/native/$member"
done
export PATH=/native:/usr/bin:/bin
printf '%s\n' '{"storage-driver":"native","features":{"containerd-snapshotter":true},"iptables":false,"ip6tables":false,"bridge":"none","ip-forward":false,"ip-masq":false}' > /tmp/daemon.json
dockerd --host=unix:///run/velnor-q/docker.sock --group "$2" --config-file=/tmp/daemon.json </dev/null &
daemon=$!
cleanup() { kill -TERM "$daemon" 2>/dev/null || :; wait "$daemon" || :; rm -f /run/velnor-q/docker.sock; }
trap cleanup EXIT HUP INT TERM
read -r _ || :
"#;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationDaemonDescriptor {
    pub container_id: String,
    pub daemon_id: String,
    pub container_started_at: String,
    pub container_pid: u64,
    pub archive_sha256: String,
    pub archive_path: String,
}
pub const IMAGE_QUALIFICATION_INPUTS: [&str; 4] = [
    "crates/velnor-actions-cli/tests/fixtures/native-image-qualification/Cargo.lock",
    "crates/velnor-actions-cli/tests/fixtures/native-image-qualification/Cargo.toml",
    "crates/velnor-actions-cli/tests/fixtures/native-image-qualification/Dockerfile",
    "crates/velnor-actions-cli/tests/fixtures/native-image-qualification/src/main.rs",
];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeImageQualificationRecord {
    pub schema: String,
    pub source_commit: String,
    pub source_ref: String,
    pub source_inputs: BTreeMap<String, String>,
    pub binary_sha256: String,
    pub workflow: ImageCandidateWorkflow,
    pub build: ImageCandidateBuild,
    pub archive: ImageCandidateArchive,
    pub oci: ImageCandidateOci,
}

pub fn validate_native_image_qualification_record(
    record: &NativeImageQualificationRecord,
) -> Result<(), ContractError> {
    let expected: BTreeSet<&str> = IMAGE_QUALIFICATION_INPUTS.into_iter().collect();
    let actual: BTreeSet<&str> = record.source_inputs.keys().map(String::as_str).collect();
    if record.schema != IMAGE_QUALIFICATION_SCHEMA
        || !lower_hex(&record.source_commit, 40)
        || record.workflow.repository != "tailrocks/velnor-new"
        || record.workflow.repository_id != 1_390_620_900
        || !native_image_qualification_identity(
            &record.source_ref,
            &record.source_commit,
            &record.workflow,
        )
        || actual != expected
        || record
            .source_inputs
            .values()
            .any(|value| !lower_hex(value, 64))
        || !lower_hex(&record.binary_sha256, 64)
    {
        return Err(ContractError::CanonicalJson(
            "native_image_qualification_identity".to_owned(),
        ));
    }
    validate_build(&record.build)?;
    validate_archive(&record.archive)?;
    validate_oci(&record.oci)
}

pub fn parse_native_image_qualification_record(
    bytes: &[u8],
) -> Result<NativeImageQualificationRecord, ContractError> {
    let value = crate::strict_json::parse_strict_json_bytes(
        bytes,
        super::image_candidate_record::IMAGE_CANDIDATE_MAX_RECORD_BYTES,
    )?;
    let record: NativeImageQualificationRecord =
        serde_json::from_value(value).map_err(|error| {
            ContractError::CanonicalJson(format!("native_image_qualification:{error}"))
        })?;
    validate_native_image_qualification_record(&record)?;
    Ok(record)
}

pub fn serialize_native_image_qualification_record(
    record: &NativeImageQualificationRecord,
) -> Result<Vec<u8>, ContractError> {
    validate_native_image_qualification_record(record)?;
    let mut bytes = crate::canonical_json_bytes(record)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[must_use]
pub fn native_image_qualification_identity(
    source_ref: &str,
    source_commit: &str,
    workflow: &ImageCandidateWorkflow,
) -> bool {
    if !lower_hex(source_commit, 40) || validate_workflow(workflow).is_err() {
        return false;
    }
    match workflow.event.as_str() {
        "pull_request" => {
            source_ref
                .strip_prefix("refs/pull/")
                .and_then(|value| value.strip_suffix("/merge"))
                .is_some_and(|value| {
                    !value.is_empty()
                        && !value.starts_with('0')
                        && value.bytes().all(|byte| byte.is_ascii_digit())
                })
                && workflow
                    .workflow_ref
                    .starts_with("tailrocks/velnor-new/.github/workflows/ci.yml@refs/")
        }
        "merge_group" => {
            let Some((number, suffix)) = source_ref
                .strip_prefix("refs/heads/gh-readonly-queue/main/pr-")
                .and_then(|value| value.split_once('-'))
            else {
                return false;
            };
            let prefix = "tailrocks/velnor-new/.github/workflows/ci.yml@";
            !number.is_empty()
                && !number.starts_with('0')
                && number.bytes().all(|byte| byte.is_ascii_digit())
                && !suffix.is_empty()
                && lower_hex(suffix, suffix.len())
                && workflow
                    .workflow_ref
                    .strip_prefix(prefix)
                    .is_some_and(|reference| {
                        reference == source_ref || reference == "refs/heads/main"
                    })
        }
        "push" | "workflow_dispatch" => {
            source_ref == "refs/heads/main"
                && workflow.authority_commit == source_commit
                && workflow.workflow_ref
                    == "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main"
        }
        _ => false,
    }
}
