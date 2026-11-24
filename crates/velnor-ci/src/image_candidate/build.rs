// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

use serde_json::{Map, Value, json};

use super::record::{SCHEMA, validate};
use crate::error::CiError;

const DROPPED: &[&str] = &[
    "DOCKER_CONTEXT",
    "DOCKER_HOST",
    "BUILDX_BUILDER",
    "BUILDX_CONFIG",
    "BUILDKIT_HOST",
];
const SECRET_MARKERS: &[&str] = &["TOKEN", "PASSWORD", "SECRET", "CREDENTIAL", "AUTH", "KEY"];

#[derive(Debug)]
pub struct Parts<'a> {
    pub commit: &'a str,
    pub reference: &'a str,
    pub inputs: Value,
    pub workflow: Map<String, Value>,
    pub builder: &'a str,
    pub archive_sha: &'a str,
    pub archive_size: u64,
    pub oci: Value,
}

pub fn record(parts: &Parts<'_>) -> Result<Value, CiError> {
    let record = json!({
        "schema": SCHEMA,
        "source": {"commit": parts.commit, "ref": parts.reference, "inputs": &parts.inputs},
        "workflow": &parts.workflow,
        "platform": "linux/amd64",
        "build": {
            "builder": parts.builder, "platform": "linux/amd64", "arguments": [],
            "exporter": "oci", "provenance": "disabled",
            "compression": "gzip", "force_compression": true,
        },
        "archive": {"sha256": parts.archive_sha, "size_bytes": parts.archive_size},
        "oci": &parts.oci,
    });
    validate(&record)?;
    Ok(record)
}

#[must_use]
pub fn buildx_arguments(dockerfile: &Path, archive: &Path, context: &Path) -> Vec<OsString> {
    let mut output = OsString::from("type=oci,");
    output.push(csv_field(archive));
    output.push(",compression=gzip,force-compression=true");
    let mut arguments: Vec<OsString> = ["buildx", "build", "--platform", "linux/amd64", "--file"]
        .into_iter()
        .map(OsString::from)
        .collect();
    arguments.push(dockerfile.into());
    arguments.push("--provenance=false".into());
    arguments.push("--output".into());
    arguments.push(output);
    arguments.push(context.into());
    arguments
}

fn csv_field(archive: &Path) -> OsString {
    let mut field = OsString::from("dest=");
    field.push(archive);
    let bytes = field.as_bytes();
    if !bytes
        .iter()
        .any(|byte| matches!(byte, b',' | b'"' | b'\n' | b'\r'))
    {
        return field;
    }
    let mut quoted = vec![b'"'];
    for byte in bytes {
        if *byte == b'"' {
            quoted.push(b'"');
        }
        quoted.push(*byte);
    }
    quoted.push(b'"');
    OsString::from_vec(quoted)
}

#[must_use]
pub fn forwards(name: &str) -> bool {
    let upper = name.to_uppercase();
    !DROPPED.contains(&name) && !SECRET_MARKERS.iter().any(|marker| upper.contains(marker))
}
